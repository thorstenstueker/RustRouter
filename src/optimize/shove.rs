//! Shoving of traces and vias (port of `TraceShover`, `ShapeTraceEntries`,
//! `ShapeEntrySide`, `ShapeAndEntrySide`, `ForcedPadRouter`, `DrillItemMover`,
//! `ForcedViaInserter` and `ShoveCheckCache`).

use crate::board::item::{Item, ItemId, ItemKind, NetNumbers, PolylineTrace};
use crate::board::{Board, FixedState};
use crate::datastructures::TimeLimit;
use crate::geometry::{
    Circle, Direction, FloatLine, FloatPoint, IntBox, IntOctagon, IntPoint, Line, LineSegment,
    Point, Polyline, Shape, Side, Simplex, TileShape, Vector, SQRT2,
};
use crate::library::PadstackId;
use crate::rules::{AngleRestriction, ViaInfo};
use rustc_hash::FxHashMap;
use std::collections::BTreeSet;

const C_OFFSET_ADD: f64 = 1.0;

// ====================================================================== ShapeEntrySide

/// The side of a shape, where a trace enters it.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeEntrySide {
    pub no: i32,
    pub border_intersection: Option<FloatPoint>,
}

impl ShapeEntrySide {
    pub const NOT_CALCULATED: ShapeEntrySide = ShapeEntrySide {
        no: -1,
        border_intersection: None,
    };

    pub fn new(no: i32, border_intersection: Option<FloatPoint>) -> ShapeEntrySide {
        ShapeEntrySide { no, border_intersection }
    }

    /// The entry side of a polyline segment `no` into `shape`.
    pub fn from_polyline(polyline: &Polyline, no: usize, shape: &TileShape) -> ShapeEntrySide {
        let mut from_side_no: i32 = -1;
        let mut intersection: Option<FloatPoint> = None;
        let mut found = false;
        let mut current_no = no;
        while current_no > 0 {
            let seg = LineSegment::from_polyline(polyline, current_no);
            let is = seg.border_intersections(shape);
            if !is.is_empty() {
                from_side_no = is[0] as i32;
                intersection = Some(seg.get_line().intersection_approx(&shape.border_line(is[0])));
                found = true;
                break;
            }
            current_no -= 1;
        }
        if !found {
            let from_point = polyline.corner_approx(0);
            let check_line = polyline.line(1);
            let mut min_dist = f64::MAX;
            for i in 0..shape.border_line_count() {
                let ci = check_line.intersection_approx(&shape.border_line(i));
                let d = ci.distance(&from_point).abs();
                if d < min_dist {
                    from_side_no = i as i32;
                    intersection = Some(ci);
                    min_dist = d;
                }
            }
        }
        ShapeEntrySide::new(from_side_no, intersection)
    }

    /// The entry side nearest to `from_point`.
    pub fn from_point(from_point: &Point, shape: &TileShape) -> ShapeEntrySide {
        let Some(proj) = shape.nearest_border_point(from_point) else {
            return ShapeEntrySide::NOT_CALCULATED;
        };
        let no = shape.contains_on_border_line_no(&proj);
        if no < 0 {
            log::warn!("CalcFromSide: this.no >= 0 expected");
        }
        ShapeEntrySide::new(no, Some(proj.to_float()))
    }

    /// The entry side for shoving a line segment to the left or right.
    pub fn from_segment(seg: &LineSegment, shape: &TileShape, shove_to_the_left: bool) -> ShapeEntrySide {
        let start_corner = seg.start_point_approx();
        let end_corner = seg.end_point_approx();
        let n = shape.border_line_count();
        let check_line = seg.get_line();
        let first_corner = shape.corner_approx(0);
        let mut prev_side = check_line.side_of_float(&first_corner);
        let mut front_side_no: i32 = -1;
        for i in 1..=n {
            let next_corner = if i == n { first_corner } else { shape.corner_approx(i) };
            let next_side = check_line.side_of_float(&next_corner);
            if prev_side != next_side {
                let ci = shape.border_line(i - 1).intersection_approx(&check_line);
                if ci.distance_square(&start_corner) < ci.distance_square(&end_corner) {
                    front_side_no = i as i32 - 1;
                    break;
                }
            }
            prev_side = next_side;
        }
        let base = if front_side_no < 0 {
            let mut min_distance = f64::MAX;
            let mut nearest_side = 0usize;
            for i in 0..n {
                let bl = shape.border_line(i);
                let fl = FloatLine::new(bl.a.to_float(), bl.b.to_float());
                let projection = fl.perpendicular_projection(&start_corner);
                let side_start = shape.corner_approx(i);
                let side_end = shape.corner_approx((i + 1) % n);
                if projection.is_contained_in_box(&side_start, &side_end, 0.01) {
                    let d = start_corner.distance(&projection);
                    if d < min_distance {
                        min_distance = d;
                        nearest_side = i;
                    }
                }
            }
            nearest_side
        } else {
            front_side_no as usize
        };
        let no = if shove_to_the_left { (base + 2) % n } else { (base + n - 2) % n };
        let prev = shape.corner_approx(no);
        let next = shape.corner_approx((no + 1) % n);
        ShapeEntrySide::new(no as i32, Some(prev.middle_point(&next)))
    }
}

/// A trace shape with its entry side (`ShapeAndEntrySide`).
pub struct ShapeAndEntrySide {
    pub shape: TileShape,
    pub from_side: Option<ShapeEntrySide>,
}

impl ShapeAndEntrySide {
    /// For a detached trace (not inserted into the board).
    pub fn new(board: &Board, trace: &Item, index: usize, orthogonal: bool, in_shove_check: bool) -> ShapeAndEntrySide {
        let t = trace.as_trace().unwrap();
        let tree = board.search_trees.default_tree();
        let comp_hw = t.half_width + tree.clearance_compensation_value(&board.rules, trace.clearance_class, t.layer);
        let mut current_shape = tree
            .offset_shape(&t.polyline, comp_hw, index)
            .unwrap_or_else(TileShape::empty);
        let mut from_side: Option<ShapeEntrySide> = None;
        if orthogonal {
            current_shape = TileShape::Box(current_shape.bounding_box());
        } else {
            current_shape = TileShape::Simplex(current_shape.to_simplex());
            let mut cut_off_at_end = false;
            let mut cut_off_at_start = false;
            let end_cut = calc_cutline_at_end(index, t, comp_hw);
            if let Some(ec) = end_cut {
                let tmp = current_shape.intersection(&TileShape::from_line(&ec));
                if !tmp.is_empty() {
                    current_shape = TileShape::Simplex(tmp.to_simplex());
                    cut_off_at_end = true;
                }
            }
            let start_cut = calc_cutline_at_start(index, t, comp_hw);
            if let Some(sc) = start_cut {
                let tmp = current_shape.intersection(&TileShape::from_line(&sc));
                if !tmp.is_empty() {
                    current_shape = TileShape::Simplex(tmp.to_simplex());
                    cut_off_at_start = true;
                }
            }
            let mut from_side_index = -1;
            let mut cut_line: Option<Line> = None;
            if cut_off_at_start {
                cut_line = start_cut;
                from_side_index = current_shape.border_line_index(start_cut.as_ref().unwrap());
            }
            if from_side_index < 0 && cut_off_at_end {
                cut_line = end_cut;
                from_side_index = current_shape.border_line_index(end_cut.as_ref().unwrap());
            }
            if from_side_index >= 0 {
                let bi = cut_line
                    .unwrap()
                    .intersection_approx(&current_shape.border_line(from_side_index as usize));
                from_side = Some(ShapeEntrySide::new(from_side_index, Some(bi)));
            }
        }
        if from_side.is_none() && !in_shove_check {
            from_side = Some(ShapeEntrySide::from_polyline(&t.polyline, index, &current_shape));
        }
        ShapeAndEntrySide {
            shape: current_shape,
            from_side,
        }
    }
}

fn calc_cutline_at_end(index: usize, t: &PolylineTrace, comp_hw: i32) -> Option<Line> {
    let pl = &t.polyline;
    let n = pl.line_count();
    if index + 3 == n || pl.corner_approx(n - 2).distance(&pl.corner_approx(index + 1)) < comp_hw as f64 {
        let cl = pl.line(n - 1);
        let is = pl.corner_approx(n - 3);
        return Some(if cl.side_of_float(&is) == Side::OnTheLeft { cl.opposite() } else { cl });
    }
    None
}

fn calc_cutline_at_start(index: usize, t: &PolylineTrace, comp_hw: i32) -> Option<Line> {
    let pl = &t.polyline;
    if index == 0 || pl.corner_approx(0).distance(&pl.corner_approx(index)) < comp_hw as f64 {
        let cl = pl.line(0);
        let is = pl.corner_approx(1);
        return Some(if cl.side_of_float(&is) == Side::OnTheLeft { cl.opposite() } else { cl });
    }
    None
}

// ====================================================================== ShapeTraceEntries

/// A trace taking part in a shape trace entries calculation: either a board trace or a
/// detached substitute trace.
#[derive(Clone, Debug)]
struct EntryPoint {
    trace: ItemId,
    trace_line_no: usize,
    entry_approx: FloatPoint,
    edge_index: i32,
    stack_level: i32,
    next: Option<usize>,
}

/// Information about the traces entering a shape (`ShapeTraceEntries`).
pub struct ShapeTraceEntries {
    pub shove_via_list: Vec<ItemId>,
    shape: TileShape,
    layer: i32,
    own_net_nos: Vec<i32>,
    clearance_class: i32,
    from_side: Option<ShapeEntrySide>,
    entries: Vec<EntryPoint>,
    list_anchor: Option<usize>,
    trace_piece_count: i32,
    max_stack_level: i32,
    shape_contains_trace_tails: bool,
    found_obstacle: Option<ItemId>,
    /// Snapshot of the traces stored (nets, half width, polyline, clearance class).
    traces: FxHashMap<ItemId, Item>,
}

fn net_nos_equal(a: &[i32], b: &[i32]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().all(|x| b.contains(x))
}

impl ShapeTraceEntries {
    pub fn new(shape: TileShape, layer: i32, own_net_nos: &[i32], clearance_class: i32, from_side: Option<ShapeEntrySide>) -> Self {
        ShapeTraceEntries {
            shove_via_list: Vec::new(),
            shape,
            layer,
            own_net_nos: own_net_nos.to_vec(),
            clearance_class,
            from_side,
            entries: Vec::new(),
            list_anchor: None,
            trace_piece_count: 0,
            max_stack_level: 0,
            shape_contains_trace_tails: false,
            found_obstacle: None,
            traces: FxHashMap::default(),
        }
    }

    pub fn stack_depth(&self) -> i32 {
        self.max_stack_level
    }

    pub fn substitute_trace_count(&self) -> i32 {
        self.trace_piece_count
    }

    pub fn trace_tails_in_shape(&self) -> bool {
        self.shape_contains_trace_tails
    }

    pub fn get_found_obstacle(&self) -> Option<ItemId> {
        self.found_obstacle
    }

    fn trace_offset_shape(&self, board: &Board, trace: &Item) -> TileShape {
        let t = trace.as_trace().unwrap();
        let tree = board.search_trees.default_tree();
        if tree.is_clearance_compensation_used() {
            let comp_hw = t.half_width + tree.clearance_compensation_value(&board.rules, trace.clearance_class, t.layer);
            let off = comp_hw as f64
                + tree.additional_clearance(&board.rules, self.clearance_class, trace.clearance_class, t.layer) as f64
                + C_OFFSET_ADD;
            self.shape.offset(off)
        } else {
            let cl = board.clearance_value(trace.clearance_class, self.clearance_class, t.layer) as f64 + C_OFFSET_ADD;
            self.shape.offset(t.half_width as f64).offset(cl)
        }
    }

    /// Stores the obstacle items. Returns false, if an item cannot be shoved.
    pub fn store_items(&mut self, board: &Board, items: &[ItemId], is_pad_check: bool, copper_sharing_allowed: bool) -> bool {
        for id in items {
            let item = board.item(*id);
            if !is_pad_check && item.is_via_obstacle_area() || item.is_component_obstacle_area() {
                continue;
            }
            let contains_own_net = item.shares_net_no(&self.own_net_nos);
            if let Some(o) = item.as_obstacle() {
                if o.kind != crate::board::ObstacleKind::Obstacle && item.is_conduction_area() && (contains_own_net || !o.conduction_is_obstacle()) {
                    continue;
                }
            }
            if item.is_board_outline() && !board.items.outline_blocks_nets(item, &self.own_net_nos) {
                continue;
            }
            if board.is_shove_fixed(item) && !contains_own_net {
                self.found_obstacle = Some(*id);
                return false;
            }
            match &item.kind {
                ItemKind::Via(_) => {
                    if is_pad_check || !contains_own_net {
                        self.shove_via_list.push(*id);
                    }
                }
                ItemKind::Trace(_) => {
                    if !self.store_trace(board, *id) {
                        return false;
                    }
                }
                _ => {
                    if contains_own_net {
                        if !copper_sharing_allowed {
                            self.found_obstacle = Some(*id);
                            return false;
                        }
                        if is_pad_check && !item.as_pin().map(|p| p.drill_allowed()).unwrap_or(false) {
                            self.found_obstacle = Some(*id);
                            return false;
                        }
                    } else {
                        self.found_obstacle = Some(*id);
                        return false;
                    }
                }
            }
        }
        self.search_from_side(board);
        self.resort();
        self.calculate_stack_levels()
    }

    fn store_trace(&mut self, board: &Board, id: ItemId) -> bool {
        let trace = board.item(id).clone();
        let t = trace.as_trace().unwrap().clone();
        let offset_shape = self.trace_offset_shape(board, &trace);
        self.traces.insert(id, trace.clone());
        let entries = offset_shape.entrance_points(&t.polyline);
        for (line_no, edge) in entries {
            let ea = t.polyline.line(line_no).intersection_approx(&offset_shape.border_line(edge));
            self.insert_entry_point(id, line_no, edge as i32, ea);
        }
        if !trace.shares_net_no(&self.own_net_nos) {
            if !trace.nets_normal() {
                return false;
            }
            let tree = board.search_trees.default_tree();
            let comp_hw = t.half_width + tree.clearance_compensation_value(&board.rules, trace.clearance_class, t.layer);
            let mut end_corner = t.first_corner();
            for i in 0..2 {
                if offset_shape.contains(&end_corner) {
                    let contacts = if i == 0 {
                        board.trace_start_contacts(id)
                    } else {
                        board.trace_end_contacts(id)
                    };
                    let mut contact_count = 0;
                    let mut store_end_corner = true;
                    for c in &contacts {
                        let ci = board.item(*c);
                        if !ci.is_routable() {
                            self.found_obstacle = Some(*c);
                            return false;
                        }
                        match &ci.kind {
                            ItemKind::Trace(ct) => {
                                if (board.is_shove_fixed(ci) || ct.half_width != t.half_width || ci.clearance_class != ci.clearance_class)
                                    && offset_shape.contains_inside(&end_corner)
                                {
                                    self.found_obstacle = Some(*c);
                                    return false;
                                }
                            }
                            ItemKind::Via(_) => {
                                let via_shape = board.drill_tile_shape_on_layer(*c, self.layer).unwrap_or_else(TileShape::empty);
                                let mut diff = via_shape.smallest_radius() - comp_hw as f64;
                                let via_cl = tree.additional_clearance(&board.rules, ci.clearance_class, self.clearance_class, self.layer);
                                let trace_cl = tree.additional_clearance(&board.rules, trace.clearance_class, self.clearance_class, self.layer);
                                if trace_cl > via_cl {
                                    diff += (via_cl - trace_cl) as f64;
                                }
                                if diff < 0.0 {
                                    self.found_obstacle = Some(*c);
                                    return false;
                                }
                                if diff == 0.0 && !offset_shape.contains_inside(&end_corner) {
                                    store_end_corner = false;
                                }
                            }
                            _ => {}
                        }
                        contact_count += 1;
                    }
                    if contact_count == 1 && store_end_corner {
                        if let Some(projection) = offset_shape.nearest_border_point(&end_corner) {
                            let side = offset_shape.contains_on_border_line_no(&projection);
                            let seg_no = if i == 0 { 0 } else { t.polyline.line_count() - 1 };
                            if side >= 0 {
                                self.insert_entry_point(id, seg_no, side, projection.to_float());
                            }
                        }
                    } else if contact_count != 1 && offset_shape.contains_inside(&end_corner) {
                        self.shape_contains_trace_tails = contact_count == 0;
                        self.found_obstacle = Some(id);
                        return false;
                    }
                }
                end_corner = t.last_corner();
            }
        }
        self.found_obstacle = Some(id);
        true
    }

    fn trace_nets(&self, id: ItemId) -> &[i32] {
        &self.traces[&id].net_numbers
    }

    fn search_from_side(&mut self, _board: &Board) {
        if let Some(fs) = &self.from_side {
            if fs.no >= 0 {
                return;
            }
        }
        let mut cur = self.list_anchor;
        let mut no = 0;
        let mut approx = None;
        while let Some(c) = cur {
            let e = &self.entries[c];
            let nets = &self.traces[&e.trace].net_numbers;
            if nets.iter().any(|n| self.own_net_nos.contains(n)) {
                no = e.edge_index;
                approx = Some(e.entry_approx);
                break;
            }
            cur = e.next;
        }
        self.from_side = Some(ShapeEntrySide::new(no, approx));
    }

    fn resort(&mut self) {
        let n = self.shape.border_line_count() as i32;
        let fs = self.from_side.clone().unwrap();
        if fs.no < 0 || fs.no >= n {
            log::warn!("ShapeTraceEntries.resort: from side not calculated");
            return;
        }
        let cc1 = self.shape.corner_approx(fs.no as usize);
        let cc2 = if fs.no == n - 1 {
            self.shape.corner_approx(0)
        } else {
            self.shape.corner_approx(fs.no as usize + 1)
        };
        let mut from_point_dist = 0.0;
        let mut from_point_projection: Option<FloatPoint> = None;
        if let Some(bi) = fs.border_intersection {
            let p = bi.projection_approx(&self.shape.border_line(fs.no as usize));
            from_point_dist = p.distance_square(&cc1);
            from_point_projection = Some(p);
            if from_point_dist >= cc1.distance_square(&cc2) {
                self.from_side = Some(ShapeEntrySide::new(fs.no, None));
            }
        }
        let fs = self.from_side.clone().unwrap();
        let mut current = self.list_anchor;
        while let Some(c) = current {
            let e = &self.entries[c];
            if e.edge_index > fs.no {
                break;
            }
            if e.edge_index == fs.no {
                if fs.border_intersection.is_some() {
                    let cp = e.entry_approx.projection_approx(&self.shape.border_line(fs.no as usize));
                    if cp.distance_square(&cc1) >= from_point_dist
                        && cp.distance_square(from_point_projection.as_ref().unwrap()) <= cp.distance_square(&cc1)
                    {
                        break;
                    }
                } else if e.entry_approx.distance_square(&cc2) <= e.entry_approx.distance_square(&cc1) {
                    break;
                }
            }
            current = e.next;
        }
        if let Some(c) = current {
            if Some(c) != self.list_anchor {
                self.rotate_entry_list_around_anchor(c, n);
            }
        }
        let Some(anchor) = self.list_anchor else { return };
        let mut prev = anchor;
        let mut prev_nets: Vec<i32> = self.trace_nets(self.entries[prev].trace).to_vec();
        let mut current = self.entries[anchor].next;
        let mut current_nets: Vec<i32>;
        let mut next: Option<usize>;
        if let Some(c) = current {
            current_nets = self.trace_nets(self.entries[c].trace).to_vec();
            next = self.entries[c].next;
        } else {
            next = None;
            current_nets = Vec::new();
        }
        let mut before_prev: Option<usize> = None;
        while let Some(nx) = next {
            let next_nets: Vec<i32> = self.trace_nets(self.entries[nx].trace).to_vec();
            if net_nos_equal(&prev_nets, &current_nets) && net_nos_equal(&current_nets, &next_nets) {
                self.entries[prev].next = Some(nx);
            } else {
                before_prev = Some(prev);
                prev = current.unwrap();
                prev_nets = current_nets.clone();
            }
            current_nets = next_nets;
            current = Some(nx);
            next = self.entries[nx].next;
        }
        if current.is_some() && net_nos_equal(&current_nets, &self.own_net_nos) {
            self.entries[prev].next = None;
            if net_nos_equal(&prev_nets, &self.own_net_nos) {
                match before_prev {
                    Some(bp) => self.entries[bp].next = None,
                    None => self.list_anchor = None,
                }
            }
        }
        if let Some(a) = self.list_anchor {
            if self.traces[&self.entries[a].trace].nets_equal(&self.own_net_nos) {
                self.list_anchor = self.entries[a].next;
                if let Some(a2) = self.list_anchor {
                    if self.traces[&self.entries[a2].trace].nets_equal(&self.own_net_nos) {
                        self.list_anchor = self.entries[a2].next;
                    }
                }
            }
        }
    }

    fn calculate_stack_levels(&mut self) -> bool {
        let Some(anchor) = self.list_anchor else {
            return true;
        };
        let mut current_entry = Some(anchor);
        let mut current_nets: Vec<i32> = self.trace_nets(self.entries[anchor].trace).to_vec();
        let mut current_level = if net_nos_equal(&current_nets, &self.own_net_nos) { 0 } else { 1 };
        while let Some(ce) = current_entry {
            if self.entries[ce].stack_level < 0 {
                self.trace_piece_count += 1;
                self.entries[ce].stack_level = current_level;
                if current_level > self.max_stack_level {
                    if self.max_stack_level > 1 {
                        self.found_obstacle = Some(self.entries[ce].trace);
                    }
                    self.max_stack_level = current_level;
                }
            }
            let mut check = self.entries[ce].next;
            let mut index_of_next_foreign_set = 0;
            let mut index_of_last_occurrence = 0;
            let mut next_index = 0;
            let mut last_own_entry: Option<usize> = None;
            let mut first_foreign_entry: Option<usize> = None;
            while let Some(ch) = check {
                next_index += 1;
                let check_nets = self.trace_nets(self.entries[ch].trace);
                if net_nos_equal(check_nets, &current_nets) {
                    index_of_last_occurrence = next_index;
                    last_own_entry = Some(ch);
                    self.entries[ch].stack_level = self.entries[ce].stack_level;
                } else if index_of_next_foreign_set == 0 {
                    index_of_next_foreign_set = next_index;
                    first_foreign_entry = Some(ch);
                }
                check = self.entries[ch].next;
            }
            if next_index != 0 {
                let next_entry;
                if index_of_next_foreign_set != 0 && index_of_next_foreign_set < index_of_last_occurrence {
                    next_entry = first_foreign_entry.unwrap();
                    if self.entries[next_entry].stack_level >= 0 {
                        return false;
                    }
                    current_level += 1;
                } else if index_of_last_occurrence != 0 {
                    next_entry = last_own_entry.unwrap();
                } else {
                    next_entry = first_foreign_entry.unwrap();
                    if self.entries[next_entry].stack_level >= 0 {
                        current_level -= 1;
                        if self.entries[next_entry].stack_level != current_level {
                            return false;
                        }
                    }
                }
                current_nets = self.trace_nets(self.entries[next_entry].trace).to_vec();
                self.entries[ce].next = Some(next_entry);
                current_entry = Some(next_entry);
            } else {
                current_entry = None;
            }
        }
        if current_level != 1 {
            log::warn!("ShapeTraceEntries.calculate_stack_levels: currentLevel inconsistent");
            return false;
        }
        true
    }

    fn pop_piece(&mut self) -> Option<(usize, usize)> {
        let Some(anchor) = self.list_anchor else {
            if self.trace_piece_count != 0 {
                log::warn!("ShapeTraceEntries: tracePieceCount is inconsistent");
            }
            return None;
        };
        let mut first = Some(anchor);
        let mut prev_first: Option<usize> = None;
        while let Some(f) = first {
            if self.entries[f].stack_level == self.max_stack_level {
                break;
            }
            prev_first = Some(f);
            first = self.entries[f].next;
        }
        let Some(first) = first else {
            log::warn!("ShapeTraceEntries: maxStackLevel not found");
            return None;
        };
        let mut last = first;
        let mut after_last = self.entries[first].next;
        let first_trace = self.entries[first].trace;
        while let Some(al) = after_last {
            let e = &self.entries[al];
            if e.stack_level == self.max_stack_level && self.traces[&e.trace].nets_equal_item(&self.traces[&first_trace]) {
                last = al;
                after_last = self.entries[al].next;
            } else {
                break;
            }
        }
        match prev_first {
            Some(pf) => self.entries[pf].next = after_last,
            None => self.list_anchor = after_last,
        }
        self.max_stack_level = 0;
        let mut cur = self.list_anchor;
        while let Some(c) = cur {
            if self.entries[c].stack_level > self.max_stack_level {
                self.max_stack_level = self.entries[c].stack_level;
            }
            cur = self.entries[c].next;
        }
        self.trace_piece_count -= 1;
        if self.traces[&first_trace].nets_equal(&self.own_net_nos) {
            return self.pop_piece();
        }
        Some((first, last))
    }

    /// The next substitute trace (detached, with a newly generated id).
    pub fn next_substitute_trace_piece(&mut self, board: &mut Board) -> Option<Item> {
        let (e0, e1) = self.pop_piece()?;
        let current_trace = self.traces[&self.entries[e0].trace].clone();
        let ct = current_trace.as_trace().unwrap();
        let offset_shape = self.trace_offset_shape(board, &current_trace);
        let edge_count = self.shape.border_line_count() as i32;
        let edge_diff = self.entries[e1].edge_index - self.entries[e0].edge_index;
        let n = (edge_diff + 3) as usize;
        let mut piece_lines: Vec<Line> = Vec::with_capacity(n);
        piece_lines.push(self.traces[&self.entries[e0].trace].as_trace().unwrap().polyline.line(self.entries[e0].trace_line_no));
        let mut current_edge_no = self.entries[e0].edge_index % edge_count;
        for _ in 1..n - 1 {
            piece_lines.push(offset_shape.border_line(current_edge_no as usize));
            if current_edge_no == edge_count - 1 {
                current_edge_no = 0;
            } else {
                current_edge_no += 1;
            }
        }
        piece_lines.push(self.traces[&self.entries[e1].trace].as_trace().unwrap().polyline.line(self.entries[e1].trace_line_no));
        let piece = Polyline::from_lines(&piece_lines);
        if piece.is_empty() {
            return self.next_substitute_trace_piece(board);
        }
        let id = board.new_item_id();
        let half_width = ct.half_width;
        Some(Item {
            id,
            net_numbers: current_trace.net_numbers.clone(),
            clearance_class: current_trace.clearance_class,
            component_id: 0,
            fixed_state: FixedState::Unfixed,
            on_the_board: false,
            smallest_clearance: -1.0,
            kind: ItemKind::Trace(PolylineTrace {
                polyline: piece,
                layer: self.layer,
                half_width,
            }),
        })
    }

    /// Cuts the shape out of the foreign traces.
    pub fn cutout_traces(&self, board: &mut Board, items: &[ItemId]) {
        for id in items {
            let Some(item) = board.try_item(*id) else { continue };
            if item.is_trace() && !item.shares_net_no(&self.own_net_nos) {
                cutout_trace(board, *id, &self.shape, self.clearance_class);
            }
        }
    }

    fn insert_entry_point(&mut self, trace: ItemId, trace_line_no: usize, edge_index: i32, entry_approx: FloatPoint) {
        let new_idx = self.entries.len();
        self.entries.push(EntryPoint {
            trace,
            trace_line_no,
            entry_approx,
            edge_index,
            stack_level: -1,
            next: None,
        });
        let mut prev: Option<usize> = None;
        let mut next = self.list_anchor;
        while let Some(nx) = next {
            let ne = &self.entries[nx];
            if ne.edge_index > edge_index {
                break;
            }
            if ne.edge_index == edge_index {
                let pc = self.shape.corner_approx(edge_index as usize);
                let nc = if edge_index as usize == self.shape.border_line_count() - 1 {
                    self.shape.corner_approx(0)
                } else {
                    self.shape.corner_approx(edge_index as usize + 1)
                };
                if pc.scalar_product(&entry_approx, &nc) <= pc.scalar_product(&ne.entry_approx, &nc) {
                    break;
                }
            }
            prev = Some(nx);
            next = ne.next;
        }
        self.entries[new_idx].next = next;
        match prev {
            Some(p) => self.entries[p].next = Some(new_idx),
            None => self.list_anchor = Some(new_idx),
        }
    }

    fn rotate_entry_list_around_anchor(&mut self, new_anchor: usize, edge_count: i32) {
        let mut current = Some(new_anchor);
        let mut prev = new_anchor;
        while let Some(c) = current {
            prev = c;
            current = self.entries[c].next;
        }
        self.entries[prev].next = self.list_anchor;
        let mut current = self.list_anchor;
        while let Some(c) = current {
            if c == new_anchor {
                break;
            }
            self.entries[c].edge_index += edge_count;
            prev = c;
            current = self.entries[c].next;
        }
        self.entries[prev].next = None;
        self.list_anchor = Some(new_anchor);
    }
}

/// Cuts a shape enlarged by the trace width and clearance out of a trace (`cutoutTrace`).
pub fn cutout_trace(board: &mut Board, id: ItemId, shape: &TileShape, clearance_class: i32) {
    let Some(item) = board.try_item(id) else {
        log::warn!("ShapeTraceEntries.cutout_trace : trace is deleted");
        return;
    };
    if !item.on_the_board {
        log::warn!("ShapeTraceEntries.cutout_trace : trace is deleted");
        return;
    }
    let item = item.clone();
    let t = item.as_trace().unwrap().clone();
    let tree = board.search_trees.default_tree();
    let offset_shape = if tree.is_clearance_compensation_used() {
        let comp_hw = t.half_width + tree.clearance_compensation_value(&board.rules, item.clearance_class, t.layer);
        let off = comp_hw as f64
            + tree.additional_clearance(&board.rules, clearance_class, item.clearance_class, t.layer) as f64
            + C_OFFSET_ADD;
        shape.offset(off)
    } else {
        let cl = board.clearance_value(item.clearance_class, clearance_class, t.layer) as f64 + C_OFFSET_ADD;
        shape.offset(t.half_width as f64).offset(cl)
    };
    let pieces = offset_shape.cutout_polyline(&t.polyline);
    if pieces.len() == 1 && pieces[0].ptr_eq(&t.polyline) {
        return;
    }
    if pieces.len() == 2 && offset_shape.is_outside(&pieces[0].first_corner()) && offset_shape.is_outside(&pieces[1].last_corner()) {
        fast_cutout_trace(board, id, pieces[0].clone(), pieces[1].clone());
    } else {
        board.remove_item(id);
        for p in pieces {
            board.insert_trace_without_cleaning(p, t.layer, t.half_width, &item.net_numbers, item.clearance_class, FixedState::Unfixed);
        }
    }
}

fn fast_cutout_trace(board: &mut Board, id: ItemId, start_piece: Polyline, end_piece: Polyline) {
    board.additional_update_after_change(id);
    let item = board.item(id).clone();
    let t = item.as_trace().unwrap().clone();
    let make = |board: &mut Board, pl: Polyline| -> Item {
        Item {
            id: board.new_item_id(),
            net_numbers: item.net_numbers.clone(),
            clearance_class: item.clearance_class,
            component_id: 0,
            fixed_state: FixedState::Unfixed,
            on_the_board: true,
            smallest_clearance: -1.0,
            kind: ItemKind::Trace(PolylineTrace {
                polyline: pl,
                layer: t.layer,
                half_width: t.half_width,
            }),
        }
    };
    let start_trace = make(board, start_piece);
    let end_trace = make(board, end_piece);
    let (sid, eid) = (start_trace.id, end_trace.id);
    board.insert_item_raw(start_trace);
    board.insert_item_raw(end_trace);
    board.reuse_entries_after_cutout(id, sid, eid);
    board.remove_item(id);
}

// ====================================================================== ShoveCheckCache

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShoveKey {
    kind: u8,
    coords: Vec<i32>,
    side_no: i32,
    side_x: u64,
    side_y: u64,
    side_has_point: bool,
    dir_x: i32,
    dir_y: i32,
    nets: Vec<i32>,
    layer: i32,
    clearance_class: i32,
    depth: i32,
    via_depth: i32,
    spring_over_depth: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct ShoveEntry {
    result: bool,
    ids: i32,
    wrote_obstacle: bool,
    obstacle: Option<ItemId>,
    wrote_layer: bool,
    layer: i32,
}

/// Cache of shove check results, valid as long as the board content is unchanged.
#[derive(Clone, Debug, Default)]
pub struct ShoveCheckCache {
    entries: FxHashMap<ShoveKey, ShoveEntry>,
    stamp: u64,
    pub hits: u64,
    pub misses: u64,
}

const SHOVE_CACHE_MAX_ENTRIES: usize = 1 << 16;
const SHOVE_CACHE_MAX_LINES: usize = 8;

impl ShoveCheckCache {
    fn reset(&mut self, stamp: u64) {
        if stamp == self.stamp {
            return;
        }
        self.entries.clear();
        self.stamp = stamp;
    }

    fn supported(shape: &TileShape, dir: Option<&Direction>, nets: &[i32]) -> bool {
        let shape_ok = match shape {
            TileShape::Box(_) | TileShape::Octagon(_) => true,
            TileShape::Simplex(s) => s.border_line_count() > 0 && s.border_line_count() <= SHOVE_CACHE_MAX_LINES,
        };
        shape_ok && dir.map(|d| d.as_int().is_some()).unwrap_or(true) && nets.len() <= 2
    }

    #[allow(clippy::too_many_arguments)]
    fn key(
        shape: &TileShape,
        from_side: Option<&ShapeEntrySide>,
        dir: Option<&Direction>,
        layer: i32,
        nets: &[i32],
        clearance_class: i32,
        depth: i32,
        via_depth: i32,
        spring_over_depth: i32,
    ) -> ShoveKey {
        let (kind, coords) = match shape {
            TileShape::Octagon(o) => (1, vec![o.lx, o.ly, o.rx, o.uy, o.ulx, o.lrx, o.llx, o.urx]),
            TileShape::Box(b) => (2, vec![b.ll.x, b.ll.y, b.ur.x, b.ur.y]),
            TileShape::Simplex(s) => {
                let mut c = Vec::with_capacity(4 * s.border_line_count());
                for l in s.lines() {
                    c.extend_from_slice(&[l.a.x, l.a.y, l.b.x, l.b.y]);
                }
                (3, c)
            }
        };
        let (side_no, side_x, side_y, side_has_point) = match from_side {
            None => (i32::MIN, 0, 0, false),
            Some(fs) => match fs.border_intersection {
                Some(p) => (fs.no, p.x.to_bits(), p.y.to_bits(), true),
                None => (fs.no, 0, 0, false),
            },
        };
        let (dir_x, dir_y) = match dir.and_then(|d| d.as_int()) {
            Some(d) => {
                let g = gcd(d.x.unsigned_abs() as i64, d.y.unsigned_abs() as i64) as i32;
                if g == 0 {
                    (0, 0)
                } else {
                    (d.x / g, d.y / g)
                }
            }
            None => (0, 0),
        };
        ShoveKey {
            kind,
            coords,
            side_no,
            side_x,
            side_y,
            side_has_point,
            dir_x,
            dir_y,
            nets: nets.to_vec(),
            layer,
            clearance_class,
            depth,
            via_depth,
            spring_over_depth,
        }
    }
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

// ====================================================================== Board helpers

impl Board {
    pub fn set_shove_failing_obstacle(&mut self, id: Option<ItemId>) {
        self.shove_failing_obstacle = id;
        self.shove_failing_obstacle_writes += 1;
    }

    pub fn set_shove_failing_layer(&mut self, layer: i32) {
        self.shove_failing_layer = layer;
        self.shove_failing_layer_writes += 1;
    }

    pub fn clear_shove_failing_obstacle(&mut self) {
        self.shove_failing_obstacle = None;
        self.shove_failing_layer = -1;
        self.shove_failing_obstacle_writes += 1;
        self.shove_failing_layer_writes += 1;
    }

    /// `containsTraceTails`.
    pub fn contains_trace_tails(&self, items: &[ItemId], except_nets: &[i32]) -> bool {
        for id in items {
            let Some(item) = self.try_item(*id) else { continue };
            if item.is_trace() && !item.nets_equal(except_nets) && self.is_tail(*id) {
                return true;
            }
        }
        false
    }

    fn compensated_half_width_of(&self, item: &Item) -> i32 {
        let t = item.as_trace().unwrap();
        t.half_width
            + self
                .search_trees
                .default_tree()
                .clearance_compensation_value(&self.rules, item.clearance_class, t.layer)
    }

    /// Moves a drill item by a vector and reconnects the contact traces (`DrillItem.moveBy`).
    pub fn move_drill_item_by(&mut self, id: ItemId, vector: &Vector) {
        let old_center = self.item(id).as_drill().unwrap().center.clone();
        // trace infos sorted by descending layer, unique per layer (TreeSet semantics)
        let mut infos: Vec<(i32, i32, i32)> = Vec::new();
        for c in self.normal_contacts(id) {
            let ci = self.item(c);
            if let Some(t) = ci.as_trace() {
                if !infos.iter().any(|(l, _, _)| *l == t.layer) {
                    infos.push((t.layer, t.half_width, ci.clearance_class));
                }
            }
        }
        infos.sort_by(|a, b| b.0.cmp(&a.0));
        // Item.moveBy
        self.remove_from_trees(id);
        {
            let item = self.items.get(id).unwrap().clone();
            let new_center = old_center.translate_by(vector);
            let new_drill = match &item.kind {
                ItemKind::Via(v) => Some(self.calculate_via_drill(v.drill.padstack, &new_center)),
                _ => None,
            };
            let item_mut = self.item_mut(id);
            match &mut item_mut.kind {
                ItemKind::Via(v) => v.drill = new_drill.unwrap(),
                ItemKind::Pin(p) => {
                    p.drill.center = new_center;
                    for s in p.drill.shapes.iter_mut().flatten() {
                        *s = s.translate_by(vector);
                    }
                }
                _ => {}
            }
        }
        self.insert_into_trees(id);
        if self.item(id).is_pin() {
            self.items.invalidate_edge_pin_nets();
        }
        let new_center = self.item(id).as_drill().unwrap().center.clone();
        let mut connect_points = vec![old_center.clone()];
        if let (Some(o), Some(n)) = (old_center.as_int(), new_center.as_int()) {
            let add = match self.rules.get_trace_angle_restriction() {
                AngleRestriction::NinetyDegree => o.ninety_degree_corner(&n, true),
                AngleRestriction::FortyfiveDegree => o.fortyfive_degree_corner(&n, true),
                AngleRestriction::None => None,
            };
            if let Some(a) = add {
                connect_points.push(Point::Int(a));
            }
        }
        connect_points.push(new_center);
        let nets = self.item(id).net_numbers.to_vec();
        for (layer, hw, cl) in infos {
            self.insert_trace_points(&connect_points, layer, hw, &nets, cl, FixedState::Unfixed);
        }
    }

    /// Inserts an item, which is already on the board, into the item list only
    /// (the search tree entries are provided separately).
    pub(crate) fn insert_item_raw(&mut self, item: Item) {
        self.items_insert_raw(item);
    }

    /// `reuseEntriesAfterCutout`: the leaves of `from` are reused by the start and end pieces.
    pub(crate) fn reuse_entries_after_cutout(&mut self, from: ItemId, start: ItemId, end: ItemId) {
        let env_items: Vec<(Item, Item)> = vec![(self.item(start).clone(), self.item(end).clone())];
        let (start_item, end_item) = env_items.into_iter().next().unwrap();
        for ti in 0..self.search_trees.trees().len() {
            let tree = &self.search_trees.trees()[ti];
            let env = self.tree_env();
            let (start_shapes, start_layers) = tree.calculate_tree_shapes(&start_item, &env);
            let (end_shapes, end_layers) = tree.calculate_tree_shapes(&end_item, &env);
            let start_last = start_shapes.last().cloned().flatten();
            let end_first = end_shapes.first().cloned().flatten();
            let sl = start_layers.first().copied().unwrap_or(0);
            let el = end_layers.first().copied().unwrap_or(0);
            let start_count = start_shapes.len();
            let end_count = end_shapes.len();
            self.search_trees.trees_mut()[ti].reuse_entries_after_cutout(
                crate::board::TreeObject::Item(from),
                crate::board::TreeObject::Item(start),
                crate::board::TreeObject::Item(end),
                start_count,
                end_count,
                start_last,
                end_first,
                sl,
                el,
            );
        }
    }
}

// ====================================================================== TraceShover

/// `TraceShover.check(board, lineSegment, ...)`: the length of the segment, which can be
/// shoved to the left or right.
#[allow(clippy::too_many_arguments)]
pub fn check_segment_shove(
    board: &mut Board,
    seg: &LineSegment,
    shove_to_the_left: bool,
    layer: i32,
    nets: &[i32],
    trace_half_width: i32,
    clearance_class: i32,
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
) -> f64 {
    let mut trace_half_width = trace_half_width;
    {
        let tree = board.search_trees.default_tree();
        if tree.is_clearance_compensation_used() {
            trace_half_width += tree.clearance_compensation_value(&board.rules, clearance_class, layer);
        }
    }
    let shapes = seg.to_polyline().offset_shapes(trace_half_width);
    if shapes.len() != 1 {
        log::warn!("TraceShover.check: traceShape count 1 expected");
        return 0.0;
    }
    let trace_shape = shapes.into_iter().next().unwrap();
    if trace_shape.is_empty() {
        log::warn!("TraceShover.check: traceShape is empty");
        return 0.0;
    }
    if !trace_shape.is_contained_in(&board.bounding_box) {
        return 0.0;
    }
    let from_side = ShapeEntrySide::from_segment(seg, &trace_shape, shove_to_the_left);
    let mut entries = ShapeTraceEntries::new(trace_shape.clone(), layer, nets, clearance_class, Some(from_side));
    let obstacles = board.overlapping_items_with_clearance(&trace_shape, layer, &[], clearance_class);
    if !entries.store_items(board, &obstacles, false, true) || entries.trace_tails_in_shape() {
        return 0.0;
    }
    let trace_piece_count = entries.substitute_trace_count();
    if entries.stack_depth() > 1 {
        return 0.0;
    }
    let start = seg.start_point_approx();
    let end = seg.end_point_approx();
    let segment_length = end.distance(&start);
    let mut result = i32::MAX as f64;
    for via in entries.shove_via_list.clone() {
        if board.item(via).shares_net_no(nets) {
            continue;
        }
        let mut shove_via_ok = false;
        if max_via_recursion_depth > 0 {
            let new_centers = try_shove_via_points(board, &trace_shape, layer, via, clearance_class, false);
            if new_centers.is_empty() {
                return 0.0;
            }
            let delta = Point::Int(new_centers[0]).difference_by(&board.item(via).as_drill().unwrap().center);
            let mut ignore = BTreeSet::new();
            shove_via_ok = drill_item_check(board, via, &delta, max_recursion_depth, max_via_recursion_depth - 1, Some(&mut ignore), None);
        }
        if !shove_via_ok {
            let vc = board.item(via).as_drill().unwrap().center.to_float();
            let projection = start.scalar_product(&end, &vc) / segment_length;
            let tree_id = board.search_trees.default_tree_id();
            let via_box = board
                .drill_tree_shape_on_layer(tree_id, via, layer)
                .map(|s| s.bounding_box())
                .unwrap_or(IntBox::EMPTY);
            let via_radius = 0.5 * via_box.max_width();
            let mut ok = projection - via_radius - trace_half_width as f64;
            ok -= board.search_trees.default_tree().additional_clearance(
                &board.rules,
                clearance_class,
                board.item(via).clearance_class,
                layer,
            ) as f64;
            if ok <= 0.0 {
                return 0.0;
            }
            result = result.min(ok);
        }
    }
    if trace_piece_count == 0 {
        return result;
    }
    if max_recursion_depth <= 0 {
        return 0.0;
    }
    let line_direction = Direction::Int(seg.get_line().direction());
    while let Some(sub) = entries.next_substitute_trace_piece(board) {
        let st = sub.as_trace().unwrap();
        for i in 0..st.tile_shape_count() {
            let mut cls = LineSegment::from_polyline(&st.polyline, i + 1);
            if shove_to_the_left {
                cls = cls.opposite();
            }
            let is_in_front = Direction::Int(cls.get_line().direction()) == line_direction;
            if is_in_front {
                let shove_ok_length = check_segment_shove(
                    board,
                    &cls,
                    shove_to_the_left,
                    layer,
                    &sub.net_numbers,
                    st.half_width,
                    sub.clearance_class,
                    max_recursion_depth - 1,
                    max_via_recursion_depth,
                );
                if shove_ok_length < i32::MAX as f64 {
                    if shove_ok_length <= 0.0 {
                        return 0.0;
                    }
                    let projection = start
                        .scalar_product(&end, &cls.start_point_approx())
                        .min(start.scalar_product(&end, &cls.end_point_approx()))
                        / segment_length;
                    let mut ok = shove_ok_length + projection - trace_half_width as f64 - st.half_width as f64;
                    let tree = board.search_trees.default_tree();
                    if tree.is_clearance_compensation_used() {
                        ok -= (tree.clearance_compensation_value(&board.rules, sub.clearance_class, layer)
                            + tree.additional_clearance(&board.rules, clearance_class, sub.clearance_class, layer))
                            as f64;
                    } else {
                        ok -= board.rules.clearance_matrix.get_value(clearance_class, sub.clearance_class, layer, true) as f64;
                    }
                    if ok <= 0.0 {
                        return 0.0;
                    }
                    result = ok.min(result);
                }
                break;
            }
        }
    }
    result
}

/// `TraceShover.check(traceShape, fromSide, ...)` with the shove check cache.
#[allow(clippy::too_many_arguments)]
pub fn shove_check(
    board: &mut Board,
    trace_shape: &TileShape,
    from_side: Option<&ShapeEntrySide>,
    dir: Option<&Direction>,
    layer: i32,
    nets: &[i32],
    clearance_class: i32,
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
    max_spring_over_recursion_depth: i32,
    time_limit: Option<&TimeLimit>,
) -> bool {
    if time_limit.is_some() || !ShoveCheckCache::supported(trace_shape, dir, nets) || trace_shape.is_empty() {
        return shove_check_uncached(
            board,
            trace_shape,
            from_side,
            dir,
            layer,
            nets,
            clearance_class,
            max_recursion_depth,
            max_via_recursion_depth,
            max_spring_over_recursion_depth,
            time_limit,
        );
    }
    let stamp = board.items.content_version();
    let key = ShoveCheckCache::key(
        trace_shape,
        from_side,
        dir,
        layer,
        nets,
        clearance_class,
        max_recursion_depth,
        max_via_recursion_depth,
        max_spring_over_recursion_depth,
    );
    board.shove_cache.reset(stamp);
    if let Some(hit) = board.shove_cache.entries.get(&key).copied() {
        board.shove_cache.hits += 1;
        for _ in 0..hit.ids {
            board.new_item_id();
        }
        if hit.wrote_obstacle {
            board.set_shove_failing_obstacle(hit.obstacle);
        }
        if hit.wrote_layer {
            board.set_shove_failing_layer(hit.layer);
        }
        return hit.result;
    }
    board.shove_cache.misses += 1;
    let ids = board.communication.id_generator.max_generated_id();
    let obstacle_writes = board.shove_failing_obstacle_writes;
    let layer_writes = board.shove_failing_layer_writes;
    let result = shove_check_uncached(
        board,
        trace_shape,
        from_side,
        dir,
        layer,
        nets,
        clearance_class,
        max_recursion_depth,
        max_via_recursion_depth,
        max_spring_over_recursion_depth,
        None,
    );
    let ids_used = board.communication.id_generator.max_generated_id() - ids;
    let wrote_obstacle = board.shove_failing_obstacle_writes != obstacle_writes;
    let obstacle = if wrote_obstacle { board.shove_failing_obstacle } else { None };
    if board.items.content_version() != stamp
        || ids_used < 0
        || obstacle.map(|o| !board.try_item(o).map(|i| i.on_the_board).unwrap_or(false)).unwrap_or(false)
    {
        return result;
    }
    let wrote_layer = board.shove_failing_layer_writes != layer_writes;
    let entry = ShoveEntry {
        result,
        ids: ids_used,
        wrote_obstacle,
        obstacle,
        wrote_layer,
        layer: if wrote_layer { board.shove_failing_layer } else { -1 },
    };
    board.shove_cache.reset(stamp);
    if board.shove_cache.entries.len() >= SHOVE_CACHE_MAX_ENTRIES {
        board.shove_cache.entries.clear();
    } else {
        board.shove_cache.entries.insert(key, entry);
    }
    result
}

fn ignore_items_at_tie_pins(board: &Board, trace_shape: &TileShape, layer: i32, nets: &[i32]) -> BTreeSet<ItemId> {
    let mut result = BTreeSet::new();
    for id in board.overlapping_items(trace_shape, layer) {
        let item = board.item(id);
        if item.is_pin() && item.shares_net_no(nets) {
            result.extend(board.all_contacts(id, Some(layer)));
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn shove_check_uncached(
    board: &mut Board,
    trace_shape: &TileShape,
    from_side: Option<&ShapeEntrySide>,
    dir: Option<&Direction>,
    layer: i32,
    nets: &[i32],
    clearance_class: i32,
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
    max_spring_over_recursion_depth: i32,
    time_limit: Option<&TimeLimit>,
) -> bool {
    let mut max_spring_over_recursion_depth = max_spring_over_recursion_depth;
    if let Some(t) = time_limit {
        if t.limit_exceeded() {
            return false;
        }
    }
    if trace_shape.is_empty() {
        log::warn!("ShoveTraceAux.check: traceShape is empty");
        return true;
    }
    if !trace_shape.is_contained_in(&board.bounding_box) {
        let o = board.items.outline_id();
        board.set_shove_failing_obstacle(o);
        return false;
    }
    let mut entries = ShapeTraceEntries::new(trace_shape.clone(), layer, nets, clearance_class, from_side.cloned());
    let mut obstacles = board.overlapping_items_with_clearance(trace_shape, layer, &[], clearance_class);
    let ignore = ignore_items_at_tie_pins(board, trace_shape, layer, nets);
    obstacles.retain(|o| !ignore.contains(o));
    if !entries.store_items(board, &obstacles, false, true) {
        let fo = entries.get_found_obstacle();
        if std::env::var_os("RR_DEBUG").is_some() {
            eprintln!("shove_check: store_items failed, obstacle {:?} shape {:?}", fo, trace_shape.bounding_box());
        }
        board.set_shove_failing_obstacle(fo);
        return false;
    }
    let trace_piece_count = entries.substitute_trace_count();
    if entries.stack_depth() > 1 {
        let fo = entries.get_found_obstacle();
        board.set_shove_failing_obstacle(fo);
        return false;
    }
    let shape_radius = 0.5 * trace_shape.bounding_box().min_width();
    for via in entries.shove_via_list.clone() {
        if board.item(via).shares_net_no(nets) {
            continue;
        }
        if max_via_recursion_depth <= 0 {
            board.set_shove_failing_obstacle(Some(via));
            return false;
        }
        let via_center = board.item(via).as_drill().unwrap().center.clone();
        let vcf = via_center.to_float();
        let try_centers = try_shove_via_points(board, trace_shape, layer, via, clearance_class, true);
        let max_dist = 0.5
            * board
                .item(via)
                .as_drill()
                .unwrap()
                .get_shape_on_layer(layer)
                .map(|s| s.bounding_box().max_width())
                .unwrap_or(0.0)
            + shape_radius;
        let max_dist_sq = max_dist * max_dist;
        let mut ok = false;
        for (i, c) in try_centers.iter().enumerate() {
            if i == 0 || vcf.distance_square(&c.to_float()) <= max_dist_sq {
                let delta = Point::Int(*c).difference_by(&via_center);
                let mut ignore = BTreeSet::new();
                if drill_item_check(board, via, &delta, max_recursion_depth, max_via_recursion_depth - 1, Some(&mut ignore), time_limit) {
                    ok = true;
                    break;
                }
            }
        }
        if !ok {
            return false;
        }
    }
    if trace_piece_count == 0 {
        return true;
    }
    if max_recursion_depth <= 0 {
        let fo = entries.get_found_obstacle();
        board.set_shove_failing_obstacle(fo);
        return false;
    }
    let is_orthogonal = matches!(trace_shape, TileShape::Box(_));
    while let Some(mut sub) = entries.next_substitute_trace_piece(board) {
        if max_spring_over_recursion_depth > 0 {
            let chw = board.compensated_half_width_of(&sub);
            let st = sub.as_trace().unwrap().clone();
            let Some(new_pl) = spring_over(
                board,
                &st.polyline,
                chw,
                layer,
                &sub.net_numbers,
                sub.clearance_class,
                false,
                max_spring_over_recursion_depth,
                None,
            ) else {
                return false;
            };
            if !new_pl.ptr_eq(&st.polyline) {
                max_spring_over_recursion_depth -= 1;
                sub.as_trace_mut().unwrap().polyline = new_pl;
            }
        }
        let st = sub.as_trace().unwrap().clone();
        for i in 0..st.tile_shape_count() {
            let cd = Direction::Int(st.polyline.line(i + 1).direction());
            let is_in_front = dir.map(|d| *d == cd).unwrap_or(true);
            if is_in_front {
                let cur = ShapeAndEntrySide::new(board, &sub, i, is_orthogonal, true);
                if !shove_check(
                    board,
                    &cur.shape,
                    cur.from_side.as_ref(),
                    Some(&cd),
                    layer,
                    &sub.net_numbers,
                    sub.clearance_class,
                    max_recursion_depth - 1,
                    max_via_recursion_depth,
                    max_spring_over_recursion_depth,
                    time_limit,
                ) {
                    return false;
                }
            }
        }
    }
    true
}

/// `TraceShover.insert`.
#[allow(clippy::too_many_arguments)]
pub fn shove_insert(
    board: &mut Board,
    trace_shape: &TileShape,
    from_side: Option<&ShapeEntrySide>,
    layer: i32,
    nets: &[i32],
    clearance_class: i32,
    ignore_items: Option<&BTreeSet<ItemId>>,
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
    max_spring_over_recursion_depth: i32,
) -> bool {
    let mut max_spring_over_recursion_depth = max_spring_over_recursion_depth;
    if trace_shape.is_empty() {
        log::warn!("ShoveTraceAux.insert: traceShape is empty");
        return true;
    }
    if !trace_shape.is_contained_in(&board.bounding_box) {
        let o = board.items.outline_id();
        board.set_shove_failing_obstacle(o);
        return false;
    }
    if !shove_vias(
        board,
        trace_shape,
        from_side,
        layer,
        nets,
        clearance_class,
        ignore_items,
        max_recursion_depth,
        max_via_recursion_depth,
        true,
    ) {
        return false;
    }
    let mut entries = ShapeTraceEntries::new(trace_shape.clone(), layer, nets, clearance_class, from_side.cloned());
    let mut obstacles = board.overlapping_items_with_clearance(trace_shape, layer, &[], clearance_class);
    let ignore = ignore_items_at_tie_pins(board, trace_shape, layer, nets);
    obstacles.retain(|o| !ignore.contains(o));
    let shovable = entries.store_items(board, &obstacles, false, true);
    if !entries.shove_via_list.is_empty() {
        let v = entries.shove_via_list[0];
        board.set_shove_failing_obstacle(Some(v));
        return false;
    }
    if !shovable {
        let fo = entries.get_found_obstacle();
        board.set_shove_failing_obstacle(fo);
        return false;
    }
    let trace_piece_count = entries.substitute_trace_count();
    if trace_piece_count == 0 {
        return true;
    }
    if max_recursion_depth <= 0 {
        let fo = entries.get_found_obstacle();
        board.set_shove_failing_obstacle(fo);
        return false;
    }
    let tails_exist_before = board.contains_trace_tails(&obstacles, nets);
    entries.cutout_traces(board, &obstacles);
    let is_orthogonal = matches!(trace_shape, TileShape::Box(_));
    while let Some(mut sub) = entries.next_substitute_trace_piece(board) {
        {
            let st = sub.as_trace().unwrap();
            if st.first_corner() == st.last_corner() {
                continue;
            }
        }
        if max_spring_over_recursion_depth > 0 {
            let chw = board.compensated_half_width_of(&sub);
            let st = sub.as_trace().unwrap().clone();
            let Some(new_pl) = spring_over(
                board,
                &st.polyline,
                chw,
                layer,
                &sub.net_numbers,
                sub.clearance_class,
                false,
                max_spring_over_recursion_depth,
                None,
            ) else {
                return false;
            };
            if !new_pl.ptr_eq(&st.polyline) {
                max_spring_over_recursion_depth -= 1;
                sub.as_trace_mut().unwrap().polyline = new_pl;
            }
        }
        let sub_nets: NetNumbers = sub.net_numbers.clone();
        let st = sub.as_trace().unwrap().clone();
        for i in 0..st.tile_shape_count() {
            let cur = ShapeAndEntrySide::new(board, &sub, i, is_orthogonal, false);
            if !shove_insert(
                board,
                &cur.shape,
                cur.from_side.as_ref(),
                layer,
                &sub_nets,
                sub.clearance_class,
                ignore_items,
                max_recursion_depth - 1,
                max_via_recursion_depth,
                max_spring_over_recursion_depth,
            ) {
                return false;
            }
        }
        for i in 0..st.corner_count() {
            board.join_changed_area(&st.polyline.corner_approx(i), layer);
        }
        let end_corners = if !tails_exist_before {
            Some([st.first_corner(), st.last_corner()])
        } else {
            None
        };
        let id = board.insert_item(sub);
        let clip = board.changed_area.as_ref().map(|c| c.get_area(layer));
        board.normalize_trace(id, clip.as_ref());
        if let Some(ec) = end_corners {
            for c in ec.iter() {
                if let Some(tail) = board.get_trace_tail(c, layer, &sub_nets) {
                    let conn: Vec<ItemId> = board
                        .connection_items(tail, crate::board::connectivity::StopConnectionOption::Via)
                        .into_iter()
                        .collect();
                    board.remove_items(&conn);
                    for n in sub_nets.iter() {
                        board.combine_traces(*n);
                    }
                }
            }
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn spring_over(
    board: &mut Board,
    polyline: &Polyline,
    half_width: i32,
    layer: i32,
    nets: &[i32],
    clearance_class: i32,
    over_connected_pins: bool,
    recursion_depth: i32,
    contact_pins: Option<&BTreeSet<ItemId>>,
) -> Option<Polyline> {
    let mut found: Option<ItemId> = None;
    let mut found_box = IntBox::EMPTY;
    let check_nets: Vec<i32> = if contact_pins.is_none() { nets.to_vec() } else { Vec::new() };
    let lc = polyline.line_count();
    for i in 0..lc.saturating_sub(2) {
        let Some(cs) = polyline.offset_shape(half_width, i) else { continue };
        let obstacles = board.overlapping_items_with_clearance(&cs, layer, &check_nets, clearance_class);
        for oid in obstacles {
            let item = board.item(oid);
            let is_obstacle = if item.shares_net_no(nets) {
                item.is_pin() && contact_pins.map(|cp| !cp.contains(&oid)).unwrap_or(false)
            } else if item.is_conduction_area() {
                item.as_obstacle().unwrap().conduction_is_obstacle()
            } else if item.is_board_outline() {
                board.items.outline_blocks_nets(item, nets)
            } else if item.is_via_obstacle_area() || item.is_component_obstacle_area() {
                false
            } else if item.is_trace() {
                if board.is_shove_fixed(item) {
                    let mut ob = true;
                    for c in board.normal_contacts(oid) {
                        if board.item(c).shares_net_no(nets) {
                            ob = false;
                        }
                    }
                    ob
                } else {
                    false
                }
            } else {
                !item.is_routable()
            };
            if is_obstacle {
                match found {
                    None => {
                        found = Some(oid);
                        found_box = item.bounding_box();
                    }
                    Some(f) if f != oid => {
                        let cb = item.bounding_box();
                        if found_box.intersects(&cb) {
                            if cb.contains_box(&found_box) {
                                found = Some(oid);
                                found_box = cb;
                            } else if !found_box.contains_box(&cb) {
                                if std::env::var_os("RR_DEBUG").is_some() { eprintln!("spring_over: None at line {} found {:?}", line!(), found); }
                                return None;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if found.is_some() {
            break;
        }
    }
    let Some(found) = found else {
        return Some(polyline.clone());
    };
    let fi = board.item(found).clone();
    if recursion_depth <= 0
        || (fi.is_board_outline() && board.items.outline_blocks_nets(&fi, nets))
        || (fi.is_trace() && !board.is_shove_fixed(&fi))
    {
        board.set_shove_failing_obstacle(Some(found));
        if std::env::var_os("RR_DEBUG").is_some() { eprintln!("spring_over: None at line {} found {:?}", line!(), found); }
        return None;
    }
    let mut try_spring_over = true;
    if !over_connected_pins {
        for c in board.all_contacts(found, Some(layer)) {
            if board.item(c).is_trace() {
                try_spring_over = false;
                break;
            }
        }
    }
    let tree_id = board.search_trees.default_tree_id();
    let mut obstacle_shape: Option<TileShape> = None;
    if try_spring_over {
        if fi.is_obstacle_area() || fi.is_trace() {
            if board.tree_shape_count(tree_id, found) == 1 {
                obstacle_shape = board.tree_shape(tree_id, found, 0);
            } else if let Some(o) = fi.as_obstacle() {
                if o.convex_pieces.len() == 1 {
                    let comp = board
                        .search_trees
                        .default_tree()
                        .clearance_compensation_value(&board.rules, fi.clearance_class, layer);
                    obstacle_shape = Some(o.convex_pieces[0].enlarge(comp as f64));
                } else {
                    try_spring_over = false;
                }
            } else {
                try_spring_over = false;
            }
        } else if fi.is_drill_item() {
            obstacle_shape = board.drill_tree_shape_on_layer(tree_id, found, layer);
        }
    }
    if !try_spring_over || obstacle_shape.is_none() {
        board.set_shove_failing_obstacle(Some(found));
        if std::env::var_os("RR_DEBUG").is_some() { eprintln!("spring_over: None at line {} found {:?}", line!(), found); }
        return None;
    }
    let obstacle_shape = obstacle_shape.unwrap();
    let tree = board.search_trees.default_tree();
    let mut offset_shape = if tree.is_clearance_compensation_used() {
        let off = half_width + 1 + tree.additional_clearance(&board.rules, clearance_class, fi.clearance_class, layer);
        obstacle_shape.enlarge(off as f64)
    } else {
        let off = half_width + 1;
        let half_cl = 0.5 * board.clearance_value(fi.clearance_class, clearance_class, layer) as f64;
        if !obstacle_shape.is_int_octagon() {
            obstacle_shape.enlarge(half_cl).enlarge(half_cl).enlarge(off as f64)
        } else {
            obstacle_shape.enlarge(off as f64 + half_cl).enlarge(half_cl)
        }
    };
    match board.rules.get_trace_angle_restriction() {
        AngleRestriction::NinetyDegree => offset_shape = TileShape::Box(offset_shape.bounding_box()),
        AngleRestriction::FortyfiveDegree => offset_shape = TileShape::Octagon(offset_shape.bounding_octagon()),
        AngleRestriction::None => {}
    }
    if offset_shape.contains_inside(&polyline.first_corner()) || offset_shape.contains_inside(&polyline.last_corner()) {
        board.set_shove_failing_obstacle(Some(found));
        if std::env::var_os("RR_DEBUG").is_some() { eprintln!("spring_over: None at line {} found {:?} nets {:?} kind {:?} search nets {:?} pl {:?} shape {:?}", line!(), found, fi.net_numbers, fi.board_item_type(), nets, polyline.corners(), offset_shape.bounding_box()); }
        return None;
    }
    let entries = offset_shape.entrance_points(polyline);
    if entries.is_empty() {
        return Some(polyline.clone());
    }
    if entries.len() < 2 {
        board.set_shove_failing_obstacle(Some(found));
        if std::env::var_os("RR_DEBUG").is_some() { eprintln!("spring_over: None at line {} found {:?}", line!(), found); }
        return None;
    }
    let first_side = entries[0].1;
    let last_side = entries[entries.len() - 1].1;
    let first_line = entries[0].0;
    let last_line = entries[entries.len() - 1].0;
    let n = offset_shape.border_line_count();
    let mut side_diff = last_side as i64 - first_side as i64;
    if side_diff < 0 {
        side_diff += n as i64;
    } else if side_diff == 0 {
        let cc = offset_shape.corner_approx(first_side);
        let fi_ = polyline.line(first_line).intersection_approx(&offset_shape.border_line(first_side));
        let si = polyline.line(last_line).intersection_approx(&offset_shape.border_line(last_side));
        if cc.distance(&si) < cc.distance(&fi_) {
            side_diff += n as i64;
        }
    }
    let side_diff = side_diff as usize;
    let mut sub_lines = Vec::with_capacity(side_diff + 3);
    sub_lines.push(polyline.line(first_line));
    let mut cur_edge = first_side;
    for _ in 1..=side_diff + 1 {
        sub_lines.push(offset_shape.border_line(cur_edge));
        cur_edge = if cur_edge == n - 1 { 0 } else { cur_edge + 1 };
    }
    sub_lines.push(polyline.line(last_line));
    let sub_pl = Polyline::from_lines(&sub_lines);
    let pieces = offset_shape.cutout_polyline(polyline);
    let mut result = sub_pl.clone();
    if !pieces.is_empty() {
        result = pieces[0].combine(&sub_pl);
    }
    if pieces.len() > 1 {
        result = result.combine(&pieces[1]);
    }
    spring_over(
        board,
        &result,
        half_width,
        layer,
        nets,
        clearance_class,
        over_connected_pins,
        recursion_depth - 1,
        contact_pins,
    )
}

/// `TraceShover.springOverObstacles`.
pub fn spring_over_obstacles(
    board: &mut Board,
    polyline: &Polyline,
    half_width: i32,
    layer: i32,
    nets: &[i32],
    clearance_class: i32,
    contact_pins: Option<&BTreeSet<ItemId>>,
) -> Option<Polyline> {
    let max_depth = 20;
    let ccw = spring_over(board, polyline, half_width, layer, nets, clearance_class, true, max_depth, contact_pins);
    if let Some(c) = &ccw {
        if c.ptr_eq(polyline) {
            return Some(polyline.clone());
        }
    }
    let cw = spring_over(board, &polyline.reverse(), half_width, layer, nets, clearance_class, true, max_depth, contact_pins);
    match (cw, ccw) {
        (Some(cw), Some(ccw)) => {
            if cw.length_approx() <= ccw.length_approx() {
                Some(cw.reverse())
            } else {
                Some(ccw)
            }
        }
        (Some(cw), None) => Some(cw.reverse()),
        (None, Some(ccw)) => Some(ccw),
        (None, None) => None,
    }
}

// ====================================================================== ForcedPadRouter

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckDrillResult {
    Drillable,
    DrillableWithAttachSmd,
    NotDrillable,
}

fn calc_check_shape_for_from_side(shape_center: &Point, border_line: &Line) -> Option<TileShape> {
    let scf = shape_center.to_float();
    let off_proj = scf.projection_approx(border_line);
    let dir = border_line.direction();
    let lines = [
        Line::from_point_direction(shape_center, &Direction::Int(dir)),
        Line::from_point_direction(shape_center, &Direction::Int(dir.turn_45_degree(2))),
        Line::from_point_dir(off_proj.round(), &dir),
    ];
    Polyline::from_lines(&lines).offset_shape(1, 0)
}

/// `ForcedPadRouter.inFrontOfPad`.
pub fn in_front_of_pad(line: &Line, pad_shape: &TileShape, from_side: i32, width: i32, with_sides: bool) -> bool {
    if !pad_shape.is_int_octagon() {
        return true;
    }
    let o = pad_shape.bounding_octagon();
    let (a, b) = (line.a, line.b);
    let w = width as f64;
    let dw = width as f64 * SQRT2;
    let (minx, maxx) = (a.x.min(b.x) as f64, a.x.max(b.x) as f64);
    let (miny, maxy) = (a.y.min(b.y) as f64, a.y.max(b.y) as f64);
    let (mind, maxd) = ((a.x - a.y).min(b.x - b.y) as f64, (a.x - a.y).max(b.x - b.y) as f64);
    let (mins, maxs) = ((a.x + a.y).min(b.x + b.y) as f64, (a.x + a.y).max(b.x + b.y) as f64);
    let (lx, ly, rx, uy) = (o.lx as f64, o.ly as f64, o.rx as f64, o.uy as f64);
    let (ulx, lrx, llx, urx) = (o.ulx as f64, o.lrx as f64, o.llx as f64, o.urx as f64);
    let mut result;
    match from_side {
        0 => {
            result = miny >= uy + w || maxd <= ulx - dw || mins >= urx + dw;
            if with_sides && !result {
                result = maxx <= lx - w && mind <= ulx - dw || minx >= rx + w && mins >= urx + dw;
            }
        }
        1 => {
            result = miny >= uy + w || maxd <= ulx - dw || maxx <= lx - w;
            if with_sides && !result {
                result = minx <= lx - w && maxs <= llx - dw || maxy >= uy + w && mins >= urx + dw;
            }
        }
        2 => {
            result = maxx <= lx - w || maxd <= ulx - dw || maxs <= llx - dw;
            if with_sides && !result {
                result = maxy <= ly - w && mins <= llx - dw || miny >= uy + w && mind <= ulx - dw;
            }
        }
        3 => {
            result = maxx <= lx - w || maxy <= ly - w || maxs <= llx - dw;
            if with_sides && !result {
                result = miny <= ly - w && mind >= lrx + dw || minx <= lx - w && maxd <= ulx - dw;
            }
        }
        4 => {
            result = maxy <= ly - w || maxs <= llx - dw || mind >= lrx + dw;
            if with_sides && !result {
                result = minx >= rx + w && maxd >= lrx + dw || maxx <= lx - w && mins <= llx - dw;
            }
        }
        5 => {
            result = maxy <= ly - w || minx >= rx + w || mind >= lrx + dw;
            if with_sides && !result {
                result = maxx >= rx + w && mins >= urx + dw || miny <= ly - w && maxs <= llx - dw;
            }
        }
        6 => {
            result = minx >= rx + w || mins >= urx + dw || mind >= lrx + dw;
            if with_sides && !result {
                result = maxy <= ly - w && maxd >= lrx + dw || miny >= uy + w && maxs >= urx + dw;
            }
        }
        7 => {
            result = miny >= uy + w || mins >= urx + dw || minx >= rx + w;
            if with_sides && !result {
                result = maxy >= uy + w && maxd <= ulx - dw || maxx >= rx + w && mind >= lrx + dw;
            }
        }
        _ => {
            log::warn!("ForcedPadAlgo.in_front_of_pad: fromSide out of range");
            result = true;
        }
    }
    result
}

/// `ForcedPadRouter.checkForcedPad`.
#[allow(clippy::too_many_arguments)]
pub fn check_forced_pad(
    board: &mut Board,
    pad_shape: &TileShape,
    from_side: &ShapeEntrySide,
    layer: i32,
    nets: &[i32],
    clearance_class: i32,
    copper_sharing_allowed: bool,
    ignore_items: Option<&BTreeSet<ItemId>>,
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
    check_only_front: bool,
    time_limit: Option<&TimeLimit>,
) -> CheckDrillResult {
    if !pad_shape.is_contained_in(&board.bounding_box) {
        let o = board.items.outline_id();
        board.set_shove_failing_obstacle(o);
        return CheckDrillResult::NotDrillable;
    }
    let mut entries = ShapeTraceEntries::new(pad_shape.clone(), layer, nets, clearance_class, Some(from_side.clone()));
    let mut obstacles = board.overlapping_items_with_clearance(pad_shape, layer, &[], clearance_class);
    if let Some(ig) = ignore_items {
        obstacles.retain(|o| !ig.contains(o));
    }
    if !entries.store_items(board, &obstacles, true, copper_sharing_allowed) {
        let fo = entries.get_found_obstacle();
        board.set_shove_failing_obstacle(fo);
        return CheckDrillResult::NotDrillable;
    }
    for via in entries.shove_via_list.clone() {
        if max_via_recursion_depth <= 0 {
            board.set_shove_failing_obstacle(Some(via));
            return CheckDrillResult::NotDrillable;
        }
        let nc = try_shove_via_points(board, pad_shape, layer, via, clearance_class, false);
        if nc.is_empty() {
            board.set_shove_failing_obstacle(Some(via));
            return CheckDrillResult::NotDrillable;
        }
        let delta = Point::Int(nc[0]).difference_by(&board.item(via).as_drill().unwrap().center);
        let mut ig = BTreeSet::new();
        if !drill_item_check(board, via, &delta, max_recursion_depth, max_via_recursion_depth - 1, Some(&mut ig), time_limit) {
            return CheckDrillResult::NotDrillable;
        }
    }
    let mut result = CheckDrillResult::Drillable;
    if copper_sharing_allowed && obstacles.iter().any(|o| board.item(*o).is_pin()) {
        result = CheckDrillResult::DrillableWithAttachSmd;
    }
    if entries.substitute_trace_count() == 0 {
        return result;
    }
    if max_recursion_depth <= 0 || entries.stack_depth() > 1 {
        let fo = entries.get_found_obstacle();
        board.set_shove_failing_obstacle(fo);
        return CheckDrillResult::NotDrillable;
    }
    let is_orthogonal = matches!(pad_shape, TileShape::Box(_));
    while let Some(sub) = entries.next_substitute_trace_piece(board) {
        let st = sub.as_trace().unwrap().clone();
        for i in 0..st.tile_shape_count() {
            let cl = st.polyline.line(i + 1);
            let cd = Direction::Int(cl.direction());
            let is_in_front = if check_only_front {
                in_front_of_pad(&cl, pad_shape, from_side.no, st.half_width, true)
            } else {
                true
            };
            if is_in_front {
                let cur = ShapeAndEntrySide::new(board, &sub, i, is_orthogonal, true);
                if !shove_check(
                    board,
                    &cur.shape,
                    cur.from_side.as_ref(),
                    Some(&cd),
                    layer,
                    &sub.net_numbers,
                    sub.clearance_class,
                    max_recursion_depth - 1,
                    max_via_recursion_depth,
                    0,
                    time_limit,
                ) {
                    return CheckDrillResult::NotDrillable;
                }
            }
        }
    }
    result
}

/// `ForcedPadRouter.forcedPad`.
#[allow(clippy::too_many_arguments)]
pub fn forced_pad(
    board: &mut Board,
    pad_shape: &TileShape,
    from_side: &ShapeEntrySide,
    layer: i32,
    nets: &[i32],
    clearance_class: i32,
    copper_sharing_allowed: bool,
    ignore_items: Option<&BTreeSet<ItemId>>,
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
) -> bool {
    if pad_shape.is_empty() {
        log::warn!("ShoveTraceAux.forced_pad: padShape is empty");
        return true;
    }
    if !pad_shape.is_contained_in(&board.bounding_box) {
        let o = board.items.outline_id();
        board.set_shove_failing_obstacle(o);
        return false;
    }
    if !shove_vias(
        board,
        pad_shape,
        Some(from_side),
        layer,
        nets,
        clearance_class,
        ignore_items,
        max_recursion_depth,
        max_via_recursion_depth,
        false,
    ) {
        return false;
    }
    let mut entries = ShapeTraceEntries::new(pad_shape.clone(), layer, nets, clearance_class, Some(from_side.clone()));
    let mut obstacles = board.overlapping_items_with_clearance(pad_shape, layer, &[], clearance_class);
    if let Some(ig) = ignore_items {
        obstacles.retain(|o| !ig.contains(o));
    }
    let shovable = entries.store_items(board, &obstacles, true, copper_sharing_allowed) && entries.shove_via_list.is_empty();
    if !shovable {
        let fo = entries.get_found_obstacle();
        board.set_shove_failing_obstacle(fo);
        return false;
    }
    if entries.substitute_trace_count() == 0 {
        return true;
    }
    if max_recursion_depth <= 0 {
        let fo = entries.get_found_obstacle();
        board.set_shove_failing_obstacle(fo);
        return false;
    }
    let tails_exist_before = board.contains_trace_tails(&obstacles, nets);
    entries.cutout_traces(board, &obstacles);
    let is_orthogonal = matches!(pad_shape, TileShape::Box(_));
    while let Some(sub) = entries.next_substitute_trace_piece(board) {
        let st = sub.as_trace().unwrap().clone();
        if st.first_corner() == st.last_corner() {
            continue;
        }
        let sub_nets = sub.net_numbers.clone();
        for i in 0..st.tile_shape_count() {
            let cur = ShapeAndEntrySide::new(board, &sub, i, is_orthogonal, false);
            if !shove_insert(
                board,
                &cur.shape,
                cur.from_side.as_ref(),
                layer,
                &sub_nets,
                sub.clearance_class,
                ignore_items,
                max_recursion_depth - 1,
                max_via_recursion_depth,
                0,
            ) {
                return false;
            }
        }
        for i in 0..st.corner_count() {
            board.join_changed_area(&st.polyline.corner_approx(i), layer);
        }
        let end_corners = if !tails_exist_before {
            Some([st.first_corner(), st.last_corner()])
        } else {
            None
        };
        let id = board.insert_item(sub);
        let clip = board.changed_area.as_ref().map(|c| c.get_area(layer));
        board.normalize_trace(id, clip.as_ref());
        if let Some(ec) = end_corners {
            for c in ec.iter() {
                if let Some(tail) = board.get_trace_tail(c, layer, &sub_nets) {
                    let conn: Vec<ItemId> = board
                        .connection_items(tail, crate::board::connectivity::StopConnectionOption::Via)
                        .into_iter()
                        .collect();
                    board.remove_items(&conn);
                    for n in sub_nets.iter() {
                        board.combine_traces(*n);
                    }
                }
            }
        }
    }
    true
}

/// `ForcedPadRouter.calcFromSide`.
pub fn calc_from_side(board: &Board, shape: &TileShape, shape_center: &Point, layer: i32, offset: i32, clearance_class: i32) -> ShapeEntrySide {
    let offset_shape = shape.offset(offset as f64);
    for cl in [clearance_class, 0] {
        for i in 0..offset_shape.border_line_count() {
            if let Some(cs) = calc_check_shape_for_from_side(shape_center, &offset_shape.border_line(i)) {
                if board.check_trace_shape(&cs, layer, &[], cl, None) {
                    return ShapeEntrySide::new(i as i32, None);
                }
            }
        }
    }
    ShapeEntrySide::NOT_CALCULATED
}

// ====================================================================== DrillItemMover

/// `DrillItemMover.check`.
pub fn drill_item_check(
    board: &mut Board,
    id: ItemId,
    vector: &Vector,
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
    ignore_items: Option<&mut BTreeSet<ItemId>>,
    time_limit: Option<&TimeLimit>,
) -> bool {
    if let Some(t) = time_limit {
        if t.limit_exceeded() {
            return false;
        }
    }
    let item = board.item(id).clone();
    if board.is_shove_fixed(&item) {
        return false;
    }
    for c in board.normal_contacts(id) {
        let ci = board.item(c);
        if !(ci.is_trace() || ci.is_conduction_area()) {
            return false;
        }
    }
    let mut local = BTreeSet::new();
    let ignore: &mut BTreeSet<ItemId> = match ignore_items {
        Some(i) => i,
        None => &mut local,
    };
    ignore.insert(id);
    let attach_allowed = item.as_via().map(|v| v.attach_allowed).unwrap_or(false);
    let tree_id = board.search_trees.default_tree_id();
    let d = item.as_drill().unwrap();
    for layer in d.first_layer..=d.last_layer {
        let ind = (layer - d.first_layer) as usize;
        let Some(cs) = board.tree_shape(tree_id, id, ind) else { continue };
        let ns = cs.translate_by(vector);
        let tile = if board.rules.get_trace_angle_restriction() == AngleRestriction::NinetyDegree {
            TileShape::Box(ns.bounding_box())
        } else {
            TileShape::Octagon(ns.bounding_octagon())
        };
        let from_side = ShapeEntrySide::from_point(&d.center, &tile);
        if check_forced_pad(
            board,
            &tile,
            &from_side,
            layer,
            &item.net_numbers,
            item.clearance_class,
            attach_allowed,
            Some(ignore),
            max_recursion_depth,
            max_via_recursion_depth,
            true,
            time_limit,
        ) == CheckDrillResult::NotDrillable
        {
            return false;
        }
    }
    true
}

/// `DrillItemMover.insert` / `insertJoiningTidyRegion`.
pub fn drill_item_insert(
    board: &mut Board,
    id: ItemId,
    vector: &Vector,
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
    tidy_region: Option<&mut IntOctagon>,
) -> bool {
    let item = board.item(id).clone();
    if board.is_shove_fixed(&item) {
        return false;
    }
    let attach_allowed = item.as_via().map(|v| v.attach_allowed).unwrap_or(false);
    let mut ignore = BTreeSet::new();
    ignore.insert(id);
    let tree_id = board.search_trees.default_tree_id();
    let d = item.as_drill().unwrap().clone();
    let mut tidy_region = tidy_region;
    for layer in d.first_layer..=d.last_layer {
        let ind = (layer - d.first_layer) as usize;
        let Some(cs) = board.tree_shape(tree_id, id, ind) else { continue };
        let ns = cs.translate_by(vector);
        let tile = if board.rules.get_trace_angle_restriction() == AngleRestriction::NinetyDegree {
            TileShape::Box(ns.bounding_box())
        } else {
            TileShape::Octagon(ns.bounding_octagon())
        };
        if let Some(tr) = tidy_region.as_deref_mut() {
            *tr = tr.union(&tile.bounding_octagon());
        }
        let from_side = ShapeEntrySide::from_point(&d.center, &tile);
        if !forced_pad(
            board,
            &tile,
            &from_side,
            layer,
            &item.net_numbers,
            item.clearance_class,
            attach_allowed,
            Some(&ignore),
            max_recursion_depth,
            max_via_recursion_depth,
        ) {
            return false;
        }
        let bb = cs.bounding_box();
        for j in 0..4 {
            board.join_changed_area(&bb.corner(j).to_float(), layer);
        }
    }
    board.move_drill_item_by(id, vector);
    true
}

/// `DrillItemMover.shoveVias`.
#[allow(clippy::too_many_arguments)]
pub fn shove_vias(
    board: &mut Board,
    obstacle_shape: &TileShape,
    from_side: Option<&ShapeEntrySide>,
    layer: i32,
    nets: &[i32],
    clearance_class: i32,
    ignore_items: Option<&BTreeSet<ItemId>>,
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
    copper_sharing_allowed: bool,
) -> bool {
    let mut entries = ShapeTraceEntries::new(obstacle_shape.clone(), layer, nets, clearance_class, from_side.cloned());
    let obstacles = board.overlapping_items_with_clearance(obstacle_shape, layer, &[], clearance_class);
    if !entries.store_items(board, &obstacles, false, copper_sharing_allowed) {
        return true;
    }
    let mut via_list = entries.shove_via_list.clone();
    if let Some(ig) = ignore_items {
        via_list.retain(|v| !ig.contains(v));
    }
    if via_list.is_empty() {
        return true;
    }
    let shape_radius = 0.5 * obstacle_shape.bounding_box().min_width();
    for via in via_list {
        if !board.items.contains(via) {
            continue;
        }
        if board.item(via).shares_net_no(nets) {
            continue;
        }
        if max_via_recursion_depth <= 0 {
            return true;
        }
        let try_centers = try_shove_via_points(board, obstacle_shape, layer, via, clearance_class, true);
        let vd = board.item(via).as_drill().unwrap();
        let max_dist = 0.5 * vd.get_shape_on_layer(layer).map(|s| s.bounding_box().max_width()).unwrap_or(0.0) + shape_radius;
        let max_dist_sq = max_dist * max_dist;
        let current_center = vd.center.to_int();
        let check_center = current_center.to_float();
        let mut new_center: Option<IntPoint> = None;
        let mut rel: Option<Vector> = None;
        for (i, c) in try_centers.iter().enumerate() {
            if i == 0 || check_center.distance_square(&c.to_float()) <= max_dist_sq {
                let mut local_ignore: BTreeSet<ItemId> = ignore_items.cloned().unwrap_or_default();
                let r = Vector::Int(c.difference_by(&current_center));
                rel = Some(r.clone());
                if drill_item_check(board, via, &r, max_recursion_depth, max_via_recursion_depth - 1, Some(&mut local_ignore), None) {
                    new_center = Some(*c);
                    break;
                }
            }
        }
        if new_center.is_none() {
            continue;
        }
        if !drill_item_insert(board, via, rel.as_ref().unwrap(), max_recursion_depth, max_via_recursion_depth - 1, None) {
            return false;
        }
    }
    true
}

/// `DrillItemMover.tryShoveViaPoints`.
pub fn try_shove_via_points(
    board: &Board,
    obstacle_shape: &TileShape,
    layer: i32,
    via: ItemId,
    clearance_class: i32,
    extended_check: bool,
) -> Vec<IntPoint> {
    let tree_id = board.search_trees.default_tree_id();
    let Some(mut via_shape) = board.drill_tree_shape_on_layer(tree_id, via, layer) else {
        return Vec::new();
    };
    let is_int_octagon = obstacle_shape.is_int_octagon();
    let via_item = board.item(via);
    let cl_value = board
        .search_trees
        .default_tree()
        .additional_clearance(&board.rules, clearance_class, via_item.clearance_class, layer) as f64;
    let ninety = board.rules.get_trace_angle_restriction() == AngleRestriction::NinetyDegree;
    let mut shove_distance = if ninety || is_int_octagon {
        0.5 * via_shape.bounding_box().max_width() + cl_value
    } else {
        0.5 * cl_value
    };
    shove_distance += 2.0;
    let center = via_item.as_drill().unwrap().center.to_int();
    if ninety {
        let ob = obstacle_shape.bounding_box().offset(shove_distance);
        ob.nearest_border_projections(&center, if extended_check { 2 } else { 1 })
    } else if is_int_octagon {
        let oo = obstacle_shape.bounding_octagon().enlarge(shove_distance);
        oo.nearest_border_projections(&center, if extended_check { 4 } else { 1 })
    } else {
        let os = obstacle_shape.enlarge(shove_distance);
        via_shape = via_shape.enlarge(0.5 * cl_value);
        let deltas = os.nearest_relative_outside_locations(&via_shape, if extended_check { 4 } else { 1 });
        deltas
            .iter()
            .map(|d| {
                let r = d.round();
                center.translate_by_int(crate::geometry::IntVector::new(r.x, r.y))
            })
            .collect()
    }
}

// ====================================================================== ForcedViaInserter

fn compensate_pad_shape(board: &Board, shape: TileShape, clearance_class: i32, layer: i32) -> TileShape {
    let comp = board
        .search_trees
        .default_tree()
        .clearance_compensation_value(&board.rules, clearance_class, layer);
    if comp == 0 {
        return shape;
    }
    match shape {
        TileShape::Box(b) => TileShape::Box(b.offset(comp as f64)),
        other => other.enlarge(comp as f64),
    }
}

fn hole_check_shape(board: &Board, padstack: PadstackId, location: &Point) -> Option<Shape> {
    let hc = board.rules.get_hole_clearance();
    let center = location.as_int()?;
    if hc <= 0 {
        return None;
    }
    let r = board.library.padstacks.get(padstack).get_drill_radius();
    if r <= 0.0 {
        return None;
    }
    Some(Shape::Circle(Circle::new(center, (r + hc as f64 + 10.0).ceil() as i32)))
}

fn tile_of_shape(board: &Board, s: &Shape) -> TileShape {
    if board.rules.get_trace_angle_restriction() == AngleRestriction::NinetyDegree {
        TileShape::Box(s.bounding_box())
    } else {
        TileShape::Octagon(s.bounding_octagon())
    }
}

fn calculate_from_side_for_via(via_location: &FloatPoint, via_shape: &TileShape, room: &Simplex, dist: f64, is_90: bool) -> Option<ShapeEntrySide> {
    let vb = via_shape.bounding_box();
    let room_tile = TileShape::Simplex(room.clone());
    for i in 0..4 {
        let (cp, bx, by) = match i {
            0 => (FloatPoint::new(via_location.x, via_location.y - dist), via_location.x, vb.ll.y as f64),
            1 => (FloatPoint::new(via_location.x + dist, via_location.y), vb.ur.x as f64, via_location.y),
            2 => (FloatPoint::new(via_location.x, via_location.y + dist), via_location.x, vb.ur.y as f64),
            _ => (FloatPoint::new(via_location.x - dist, via_location.y), vb.ll.x as f64, via_location.y),
        };
        if room_tile.contains_float(&cp) {
            let idx = if is_90 { i } else { 2 * i };
            return Some(ShapeEntrySide::new(idx, Some(FloatPoint::new(bx, by))));
        }
    }
    if is_90 {
        return None;
    }
    let dist = dist / SQRT2;
    let bd = vb.max_width() / (2.0 * SQRT2);
    for i in 0..4 {
        let (sx, sy) = match i {
            0 => (1.0, -1.0),
            1 => (1.0, 1.0),
            2 => (-1.0, 1.0),
            _ => (-1.0, -1.0),
        };
        let cp = FloatPoint::new(via_location.x + sx * dist, via_location.y + sy * dist);
        if room_tile.contains_float(&cp) {
            let bp = FloatPoint::new(via_location.x + sx * bd, via_location.y + sy * bd);
            return Some(ShapeEntrySide::new(2 * i + 1, Some(bp)));
        }
    }
    None
}

/// `ForcedViaInserter.checkLayer`.
#[allow(clippy::too_many_arguments)]
pub fn forced_via_check_layer(
    board: &mut Board,
    via_radius: f64,
    clearance_class: i32,
    attach_smd_allowed: bool,
    room_shape: &TileShape,
    location: &Point,
    layer: i32,
    nets: &[i32],
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
    trace_half_width: i32,
    trace_clearance_class: i32,
) -> CheckDrillResult {
    if via_radius <= 0.0 {
        return CheckDrillResult::Drillable;
    }
    let Some(il) = location.as_int() else {
        return CheckDrillResult::NotDrillable;
    };
    let via_shape = Shape::Circle(Circle::new(il, via_radius.ceil() as i32));
    let check_radius = via_radius + 0.5 * board.clearance_value(clearance_class, clearance_class, layer) as f64 + board.get_min_trace_half_width() as f64;
    let is_90 = board.rules.get_trace_angle_restriction() == AngleRestriction::NinetyDegree;
    let tile = tile_of_shape(board, &via_shape);
    let tile = compensate_pad_shape(board, tile, clearance_class, layer);
    let Some(from_side) = calculate_from_side_for_via(&location.to_float(), &tile, &room_shape.to_simplex(), check_radius, is_90) else {
        return CheckDrillResult::NotDrillable;
    };
    let via_result = check_forced_pad(
        board,
        &tile,
        &from_side,
        layer,
        nets,
        clearance_class,
        attach_smd_allowed,
        None,
        max_recursion_depth,
        max_via_recursion_depth,
        false,
        None,
    );
    if via_result == CheckDrillResult::NotDrillable || trace_half_width <= 0 {
        return via_result;
    }
    let stc = Shape::Circle(Circle::new(il, trace_half_width));
    let sts = compensate_pad_shape(board, tile_of_shape(board, &stc), trace_clearance_class, layer);
    let trace_result = check_forced_pad(
        board,
        &sts,
        &from_side,
        layer,
        nets,
        trace_clearance_class,
        true,
        None,
        max_recursion_depth,
        max_via_recursion_depth,
        false,
        None,
    );
    if trace_result == CheckDrillResult::NotDrillable {
        return trace_result;
    }
    if via_result == CheckDrillResult::DrillableWithAttachSmd || trace_result == CheckDrillResult::DrillableWithAttachSmd {
        return CheckDrillResult::DrillableWithAttachSmd;
    }
    CheckDrillResult::Drillable
}

/// `ForcedViaInserter.check`.
#[allow(clippy::too_many_arguments)]
pub fn forced_via_check(
    board: &mut Board,
    via_info: &ViaInfo,
    location: &Point,
    nets: &[i32],
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
    trace_pen_half_widths: Option<&[i32]>,
    trace_clearance_class: i32,
) -> bool {
    forced_via_impl(board, via_info, location, nets, max_recursion_depth, max_via_recursion_depth, trace_pen_half_widths, trace_clearance_class, false)
}

/// `ForcedViaInserter.insert`.
#[allow(clippy::too_many_arguments)]
pub fn forced_via_insert(
    board: &mut Board,
    via_info: &ViaInfo,
    location: &Point,
    nets: &[i32],
    trace_clearance_class: i32,
    trace_pen_half_widths: &[i32],
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
) -> bool {
    if !forced_via_impl(
        board,
        via_info,
        location,
        nets,
        max_recursion_depth,
        max_via_recursion_depth,
        Some(trace_pen_half_widths),
        trace_clearance_class,
        true,
    ) {
        return false;
    }
    board.insert_via(via_info.padstack, location, nets, via_info.clearance_class, FixedState::Unfixed, via_info.attach_smd_allowed);
    true
}

#[allow(clippy::too_many_arguments)]
fn forced_via_impl(
    board: &mut Board,
    via_info: &ViaInfo,
    location: &Point,
    nets: &[i32],
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
    trace_pen_half_widths: Option<&[i32]>,
    trace_clearance_class: i32,
    insert: bool,
) -> bool {
    let tv = location.difference_by(&Point::ZERO);
    let offset = board.get_min_trace_half_width();
    let ps_id = via_info.padstack;
    let hole = hole_check_shape(board, ps_id, location);
    let (from_l, to_l) = {
        let ps = board.library.padstacks.get(ps_id);
        (ps.from_layer(), ps.to_layer())
    };
    for i in from_l..=to_l {
        let pad = board.library.padstacks.get(ps_id).get_shape(i).cloned();
        let mut cl = via_info.clearance_class;
        let pad_shape = match pad {
            None => match &hole {
                None => continue,
                Some(h) => {
                    cl = 0;
                    h.clone()
                }
            },
            Some(p) => p.translate_by(&tv),
        };
        let tile = compensate_pad_shape(board, tile_of_shape(board, &pad_shape), cl, i);
        let from_side = calc_from_side(board, &tile, location, i, offset, cl);
        let ok = if insert {
            forced_pad(board, &tile, &from_side, i, nets, cl, via_info.attach_smd_allowed, None, max_recursion_depth, max_via_recursion_depth)
        } else {
            check_forced_pad(
                board,
                &tile,
                &from_side,
                i,
                nets,
                cl,
                via_info.attach_smd_allowed,
                None,
                max_recursion_depth,
                max_via_recursion_depth,
                false,
                None,
            ) != CheckDrillResult::NotDrillable
        };
        if !ok {
            board.set_shove_failing_layer(i);
            return false;
        }
        if cl != 0 {
            if let Some(h) = &hole {
                let ht = tile_of_shape(board, h);
                let ok = if insert {
                    forced_pad(board, &ht, &from_side, i, nets, 0, via_info.attach_smd_allowed, None, max_recursion_depth, max_via_recursion_depth)
                } else {
                    check_forced_pad(
                        board,
                        &ht,
                        &from_side,
                        i,
                        nets,
                        0,
                        via_info.attach_smd_allowed,
                        None,
                        max_recursion_depth,
                        max_via_recursion_depth,
                        false,
                        None,
                    ) != CheckDrillResult::NotDrillable
                };
                if !ok {
                    board.set_shove_failing_layer(i);
                    return false;
                }
            }
        }
        if let Some(widths) = trace_pen_half_widths {
            if (i as usize) < widths.len() && widths[i as usize] > 0 {
                if let Some(tp) = location.as_int() {
                    let stc = Shape::Circle(Circle::new(tp, widths[i as usize]));
                    let sts = compensate_pad_shape(board, tile_of_shape(board, &stc), trace_clearance_class, i);
                    let ok = if insert {
                        forced_pad(board, &sts, &from_side, i, nets, trace_clearance_class, true, None, max_recursion_depth, max_via_recursion_depth)
                    } else {
                        check_forced_pad(
                            board,
                            &sts,
                            &from_side,
                            i,
                            nets,
                            trace_clearance_class,
                            true,
                            None,
                            max_recursion_depth,
                            max_via_recursion_depth,
                            false,
                            None,
                        ) != CheckDrillResult::NotDrillable
                    };
                    if !ok {
                        board.set_shove_failing_layer(i);
                        return false;
                    }
                }
            }
        }
    }
    true
}
