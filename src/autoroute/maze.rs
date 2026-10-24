//! The maze search (port of `MazeSearchEngine`, `MazeExpansionEngine`,
//! `MazeRipupResolver`, `MazeTraceShover`, `MazeListElement` and `DestinationDistance`).

use super::connection::Connection;
use super::control::AutorouteControl;
use super::engine::{Adjustment, AutorouteEngine, DoorKey, DrillKey, Expandable, PageKey, RoomKey, RoomKind, TargetKey, TRACE_WIDTH_TOLERANCE};
use crate::board::structure::{FixedState, Unit};
use crate::board::{Board, ItemId, ItemKind, TreeObject};
use crate::geometry::basics::JavaRandom;
use crate::geometry::{FloatLine, FloatPoint, IntBox, IntPoint, Line, LineSegment, Point, Polyline, Side, TileShape};
use crate::optimize::shove::{self, CheckDrillResult};
use crate::optimize::ExpansionCostFactor;
use crate::rules::AngleRestriction;
use std::cmp::Ordering;
use std::collections::{BTreeSet, LinkedList};

pub const ALREADY_RIPPED_COSTS: i32 = 1;
const FANOUT_COST_CONSTANT: f64 = 20000.0;

/// `MazeListElement`
#[derive(Clone, Debug)]
pub struct ListElement {
    pub door: Expandable,
    pub section_no_of_door: usize,
    pub backtrack_door: Option<Expandable>,
    pub section_no_of_backtrack_door: usize,
    pub expansion_value: f64,
    pub sorting_value: f64,
    pub next_room: Option<RoomKey>,
    pub shape_entry: FloatLine,
    pub room_ripped: bool,
    pub adjustment: Adjustment,
    pub already_checked: bool,
    pub ripup_cost: i32,
    door_id: i32,
}

impl PartialEq for ListElement {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for ListElement {}
impl PartialOrd for ListElement {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ListElement {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.sorting_value < other.sorting_value {
            return Ordering::Less;
        }
        if self.sorting_value > other.sorting_value {
            return Ordering::Greater;
        }
        if self.expansion_value < other.expansion_value {
            return Ordering::Less;
        }
        if self.expansion_value > other.expansion_value {
            return Ordering::Greater;
        }
        self.door_id
            .cmp(&other.door_id)
            .then(self.section_no_of_door.cmp(&other.section_no_of_door))
    }
}

/// `MazeSearchEngine.Result`
#[derive(Clone, Copy, Debug)]
pub struct MazeResult {
    pub destination_door: Expandable,
    pub section_no_of_door: usize,
}

// ==================================================================== DestinationDistance

#[derive(Clone, Debug)]
pub struct DestinationDistance {
    trace_costs: Vec<ExpansionCostFactor>,
    layer_count: usize,
    active_layer_count: usize,
    min_cheap_via_cost: f64,
    min_component_side_trace_cost: f64,
    max_component_side_trace_cost: f64,
    min_solder_side_trace_cost: f64,
    max_solder_side_trace_cost: f64,
    max_inner_side_trace_cost: f64,
    min_component_inner_trace_cost: f64,
    min_solder_inner_trace_cost: f64,
    min_component_solder_inner_trace_cost: f64,
    min_normal_via_cost: f64,
    component_side_box: IntBox,
    solder_side_box: IntBox,
    inner_side_box: IntBox,
    box_is_empty: bool,
    component_side_box_is_empty: bool,
    solder_side_box_is_empty: bool,
    inner_side_box_is_empty: bool,
}

impl DestinationDistance {
    pub fn new(trace_costs: &[ExpansionCostFactor], layer_active: &[bool], min_normal_via_cost: f64, min_cheap_via_cost: f64) -> Self {
        let layer_count = layer_active.len();
        let active_layer_count = layer_active.iter().filter(|a| **a).count();
        let mut d = DestinationDistance {
            trace_costs: trace_costs.to_vec(),
            layer_count,
            active_layer_count,
            min_cheap_via_cost,
            min_component_side_trace_cost: 0.0,
            max_component_side_trace_cost: 0.0,
            min_solder_side_trace_cost: 0.0,
            max_solder_side_trace_cost: 0.0,
            max_inner_side_trace_cost: 0.0,
            min_component_inner_trace_cost: 0.0,
            min_solder_inner_trace_cost: 0.0,
            min_component_solder_inner_trace_cost: 0.0,
            min_normal_via_cost,
            component_side_box: IntBox::EMPTY,
            solder_side_box: IntBox::EMPTY,
            inner_side_box: IntBox::EMPTY,
            box_is_empty: true,
            component_side_box_is_empty: true,
            solder_side_box_is_empty: true,
            inner_side_box_is_empty: true,
        };
        if layer_active[0] {
            let c = trace_costs[0];
            if c.horizontal < c.vertical {
                d.min_component_side_trace_cost = c.horizontal;
                d.max_component_side_trace_cost = c.vertical;
            } else {
                d.min_component_side_trace_cost = c.vertical;
                d.max_component_side_trace_cost = c.horizontal;
            }
        }
        if layer_active[layer_count - 1] {
            let c = trace_costs[layer_count - 1];
            if c.horizontal < c.vertical {
                d.min_solder_side_trace_cost = c.horizontal;
                d.max_solder_side_trace_cost = c.vertical;
            } else {
                d.min_solder_side_trace_cost = c.vertical;
                d.max_solder_side_trace_cost = c.horizontal;
            }
        }
        d.max_inner_side_trace_cost = d.max_component_side_trace_cost.min(d.max_solder_side_trace_cost);
        for i in 1..layer_count.saturating_sub(1) {
            if !layer_active[i] {
                continue;
            }
            let m = trace_costs[i].horizontal.max(trace_costs[i].vertical);
            d.max_inner_side_trace_cost = d.max_inner_side_trace_cost.min(m);
        }
        d.min_component_inner_trace_cost = d.min_component_side_trace_cost.min(d.max_inner_side_trace_cost);
        d.min_solder_inner_trace_cost = d.min_solder_side_trace_cost.min(d.max_inner_side_trace_cost);
        d.min_component_solder_inner_trace_cost = d.min_component_inner_trace_cost.min(d.min_solder_inner_trace_cost);
        d
    }

    pub fn join(&mut self, b: &IntBox, layer: i32) {
        if layer == 0 {
            self.component_side_box = self.component_side_box.union(b);
            self.component_side_box_is_empty = false;
        } else if layer as usize == self.layer_count - 1 {
            self.solder_side_box = self.solder_side_box.union(b);
            self.solder_side_box_is_empty = false;
        } else {
            self.inner_side_box = self.inner_side_box.union(b);
            self.inner_side_box_is_empty = false;
        }
        self.box_is_empty = false;
    }

    pub fn calculate_point(&self, p: &FloatPoint, layer: i32) -> f64 {
        self.calculate(&p.bounding_box(), layer)
    }

    fn deltas(b: &IntBox, o: &IntBox) -> (f64, f64) {
        let dx = if b.ll.x > o.ur.x {
            (b.ll.x - o.ur.x) as f64
        } else if b.ur.x < o.ll.x {
            (o.ll.x - b.ur.x) as f64
        } else {
            0.0
        };
        let dy = if b.ll.y > o.ur.y {
            (b.ll.y - o.ur.y) as f64
        } else if b.ur.y < o.ll.y {
            (o.ll.y - b.ur.y) as f64
        } else {
            0.0
        };
        if dx > dy {
            (dx, dy)
        } else {
            (dy, dx)
        }
    }

    pub fn calculate(&self, b: &IntBox, layer: i32) -> f64 {
        if self.box_is_empty {
            return i32::MAX as f64;
        }
        let (c_max, c_min) = Self::deltas(b, &self.component_side_box);
        let (s_max, s_min) = Self::deltas(b, &self.solder_side_box);
        let (i_max, i_min) = Self::deltas(b, &self.inner_side_box);
        let via = self.min_normal_via_cost;
        let mut result = i32::MAX as f64;
        let alc = self.active_layer_count;
        if layer == 0 {
            if !self.component_side_box_is_empty {
                result = b.weighted_distance(&self.component_side_box, self.trace_costs[0].horizontal, self.trace_costs[0].vertical);
            }
            if alc <= 1 {
                return result;
            }
            let tmp = if self.min_solder_side_trace_cost < self.min_component_side_trace_cost {
                self.min_solder_side_trace_cost * s_max + self.min_component_side_trace_cost * s_min + via
            } else {
                self.min_component_side_trace_cost * s_max + self.min_solder_side_trace_cost * s_min + via
            };
            result = result.min(tmp);
            result = result.min(c_max + c_min * self.min_component_inner_trace_cost + 2.0 * via);
            if alc == 2 {
                return result;
            }
            result = result.min(i_max + i_min * self.min_component_inner_trace_cost + via);
            result = result.min(s_max + self.min_component_solder_inner_trace_cost * s_min + 2.0 * via);
            result = result.min(c_max + c_min + 2.0 * via);
            if alc == 3 {
                return result;
            }
            result = result.min(i_max + i_min + 2.0 * via);
            return result.min(s_max + s_min + 3.0 * via);
        }
        if layer as usize == self.layer_count - 1 {
            if !self.solder_side_box_is_empty {
                let c = self.trace_costs[layer as usize];
                result = b.weighted_distance(&self.solder_side_box, c.horizontal, c.vertical);
            }
            let tmp = if self.min_component_side_trace_cost < self.min_solder_side_trace_cost {
                self.min_component_side_trace_cost * c_max + self.min_solder_side_trace_cost * c_min + via
            } else {
                self.min_solder_side_trace_cost * c_max + self.min_component_side_trace_cost * c_min + via
            };
            result = result.min(tmp);
            result = result.min(s_max + s_min * self.min_solder_inner_trace_cost + 2.0 * via);
            if alc <= 2 {
                return result;
            }
            result = result.min(i_min * self.min_solder_inner_trace_cost + i_max + via);
            result = result.min(c_max + self.min_component_solder_inner_trace_cost * c_min + 2.0 * via);
            result = result.min(s_max + s_min + 2.0 * via);
            if alc == 3 {
                return result;
            }
            result = result.min(i_max + i_min + 2.0 * via);
            return result.min(c_max + c_min + 3.0 * via);
        }
        if !self.inner_side_box_is_empty {
            let c = self.trace_costs[layer as usize];
            result = b.weighted_distance(&self.inner_side_box, c.horizontal, c.vertical);
        }
        result = result.min(i_max + i_min + via);
        result = result.min(c_max + c_min * self.min_component_inner_trace_cost + via);
        result = result.min(s_max + s_min * self.min_solder_inner_trace_cost + via);
        result = result.min(c_max + c_min + 2.0 * via);
        result.min(s_max + s_min + 2.0 * via)
    }

    pub fn calculate_cheap_distance(&mut self, b: &IntBox, layer: i32) -> f64 {
        let save = self.min_normal_via_cost;
        self.min_normal_via_cost = self.min_cheap_via_cost;
        let r = self.calculate(b, layer);
        self.min_normal_via_cost = save;
        r
    }
}

// ==================================================================== MazeSearchEngine

pub struct MazeSearch<'a> {
    pub ctrl: &'a AutorouteControl,
    list: BTreeSet<ListElement>,
    pub destination_distance: DestinationDistance,
    rng: JavaRandom,
    destination: Option<(Expandable, usize)>,
}

struct DoorSection {
    door: DoorKey,
    section_index: usize,
    section_line: FloatLine,
}

#[inline]
fn middle(l: &FloatLine) -> FloatPoint {
    l.a.middle_point(&l.b)
}

impl<'a> MazeSearch<'a> {
    /// `MazeSearchEngine.getInstance`: None, if the initialisation failed.
    pub fn get_instance(
        eng: &mut AutorouteEngine,
        board: &mut Board,
        ctrl: &'a AutorouteControl,
        start_items: &BTreeSet<ItemId>,
        destination_items: &BTreeSet<ItemId>,
    ) -> Option<MazeSearch<'a>> {
        let mut s = MazeSearch {
            ctrl,
            list: BTreeSet::new(),
            destination_distance: DestinationDistance::new(&ctrl.trace_costs, &ctrl.layer_active, ctrl.min_normal_via_cost, ctrl.min_cheap_via_cost),
            rng: JavaRandom::new(ctrl.ripup_costs as i64),
            destination: None,
        };
        if s.init(eng, board, start_items, destination_items) {
            Some(s)
        } else {
            None
        }
    }

    fn add(&mut self, eng: &AutorouteEngine, board: &Board, mut e: ListElement) {
        let ctrl = self.ctrl;
        if ctrl.is_fanout && !ctrl.ignore_escape_window {
            if let Some(pc) = &ctrl.fanout_start_pin_center {
                let pin_center = pc.to_float();
                let on_start_layer = e.next_room.map(|r| eng.room(r).layer == ctrl.fanout_start_pin_layer).unwrap_or(false);
                let resolution = board.communication.get_resolution(Unit::Um);
                if on_start_layer {
                    let max_len = ctrl.settings.fanout.max_escape_length_mm * 1000.0;
                    let entry = middle(&e.shape_entry);
                    if entry.distance(&pin_center) > max_len * resolution {
                        return;
                    }
                }
                if let Expandable::Drill(d) = e.door {
                    let min_len = ctrl.settings.fanout.min_escape_length_mm * 1000.0;
                    let drill_dist = eng.drill(d).location.to_float().distance(&pin_center);
                    if drill_dist < min_len * resolution {
                        return;
                    }
                }
            }
        }
        e.door_id = eng.expandable_id(e.door);
        if crate::board::snapshot::evlog::maze_log_enabled() && std::env::var_os("RR_PUSHLOG").is_some() {
            crate::board::snapshot::evlog::line(&format!(
                "PUSH {} s{} e{:.3} v{:.3}",
                e.door_id, e.section_no_of_door, e.expansion_value, e.sorting_value
            ));
        }
        self.list.insert(e);
    }

    #[allow(clippy::too_many_arguments)]
    fn new_element(
        door: Expandable,
        section_no_of_door: usize,
        backtrack_door: Option<Expandable>,
        section_no_of_backtrack_door: usize,
        expansion_value: f64,
        sorting_value: f64,
        next_room: Option<RoomKey>,
        shape_entry: FloatLine,
        room_ripped: bool,
        adjustment: Adjustment,
        already_checked: bool,
    ) -> ListElement {
        ListElement {
            door,
            section_no_of_door,
            backtrack_door,
            section_no_of_backtrack_door,
            expansion_value,
            sorting_value,
            next_room,
            shape_entry,
            room_ripped,
            adjustment,
            already_checked,
            ripup_cost: 0,
            door_id: 0,
        }
    }

    pub fn find_connection(&mut self, eng: &mut AutorouteEngine, board: &mut Board) -> Option<MazeResult> {
        while self.occupy_next_element(eng, board) {}
        self.destination.map(|(d, s)| MazeResult {
            destination_door: d,
            section_no_of_door: s,
        })
    }

    pub fn occupy_next_element(&mut self, eng: &mut AutorouteEngine, board: &mut Board) -> bool {
        if self.destination.is_some() {
            return false;
        }
        let mut found: Option<ListElement> = None;
        while let Some(e) = self.list.pop_first() {
            if eng.is_stop_requested() {
                return false;
            }
            if !eng.element(e.door, e.section_no_of_door).is_occupied {
                found = Some(e);
                break;
            }
        }
        let Some(e) = found else {
            return false;
        };
        if crate::board::snapshot::evlog::maze_log_enabled() {
            let kind = match e.door {
                Expandable::Door(_) => "D",
                Expandable::Target(_) => "T",
                Expandable::Drill(_) => "X",
                Expandable::Page(_) => "P",
            };
            let room = match e.next_room {
                None => "-".to_string(),
                Some(r) => {
                    let b = eng.room(r).shape().bounding_box();
                    format!("{}:{},{}", eng.room(r).layer, b.ll.x, b.ll.y)
                }
            };
            let mut rooms = String::new();
            if let Expandable::Door(d) = e.door {
                let desc = |r: RoomKey| {
                    let rm = eng.room(r);
                    match rm.kind {
                        RoomKind::Obstacle { item, index, .. } => format!("O{}.{}{}", item.0, index, rm.get_id()),
                        RoomKind::Complete { .. } => format!("C{}", rm.get_id()),
                        RoomKind::Incomplete { .. } => format!("I{}", rm.get_id()),
                    }
                };
                rooms = format!(" [{}|{}]", desc(eng.door(d).first), desc(eng.door(d).second));
            }
            crate::board::snapshot::evlog::line(&rooms);
            crate::board::snapshot::evlog::line(&format!(
                "POP {}{} s{} e{:.3} v{:.3} r{}",
                kind,
                eng.expandable_id(e.door),
                e.section_no_of_door,
                e.expansion_value,
                e.sorting_value,
                room
            ));
        }
        {
            let s = eng.element_mut(e.door, e.section_no_of_door);
            s.backtrack_door = e.backtrack_door;
            s.section_no_of_backtrack_door = e.section_no_of_backtrack_door;
            s.room_ripped = e.room_ripped;
            s.ripup_cost = e.ripup_cost;
            s.adjustment = e.adjustment;
        }
        if let Expandable::Page(_) = e.door {
            self.expand_to_drills_of_page(eng, board, &e);
            return true;
        }
        if let Expandable::Target(t) = e.door {
            if eng.is_destination_door(t) {
                self.destination = Some((e.door, e.section_no_of_door));
                return false;
            }
        }
        if self.ctrl.is_fanout && matches!(e.door, Expandable::Drill(_)) && matches!(e.backtrack_door, Some(Expandable::Drill(_))) {
            self.destination = Some((e.door, e.section_no_of_door));
            return false;
        }
        if self.ctrl.vias_allowed && matches!(e.door, Expandable::Drill(_)) && !matches!(e.backtrack_door, Some(Expandable::Drill(_))) {
            self.expand_to_other_layers(eng, board, &e);
        }
        if e.next_room.is_some() && !self.expand_to_room_doors(eng, board, &e) {
            return true;
        }
        eng.element_mut(e.door, e.section_no_of_door).is_occupied = true;
        true
    }

    fn expand_to_room_doors(&mut self, eng: &mut AutorouteEngine, board: &mut Board, e: &ListElement) -> bool {
        let ctrl = self.ctrl;
        let next_room = e.next_room.unwrap();
        let layer = eng.room(next_room).layer;
        let layer_active = ctrl.layer_active[layer as usize];
        if !layer_active && board.layer_structure.layers[layer as usize].is_signal {
            return true;
        }
        let mut half_width = ctrl.compensated_trace_half_width[layer as usize] as f64;
        let mut current_door_is_small = false;
        if let Expandable::Door(d) = e.door {
            let mut half_width_add = half_width + TRACE_WIDTH_TOLERANCE as f64;
            if ctrl.with_neckdown {
                let neck = self.check_neck_down_at_dest_pin(eng, board, next_room);
                if neck > 0.0 {
                    half_width_add = half_width_add.min(neck);
                    half_width = half_width_add;
                }
            }
            current_door_is_small = self.door_is_small(eng, board, d, 2.0 * half_width_add);
        }
        eng.complete_neighbour_rooms(board, next_room);
        let shape_entry_middle = middle(&e.shape_entry);
        if ctrl.with_neckdown {
            if let Expandable::Target(t) = e.door {
                let item = eng.target(t).item;
                if board.item(item).is_pin() {
                    let n = board.pin_trace_neckdown_halfwidth(item, layer) as f64;
                    if n > 0.0 {
                        half_width = half_width.min(n);
                    }
                }
            }
        }
        let next_room_is_thick = if eng.room(next_room).is_obstacle() {
            self.room_shape_is_thick(eng, board, next_room)
        } else {
            let shape = eng.room(next_room).shape();
            if shape.min_width() < 2.0 * half_width {
                false
            } else if !e.already_checked && eng.expandable_dimension(e.door) == 1 && !current_door_is_small {
                let nearest = shape.nearest_border_points_approx(&shape_entry_middle, 2);
                if nearest.len() < 2 {
                    log::warn!("MazeSearchEngine.expand_to_room_doors: nearestPoints.length == 2 expected");
                    false
                } else {
                    nearest[1].distance(&shape_entry_middle) > half_width + 1.0
                }
            } else {
                true
            }
        };
        if !layer_active {
            if let Expandable::Drill(dk) = e.door {
                let loc = eng.drill(dk).location.clone();
                for id in board.pick_items(&loc, layer) {
                    let it = board.item(id);
                    if it.is_conduction_area() && !it.contains_net(ctrl.net_no) {
                        return true;
                    }
                }
            }
        }
        let mut something_expanded = self.expand_to_target_doors(eng, board, e, next_room_is_thick, current_door_is_small, &shape_entry_middle);
        if !layer_active {
            return true;
        }
        let mut ripup_costs = 0;
        match eng.room(next_room).kind {
            RoomKind::Complete { .. } | RoomKind::Incomplete { .. } => {
                if !e.already_checked && current_door_is_small {
                    let mut enter_through_small_door = false;
                    if next_room_is_thick {
                        enter_through_small_door = self.check_leaving_ripped_item(eng, board, e);
                    }
                    if !enter_through_small_door {
                        return something_expanded;
                    }
                }
            }
            RoomKind::Obstacle { item: obstacle_item, .. } => {
                if !e.already_checked {
                    let mut room_rippable = false;
                    if ctrl.ripup_allowed {
                        ripup_costs = self.check_ripup(eng, board, e, obstacle_item, current_door_is_small);
                        room_rippable = ripup_costs >= 0;
                    }
                    if ripup_costs != ALREADY_RIPPED_COSTS && next_room_is_thick {
                        if !current_door_is_small && ctrl.max_shove_trace_recursion_depth > 0 && board.item(obstacle_item).is_trace() {
                            let shoved = self.shove_trace_room(eng, board, e, next_room);
                            if !shoved {
                                if ripup_costs > 0 {
                                    let mut ne = Self::new_element(
                                        e.door,
                                        e.section_no_of_door,
                                        e.backtrack_door,
                                        e.section_no_of_backtrack_door,
                                        e.expansion_value + ripup_costs as f64,
                                        e.sorting_value + ripup_costs as f64,
                                        e.next_room,
                                        e.shape_entry,
                                        true,
                                        e.adjustment,
                                        true,
                                    );
                                    ne.ripup_cost = ripup_costs;
                                    self.add(eng, board, ne);
                                }
                                return something_expanded;
                            }
                        }
                    }
                    if !room_rippable {
                        return true;
                    }
                }
            }
        }
        let doors: Vec<DoorKey> = eng.room(next_room).doors.clone();
        for d in doors {
            if Expandable::Door(d) == e.door {
                continue;
            }
            if self.expand_to_door(eng, board, d, e, ripup_costs, next_room_is_thick, Adjustment::None) {
                something_expanded = true;
            }
        }
        if ctrl.vias_allowed && !matches!(e.door, Expandable::Drill(_)) {
            if (something_expanded || next_room_is_thick) && eng.room(next_room).is_complete_free() {
                let pages = eng.drill_pages.overlapping_pages(eng.room(next_room).shape());
                for p in pages {
                    self.expand_to_drill_page(eng, board, p, e);
                    something_expanded = true;
                }
            } else if let Some(obstacle_item) = eng.room(next_room).obstacle_item() {
                if board.item(obstacle_item).is_via() {
                    let drill = eng.via_drill_info(board, obstacle_item);
                    self.expand_to_drill(eng, board, drill, e, ripup_costs);
                }
            }
        }
        something_expanded
    }

    fn expand_to_target_doors(
        &mut self,
        eng: &mut AutorouteEngine,
        board: &mut Board,
        e: &ListElement,
        next_room_is_thick: bool,
        current_door_is_small: bool,
        shape_entry_middle: &FloatPoint,
    ) -> bool {
        let next_room = e.next_room.unwrap();
        if current_door_is_small {
            let mut enter = false;
            if let Expandable::Door(_) = e.door {
                if let Some(from_room) = eng.expandable_other_room(e.door, Some(next_room)) {
                    if eng.room(from_room).is_obstacle() {
                        enter = true;
                    }
                }
            }
            if !enter {
                return false;
            }
        }
        let mut result = false;
        let targets: Vec<TargetKey> = eng.room(next_room).target_doors().to_vec();
        for t in targets {
            if let Some(r) = eng.target(t).room {
                if !self.ctrl.layer_active[eng.room(r).layer as usize] {
                    continue;
                }
            }
            if Expandable::Target(t) == e.door {
                continue;
            }
            let Some(safe) = self.get_width_safe_target_shape(eng, board, t) else {
                continue;
            };
            if safe.is_empty() {
                continue;
            }
            let Some(connection_point) = safe.nearest_point_approx(shape_entry_middle) else {
                continue;
            };
            if !next_room_is_thick {
                let nets = [self.ctrl.net_no];
                let layer = eng.room(next_room).layer;
                let p0 = shape_entry_middle.round();
                let p1 = connection_point.round();
                if p0 != p1 {
                    let pl = Polyline::from_int_points(&[p0, p1]);
                    let ok = board.check_forced_trace_polyline(
                        &pl,
                        self.ctrl.trace_half_width[layer as usize],
                        layer,
                        &nets,
                        self.ctrl.trace_clearance_class,
                        self.ctrl.max_shove_trace_recursion_depth,
                        self.ctrl.max_shove_via_recursion_depth,
                        self.ctrl.max_spring_over_recursion_depth,
                    );
                    if !ok {
                        continue;
                    }
                }
            }
            let entry = FloatLine::new(connection_point, connection_point);
            if self.expand_to_door_section(eng, board, Expandable::Target(t), 0, Some(entry), e, 0, Adjustment::None) {
                result = true;
            }
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_to_door(
        &mut self,
        eng: &mut AutorouteEngine,
        board: &Board,
        to_door: DoorKey,
        e: &ListElement,
        add_costs: i32,
        next_room_is_thick: bool,
        adjustment: Adjustment,
    ) -> bool {
        let next_room = e.next_room.unwrap();
        let half_width = self.ctrl.compensated_trace_half_width[eng.room(next_room).layer as usize] as f64;
        let mut something_expanded = false;
        let sections = eng.door_section_segments(to_door, half_width);
        let (dim, first, second) = {
            let d = eng.door(to_door);
            (d.dimension, d.first, d.second)
        };
        for (i, section) in sections.iter().enumerate() {
            if eng.door(to_door).sections[i].is_occupied {
                continue;
            }
            let new_entry;
            if next_room_is_thick {
                new_entry = Some(*section);
                if dim == 1 && sections.len() == 1 && eng.room(first).is_complete_free() && eng.room(second).is_complete_free() {
                    let m = middle(section);
                    let room_shape = eng.room(next_room).shape();
                    if room_shape.min_width() < 2.0 * half_width {
                        return false;
                    }
                    let nearest = room_shape.nearest_border_points_approx(&m, 2);
                    if nearest.len() < 2 || nearest[1].distance(&m) <= half_width + 1.0 {
                        return false;
                    }
                }
            } else {
                if dim == 1 && i == 0 && sections[0].b.distance_square(&sections[0].a) < 1.0 {
                    continue;
                }
                new_entry = segment_projection(&e.shape_entry, section);
                if new_entry.is_none() {
                    continue;
                }
            }
            if self.expand_to_door_section(eng, board, Expandable::Door(to_door), i, new_entry, e, add_costs, adjustment) {
                something_expanded = true;
            }
        }
        something_expanded
    }

    fn door_is_small(&self, eng: &AutorouteEngine, board: &Board, door: DoorKey, trace_width: f64) -> bool {
        let d = eng.door(door);
        if d.dimension == 1 || (eng.room(d.first).is_complete_free() && eng.room(d.second).is_complete_free()) {
            let shape = eng.door_shape(door);
            if shape.is_empty() {
                return true;
            }
            let door_length = match board.rules.get_trace_angle_restriction() {
                AngleRestriction::NinetyDegree => shape.bounding_box().max_width(),
                AngleRestriction::FortyfiveDegree => shape.bounding_octagon().max_width(),
                AngleRestriction::None => match shape.diagonal_corner_segment() {
                    Some(s) => s.b.distance(&s.a),
                    None => 0.0,
                },
            };
            return door_length < trace_width;
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_to_door_section(
        &mut self,
        eng: &mut AutorouteEngine,
        board: &Board,
        door: Expandable,
        section_index: usize,
        shape_entry: Option<FloatLine>,
        from: &ListElement,
        add_costs: i32,
        adjustment: Adjustment,
    ) -> bool {
        let occupied = eng.element(door, section_index).is_occupied;
        let Some(shape_entry) = shape_entry.filter(|_| !occupied) else {
            return false;
        };
        let next_room = eng.expandable_other_room(door, from.next_room);
        let layer = eng.room(from.next_room.unwrap()).layer as usize;
        let shape_entry_middle = middle(&shape_entry);
        let ctrl = self.ctrl;
        let mut bend_cost_penalty = 0.0;
        if ctrl.bend_costs[layer] > 0.0 {
            if let Some(bt) = from.backtrack_door {
                let from_mid = middle(&from.shape_entry);
                let backtrack_cog = eng.expandable_shape(bt).centre_of_gravity();
                let prev_dx = from_mid.x - backtrack_cog.x;
                let prev_dy = from_mid.y - backtrack_cog.y;
                let next_dx = shape_entry_middle.x - from_mid.x;
                let next_dy = shape_entry_middle.y - from_mid.y;
                let cross = prev_dx * next_dy - prev_dy * next_dx;
                let sq_prev = prev_dx * prev_dx + prev_dy * prev_dy;
                let sq_next = next_dx * next_dx + next_dy * next_dy;
                if sq_prev > 0.0 && sq_next > 0.0 && cross * cross > 0.01 * sq_prev * sq_next {
                    bend_cost_penalty = ctrl.bend_costs[layer];
                }
            }
        }
        let expansion_value = from.expansion_value
            + add_costs as f64
            + bend_cost_penalty
            + shape_entry_middle.weighted_distance(&middle(&from.shape_entry), ctrl.trace_costs[layer].horizontal, ctrl.trace_costs[layer].vertical);
        let sorting_value = expansion_value + self.destination_distance.calculate_point(&shape_entry_middle, layer as i32);
        let room_ripped = (add_costs > 0 && adjustment == Adjustment::None) || (from.already_checked && from.room_ripped);
        let mut ne = Self::new_element(
            door,
            section_index,
            Some(from.door),
            from.section_no_of_door,
            expansion_value,
            sorting_value,
            next_room,
            shape_entry,
            room_ripped,
            adjustment,
            false,
        );
        if add_costs > 0 && adjustment == Adjustment::None {
            ne.ripup_cost = add_costs;
        }
        self.add(eng, board, ne);
        true
    }

    fn init(&mut self, eng: &mut AutorouteEngine, board: &mut Board, start_items: &BTreeSet<ItemId>, destination_items: &BTreeSet<ItemId>) -> bool {
        reduce_trace_shapes_at_tie_pins(eng, board, start_items, self.ctrl.net_no);
        reduce_trace_shapes_at_tie_pins(eng, board, destination_items, self.ctrl.net_no);
        let mut destination_ok = false;
        for id in destination_items {
            if eng.is_stop_requested() {
                return false;
            }
            eng.item_info_mut(*id).start_info = false;
            let mut any_inactive = false;
            let mut any_active = false;
            let count = super::engine::item_tree_shape_count(board, eng.tree, *id);
            for i in 0..count {
                let layer = super::engine::item_shape_layer(board, eng.tree, *id, i);
                if let Some(s) = board.tree_shape(eng.tree, *id, i) {
                    self.destination_distance.join(&s.bounding_box(), layer);
                    if crate::board::snapshot::evlog::maze_log_enabled() {
                        let bb = s.bounding_box();
                        crate::board::snapshot::evlog::line(&format!(
                            "DEST item={} i={} layer={} box={},{},{},{}",
                            id.0, i, layer, bb.ll.x, bb.ll.y, bb.ur.x, bb.ur.y
                        ));
                    }
                }
                if self.ctrl.layer_active[layer as usize] {
                    any_active = true;
                } else {
                    any_inactive = true;
                }
            }
            if any_active || !any_inactive {
                destination_ok = true;
            }
        }
        if !destination_ok && self.ctrl.is_fanout {
            let bb = board.bounding_box;
            self.destination_distance.join(&bb, 0);
            self.destination_distance.join(&bb, self.ctrl.layer_count as i32 - 1);
            destination_ok = true;
        }
        if !destination_ok {
            log::debug!(
                "MazeSearchEngine.init: Failed - no valid destination items found (dest set size: {}, isFanout: {})",
                destination_items.len(),
                self.ctrl.is_fanout
            );
            return false;
        }
        let mut start_rooms = Vec::new();
        for id in start_items {
            if eng.is_stop_requested() {
                return false;
            }
            eng.item_info_mut(*id).start_info = true;
            if board.any_item(*id).map(|i| i.is_connectable_type()).unwrap_or(false) {
                let count = super::engine::item_tree_shape_count(board, eng.tree, *id);
                for i in 0..count {
                    let contained = board.trace_connection_shape(eng.tree, *id, i);
                    let layer = super::engine::item_shape_layer(board, eng.tree, *id, i);
                    start_rooms.push(eng.add_incomplete_expansion_room(None, layer, contained));
                }
            }
        }
        let mut completed_start_rooms: Vec<RoomKey> = Vec::new();
        if eng.maintain_database {
            completed_start_rooms.extend(eng.get_rooms_with_target_items(start_items));
        }
        for r in start_rooms {
            if eng.is_stop_requested() {
                return false;
            }
            completed_start_rooms.extend(eng.complete_expansion_room(board, r));
        }
        let mut start_ok = false;
        let mut doors_found = 0;
        let mut doors_destination = 0;
        for r in completed_start_rooms {
            for t in eng.room(r).target_doors().to_vec() {
                doors_found += 1;
                if eng.is_stop_requested() {
                    return false;
                }
                if eng.is_destination_door(t) {
                    doors_destination += 1;
                    continue;
                }
                let Some(safe) = self.get_width_safe_target_shape(eng, board, t) else {
                    continue;
                };
                if safe.is_empty() {
                    continue;
                }
                let center = safe.centre_of_gravity();
                let entry = FloatLine::new(center, center);
                let sorting_value = self.destination_distance.calculate_point(&center, eng.room(r).layer);
                let ne = Self::new_element(Expandable::Target(t), 0, None, 0, 0.0, sorting_value, Some(r), entry, false, Adjustment::None, false);
                self.add(eng, board, ne);
                start_ok = true;
            }
        }
        if !start_ok {
            log::debug!(
                "MazeSearchEngine.init: Failed - no accessible expansion doors found (start items: {}, expansion doors found: {}, destination doors: {}, ripupAllowed: {}, ripupCosts: {})",
                start_items.len(),
                doors_found,
                doors_destination,
                self.ctrl.ripup_allowed,
                self.ctrl.ripup_costs
            );
        }
        start_ok
    }

    fn room_shape_is_thick(&self, eng: &AutorouteEngine, board: &Board, room: RoomKey) -> bool {
        let r = eng.room(room);
        let item_id = r.obstacle_item().unwrap();
        let layer = r.layer;
        let item = board.item(item_id);
        let obstacle_half_width = match &item.kind {
            ItemKind::Trace(t) => {
                t.half_width as f64
                    + board
                        .search_trees
                        .get(eng.tree)
                        .clearance_compensation_value(&board.rules, item.clearance_class, layer) as f64
            }
            ItemKind::Via(_) => match board.drill_tree_shape_on_layer(eng.tree, item_id, layer) {
                Some(s) => 0.5 * s.max_width(),
                None => 0.0,
            },
            _ => {
                log::warn!("MazeSearchEngine. room_shape_is_thick: unexpected obstacle item");
                0.0
            }
        };
        obstacle_half_width >= self.ctrl.compensated_trace_half_width[layer as usize] as f64
    }

    fn shove_trace_room(&mut self, eng: &mut AutorouteEngine, board: &mut Board, e: &ListElement, obstacle_room: RoomKey) -> bool {
        if e.section_no_of_door != 0 && e.section_no_of_door != eng.element_count(e.door).wrapping_sub(1) {
            return true;
        }
        let mut result = false;
        if e.adjustment != Adjustment::Right {
            let mut left = Vec::new();
            if check_shove_trace_line(eng, board, self.ctrl, e, obstacle_room, false, &mut left) {
                result = true;
            }
            for ds in left {
                let adj = if eng.door(ds.door).dimension == 2 { Adjustment::Left } else { Adjustment::None };
                self.expand_to_door_section(eng, board, Expandable::Door(ds.door), ds.section_index, Some(ds.section_line), e, 0, adj);
            }
        }
        if e.adjustment != Adjustment::Left {
            let mut right = Vec::new();
            if check_shove_trace_line(eng, board, self.ctrl, e, obstacle_room, true, &mut right) {
                result = true;
            }
            for ds in right {
                let adj = if eng.door(ds.door).dimension == 2 { Adjustment::Right } else { Adjustment::None };
                self.expand_to_door_section(eng, board, Expandable::Door(ds.door), ds.section_index, Some(ds.section_line), e, 0, adj);
            }
        }
        result
    }

    pub fn get_width_safe_target_shape(&self, eng: &AutorouteEngine, board: &Board, t: TargetKey) -> Option<TileShape> {
        let td = eng.target(t);
        let room = td.room?;
        let layer = eng.room(room).layer;
        if layer < 0 || layer as usize >= self.ctrl.layer_count {
            return None;
        }
        let item = board.item(td.item);
        if !item.is_connectable_type() {
            return None;
        }
        let count = board.search_trees.get(eng.tree).object_shape_count(&TreeObject::Item(td.item));
        if td.tree_entry_no >= count {
            return None;
        }
        let target_shape = board.trace_connection_shape(eng.tree, td.item, td.tree_entry_no)?;
        if target_shape.is_empty() {
            return None;
        }
        let room_shape = eng.room(room).shape();
        if item.is_drill_item() {
            let s = target_shape.intersection(room_shape);
            return if s.is_empty() { None } else { Some(s) };
        }
        let shrink = if eng.room(room).is_obstacle() {
            self.ctrl.compensated_trace_half_width[layer as usize]
        } else {
            self.ctrl.get_trace_clearance_offset(layer)
        };
        let safe_room = room_shape.offset(-(shrink as f64));
        if !safe_room.is_empty() {
            let s = target_shape.intersection(&safe_room);
            if !s.is_empty() {
                return Some(s);
            }
        }
        None
    }

    fn check_neck_down_at_dest_pin(&self, eng: &AutorouteEngine, board: &Board, room: RoomKey) -> f64 {
        let layer = eng.room(room).layer;
        for t in eng.room(room).target_doors() {
            let item = eng.target(*t).item;
            if board.item(item).is_pin() {
                return board.pin_trace_neckdown_halfwidth(item, layer) as f64;
            }
        }
        0.0
    }

    // ---------------------------------------------------------------- MazeExpansionEngine

    fn expand_to_drill(&mut self, eng: &mut AutorouteEngine, board: &Board, drill: DrillKey, from: &ListElement, add_costs: i32) {
        let ctrl = self.ctrl;
        let from_room = from.next_room.unwrap();
        let layer = eng.room(from_room).layer;
        let trace_half_width = ctrl.compensated_trace_half_width[layer as usize];
        let room_shape_is_thin = eng.room(from_room).shape().min_width() < 2.0 * trace_half_width as f64;
        if room_shape_is_thin {
            let intersects = match from.backtrack_door {
                None => false,
                Some(bt) => eng.drill(drill).shape.intersects(&eng.expandable_shape(bt)),
            };
            if !intersects {
                return;
            }
        }
        let via_radius = ctrl.via_radii[layer as usize];
        let shrinked = eng.drill(drill).shape.shrink(via_radius);
        let mut compare_corner = middle(&from.shape_entry);
        if let (Expandable::Page(_), Some(Expandable::Target(t))) = (from.door, from.backtrack_door) {
            let item = eng.target(t).item;
            if board.item(item).is_pin() {
                let loc = eng.drill(drill).location.to_float();
                if let Some(c) = board.pin_nearest_trace_exit_corner(item, &loc, trace_half_width, layer) {
                    compare_corner = c;
                }
            }
        }
        let Some(nearest_point) = shrinked.nearest_point_approx(&compare_corner) else {
            return;
        };
        let shape_entry = FloatLine::new(nearest_point, nearest_point);
        let section_index = (layer - eng.drill(drill).first_layer) as usize;
        let mut expansion_value = from.expansion_value
            + add_costs as f64
            + nearest_point.weighted_distance(&compare_corner, ctrl.trace_costs[layer as usize].horizontal, ctrl.trace_costs[layer as usize].vertical);
        let (bt, bts) = if let Expandable::Page(_) = from.door {
            (from.backtrack_door, from.section_no_of_backtrack_door)
        } else {
            expansion_value += ctrl.min_normal_via_cost;
            (Some(from.door), from.section_no_of_door)
        };
        let sorting_value = expansion_value + self.destination_distance.calculate_point(&nearest_point, layer);
        let ne = Self::new_element(
            Expandable::Drill(drill),
            section_index,
            bt,
            bts,
            expansion_value,
            sorting_value,
            None,
            shape_entry,
            from.room_ripped,
            Adjustment::None,
            false,
        );
        self.add(eng, board, ne);
    }

    fn expand_to_drill_page(&mut self, eng: &mut AutorouteEngine, board: &Board, page: PageKey, from: &ListElement) {
        let ctrl = self.ctrl;
        let layer = eng.room(from.next_room.unwrap()).layer;
        let from_middle = middle(&from.shape_entry);
        let nearest_point = eng.drill_pages.pages[page.0 as usize].shape.nearest_point(&from_middle);
        let expansion_value = from.expansion_value + ctrl.min_normal_via_cost;
        let sorting_value = expansion_value
            + nearest_point.weighted_distance(&from_middle, ctrl.trace_costs[layer as usize].horizontal, ctrl.trace_costs[layer as usize].vertical)
            + self.destination_distance.calculate_point(&nearest_point, layer);
        let ne = Self::new_element(
            Expandable::Page(page),
            layer as usize,
            Some(from.door),
            from.section_no_of_door,
            expansion_value,
            sorting_value,
            from.next_room,
            from.shape_entry,
            from.room_ripped,
            Adjustment::None,
            false,
        );
        self.add(eng, board, ne);
    }

    fn expand_to_drills_of_page(&mut self, eng: &mut AutorouteEngine, board: &mut Board, from: &ListElement) {
        let from_room_layer = from.section_no_of_door as i32;
        let Expandable::Page(page) = from.door else {
            return;
        };
        let drills = eng.page_drills(board, page, self.ctrl.attach_smd_allowed);
        for d in drills {
            let section_index = from_room_layer - eng.drill(d).first_layer;
            if section_index < 0 || section_index as usize >= eng.drill(d).rooms.len() {
                continue;
            }
            if eng.drill(d).rooms[section_index as usize] != from.next_room {
                continue;
            }
            if eng.drill(d).elems[section_index as usize].is_occupied {
                continue;
            }
            self.expand_to_drill(eng, board, d, from, 0);
        }
    }

    fn expand_to_other_layers(&mut self, eng: &mut AutorouteEngine, board: &mut Board, e: &ListElement) {
        let ctrl = self.ctrl;
        let via_lower_bound;
        let via_upper_bound;
        let Expandable::Drill(drill) = e.door else {
            return;
        };
        let (first, last) = (eng.drill(drill).first_layer, eng.drill(drill).last_layer);
        let from_layer = first + e.section_no_of_door as i32;
        let mut smd_attached_on_component_side = false;
        let mut smd_attached_on_solder_side = false;
        let room_ripped;
        let section_room = eng.drill(drill).rooms[e.section_no_of_door];
        if let Some(obstacle_item) = section_room.and_then(|r| eng.room(r).obstacle_item()) {
            if !ctrl.ripup_allowed {
                return;
            }
            let item = board.item(obstacle_item);
            let Some(via) = item.as_via() else {
                return;
            };
            let _ = via;
            let ps_id = item.as_drill().unwrap().padstack;
            if !ctrl.via_rule.contains_padstack(ps_id) || item.clearance_class != ctrl.via_clearance_class {
                return;
            }
            let ps = board.library.padstacks.get(ps_id);
            via_lower_bound = ps.from_layer();
            via_upper_bound = ps.to_layer();
            room_ripped = true;
        } else {
            let nets = [ctrl.net_no];
            room_ripped = false;
            let via_lower_limit = first.max(ctrl.via_lower_bound);
            let via_upper_limit = last.min(ctrl.via_upper_bound);
            let mut current_layer = from_layer;
            loop {
                let room_shape = eng.drill(drill).rooms[(current_layer - first) as usize]
                    .map(|r| eng.room(r).shape().clone())
                    .unwrap_or_else(TileShape::empty);
                let r = self.check_layer_with_any_matching_via(eng, board, drill, current_layer, &room_shape, &nets);
                if r == CheckDrillResult::NotDrillable {
                    via_lower_bound = current_layer + 1;
                    break;
                } else if r == CheckDrillResult::DrillableWithAttachSmd {
                    if current_layer == 0 {
                        smd_attached_on_component_side = true;
                    } else if current_layer == ctrl.layer_count as i32 - 1 {
                        smd_attached_on_solder_side = true;
                    }
                }
                if current_layer <= via_lower_limit {
                    via_lower_bound = via_lower_limit;
                    break;
                }
                current_layer -= 1;
            }
            if via_lower_bound > first {
                return;
            }
            current_layer = from_layer + 1;
            loop {
                if current_layer > via_upper_limit {
                    via_upper_bound = via_upper_limit;
                    break;
                }
                let room_shape = eng.drill(drill).rooms[(current_layer - first) as usize]
                    .map(|r| eng.room(r).shape().clone())
                    .unwrap_or_else(TileShape::empty);
                let r = self.check_layer_with_any_matching_via(eng, board, drill, current_layer, &room_shape, &nets);
                if r == CheckDrillResult::NotDrillable {
                    via_upper_bound = current_layer - 1;
                    break;
                } else if r == CheckDrillResult::DrillableWithAttachSmd && current_layer == ctrl.layer_count as i32 - 1 {
                    smd_attached_on_solder_side = true;
                }
                current_layer += 1;
            }
            if via_upper_bound < last {
                return;
            }
        }
        for to_layer in via_lower_bound..=via_upper_bound {
            if to_layer == from_layer {
                continue;
            }
            let (cf, cl) = if to_layer < from_layer { (to_layer, from_layer) } else { (from_layer, to_layer) };
            let mut mask_found = false;
            for m in &ctrl.via_infos {
                if cf >= m.from_layer && cl <= m.to_layer && m.from_layer >= via_lower_bound && m.to_layer <= via_upper_bound {
                    let mask_ok = !((m.from_layer == 0 && smd_attached_on_component_side)
                        || (m.to_layer == ctrl.layer_count as i32 - 1 && smd_attached_on_solder_side))
                        || m.attach_smd_allowed;
                    if mask_ok {
                        mask_found = true;
                        break;
                    }
                }
            }
            if !mask_found {
                continue;
            }
            let idx = (to_layer - first) as usize;
            if eng.drill(drill).elems[idx].is_occupied {
                continue;
            }
            let expansion_value = e.expansion_value + ctrl.add_via_costs[from_layer as usize][to_layer as usize] as f64;
            let m = middle(&e.shape_entry);
            let sorting_value = expansion_value + self.destination_distance.calculate_point(&m, to_layer);
            let next_room = eng.drill(drill).rooms[idx];
            let ne = Self::new_element(
                Expandable::Drill(drill),
                idx,
                Some(Expandable::Drill(drill)),
                e.section_no_of_door,
                expansion_value,
                sorting_value,
                next_room,
                e.shape_entry,
                room_ripped,
                Adjustment::None,
                false,
            );
            self.add(eng, board, ne);
        }
    }

    fn check_layer_with_any_matching_via(
        &self,
        eng: &AutorouteEngine,
        board: &mut Board,
        drill: DrillKey,
        layer: i32,
        room_shape: &TileShape,
        nets: &[i32],
    ) -> CheckDrillResult {
        let ctrl = self.ctrl;
        let mut with_attach_smd = false;
        let location = eng.drill(drill).location.clone();
        for i in 0..ctrl.via_rule.via_count() {
            let v = ctrl.via_rule.get_via(i).clone();
            let ps = board.library.padstacks.get(v.padstack);
            if layer < ps.from_layer() || layer > ps.to_layer() {
                continue;
            }
            let via_radius = ps.get_shape(layer).map(|s| 0.5 * s.max_width()).unwrap_or(0.0);
            let required = via_radius.max(ctrl.trace_half_width[layer as usize] as f64);
            let r = shove::forced_via_check_layer(
                board,
                required,
                v.clearance_class,
                v.attach_smd_allowed,
                room_shape,
                &location,
                layer,
                nets,
                ctrl.max_shove_trace_recursion_depth,
                0,
                ctrl.trace_half_width[layer as usize],
                ctrl.trace_clearance_class,
            );
            if r == CheckDrillResult::Drillable {
                return r;
            }
            if r == CheckDrillResult::DrillableWithAttachSmd {
                with_attach_smd = true;
            }
        }
        if with_attach_smd {
            CheckDrillResult::DrillableWithAttachSmd
        } else {
            CheckDrillResult::NotDrillable
        }
    }

    // ---------------------------------------------------------------- MazeRipupResolver

    fn check_ripup(&mut self, eng: &mut AutorouteEngine, board: &Board, e: &ListElement, obstacle_item: ItemId, door_is_small: bool) -> i32 {
        let ctrl = self.ctrl;
        let item = board.item(obstacle_item);
        if !item.is_routable() {
            return -1;
        }
        if door_is_small && !self.enter_through_small_door(eng, board, e, obstacle_item) {
            return -1;
        }
        let previous_room = eng.expandable_other_room(e.door, e.next_room);
        let room_was_shoved = e.adjustment != Adjustment::None;
        let previous_item = previous_room.and_then(|r| eng.room(r).obstacle_item());
        if room_was_shoved {
            if let Some(pi) = previous_item {
                if pi != obstacle_item && board.item(pi).shares_net(item) {
                    return -1;
                }
            }
        } else if previous_item == Some(obstacle_item) {
            return ALREADY_RIPPED_COSTS;
        }
        let mut fanout_via_cost_factor = 1.0;
        let mut cost_factor = 1.0;
        let preserve_fanout_protection = !ctrl.remove_unconnected_vias && ctrl.ripup_costs <= ctrl.settings.get_start_ripup_costs() * 2;
        match &item.kind {
            ItemKind::Trace(t) => {
                cost_factor = t.half_width as f64;
                if preserve_fanout_protection {
                    fanout_via_cost_factor = calc_fanout_via_ripup_cost_factor(board, obstacle_item);
                }
            }
            ItemKind::Via(_) => {
                let mut look_if_fanout_via = preserve_fanout_protection;
                let contacts = board.normal_contacts(obstacle_item);
                let mut contact_count = 0;
                for c in contacts {
                    let ci = board.item(c);
                    let Some(ct) = ci.as_trace() else {
                        return -1;
                    };
                    if ci.is_user_fixed() {
                        return -1;
                    }
                    contact_count += 1;
                    cost_factor = cost_factor.max(ct.half_width as f64);
                    if look_if_fanout_via && !ctrl.is_fanout {
                        let f = calc_fanout_via_ripup_cost_factor(board, c);
                        if f > 1.0 {
                            fanout_via_cost_factor = f;
                            look_if_fanout_via = false;
                        }
                    }
                }
                if fanout_via_cost_factor <= 1.0 {
                    cost_factor *= 0.5 * (contact_count - 1).max(0) as f64;
                }
            }
            _ => {}
        }
        let mut ripup_cost = ctrl.ripup_costs as f64 * cost_factor;
        let mut detour = 1.0;
        let mut dbg_len = 0.0;
        let mut dbg_n = 0;
        let mut dbg_c = String::from("-");
        if fanout_via_cost_factor <= 1.0 && !ctrl.is_fanout {
            if let Some(c) = Connection::get(eng, board, obstacle_item) {
                detour = c.get_detour(board);
                dbg_len = c.trace_length(board);
                dbg_n = c.item_list.len();
                dbg_c = format!("{:?}/{} {:?}/{}", c.start_point, c.start_layer, c.end_point, c.end_layer);
            }
        }
        let randomize = ctrl.ripup_pass_no >= 4 && ctrl.ripup_pass_no % 3 != 0;
        if randomize {
            let r = self.rng.next_double();
            detour *= 0.5 + r * r;
        }
        ripup_cost /= detour;
        ripup_cost *= fanout_via_cost_factor;
        let mut result = (ripup_cost as i32).max(1);
        result = result.min(i32::MAX / 100);
        if crate::board::snapshot::evlog::maze_log_enabled() && std::env::var_os("RR_PUSHLOG").is_some() {
            crate::board::snapshot::evlog::line(&format!(
                "RIPUP item={} cf={:.3} fv={:.3} det={:.6} len={:.3} n={} res={} {}",
                obstacle_item.0, cost_factor, fanout_via_cost_factor, detour, dbg_len, dbg_n, result, dbg_c
            ));
        }
        result
    }

    fn check_leaving_ripped_item(&self, eng: &AutorouteEngine, board: &Board, e: &ListElement) -> bool {
        let Expandable::Door(_) = e.door else {
            return false;
        };
        let Some(from_room) = eng.expandable_other_room(e.door, e.next_room) else {
            return false;
        };
        let Some(item) = eng.room(from_room).obstacle_item() else {
            return false;
        };
        if !board.item(item).is_routable() {
            return false;
        }
        self.enter_through_small_door(eng, board, e, item)
    }

    fn enter_through_small_door(&self, eng: &AutorouteEngine, board: &Board, e: &ListElement, ignore_item: ItemId) -> bool {
        if eng.expandable_dimension(e.door) != 1 {
            return false;
        }
        let door_shape = eng.expandable_shape(e.door);
        let mut door_line: Option<Line> = None;
        let n = door_shape.border_line_count();
        if n == 0 {
            return false;
        }
        let mut prev = door_shape.corner_approx(0);
        for i in 1..n {
            let next = door_shape.corner_approx(i);
            if next.distance_square(&prev) > 1.0 {
                door_line = Some(door_shape.border_line(i - 1));
                break;
            }
            prev = next;
        }
        let Some(door_line) = door_line else {
            return false;
        };
        let door_center: IntPoint = door_shape.centre_of_gravity().round();
        let layer = eng.room(e.next_room.unwrap()).layer;
        let check_radius = self.ctrl.compensated_trace_half_width[layer as usize] + TRACE_WIDTH_TOLERANCE;
        let lines = [
            door_line.translate(check_radius as f64),
            Line::from_point_dir(door_center, &door_line.direction().turn_45_degree(2)),
            door_line.translate(-(check_radius as f64)),
        ];
        let check_polyline = Polyline::from_lines(&lines);
        let Some(check_shape) = check_polyline.offset_shape(check_radius, 0) else {
            return true;
        };
        let mut entries = Vec::new();
        board
            .search_trees
            .get(eng.tree)
            .overlapping_tree_entries(&check_shape, layer, &[self.ctrl.net_no], &board.lookup(), &mut entries);
        let mut objects: Vec<TreeObject> = entries.into_iter().map(|e| e.object).collect();
        objects.sort();
        objects.dedup();
        let ignore = board.item(ignore_item);
        for o in objects {
            let TreeObject::Item(id) = o else {
                continue;
            };
            if id == ignore_item {
                continue;
            }
            if !board.item(id).shares_net(ignore) {
                return false;
            }
            if !board.normal_contacts(id).contains(&ignore_item) {
                return false;
            }
        }
        true
    }
}

fn calc_fanout_via_ripup_cost_factor(board: &Board, trace: ItemId) -> f64 {
    let lc = board.layer_count();
    for i in 0..2 {
        let contacts = if i == 0 {
            board.trace_start_contacts(trace)
        } else {
            board.trace_end_contacts(trace)
        };
        if contacts.len() != 1 {
            continue;
        }
        let c = *contacts.iter().next().unwrap();
        let ci = board.item(c);
        let mut protect = false;
        if ci.is_pin() && ci.first_layer() == ci.last_layer_with(lc) {
            protect = true;
        } else if let Some(ct) = ci.as_trace() {
            if ci.fixed_state == FixedState::ShoveFixed && ct.corner_count() == 2 {
                protect = true;
            }
        }
        if protect {
            let t = board.item(trace).as_trace().unwrap();
            let mut f = t.half_width as f64 / t.get_length();
            f *= f;
            f *= FANOUT_COST_CONSTANT;
            return f.max(1.0);
        }
    }
    1.0
}

/// `MazeSearchEngine.segmentProjection`
fn segment_projection(from: &FloatLine, to: &FloatLine) -> Option<FloatLine> {
    let check = from.adjust_direction(to);
    let first = to.segment_projection(&check);
    let second = to.segment_projection_2(&check);
    match (first, second) {
        (Some(f), Some(s)) => {
            // The original compares identities: a projection end point is the
            // segment end point itself, if it was not moved.
            let a = if f.a == to.a || s.a == to.a {
                to.a
            } else if f.a.distance_square(&to.a) <= s.a.distance_square(&to.a) {
                f.a
            } else {
                s.a
            };
            let b = if f.b == to.b || s.b == to.b {
                to.b
            } else if f.b.distance_square(&to.b) <= s.b.distance_square(&to.b) {
                f.b
            } else {
                s.b
            };
            Some(FloatLine::new(a, b))
        }
        (Some(f), None) => Some(f),
        (None, s) => s,
    }
}

/// `MazeSearchEngine.reduceTraceShapesAtTiePins`
fn reduce_trace_shapes_at_tie_pins(eng: &AutorouteEngine, board: &mut Board, items: &BTreeSet<ItemId>, own_net_no: i32) {
    for id in items {
        let Some(item) = board.try_item(*id) else {
            continue;
        };
        if !(item.is_pin() && item.net_count() > 1) {
            continue;
        }
        let contacts = board.normal_contacts(*id);
        for c in contacts {
            let ci = board.item(c);
            if !ci.is_trace() || ci.contains_net(own_net_no) {
                continue;
            }
            reduce_trace_shape_at_tie_pin(eng, board, *id, c);
        }
    }
}

/// `ShapeSearchTree.reduceTraceShapeAtTiePin`
fn reduce_trace_shape_at_tie_pin(eng: &AutorouteEngine, board: &mut Board, tie_pin: ItemId, trace: ItemId) {
    let t = board.item(trace).as_trace().unwrap().clone();
    let Some(pin_shape) = board.drill_tree_shape_on_layer(eng.tree, tie_pin, t.layer) else {
        return;
    };
    let center = board.item(tie_pin).as_drill().unwrap().center.clone();
    let (shape_no, compare_corner) = if t.first_corner() == center {
        (0, t.polyline.corner_approx(1))
    } else if t.last_corner() == center {
        (t.corner_count() - 2, t.polyline.corner_approx(t.corner_count() - 2))
    } else {
        return;
    };
    let Some(trace_shape) = board.tree_shape(eng.tree, trace, shape_no) else {
        return;
    };
    let intersection = trace_shape.intersection(&pin_shape);
    if intersection.dimension() < 2 {
        return;
    }
    let pieces = trace_shape.cutout(&pin_shape);
    let mut new_shape: Option<TileShape> = None;
    for p in pieces {
        if p.dimension() == 2 && (new_shape.is_none() || p.contains_float(&compare_corner)) {
            new_shape = Some(p);
        }
    }
    board
        .search_trees
        .get_mut(eng.tree)
        .change_item_shape(TreeObject::Item(trace), shape_no, new_shape.unwrap_or_else(TileShape::empty));
}

/// `MazeTraceShover.checkShoveTraceLine` (with the debug event log).
#[allow(clippy::too_many_arguments)]
fn check_shove_trace_line(
    eng: &mut AutorouteEngine,
    board: &mut Board,
    ctrl: &AutorouteControl,
    e: &ListElement,
    obstacle_room: RoomKey,
    shove_to_the_left: bool,
    to_door_list: &mut Vec<DoorSection>,
) -> bool {
    let r = check_shove_trace_line0(eng, board, ctrl, e, obstacle_room, shove_to_the_left, to_door_list);
    if crate::board::snapshot::evlog::maze_log_enabled() {
        let room = eng.room(obstacle_room);
        crate::board::snapshot::evlog::line(&format!(
            "SHOVELINE item={} idx={} left={} sec={} entry={},{} -> {} doors={}",
            room.obstacle_item().unwrap().0,
            room.obstacle_index(),
            shove_to_the_left,
            e.section_no_of_door,
            crate::geometry::basics::java_round(e.shape_entry.a.x),
            crate::geometry::basics::java_round(e.shape_entry.a.y),
            r,
            to_door_list.len()
        ));
    }
    r
}

#[allow(clippy::too_many_arguments)]
fn check_shove_trace_line0(
    eng: &mut AutorouteEngine,
    board: &mut Board,
    ctrl: &AutorouteControl,
    e: &ListElement,
    obstacle_room: RoomKey,
    shove_to_the_left: bool,
    to_door_list: &mut Vec<DoorSection>,
) -> bool {
    let Expandable::Door(from_door) = e.door else {
        return true;
    };
    let obstacle_item = eng.room(obstacle_room).obstacle_item().unwrap();
    let Some(obstacle_trace) = board.item(obstacle_item).as_trace().cloned() else {
        return true;
    };
    let trace_layer = eng.room(obstacle_room).layer;
    if obstacle_trace.half_width != ctrl.trace_half_width[trace_layer as usize]
        || board.item(obstacle_item).clearance_class != ctrl.trace_clearance_class
    {
        return true;
    }
    let comp_hw = ctrl.compensated_trace_half_width[trace_layer as usize] as f64;
    let from_door_shape = eng.door_shape(from_door);
    if from_door_shape.max_width() < 2.0 * comp_hw {
        return true;
    }
    let trace_corner_no = eng.room(obstacle_room).obstacle_index();
    let pl = &obstacle_trace.polyline;
    if trace_corner_no >= pl.line_count().saturating_sub(2) {
        return false;
    }
    let room_doors: Vec<DoorKey> = eng.room(obstacle_room).doors.clone();
    let from_dim = eng.door(from_door).dimension;
    let mut shove_seg: LineSegment;
    if from_dim == 2 {
        let Some(other_room) = eng.expandable_other_room(e.door, Some(obstacle_room)) else {
            return false;
        };
        let Some(other_item) = eng.room(other_room).obstacle_item() else {
            return false;
        };
        if !end_points_matching(board, obstacle_item, other_item) {
            return false;
        }
        let door_center = from_door_shape.centre_of_gravity();
        let c1 = pl.corner_approx(trace_corner_no);
        let c2 = pl.corner_approx(trace_corner_no + 1);
        if c1.distance_square(&c2) < 1.0 {
            return false;
        }
        let into_start = door_center.distance_square(&c2) < door_center.distance_square(&c1);
        shove_seg = LineSegment::from_polyline(pl, trace_corner_no + 1);
        if into_start {
            shove_seg = shove_seg.opposite();
        }
    } else {
        let Some(from_room) = eng.expandable_other_room(e.door, Some(obstacle_room)) else {
            return false;
        };
        let from_point = eng.room(from_room).shape().centre_of_gravity();
        let shove_trace_line = pl.line(trace_corner_no + 1);
        let Some(door_seg) = from_door_shape.diagonal_corner_segment() else {
            return false;
        };
        let side_of_trace_line = shove_trace_line.side_of_float(&door_seg.a);
        let Some(polar) = from_door_shape.polar_line_segment(&from_point) else {
            return false;
        };
        let swapped = polar.b.distance_square(&door_seg.a) < polar.a.distance_square(&door_seg.a);
        let check_dist = comp_hw + 5.0;
        let check_dist_sq = check_dist * check_dist;
        let section_ok = if (shove_to_the_left && !swapped) || (!shove_to_the_left && swapped) {
            e.section_no_of_door == eng.element_count(e.door).wrapping_sub(1)
                && (e.shape_entry.a.distance_square(&door_seg.b) <= check_dist_sq || e.shape_entry.b.distance_square(&door_seg.b) <= check_dist_sq)
        } else {
            e.section_no_of_door == 0
                && (e.shape_entry.a.distance_square(&door_seg.a) <= check_dist_sq || e.shape_entry.b.distance_square(&door_seg.a) <= check_dist_sq)
        };
        if !section_ok {
            return false;
        }
        let shrinked = polar.shrink_segment(comp_hw);
        let perp = shove_trace_line.direction().turn_45_degree(2);
        let l1 = pl.line(trace_corner_no + 1);
        let l0 = pl.line(trace_corner_no);
        let l2 = pl.line(trace_corner_no + 2);
        let left_case = |p: FloatPoint| Line::from_point_dir(p.round(), &perp);
        if side_of_trace_line == Side::OnTheLeft {
            if shove_to_the_left {
                shove_seg = LineSegment::new(left_case(shrinked.b), l1, l2);
            } else {
                shove_seg = LineSegment::new(left_case(shrinked.a), l1.opposite(), l0.opposite());
            }
        } else if shove_to_the_left {
            shove_seg = LineSegment::new(left_case(shrinked.b), l1.opposite(), l0.opposite());
        } else {
            shove_seg = LineSegment::new(left_case(shrinked.a), l1, l2);
        }
    }
    let trace_half_width = ctrl.trace_half_width[trace_layer as usize];
    let nets = [ctrl.net_no];
    let mut shove_width = board.check_trace_segment(&shove_seg, trace_layer, &nets, trace_half_width, ctrl.trace_clearance_class, true);
    let mut segment_shortened = false;
    if shove_width < i32::MAX as f64 {
        shove_width -= 1.0;
        if shove_width <= 0.0 {
            return true;
        }
        shove_seg = shove_seg.change_length_approx(shove_width);
        segment_shortened = true;
    }
    let from_corner = shove_seg.start_point_approx();
    let to_corner = shove_seg.end_point_approx();
    let segment_is_point = from_corner.distance_square(&to_corner) < 0.1;
    if !segment_is_point {
        shove_width = shove::check_segment_shove(
            board,
            &shove_seg,
            shove_to_the_left,
            trace_layer,
            &nets,
            trace_half_width,
            ctrl.trace_clearance_class,
            ctrl.max_shove_trace_recursion_depth,
            ctrl.max_shove_via_recursion_depth,
        );
        if shove_width <= 0.0 {
            return true;
        }
    }
    if segment_shortened {
        shove_width = shove_width.min(from_corner.distance(&to_corner));
    }
    let shove_line = shove_seg.get_line();
    let from_door_compare_distance = if from_dim == 2 || segment_is_point {
        f64::MAX
    } else {
        to_corner.distance_square(&from_door_shape.corner_approx(0))
    };
    for d in room_doors {
        if d == from_door {
            continue;
        }
        let (first, second, dim) = {
            let dd = eng.door(d);
            (dd.first, dd.second, dd.dimension)
        };
        if let (Some(a), Some(b)) = (eng.room(first).obstacle_item(), eng.room(second).obstacle_item()) {
            if a != b {
                continue;
            }
        }
        let door_shape = eng.door_shape(d);
        if dim == 2 && shove_width >= i32::MAX as f64 {
            if door_shape.contains_float(&to_corner) {
                let sections = eng.door_section_segments(d, comp_hw);
                if let Some(s0) = sections.first() {
                    to_door_list.push(DoorSection {
                        door: d,
                        section_index: 0,
                        section_line: *s0,
                    });
                }
            }
        } else if !segment_is_point {
            let Some(door_seg) = door_shape.diagonal_corner_segment() else {
                continue;
            };
            let s0 = shove_line.side_of_float(&door_seg.a);
            let s1 = shove_line.side_of_float(&door_seg.b);
            if shove_to_the_left {
                if s0 != Side::OnTheLeft || s1 != Side::OnTheLeft {
                    continue;
                }
            } else if s0 != Side::OnTheRight || s1 != Side::OnTheRight {
                continue;
            }
            let Some(door_line) = door_shape.polar_line_segment(&from_corner) else {
                continue;
            };
            let nearest_corner = if door_line.a.distance_square(&from_corner) <= door_line.b.distance_square(&from_corner) {
                door_line.a
            } else {
                door_line.b
            };
            if to_corner.distance_square(&nearest_corner) >= from_door_compare_distance {
                continue;
            }
            let proj = nearest_corner.projection_approx(&shove_line);
            if proj.distance(&from_corner) + comp_hw <= shove_width {
                let sections = eng.door_section_segments(d, comp_hw);
                for (i, s) in sections.iter().enumerate() {
                    let nc = if s.a.distance_square(&from_corner) <= s.b.distance_square(&from_corner) { s.a } else { s.b };
                    let sp = nc.projection_approx(&shove_line);
                    if sp.distance(&from_corner) <= shove_width {
                        to_door_list.push(DoorSection {
                            door: d,
                            section_index: i,
                            section_line: *s,
                        });
                    }
                }
            }
        }
    }
    true
}

fn end_points_matching(board: &Board, trace: ItemId, from_item: ItemId) -> bool {
    if trace == from_item {
        return true;
    }
    let t = board.item(trace);
    let f = board.item(from_item);
    if !t.shares_net(f) {
        return false;
    }
    let tt = t.as_trace().unwrap();
    if let Some(d) = f.as_drill() {
        d.center == tt.first_corner() || d.center == tt.last_corner()
    } else if let Some(ft) = f.as_trace() {
        tt.first_corner() == ft.first_corner()
            || tt.first_corner() == ft.last_corner()
            || tt.last_corner() == ft.first_corner()
            || tt.last_corner() == ft.last_corner()
    } else {
        false
    }
}

#[allow(dead_code)]
fn _unused(_: LinkedList<()>, _: Point) {}
