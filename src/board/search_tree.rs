//! Shape search trees for fast overlap queries (port of `app.freerouting.board.searchtree`).
//!
//! The leaves store the tree shape, the layer and the object key directly, so queries
//! do not need to look up the objects for the geometric part.

use super::item::{Item, ItemId, ItemKind, ObstacleKind};
use crate::datastructures::shape_tree::{MinAreaTree, NodeIdx};
use crate::geometry::{
    BoundingDirections, Circle, IntBox, IntOctagon, Line, Point, Polyline, RegularTileShape, Shape,
    TileShape,
};
use crate::library::Padstacks;
use crate::rules::{AngleRestriction, BoardRules};
use rustc_hash::FxHashMap;
use std::cmp::Ordering;

/// Id of a complete free space expansion room of the autorouter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RoomId(pub u32);

/// The objects stored in a search tree: board items or autoroute expansion rooms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TreeObject {
    Item(ItemId),
    Room(RoomId, i32),
}

impl TreeObject {
    #[inline]
    pub fn item(&self) -> Option<ItemId> {
        match self {
            TreeObject::Item(i) => Some(*i),
            _ => None,
        }
    }

    #[inline]
    pub fn room(&self) -> Option<RoomId> {
        match self {
            TreeObject::Room(r, _) => Some(*r),
            _ => None,
        }
    }
}

/// Ordering of the original `compareTo`: rooms before items, both by descending id.
impl Ord for TreeObject {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (TreeObject::Item(a), TreeObject::Item(b)) => a.cmp(b),
            (TreeObject::Room(_, a), TreeObject::Room(_, b)) => b.cmp(a),
            (TreeObject::Item(_), TreeObject::Room(..)) => Ordering::Greater,
            (TreeObject::Room(..), TreeObject::Item(_)) => Ordering::Less,
        }
    }
}

impl PartialOrd for TreeObject {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug)]
pub struct TreeLeaf {
    pub object: TreeObject,
    pub shape_index: u32,
    pub layer: i32,
    pub shape: TileShape,
}

/// An entry found by a search tree query.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TreeEntry {
    pub object: TreeObject,
    pub shape_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    /// General shapes with octagon bounding (class `ShapeSearchTree`).
    Default,
    /// Only octagon shapes (class `ShapeSearchTree45Degree`).
    Degree45,
    /// Only box shapes (class `ShapeSearchTree90Degree`).
    Degree90,
}

/// Board data needed to calculate the tree shapes of items.
pub struct TreeEnv<'a> {
    pub rules: &'a BoardRules,
    pub padstacks: &'a Padstacks,
    pub layer_count: i32,
    pub bounding_box: IntBox,
    pub host_cad_exists: bool,
    pub mil_resolution: f64,
}

/// Item information needed for tree queries.
pub trait ItemLookup {
    fn item(&self, id: ItemId) -> &Item;
    /// `isObstacle(netNumber)` of the original.
    fn is_obstacle_net(&self, id: ItemId, net_no: i32) -> bool;
    /// `isTraceObstacle(netNumber)` of the original.
    fn is_trace_obstacle(&self, id: ItemId, net_no: i32) -> bool;
    /// `a.isObstacle(b)` of the original.
    fn item_is_obstacle(&self, a: &Item, b: &Item) -> bool;
}

const DRILL_HOLE_CLEARANCE_MARGIN: i32 = 10;

#[derive(Clone, Debug)]
pub struct ShapeSearchTree {
    pub kind: TreeKind,
    pub compensated_clearance_class: i32,
    tree: MinAreaTree<TreeLeaf>,
    entries: FxHashMap<TreeObject, Vec<NodeIdx>>,
}

impl ShapeSearchTree {
    pub fn new(kind: TreeKind, compensated_clearance_class: i32) -> ShapeSearchTree {
        let mut tree = MinAreaTree::new();
        tree.tag = compensated_clearance_class * 10
            + match kind {
                TreeKind::Default => 0,
                TreeKind::Degree45 => 1,
                TreeKind::Degree90 => 2,
            };
        ShapeSearchTree {
            kind,
            compensated_clearance_class,
            tree,
            entries: FxHashMap::default(),
        }
    }

    #[inline]
    pub fn bounding_directions(&self) -> BoundingDirections {
        match self.kind {
            TreeKind::Degree90 => BoundingDirections::Orthogonal,
            _ => BoundingDirections::FortyfiveDegree,
        }
    }

    #[inline]
    pub fn is_clearance_compensation_used(&self) -> bool {
        self.compensated_clearance_class > 0
    }

    pub fn size(&self) -> usize {
        self.tree.size()
    }

    #[inline]
    pub fn leaf(&self, idx: NodeIdx) -> &TreeLeaf {
        self.tree.leaf(idx)
    }

    /// The tree entries of an object, None if not in the tree.
    #[inline]
    pub fn entries_of(&self, obj: &TreeObject) -> Option<&[NodeIdx]> {
        self.entries.get(obj).map(|v| v.as_slice())
    }

    #[inline]
    pub fn contains_object(&self, obj: &TreeObject) -> bool {
        self.entries.contains_key(obj)
    }

    /// The tree shape with index `index` of an object stored in this tree.
    #[inline]
    pub fn object_shape(&self, obj: &TreeObject, index: usize) -> Option<&TileShape> {
        let e = self.entries.get(obj)?;
        let idx = *e.get(index)?;
        if idx == crate::datastructures::shape_tree::NONE {
            return None;
        }
        Some(&self.tree.leaf(idx).shape)
    }

    pub fn object_shape_count(&self, obj: &TreeObject) -> usize {
        self.entries.get(obj).map(|v| v.len()).unwrap_or(0)
    }

    fn insert_leaf(&mut self, object: TreeObject, index: usize, layer: i32, shape: TileShape) -> NodeIdx {
        let bounds = shape.bounding_shape(self.bounding_directions());
        self.tree.insert(
            bounds,
            TreeLeaf {
                object,
                shape_index: index as u32,
                layer,
                shape,
            },
        )
    }

    /// Inserts an object with precalculated shapes. `layers[i]` is the layer of `shapes[i]`.
    /// Empty (None) shapes are skipped like in the original.
    pub fn insert_shapes(&mut self, object: TreeObject, shapes: Vec<Option<TileShape>>, layers: &[i32]) {
        if shapes.is_empty() {
            return;
        }
        let mut leaves = Vec::with_capacity(shapes.len());
        for (i, s) in shapes.into_iter().enumerate() {
            match s {
                Some(s) => leaves.push(self.insert_leaf(object, i, layers[i], s)),
                None => leaves.push(crate::datastructures::shape_tree::NONE),
            }
        }
        self.entries.insert(object, leaves);
    }

    pub fn remove_object(&mut self, object: &TreeObject) {
        if let Some(leaves) = self.entries.remove(object) {
            for l in leaves {
                if l != crate::datastructures::shape_tree::NONE {
                    self.tree.remove_leaf(l);
                }
            }
        }
    }

    // ---------------------------------------------------------------- tree shape calculation

    pub fn clearance_compensation_value(&self, rules: &BoardRules, clearance_class: i32, layer: i32) -> i32 {
        if clearance_class <= 0 {
            return 0;
        }
        let r = rules
            .clearance_matrix
            .get_value(clearance_class, self.compensated_clearance_class, layer, false)
            - rules
                .clearance_matrix
                .clearance_compensation_value(self.compensated_clearance_class, layer);
        r.max(0)
    }

    pub fn additional_clearance(&self, rules: &BoardRules, first: i32, second: i32, layer: i32) -> i32 {
        if !self.is_clearance_compensation_used() {
            return rules.clearance_matrix.get_value(first, second, layer, true);
        }
        let c = rules.clearance_matrix.get_value(first, second, layer, false);
        (c - self.clearance_compensation_value(rules, first, layer)
            - self.clearance_compensation_value(rules, second, layer))
        .max(0)
    }

    /// Calculates the shapes of an item for this tree. Returns (shapes, layers).
    pub fn calculate_tree_shapes(&self, item: &Item, env: &TreeEnv) -> (Vec<Option<TileShape>>, Vec<i32>) {
        match &item.kind {
            ItemKind::Pin(_) | ItemKind::Via(_) => self.calculate_drill_shapes(item, env),
            ItemKind::Obstacle(_) => {
                let shapes = self.calculate_obstacle_shapes(item, env);
                let layer = item.first_layer();
                let layers = vec![layer; shapes.len()];
                (shapes.into_iter().map(Some).collect(), layers)
            }
            ItemKind::BoardOutline(_) => {
                let shapes = self.calculate_outline_shapes(item, env);
                let n = shapes.len();
                let layers = (0..n)
                    .map(|i| item.shape_layer_with(i, env.layer_count, n))
                    .collect();
                (shapes.into_iter().map(Some).collect(), layers)
            }
            ItemKind::Trace(t) => {
                let offset = t.half_width + self.clearance_compensation_value(env.rules, item.clearance_class, t.layer);
                let n = t.tile_shape_count();
                let shapes = self.offset_shapes(&t.polyline, offset, 0, t.polyline.line_count() as i32 - 1);
                debug_assert_eq!(shapes.len(), n);
                (shapes.into_iter().map(Some).collect(), vec![t.layer; n])
            }
            ItemKind::ComponentOutline(_) => (Vec::new(), Vec::new()),
        }
    }

    fn calculate_drill_shapes(&self, item: &Item, env: &TreeEnv) -> (Vec<Option<TileShape>>, Vec<i32>) {
        let drill = item.as_drill().unwrap();
        let n = drill.tile_shape_count;
        let mut result = Vec::with_capacity(n);
        let mut layers = Vec::with_capacity(n);
        for i in 0..n {
            let layer = drill.shape_layer(i);
            layers.push(layer);
            let mut current_shape: Option<Shape> = drill.get_shape(i).cloned();
            if current_shape.is_none() {
                current_shape = self.drill_hole_obstacle(item, env);
            }
            let Some(cs) = current_shape else {
                result.push(None);
                continue;
            };
            let mut offset = self.clearance_compensation_value(env.rules, item.clearance_class, layer);
            offset += self.drill_hole_clearance_delta(item, &cs, layer, env);
            let tile = match self.kind {
                TreeKind::Degree90 => TileShape::Box(cs.bounding_box().offset(offset as f64)),
                TreeKind::Degree45 => {
                    let mut t = TileShape::Octagon(cs.bounding_octagon());
                    if t.is_int_box() {
                        t = TileShape::Box(cs.bounding_box());
                    }
                    let t = t.offset(offset as f64);
                    TileShape::Octagon(t.bounding_octagon())
                }
                TreeKind::Default => {
                    let t = match env.rules.get_trace_angle_restriction() {
                        AngleRestriction::NinetyDegree => TileShape::Box(cs.bounding_box()),
                        AngleRestriction::FortyfiveDegree => TileShape::Octagon(cs.bounding_octagon()),
                        AngleRestriction::None => cs.bounding_tile(),
                    };
                    t.enlarge(offset as f64)
                }
            };
            result.push(Some(tile));
        }
        (result, layers)
    }

    fn drill_hole_obstacle(&self, item: &Item, env: &TreeEnv) -> Option<Shape> {
        if env.rules.get_hole_clearance() <= 0 {
            return None;
        }
        let drill = item.as_drill()?;
        let radius = env.padstacks.get(drill.padstack).get_drill_radius();
        if radius <= 0.0 {
            return None;
        }
        let center = drill.center.to_int();
        Some(Shape::Circle(Circle::new(center, radius.ceil() as i32)))
    }

    fn drill_hole_clearance_delta(&self, item: &Item, shape: &Shape, layer: i32, env: &TreeEnv) -> i32 {
        let hole_clearance = env.rules.get_hole_clearance();
        if hole_clearance <= 0 {
            return 0;
        }
        let Some(drill) = item.as_drill() else {
            return 0;
        };
        let ps = env.padstacks.get(drill.padstack);
        let drill_radius = ps.get_drill_radius();
        if drill_radius <= 0.0 {
            return 0;
        }
        let copper_radius = if ps.hole_only {
            drill_radius
        } else {
            let r = shape.border_distance(&drill.center.to_float());
            if r <= 0.0 {
                match ps.get_shape(layer) {
                    Some(s) => s.border_distance(&crate::geometry::FloatPoint::ZERO),
                    None => drill_radius,
                }
            } else {
                r
            }
        };
        let cl_class = if self.compensated_clearance_class > 0 {
            self.compensated_clearance_class
        } else {
            BoardRules::default_clearance_class()
        };
        let copper_clearance = env
            .rules
            .clearance_matrix
            .get_value(item.clearance_class, cl_class, layer, false);
        0.max(
            (drill_radius + hole_clearance as f64 + DRILL_HOLE_CLEARANCE_MARGIN as f64
                - copper_radius
                - copper_clearance as f64)
                .ceil() as i32,
        )
    }

    fn calculate_obstacle_shapes(&self, item: &Item, env: &TreeEnv) -> Vec<TileShape> {
        let obs = item.as_obstacle().unwrap();
        let mut max_width = 50000.0;
        if env.host_cad_exists {
            max_width = (500.0 * env.mil_resolution).min(max_width);
        }
        let mut result = Vec::new();
        for convex in &obs.convex_pieces {
            let offset = self.clearance_compensation_value(env.rules, item.clearance_class, obs.layer);
            let enlarged = convex.enlarge(offset as f64);
            result.extend(enlarged.divide_into_sections(max_width));
        }
        match self.kind {
            TreeKind::Degree45 => result.into_iter().map(|t| TileShape::Octagon(t.bounding_octagon())).collect(),
            TreeKind::Degree90 => result.into_iter().map(|t| TileShape::Box(t.bounding_box())).collect(),
            TreeKind::Default => result,
        }
    }

    fn calculate_outline_shapes(&self, item: &Item, env: &TreeEnv) -> Vec<TileShape> {
        let outline = item.as_board_outline().unwrap();
        let mut result = Vec::new();
        if outline.keepout_outside_outline {
            let keepout = outline_keepout_area(outline, env.bounding_box);
            let convex = keepout.split_to_convex();
            for layer in 0..env.layer_count {
                for c in &convex {
                    let offset = self.clearance_compensation_value(env.rules, item.clearance_class, layer);
                    result.push(c.enlarge(offset as f64));
                }
            }
        } else {
            let half_width = super::item::BoardOutline::HALF_WIDTH;
            for layer in 0..env.layer_count {
                for s in &outline.shapes {
                    let n = s.border_line_count();
                    if n == 0 {
                        continue;
                    }
                    let lines: Vec<Line> = (0..n).map(|i| shape_border_line(s, i)).collect();
                    let mut prev = lines[n - 1];
                    for i in 0..n {
                        let cur = lines[i];
                        let next = lines[(i + 1) % n];
                        let pl = Polyline::from_lines(&[prev, cur, next]);
                        let cmp = self.clearance_compensation_value(env.rules, item.clearance_class, layer);
                        if let Some(t) = pl.offset_shape(half_width + cmp, 0) {
                            result.push(t);
                        } else {
                            result.push(TileShape::empty());
                        }
                        prev = cur;
                    }
                }
            }
        }
        match self.kind {
            TreeKind::Degree45 => result.into_iter().map(|t| TileShape::Octagon(t.bounding_octagon())).collect(),
            TreeKind::Degree90 => result.into_iter().map(|t| TileShape::Box(t.bounding_box())).collect(),
            TreeKind::Default => result,
        }
    }

    pub fn offset_shape(&self, polyline: &Polyline, half_width: i32, no: usize) -> Option<TileShape> {
        match self.kind {
            TreeKind::Degree90 => Some(TileShape::Box(polyline.offset_box(half_width, no))),
            _ => polyline.offset_shape(half_width, no),
        }
    }

    pub fn offset_shapes(&self, polyline: &Polyline, half_width: i32, from_no: i32, to_no: i32) -> Vec<TileShape> {
        match self.kind {
            TreeKind::Degree90 => {
                let from_no = from_no.max(0);
                let to_no = to_no.min(polyline.line_count() as i32 - 1);
                let mut shapes = Vec::new();
                let mut j = from_no;
                while j < to_no - 1 {
                    shapes.push(TileShape::Box(polyline.offset_box(half_width, j as usize)));
                    j += 1;
                }
                shapes
            }
            _ => polyline.offset_shapes_range(half_width, from_no, to_no),
        }
    }

    // ---------------------------------------------------------------- queries

    /// Leaves whose bounding shape overlaps `bounds`, sorted like the original.
    fn overlapping_leaves_sorted(&self, bounds: &RegularTileShape, result: &mut Vec<NodeIdx>) {
        let first = result.len();
        self.tree.overlaps_unsorted(bounds, result);
        let tree = &self.tree;
        result[first..].sort_unstable_by(|a, b| {
            let la = tree.leaf(*a);
            let lb = tree.leaf(*b);
            la.object.cmp(&lb.object).then(la.shape_index.cmp(&lb.shape_index))
        });
    }

    /// All tree entries overlapping `shape` on `layer` (layer < 0: all layers),
    /// ignoring objects which are no obstacles for the `ignore_nets`.
    pub fn overlapping_tree_entries<L: ItemLookup + ?Sized>(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        lookup: &L,
        result: &mut Vec<TreeEntry>,
    ) {
        self.overlapping_tree_entries_of(shape, layer, ignore_nets, None, lookup, result);
    }

    pub fn overlapping_tree_entries_of<L: ItemLookup + ?Sized>(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        obstacles_of: Option<&Item>,
        lookup: &L,
        result: &mut Vec<TreeEntry>,
    ) {
        let bounds = shape.bounding_shape(self.bounding_directions());
        let mut leaves = Vec::new();
        self.tree.overlaps_unsorted(&bounds, &mut leaves);
        // Filtering by layer before sorting keeps the order of the remaining leaves.
        if layer >= 0 {
            leaves.retain(|l| self.tree.leaf(*l).layer == layer);
        }
        let tree = &self.tree;
        leaves.sort_unstable_by(|a, b| {
            let la = tree.leaf(*a);
            let lb = tree.leaf(*b);
            la.object.cmp(&lb.object).then(la.shape_index.cmp(&lb.shape_index))
        });
        let is_45 = matches!(shape, TileShape::Octagon(_));
        for l in leaves {
            let leaf = self.tree.leaf(l);
            let mut ignore = false;
            for n in ignore_nets {
                if !self.object_is_obstacle_net(&leaf.object, *n, lookup) {
                    ignore = true;
                }
            }
            if ignore {
                continue;
            }
            if let Some(of) = obstacles_of {
                match leaf.object {
                    TreeObject::Item(id) => {
                        if id == of.id {
                            continue;
                        }
                        let other = lookup.item(id);
                        if !lookup.item_is_obstacle(other, of) {
                            continue;
                        }
                    }
                    _ => continue,
                }
            }
            let add = if is_45 && matches!(leaf.shape, TileShape::Octagon(_)) {
                true
            } else {
                leaf.shape.intersects(shape)
            };
            if add {
                result.push(TreeEntry {
                    object: leaf.object,
                    shape_index: leaf.shape_index as usize,
                });
            }
        }
    }

    #[inline]
    fn object_is_obstacle_net<L: ItemLookup + ?Sized>(&self, obj: &TreeObject, net_no: i32, lookup: &L) -> bool {
        match obj {
            TreeObject::Item(id) => lookup.is_obstacle_net(*id, net_no),
            TreeObject::Room(..) => true,
        }
    }

    #[inline]
    pub fn object_is_trace_obstacle<L: ItemLookup + ?Sized>(&self, obj: &TreeObject, net_no: i32, lookup: &L) -> bool {
        match obj {
            TreeObject::Item(id) => lookup.is_trace_obstacle(*id, net_no),
            TreeObject::Room(..) => true,
        }
    }

    /// The objects overlapping `shape` (sorted set semantics of the original).
    pub fn overlapping_objects<L: ItemLookup + ?Sized>(&self, shape: &TileShape, layer: i32, lookup: &L) -> Vec<TreeObject> {
        let mut entries = Vec::new();
        self.overlapping_tree_entries(shape, layer, &[], lookup, &mut entries);
        let mut objs: Vec<TreeObject> = entries.into_iter().map(|e| e.object).collect();
        objs.sort();
        objs.dedup();
        objs
    }

    /// Items overlapping `shape` (sorted by descending id, unique).
    pub fn overlapping_items<L: ItemLookup + ?Sized>(&self, shape: &TileShape, layer: i32, lookup: &L) -> Vec<ItemId> {
        let mut entries = Vec::new();
        self.overlapping_tree_entries(shape, layer, &[], lookup, &mut entries);
        let mut items: Vec<ItemId> = entries.into_iter().filter_map(|e| e.object.item()).collect();
        items.sort();
        items.dedup();
        items
    }

    /// Like `overlapping_tree_entries`, but considers the clearance between the
    /// `clearance_class` and the classes of the found items.
    #[allow(clippy::too_many_arguments)]
    pub fn overlapping_tree_entries_with_clearance<L: ItemLookup + ?Sized>(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
        obstacles_of: Option<&Item>,
        rules: &BoardRules,
        layer_count: i32,
        board_box: &IntBox,
        lookup: &L,
        result: &mut Vec<TreeEntry>,
    ) {
        if self.is_clearance_compensation_used() && clearance_class == self.compensated_clearance_class {
            self.overlapping_tree_entries_of(shape, layer, ignore_nets, obstacles_of, lookup, result);
        } else {
            self.overlapping_with_clearance_unlocked(
                shape,
                layer,
                ignore_nets,
                clearance_class,
                obstacles_of,
                rules,
                layer_count,
                board_box,
                lookup,
                result,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn overlapping_with_clearance_unlocked<L: ItemLookup + ?Sized>(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
        obstacles_of: Option<&Item>,
        rules: &BoardRules,
        layer_count: i32,
        board_box: &IntBox,
        lookup: &L,
        result: &mut Vec<TreeEntry>,
    ) {
        if layer < 0 {
            for l in 0..layer_count {
                self.overlapping_with_clearance_unlocked(
                    shape,
                    l,
                    ignore_nets,
                    clearance_class,
                    obstacles_of,
                    rules,
                    layer_count,
                    board_box,
                    lookup,
                    result,
                );
            }
            return;
        }
        let mut bounds = shape.bounding_shape(self.bounding_directions());
        if bounds.is_empty() && !shape.is_bounded() {
            bounds = RegularTileShape::Box(*board_box);
        }
        let max_clearance = (1.2 * rules.clearance_matrix.max_value(clearance_class, layer) as f64) as i32;
        let offset_bounds = bounds.offset(max_clearance as f64);
        let mut leaves = Vec::new();
        self.overlapping_leaves_sorted(&offset_bounds, &mut leaves);
        // (clearance, entry id, leaf)
        let mut sorted: Vec<(i32, usize, NodeIdx)> = Vec::new();
        for l in leaves {
            let leaf = self.tree.leaf(l);
            if leaf.layer != layer {
                continue;
            }
            let mut ignore = false;
            for n in ignore_nets {
                if !self.object_is_obstacle_net(&leaf.object, *n, lookup) {
                    ignore = true;
                }
            }
            if ignore {
                continue;
            }
            match leaf.object {
                TreeObject::Item(id) => {
                    let item = lookup.item(id);
                    if let Some(of) = obstacles_of {
                        if id == of.id || !lookup.item_is_obstacle(item, of) {
                            continue;
                        }
                    }
                    let c = self.additional_clearance(rules, clearance_class, item.clearance_class, layer);
                    let entry_id = sorted.len();
                    sorted.push((c, entry_id, l));
                }
                TreeObject::Room(..) => {
                    if obstacles_of.is_none() && leaf.shape.intersects(shape) {
                        result.push(TreeEntry {
                            object: leaf.object,
                            shape_index: leaf.shape_index as usize,
                        });
                    }
                }
            }
        }
        sorted.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut current_half_clearance = 0;
        let mut current_offset_shape = shape.clone();
        for (c, _, l) in sorted {
            let half = if self.is_clearance_compensation_used() { (c + 1) / 2 } else { c / 2 };
            if half != current_half_clearance {
                current_half_clearance = half;
                current_offset_shape = shape.enlarge(half as f64);
            }
            let leaf = self.tree.leaf(l);
            let tmp_offset = leaf.shape.enlarge(current_half_clearance as f64);
            if !bounding_octagons_apart(&current_offset_shape, &tmp_offset)
                && current_offset_shape.intersects(&tmp_offset)
            {
                result.push(TreeEntry {
                    object: leaf.object,
                    shape_index: leaf.shape_index as usize,
                });
            }
        }
    }

    /// Items overlapping with clearance, sorted by descending id, unique.
    #[allow(clippy::too_many_arguments)]
    pub fn overlapping_items_with_clearance<L: ItemLookup + ?Sized>(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
        rules: &BoardRules,
        layer_count: i32,
        board_box: &IntBox,
        lookup: &L,
    ) -> Vec<ItemId> {
        let mut entries = Vec::new();
        self.overlapping_tree_entries_with_clearance(
            shape,
            layer,
            ignore_nets,
            clearance_class,
            None,
            rules,
            layer_count,
            board_box,
            lookup,
            &mut entries,
        );
        let mut items: Vec<ItemId> = entries.into_iter().filter_map(|e| e.object.item()).collect();
        items.sort();
        items.dedup();
        items
    }

    // ---------------------------------------------------------------- entry manipulation for traces

    /// Replaces the shapes of `obj` keeping the first `keep_at_start` and the last
    /// `keep_at_end` entries (`changeEntries` of the original).
    pub fn change_entries(
        &mut self,
        obj: TreeObject,
        layer: i32,
        changed_shapes: Vec<TileShape>,
        keep_at_start: usize,
        keep_at_end: usize,
    ) {
        let old = self.entries.remove(&obj).unwrap_or_default();
        let old_count = old.len();
        let new_count = changed_shapes.len() + keep_at_start + keep_at_end;
        let mut new_leaves = vec![crate::datastructures::shape_tree::NONE; new_count];
        new_leaves[..keep_at_start].copy_from_slice(&old[..keep_at_start]);
        for l in old.iter().take(old_count - keep_at_end).skip(keep_at_start) {
            self.tree.remove_leaf(*l);
        }
        for i in 0..keep_at_end {
            let new_index = new_count - keep_at_end + i;
            let old_index = old_count - keep_at_end + i;
            new_leaves[new_index] = old[old_index];
            self.tree.leaf_mut(old[old_index]).shape_index = new_index as u32;
        }
        for (k, s) in changed_shapes.into_iter().enumerate() {
            let i = keep_at_start + k;
            new_leaves[i] = self.insert_leaf(obj, i, layer, s);
        }
        self.entries.insert(obj, new_leaves);
    }

    /// `mergeEntriesInFront`: `from` is joined in front of `to`.
    /// `link_shapes` are the new shapes between the reused entries.
    pub fn merge_entries_in_front(
        &mut self,
        from: TreeObject,
        to: TreeObject,
        layer: i32,
        change_order: bool,
        link_shapes: Vec<TileShape>,
    ) {
        let from_entries = self.entries.remove(&from).unwrap_or_default();
        let to_entries = self.entries.remove(&to).unwrap_or_default();
        let from_count_m1 = from_entries.len() - 1;
        let remove_no = if change_order { 0 } else { from_count_m1 };
        self.tree.remove_leaf(from_entries[remove_no]);
        self.tree.remove_leaf(to_entries[0]);
        let new_count = from_entries.len() + link_shapes.len() + to_entries.len() - 2;
        let mut new_leaves = vec![crate::datastructures::shape_tree::NONE; new_count];
        for i in 0..from_count_m1 {
            let from_no = if change_order { from_count_m1 - i } else { i };
            let l = from_entries[from_no];
            let leaf = self.tree.leaf_mut(l);
            leaf.object = to;
            leaf.shape_index = i as u32;
            new_leaves[i] = l;
        }
        let link_count = link_shapes.len();
        for i in 1..to_entries.len() {
            let ci = from_count_m1 + link_count + i - 1;
            let l = to_entries[i];
            self.tree.leaf_mut(l).shape_index = ci as u32;
            new_leaves[ci] = l;
        }
        for (i, s) in link_shapes.into_iter().enumerate() {
            let ci = from_count_m1 + i;
            new_leaves[ci] = self.insert_leaf(to, ci, layer, s);
        }
        self.entries.insert(to, new_leaves);
    }

    /// `mergeEntriesAtEnd`: `from` is joined at the end of `to`.
    pub fn merge_entries_at_end(
        &mut self,
        from: TreeObject,
        to: TreeObject,
        layer: i32,
        change_order: bool,
        link_shapes: Vec<TileShape>,
    ) {
        let from_entries = self.entries.remove(&from).unwrap_or_default();
        let to_entries = self.entries.remove(&to).unwrap_or_default();
        let to_count_m1 = to_entries.len() - 1;
        self.tree.remove_leaf(to_entries[to_count_m1]);
        let remove_no = if change_order { from_entries.len() - 1 } else { 0 };
        self.tree.remove_leaf(from_entries[remove_no]);
        let link_count = link_shapes.len();
        let new_count = from_entries.len() + link_count + to_entries.len() - 2;
        let mut new_leaves = vec![crate::datastructures::shape_tree::NONE; new_count];
        new_leaves[..to_count_m1].copy_from_slice(&to_entries[..to_count_m1]);
        for i in 1..from_entries.len() {
            let ci = to_count_m1 + link_count + i - 1;
            let from_no = if change_order { from_entries.len() - i - 1 } else { i };
            let l = from_entries[from_no];
            let leaf = self.tree.leaf_mut(l);
            leaf.object = to;
            leaf.shape_index = ci as u32;
            new_leaves[ci] = l;
        }
        for (i, s) in link_shapes.into_iter().enumerate() {
            let ci = to_count_m1 + i;
            new_leaves[ci] = self.insert_leaf(to, ci, layer, s);
        }
        self.entries.insert(to, new_leaves);
    }

    /// `reuseEntriesAfterCutout`: the leaves of `from` are reused for the start and end piece.
    /// `start_last` and `end_first` are the shapes of the new leaves.
    #[allow(clippy::too_many_arguments)]
    pub fn reuse_entries_after_cutout(
        &mut self,
        from: TreeObject,
        start: TreeObject,
        end: TreeObject,
        start_count: usize,
        end_count: usize,
        start_last: Option<TileShape>,
        end_first: Option<TileShape>,
        start_layer: i32,
        end_layer: i32,
    ) {
        let none = crate::datastructures::shape_tree::NONE;
        let Some(mut from_entries) = self.entries.remove(&from) else {
            return;
        };
        let mut start_leaves = vec![none; start_count];
        for i in 0..start_count.saturating_sub(1) {
            let l = from_entries[i];
            if l != none {
                let leaf = self.tree.leaf_mut(l);
                leaf.object = start;
                leaf.shape_index = i as u32;
            }
            start_leaves[i] = l;
            from_entries[i] = none;
        }
        if start_count > 0 {
            if let Some(s) = start_last {
                start_leaves[start_count - 1] = self.insert_leaf(start, start_count - 1, start_layer, s);
            }
        }
        let mut end_leaves = vec![none; end_count];
        if end_count > 0 {
            if let Some(s) = end_first {
                end_leaves[0] = self.insert_leaf(end, 0, end_layer, s);
            }
        }
        for i in 1..end_count {
            let from_index = from_entries.len() - end_count + i;
            let l = from_entries[from_index];
            if l != none {
                let leaf = self.tree.leaf_mut(l);
                leaf.object = end;
                leaf.shape_index = i as u32;
            }
            end_leaves[i] = l;
            from_entries[from_index] = none;
        }
        self.entries.insert(start, start_leaves);
        self.entries.insert(end, end_leaves);
        self.entries.insert(from, from_entries);
    }

    /// Replaces the shape with index `shape_index` of an object.
    pub fn change_item_shape(&mut self, obj: TreeObject, shape_index: usize, new_shape: TileShape) {
        let Some(old) = self.entries.get(&obj).cloned() else {
            return;
        };
        let layer = self.tree.leaf(old[shape_index]).layer;
        self.tree.remove_leaf(old[shape_index]);
        let l = self.insert_leaf(obj, shape_index, layer, new_shape);
        self.entries.get_mut(&obj).unwrap()[shape_index] = l;
    }

    // ---------------------------------------------------------------- raw access for the autorouter

    #[inline]
    pub fn raw(&self) -> &MinAreaTree<TreeLeaf> {
        &self.tree
    }
}

fn shape_border_line(s: &Shape, i: usize) -> Line {
    match s {
        Shape::Tile(t) => t.border_line(i),
        Shape::Polygon(p) => p.border_line(i),
        Shape::Circle(_) => Line::from_coords(0, 0, 1, 0),
    }
}

/// The area outside the board outline inside the board bounding box.
pub fn outline_keepout_area(outline: &super::item::BoardOutline, bounding_box: IntBox) -> crate::geometry::Area {
    crate::geometry::Area::PolylineArea(crate::geometry::PolylineArea::new(
        Shape::Tile(TileShape::Box(bounding_box)),
        outline.shapes.clone(),
    ))
}

/// Returns true, if at least one shape is a simplex and the bounding octagons do not intersect.
pub fn bounding_octagons_apart(a: &TileShape, b: &TileShape) -> bool {
    if !matches!(a, TileShape::Simplex(_)) && !matches!(b, TileShape::Simplex(_)) {
        return false;
    }
    let oa = match a {
        TileShape::Simplex(s) => s.bounding_octagon(),
        _ => Some(a.bounding_octagon()),
    };
    let ob = match b {
        TileShape::Simplex(s) => s.bounding_octagon(),
        _ => Some(b.bounding_octagon()),
    };
    match (oa, ob) {
        (Some(x), Some(y)) => !x.intersects(&y),
        _ => false,
    }
}

/// Kept for symmetry with the original: object kinds that block traces in the autorouter.
pub fn obstacle_kind_blocks_traces(kind: &ObstacleKind) -> bool {
    !matches!(kind, ObstacleKind::ComponentObstacle | ObstacleKind::ViaObstacle)
}

#[allow(dead_code)]
fn _unused(_: IntOctagon, _: Point) {}
