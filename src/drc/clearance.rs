//! Clearance violations: port of `Item.clearanceViolations()`, `drc.ClearanceViolation`
//! and `drc.ClearanceBaseline`.

use crate::board::item::BoardItemType;
use crate::board::{Board, ItemId, ItemKind, ShapeSearchTree, TreeEntry, TreeId, TreeKind, TreeObject, Unit};
use crate::geometry::basics::java_round_l;
use crate::geometry::{Point, TileShape};
use std::cmp::Ordering;
use std::collections::HashMap;

/// Category of a clearance violation (`ClearanceViolation.Category`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClearanceViolationCategory {
    PinToPin,
    PinToOutlineOrKeepout,
    FixedRoute,
    OtherUnfixable,
    PotentiallyFixable,
}

/// A clearance violation between two items.
#[derive(Clone, Debug)]
pub struct ClearanceViolation {
    pub first_item: ItemId,
    pub second_item: ItemId,
    /// The intersection of the enlarged shapes.
    pub shape: TileShape,
    pub layer: i32,
    pub expected_clearance: f64,
    pub actual_clearance: f64,
}

impl ClearanceViolation {
    /// True, if neither item is routable (the violation cannot be fixed by the router).
    pub fn is_unfixable(&self, board: &Board) -> bool {
        let routable = |id: ItemId| board.try_item(id).map(|i| i.is_routable()).unwrap_or(false);
        !routable(self.first_item) && !routable(self.second_item)
    }

    pub fn get_category(&self, board: &Board) -> ClearanceViolationCategory {
        if !self.is_unfixable(board) {
            return ClearanceViolationCategory::PotentiallyFixable;
        }
        let first = board.try_item(self.first_item);
        let second = board.try_item(self.second_item);
        let is_pin = |i: Option<&crate::board::Item>| i.map(|i| i.is_pin()).unwrap_or(false);
        let is_outline_or_keepout = |i: Option<&crate::board::Item>| {
            i.map(|i| {
                matches!(
                    i.kind,
                    ItemKind::BoardOutline(_) | ItemKind::ComponentOutline(_) | ItemKind::Obstacle(_)
                )
            })
            .unwrap_or(false)
        };
        let is_route = |i: Option<&crate::board::Item>| i.map(|i| i.is_trace() || i.is_via()).unwrap_or(false);
        let (fp, sp) = (is_pin(first), is_pin(second));
        if fp && sp {
            return ClearanceViolationCategory::PinToPin;
        }
        let (fo, so) = (is_outline_or_keepout(first), is_outline_or_keepout(second));
        if (fp && so) || (sp && fo) {
            return ClearanceViolationCategory::PinToOutlineOrKeepout;
        }
        if is_route(first) || is_route(second) {
            return ClearanceViolationCategory::FixedRoute;
        }
        ClearanceViolationCategory::OtherUnfixable
    }

    /// `aggregateSortedBySeverity`: the violations of all items, sorted by descending
    /// shortfall (stable sort like `List.sort`).
    pub fn aggregate_sorted_by_severity(board: &Board, items: &[ItemId]) -> Vec<ClearanceViolation> {
        let checker = ClearanceChecker::new(board);
        let mut v = Vec::new();
        for &id in items {
            v.extend(checker.clearance_violations(id));
        }
        v.sort_by(|a, b| {
            let da = a.expected_clearance - a.actual_clearance;
            let db = b.expected_clearance - b.actual_clearance;
            java_double_compare(db, da)
        });
        v
    }

    /// `smallestClearance(items)`: the smallest actual clearance of the violations of the
    /// items (the original reads the `smallestClearance` field cached by
    /// `clearanceViolations()`; `f64::MAX` if there is none).
    pub fn smallest_clearance(board: &Board, items: &[ItemId]) -> f64 {
        let checker = ClearanceChecker::new(board);
        let mut smallest = f64::MAX;
        for &id in items {
            let mut item_smallest = -1.0f64;
            for v in checker.clearance_violations(id) {
                if item_smallest < 0.0 || v.actual_clearance < item_smallest {
                    item_smallest = v.actual_clearance;
                }
            }
            if item_smallest >= 0.0 && item_smallest < smallest {
                smallest = item_smallest;
            }
        }
        smallest
    }
}

/// `Double.compare`.
pub(crate) fn java_double_compare(a: f64, b: f64) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        let ba = if a.is_nan() { 0x7ff8000000000000u64 as i64 } else { a.to_bits() as i64 };
        let bb = if b.is_nan() { 0x7ff8000000000000u64 as i64 } else { b.to_bits() as i64 };
        ba.cmp(&bb)
    }
}

/// The uncompensated search tree used for design rule checks
/// (`SearchTreeManager.getUncompensatedTree`). If the board does not contain such a
/// tree, a temporary one is built (the original adds it to the board permanently;
/// the tree content and thereby the results are the same).
enum DrcTree {
    Board(TreeId),
    Owned(Box<ShapeSearchTree>),
}

/// Computes clearance violations on a board. Construct once and reuse for many items,
/// because constructing may build a search tree.
pub struct ClearanceChecker<'a> {
    board: &'a Board,
    tree: DrcTree,
}

impl<'a> ClearanceChecker<'a> {
    pub fn new(board: &'a Board) -> ClearanceChecker<'a> {
        let tree = match board.search_trees.find(true, 0) {
            Some(t) => DrcTree::Board(t),
            None => {
                let mut tree = ShapeSearchTree::new(TreeKind::Default, 0);
                let env = board.tree_env();
                for item in board.items.iter() {
                    let (shapes, layers) = tree.calculate_tree_shapes(item, &env);
                    tree.insert_shapes(TreeObject::Item(item.id), shapes, &layers);
                }
                DrcTree::Owned(Box::new(tree))
            }
        };
        ClearanceChecker { board, tree }
    }

    pub fn board(&self) -> &'a Board {
        self.board
    }

    fn tree(&self) -> &ShapeSearchTree {
        match &self.tree {
            DrcTree::Board(id) => self.board.search_trees.get(*id),
            DrcTree::Owned(t) => t,
        }
    }

    fn tree_shape_count(&self, id: ItemId) -> usize {
        match &self.tree {
            DrcTree::Board(t) => self.board.tree_shape_count(*t, id),
            DrcTree::Owned(t) => {
                let obj = TreeObject::Item(id);
                if t.contains_object(&obj) {
                    t.object_shape_count(&obj)
                } else {
                    match self.board.items.get(id) {
                        Some(item) => t.calculate_tree_shapes(item, &self.board.tree_env()).0.len(),
                        None => 0,
                    }
                }
            }
        }
    }

    fn tree_shape(&self, id: ItemId, index: usize) -> Option<TileShape> {
        match &self.tree {
            DrcTree::Board(t) => self.board.tree_shape(*t, id, index),
            DrcTree::Owned(t) => {
                let obj = TreeObject::Item(id);
                if t.contains_object(&obj) {
                    return t.object_shape(&obj, index).cloned();
                }
                let item = self.board.items.get(id)?;
                let (shapes, _) = t.calculate_tree_shapes(item, &self.board.tree_env());
                shapes.into_iter().nth(index).flatten()
            }
        }
    }

    /// `Item.clearanceViolationCount()`.
    pub fn clearance_violation_count(&self, id: ItemId) -> usize {
        self.clearance_violations(id).len()
    }

    /// `Item.clearanceViolations()`: the clearance violations of an item.
    pub fn clearance_violations(&self, id: ItemId) -> Vec<ClearanceViolation> {
        let board = self.board;
        let mut result = Vec::new();
        let Some(this) = board.try_item(id) else { return result };
        let tree = self.tree();
        let lookup = board.lookup();
        let layer_count = board.layer_count();
        let board_unit_to_um_factor =
            Unit::scale(1.0, board.communication.unit, Unit::Um) / board.communication.resolution.max(1) as f64;
        let mut tolerance_um = board.rules.clearance_tolerance_um;
        if !tolerance_um.is_finite() || tolerance_um < 0.0 {
            tolerance_um = 0.0;
        }
        for i in 0..self.tree_shape_count(id) {
            let Some(current_tile_shape) = self.tree_shape(id, i) else { continue };
            let shape_layer = board.shape_layer(id, i);
            let mut entries: Vec<TreeEntry> = Vec::new();
            tree.overlapping_tree_entries_with_clearance(
                &current_tile_shape,
                shape_layer,
                &[],
                this.clearance_class,
                Some(this),
                &board.rules,
                layer_count,
                &board.bounding_box,
                &lookup,
                &mut entries,
            );
            for entry in entries {
                let TreeObject::Item(other_id) = entry.object else { continue };
                if other_id == id {
                    continue;
                }
                let other = board.item(other_id);
                let mut is_obstacle = board.is_obstacle(other, this);
                if is_obstacle && this.is_trace() && other.is_trace() {
                    let t = this.as_trace().unwrap();
                    let mut contact_point = t.first_corner();
                    let mut contact_found = false;
                    let mut contacts = board.trace_contacts_at(id, &contact_point, true);
                    if contacts.contains(&other_id) {
                        contact_found = true;
                    }
                    if !contact_found {
                        contact_point = t.last_corner();
                        contacts = board.trace_contacts_at(id, &contact_point, true);
                        if contacts.contains(&other_id) {
                            contact_found = true;
                        }
                    }
                    if contact_found {
                        for c in &contacts {
                            let ci = board.item(*c);
                            if ci.is_pin() && ci.shares_net(this) && ci.shares_net(other) {
                                is_obstacle = false;
                                break;
                            }
                        }
                    }
                }
                if is_obstacle {
                    if this.is_board_outline() && other.is_trace() {
                        if !board.items.outline_blocks_nets(this, &other.net_numbers) {
                            is_obstacle = false;
                        }
                    } else if this.is_trace() && other.is_board_outline() {
                        if !board.items.outline_blocks_nets(other, &this.net_numbers) {
                            is_obstacle = false;
                        }
                    } else if (this.is_board_outline() && other.is_pin()) || (this.is_pin() && other.is_board_outline()) {
                        let outline = if this.is_board_outline() { this } else { other };
                        let pin_tile_shape = if this.is_pin() {
                            Some(current_tile_shape.clone())
                        } else {
                            self.tree_shape(other_id, entry.shape_index)
                        };
                        if let Some(pts) = pin_tile_shape {
                            if outline_contains_tile_shape(outline.as_board_outline().unwrap(), &pts) {
                                is_obstacle = false;
                            }
                        }
                    }
                }
                if !is_obstacle {
                    continue;
                }
                let shape1 = &current_tile_shape;
                let Some(shape2) = self.tree_shape(other_id, entry.shape_index) else {
                    log::warn!("Item.clearanceViolations: unexpected null shape");
                    continue;
                };
                let minimum_clearance =
                    board.rules.clearance_matrix.get_value(other.clearance_class, this.clearance_class, shape_layer, false)
                        as f64;
                let cl_comp1 = java_round_l(0.5 * minimum_clearance) as i32;
                let cl_comp2 = java_round_l(minimum_clearance - cl_comp1 as f64) as i32;
                let enlarged1 = if cl_comp1 > 0 { shape1.enlarge(cl_comp1 as f64) } else { shape1.clone() };
                let enlarged2 = if cl_comp2 > 0 { shape2.enlarge(cl_comp2 as f64) } else { shape2.clone() };
                let intersection = enlarged1.intersection(&enlarged2);
                if intersection.dimension() != 2 {
                    continue;
                }
                let actual_clearance =
                    calculate_clearance_between_two_shapes(shape1, &shape2, minimum_clearance, cl_comp1, cl_comp2);
                let shortfall = minimum_clearance - actual_clearance;
                let shortfall_um = shortfall * board_unit_to_um_factor;
                if shortfall_um > tolerance_um
                    || (minimum_clearance <= 0.0
                        && overlaps_beyond(shape1, &shape2, tolerance_um / board_unit_to_um_factor))
                {
                    result.push(ClearanceViolation {
                        first_item: id,
                        second_item: other_id,
                        shape: intersection,
                        layer: shape_layer,
                        expected_clearance: minimum_clearance,
                        actual_clearance,
                    });
                }
            }
        }
        result
    }

    /// `DesignRulesChecker.getAllClearanceViolations()`: the violations of all items,
    /// each pair of items and layer reported once.
    pub fn all_clearance_violations(&self) -> Vec<ClearanceViolation> {
        let mut all = Vec::new();
        let mut seen: std::collections::HashSet<(i32, i32, i32)> = std::collections::HashSet::new();
        for id in self.board.items.ids() {
            for v in self.clearance_violations(id) {
                let (id1, id2) = (v.first_item.0, v.second_item.0);
                let key = if id1 < id2 { (id1, id2, v.layer) } else { (id2, id1, v.layer) };
                if seen.insert(key) {
                    all.push(v);
                }
            }
        }
        all
    }
}

fn overlaps_beyond(shape1: &TileShape, shape2: &TileShape, depth: f64) -> bool {
    let shrunk = if depth > 0.0 { shape1.offset(-depth) } else { shape1.clone() };
    !shrunk.is_empty() && shrunk.intersection(shape2).dimension() == 2
}

fn calculate_clearance_between_two_shapes(
    raw1: &TileShape,
    raw2: &TileShape,
    minimum_clearance: f64,
    cl_comp1: i32,
    cl_comp2: i32,
) -> f64 {
    if raw1.intersection(raw2).dimension() == 2 {
        return 0.0;
    }
    let mut low = 0.0;
    let mut high = minimum_clearance;
    let sum_comp = (cl_comp1 + cl_comp2) as f64;
    let factor1 = if sum_comp > 0.0 { cl_comp1 as f64 / sum_comp } else { 0.5 };
    let factor2 = if sum_comp > 0.0 { cl_comp2 as f64 / sum_comp } else { 0.5 };
    for _ in 0..16 {
        let mid = (low + high) * 0.5;
        let s1 = raw1.enlarge(mid * factor1);
        let s2 = raw2.enlarge(mid * factor2);
        if s1.intersection(&s2).dimension() == 2 {
            high = mid;
        } else {
            low = mid;
        }
    }
    low
}

fn outline_contains_tile_shape(outline: &crate::board::item::BoardOutline, tile: &TileShape) -> bool {
    for ci in 0..tile.border_line_count() {
        if !outline.contains(&tile.corner(ci)) {
            return false;
        }
    }
    true
}

/// `Item.clearanceViolations()` for a single item. Builds the DRC search tree if the
/// board has none; use [`ClearanceChecker`] for many items.
pub fn clearance_violations(board: &Board, id: ItemId) -> Vec<ClearanceViolation> {
    ClearanceChecker::new(board).clearance_violations(id)
}

/// `DesignRulesChecker.getAllClearanceViolations()`.
pub fn all_clearance_violations(board: &Board) -> Vec<ClearanceViolation> {
    ClearanceChecker::new(board).all_clearance_violations()
}

/// The number of (deduplicated) clearance violations of the board.
pub fn clearance_violation_count(board: &Board) -> usize {
    all_clearance_violations(board).len()
}

// -------------------------------------------------------------------- ClearanceBaseline

/// The clearance violations of a reference board state, used to distinguish pre-existing
/// violations from new or changed ones (`drc.ClearanceBaseline`).
#[derive(Clone, Debug)]
pub struct ClearanceBaseline {
    entries: Vec<BaselineEntry>,
}

#[derive(Clone, Debug, PartialEq)]
struct ShapeGeometry {
    layer: i32,
    corners: Option<Vec<Point>>,
}

#[derive(Clone, Debug)]
struct ItemGeometry {
    item_type: BoardItemType,
    nets: Vec<i32>,
    shapes: Vec<ShapeGeometry>,
}

#[derive(Clone, Debug)]
struct BaselineEntry {
    first: Option<std::sync::Arc<ItemGeometry>>,
    second: Option<std::sync::Arc<ItemGeometry>>,
    layer: i32,
    expected: f64,
    actual: f64,
}

impl ItemGeometry {
    fn new(board: &Board, id: ItemId) -> ItemGeometry {
        let item = board.item(id);
        let mut nets: Vec<i32> = item.net_numbers.to_vec();
        nets.sort();
        let n = board.tile_shape_count(id);
        let mut shapes = Vec::with_capacity(n);
        for i in 0..n {
            let corners = board.tile_shape(id, i).map(|shape| {
                let count = shape.border_line_count();
                let mut c: Vec<Point> = Vec::with_capacity(count * 2);
                for j in 0..count {
                    let border = shape.border_line(j);
                    c.push(Point::Int(border.a));
                    c.push(Point::Int(border.b));
                }
                c.sort_by(|a, b| a.compare_xy(b));
                c
            });
            shapes.push(ShapeGeometry {
                layer: board.shape_layer(id, i),
                corners,
            });
        }
        ItemGeometry {
            item_type: item.board_item_type(),
            nets,
            shapes,
        }
    }

    fn matches(&self, other: &ItemGeometry) -> bool {
        self.item_type == other.item_type && self.nets == other.nets && self.shapes == other.shapes
    }
}

impl BaselineEntry {
    fn matches(&self, other: &BaselineEntry) -> bool {
        if self.layer != other.layer
            || java_double_compare(self.expected, other.expected) != Ordering::Equal
            || java_double_compare(self.actual, other.actual) != Ordering::Equal
        {
            return false;
        }
        (matches_items(&self.first, &other.first) && matches_items(&self.second, &other.second))
            || (matches_items(&self.first, &other.second) && matches_items(&self.second, &other.first))
    }
}

fn matches_items(a: &Option<std::sync::Arc<ItemGeometry>>, b: &Option<std::sync::Arc<ItemGeometry>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(_), None) => false,
        (Some(a), Some(b)) => a.matches(b),
    }
}

fn capture(board: &Board, violations: &[ClearanceViolation]) -> Vec<BaselineEntry> {
    let mut cache: HashMap<ItemId, std::sync::Arc<ItemGeometry>> = HashMap::new();
    let mut get = |id: ItemId| -> Option<std::sync::Arc<ItemGeometry>> {
        board.try_item(id)?;
        Some(cache.entry(id).or_insert_with(|| std::sync::Arc::new(ItemGeometry::new(board, id))).clone())
    };
    violations
        .iter()
        .map(|v| BaselineEntry {
            first: get(v.first_item),
            second: get(v.second_item),
            layer: v.layer,
            expected: v.expected_clearance,
            actual: v.actual_clearance,
        })
        .collect()
}

impl ClearanceBaseline {
    /// Captures the violations (with the geometry of the involved items of `board`).
    pub fn new(board: &Board, violations: &[ClearanceViolation]) -> ClearanceBaseline {
        ClearanceBaseline {
            entries: capture(board, violations),
        }
    }

    pub fn count_new_or_changed(&self, board: &Board, violations: &[ClearanceViolation]) -> usize {
        if self.entries.is_empty() {
            return violations.len();
        }
        self.get_new_or_changed(board, violations).len()
    }

    pub fn get_new_or_changed(&self, board: &Board, violations: &[ClearanceViolation]) -> Vec<ClearanceViolation> {
        if self.entries.is_empty() {
            return violations.to_vec();
        }
        let current = capture(board, violations);
        let mut used = vec![false; self.entries.len()];
        let mut result = Vec::new();
        for (n, c) in current.iter().enumerate() {
            let mut matched = None;
            for (i, e) in self.entries.iter().enumerate() {
                if !used[i] && e.matches(c) {
                    matched = Some(i);
                    break;
                }
            }
            match matched {
                None => result.push(violations[n].clone()),
                Some(i) => used[i] = true,
            }
        }
        result
    }
}
