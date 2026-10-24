//! Calculation of the trace corners of a found connection by backtracking the maze search
//! (port of `FoundConnectionLocator`, `FoundConnectionLocator45Degree` and
//! `FoundConnectionLocatorAnyAngle`).
//!
//! The original compares `FloatPoint` objects by identity at a few places. Points carry
//! an identity token here (`P`), so these comparisons are reproduced exactly.

use super::control::AutorouteControl;
use super::engine::{AutorouteEngine, Expandable, RoomKey};
use super::maze::MazeResult;
use crate::board::{Board, ItemId};
use crate::geometry::{FloatLine, FloatPoint, IntPoint, Point, Side, Signum, TileShape};
use crate::rules::AngleRestriction;
use std::collections::{BTreeSet, HashMap};

const ANY_ANGLE_TOLERANCE: f64 = 1.0;

#[derive(Clone, Copy, Debug)]
struct P {
    p: FloatPoint,
    id: u64,
}

impl PartialEq for P {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Clone, Debug)]
pub struct ResultItem {
    pub corners: Vec<IntPoint>,
    pub layer: i32,
}

#[derive(Clone, Copy, Debug)]
struct BacktrackElement {
    door: Expandable,
    section_no_of_door: usize,
    next_room: Option<RoomKey>,
}

/// The result of `FoundConnectionLocator.getInstance`.
#[derive(Clone, Debug)]
pub struct FoundConnection {
    pub connection_items: Vec<ResultItem>,
    pub start_item: Option<ItemId>,
    pub start_layer: i32,
    pub target_item: Option<ItemId>,
    pub target_layer: i32,
}

struct Locator<'a> {
    eng: &'a mut AutorouteEngine,
    board: &'a Board,
    ctrl: &'a AutorouteControl,
    angle_restriction: AngleRestriction,
    backtrack: Vec<BacktrackElement>,
    current_from_point: P,
    previous_from_point: P,
    current_trace_layer: i32,
    current_from_door_index: i32,
    current_to_door_index: i32,
    current_target_door_index: i32,
    current_target_shape: TileShape,
    next_id: u64,
}

/// `FoundConnectionLocator.getInstance`. `None` corresponds to an exception or a null
/// result of the original.
pub fn locate(
    eng: &mut AutorouteEngine,
    board: &Board,
    ctrl: &AutorouteControl,
    result: &MazeResult,
    ripped_items: &mut BTreeSet<ItemId>,
    ripup_costs: Option<&mut HashMap<ItemId, i32>>,
) -> Option<FoundConnection> {
    let backtrack = backtrack(eng, result, ripped_items, ripup_costs);
    let mut loc = Locator {
        eng,
        board,
        ctrl,
        angle_restriction: board.rules.get_trace_angle_restriction(),
        backtrack,
        current_from_point: P { p: FloatPoint::ZERO, id: 0 },
        previous_from_point: P { p: FloatPoint::ZERO, id: 0 },
        current_trace_layer: 0,
        current_from_door_index: 0,
        current_to_door_index: 0,
        current_target_door_index: 0,
        current_target_shape: TileShape::empty(),
        next_id: 1,
    };
    loc.run(result)
}

fn backtrack(
    eng: &AutorouteEngine,
    result: &MazeResult,
    ripped_items: &mut BTreeSet<ItemId>,
    mut ripup_costs: Option<&mut HashMap<ItemId, i32>>,
) -> Vec<BacktrackElement> {
    let mut list = Vec::new();
    let mut current_door = result.destination_door;
    let mut current_element = eng.element(current_door, result.section_no_of_door).clone();
    let mut current_next_room: Option<RoomKey> = None;
    match current_door {
        Expandable::Target(t) => current_next_room = eng.target(t).room,
        Expandable::Drill(d) => {
            let drill = eng.drill(d);
            let idx = (drill.first_layer + result.section_no_of_door as i32) as usize;
            current_next_room = drill.rooms.get(idx).copied().flatten();
            if current_element.room_ripped {
                for r in drill.rooms.iter().flatten() {
                    if let Some(item) = eng.room(*r).obstacle_item() {
                        ripped_items.insert(item);
                        if let Some(c) = ripup_costs.as_deref_mut() {
                            c.insert(item, current_element.ripup_cost);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    let mut current = BacktrackElement {
        door: current_door,
        section_no_of_door: result.section_no_of_door,
        next_room: current_next_room,
    };
    loop {
        list.push(current);
        let Some(bt) = current_element.backtrack_door else {
            break;
        };
        current_door = bt;
        let mut section_no = current_element.section_no_of_backtrack_door;
        let count = eng.element_count(current_door);
        if section_no >= count {
            log::warn!("FoundConnectionLocator: currentSectionNo to big");
            section_no = count.saturating_sub(1);
        }
        current_next_room = match current_door {
            Expandable::Drill(d) => eng.drill(d).rooms.get(section_no).copied().flatten(),
            _ => eng.expandable_other_room(current_door, current_next_room),
        };
        current_element = eng.element(current_door, section_no).clone();
        current = BacktrackElement {
            door: current_door,
            section_no_of_door: section_no,
            next_room: current_next_room,
        };
        if current_element.room_ripped {
            if let Some(item) = current_next_room.and_then(|r| eng.room(r).obstacle_item()) {
                ripped_items.insert(item);
                if let Some(c) = ripup_costs.as_deref_mut() {
                    c.insert(item, current_element.ripup_cost);
                }
            }
        }
    }
    list
}

fn ninety_degree_corner(from: &FloatPoint, to: &FloatPoint, horizontal_first: bool) -> FloatPoint {
    if horizontal_first {
        FloatPoint::new(to.x, from.y)
    } else {
        FloatPoint::new(from.x, to.y)
    }
}

fn fortyfive_degree_corner(from: &FloatPoint, to: &FloatPoint, horizontal_first: bool) -> FloatPoint {
    let abs_dx = (to.x - from.x).abs();
    let abs_dy = (to.y - from.y).abs();
    let (x, y);
    if abs_dx <= abs_dy {
        if horizontal_first {
            x = to.x;
            y = if to.y >= from.y { from.y + abs_dx } else { from.y - abs_dx };
        } else {
            x = from.x;
            y = if to.y > from.y { to.y - abs_dx } else { to.y + abs_dx };
        }
    } else if horizontal_first {
        y = from.y;
        x = if to.x > from.x { to.x - abs_dy } else { to.x + abs_dy };
    } else {
        y = to.y;
        x = if to.x > from.x { from.x + abs_dy } else { from.x - abs_dy };
    }
    FloatPoint::new(x, y)
}

/// `FoundConnectionLocator.calculateAdditionalCorner`
pub fn calculate_additional_corner(from: &FloatPoint, to: &FloatPoint, horizontal_first: bool, a: AngleRestriction) -> FloatPoint {
    match a {
        AngleRestriction::NinetyDegree => ninety_degree_corner(from, to, horizontal_first),
        AngleRestriction::FortyfiveDegree => fortyfive_degree_corner(from, to, horizontal_first),
        AngleRestriction::None => *to,
    }
}

impl Locator<'_> {
    fn fresh(&mut self, p: FloatPoint) -> P {
        self.next_id += 1;
        P { p, id: self.next_id }
    }

    fn room_shape(&self, r: RoomKey) -> TileShape {
        self.eng.room(r).shape().clone()
    }

    fn width_safe_target_shape(&self, item: ItemId, room: RoomKey, target_shape: &TileShape) -> TileShape {
        let room_shape = self.room_shape(room);
        if self.board.item(item).is_drill_item() {
            return target_shape.intersection(&room_shape);
        }
        let layer = self.eng.room(room).layer;
        let shrink = if self.eng.room(room).is_obstacle() {
            self.ctrl.compensated_trace_half_width[layer as usize]
        } else {
            self.ctrl.get_trace_clearance_offset(layer)
        };
        let safe_room = room_shape.offset(-(shrink as f64));
        if !safe_room.is_empty() {
            let s = target_shape.intersection(&safe_room);
            if !s.is_empty() {
                return s;
            }
        }
        target_shape.intersection(&room_shape)
    }

    fn calculate_starting_point(&self, item: ItemId, tree_entry_no: usize, room: RoomKey) -> FloatPoint {
        match self.board.trace_connection_shape(self.eng.tree, item, tree_entry_no) {
            Some(cs) if !cs.is_empty() => {
                let s = self.width_safe_target_shape(item, room, &cs);
                s.centre_of_gravity().round().to_float()
            }
            _ => self.room_shape(room).centre_of_gravity().round().to_float(),
        }
    }

    fn run(&mut self, result: &MazeResult) -> Option<FoundConnection> {
        let mut out = FoundConnection {
            connection_items: Vec::new(),
            start_item: None,
            start_layer: 0,
            target_item: None,
            target_layer: 0,
        };
        let start_info = *self.backtrack.last()?;
        let Expandable::Target(start_door) = start_info.door else {
            log::warn!("FoundConnectionLocator: ItemExpansionDoor expected for startInfo.door");
            return Some(out);
        };
        let (start_item, start_tree_entry, start_room) = {
            let t = self.eng.target(start_door);
            (t.item, t.tree_entry_no, t.room?)
        };
        out.start_item = Some(start_item);
        out.start_layer = self.eng.room(start_room).layer;
        self.current_from_door_index = 0;
        let mut at_fanout_end = false;
        match result.destination_door {
            Expandable::Target(t) => {
                let (item, entry, room) = {
                    let td = self.eng.target(t);
                    (td.item, td.tree_entry_no, td.room?)
                };
                out.target_item = Some(item);
                out.target_layer = self.eng.room(room).layer;
                let p = self.calculate_starting_point(item, entry, room);
                self.current_from_point = self.fresh(p);
            }
            Expandable::Drill(d) => {
                let drill = self.eng.drill(d);
                let p = drill.location.to_float();
                out.target_layer = drill.first_layer + result.section_no_of_door as i32;
                self.current_from_point = self.fresh(p);
                at_fanout_end = true;
            }
            _ => {
                log::warn!("FoundConnectionLocator: unexpected type of destinationDoor");
                return Some(out);
            }
        }
        self.current_trace_layer = out.target_layer;
        self.previous_from_point = self.current_from_point;
        let n = self.backtrack.len() as i32;
        let mut connection_done = false;
        while !connection_done {
            let mut layer_changed = false;
            if at_fanout_end {
                layer_changed = true;
            } else {
                self.current_target_door_index = self.current_from_door_index + 1;
                while self.current_target_door_index < n && !layer_changed {
                    if matches!(self.backtrack[self.current_target_door_index as usize].door, Expandable::Drill(_)) {
                        layer_changed = true;
                    } else {
                        self.current_target_door_index += 1;
                    }
                }
            }
            if layer_changed {
                let Expandable::Drill(d) = self.backtrack.get(self.current_target_door_index as usize)?.door else {
                    return None;
                };
                self.current_target_shape = TileShape::from_point(&self.eng.drill(d).location);
            } else {
                connection_done = true;
                self.current_target_door_index = n - 1;
                let target_shape = self.board.trace_connection_shape(self.eng.tree, start_item, start_tree_entry)?;
                let mut bounded = self.width_safe_target_shape(start_item, start_room, &target_shape);
                if bounded.dimension() >= 2 && !self.board.item(start_item).is_drill_item() {
                    let hw = self.ctrl.compensated_trace_half_width[self.eng.room(start_room).layer as usize];
                    let shrinked = bounded.offset(-(hw as f64));
                    if !shrinked.is_empty() {
                        bounded = shrinked;
                    }
                }
                self.current_target_shape = bounded;
            }
            self.current_to_door_index = self.current_from_door_index + 1;
            let next_trace = self.calculate_next_trace(layer_changed, at_fanout_end)?;
            at_fanout_end = false;
            out.connection_items.push(next_trace);
        }
        Some(out)
    }

    fn adjust_start_corner(&mut self) -> P {
        if self.current_from_door_index < 0 {
            return self.current_from_point;
        }
        let Some(info) = self.backtrack.get(self.current_from_door_index as usize).copied() else {
            return self.current_from_point;
        };
        let Some(room) = info.next_room else {
            return self.current_from_point;
        };
        let shrink = if self.eng.room(room).is_obstacle() {
            self.ctrl.compensated_trace_half_width[self.current_trace_layer as usize]
        } else {
            self.ctrl.get_trace_clearance_offset(self.current_trace_layer)
        };
        let shrinked = self.room_shape(room).offset(-(shrink as f64));
        if shrinked.is_empty() || shrinked.contains_float(&self.current_from_point.p) {
            return self.current_from_point;
        }
        match shrinked.nearest_point_approx(&self.current_from_point.p) {
            Some(p) => {
                let r = p.round().to_float();
                self.fresh(r)
            }
            None => self.current_from_point,
        }
    }

    fn calculate_next_trace(&mut self, layer_changed: bool, at_fanout_end: bool) -> Option<ResultItem> {
        let mut corners: Vec<FloatPoint> = vec![self.current_from_point.p];
        if !at_fanout_end {
            let adjusted = self.adjust_start_corner();
            if adjusted != self.current_from_point {
                let add = calculate_additional_corner(&self.current_from_point.p, &adjusted.p, true, self.angle_restriction);
                corners.push(add);
                corners.push(adjusted.p);
                self.previous_from_point = self.current_from_point;
                self.current_from_point = adjusted;
            }
        }
        let mut prev_corner = self.current_from_point;
        loop {
            let next = match self.angle_restriction {
                AngleRestriction::NinetyDegree | AngleRestriction::FortyfiveDegree => self.next_trace_corners_45()?,
                AngleRestriction::None => self.next_trace_corners_any()?,
            };
            if next.is_empty() {
                break;
            }
            for c in next {
                if c != prev_corner {
                    corners.push(c.p);
                    self.previous_from_point = self.current_from_point;
                    self.current_from_point = c;
                    prev_corner = c;
                }
            }
        }
        let mut next_layer = self.current_trace_layer;
        if layer_changed {
            self.current_from_door_index = self.current_target_door_index + 1;
            if let Some(info) = self.backtrack.get(self.current_from_door_index as usize) {
                if let Some(r) = info.next_room {
                    next_layer = self.eng.room(r).layer;
                }
            }
        }
        let mut rounded: Vec<IntPoint> = Vec::new();
        for c in corners {
            let p = c.round();
            if rounded.last() != Some(&p) {
                rounded.push(p);
            }
        }
        let result = ResultItem {
            corners: rounded,
            layer: self.current_trace_layer,
        };
        self.current_trace_layer = next_layer;
        Some(result)
    }

    // ---------------------------------------------------------------- 45 degree

    fn horizontal_first_door(&self, door: Expandable, from: &FloatPoint, to: &FloatPoint, to_door: bool) -> bool {
        let shape = self.eng.expandable_shape(door);
        let b = shape.bounding_box();
        if self.eng.expandable_dimension(door) != 1 {
            return if to_door { b.height() <= b.width() } else { b.height() >= b.width() };
        }
        let Some(seg) = shape.diagonal_corner_segment() else {
            return !to_door;
        };
        let (left, right) = if seg.a.x < seg.b.x || (seg.a.x == seg.b.x && seg.a.y <= seg.b.y) {
            (seg.a, seg.b)
        } else {
            (seg.b, seg.a)
        };
        let door_dx = right.x - left.x;
        let door_dy = right.y - left.y;
        let door_max_width = door_dx.max(door_dy.abs());
        let half = 0.5 * door_max_width;
        if (b.width() as f64) <= half {
            return !to_door;
        }
        if (b.height() as f64) <= half {
            return to_door;
        }
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let same_sign = Signum::of(dx) == Signum::of(dy);
        // "x dominates" resp. "y dominates"; both are false for |dx| == |dy|.
        let x_dom = dx.abs() > dy.abs();
        let y_dom = dx.abs() < dy.abs();
        let first_case = (left.y < right.y) == same_sign;
        match (to_door, first_case) {
            (false, true) => x_dom,
            (false, false) => y_dom,
            (true, true) => y_dom,
            (true, false) => x_dom,
        }
    }

    fn next_trace_corners_45(&mut self) -> Option<Vec<P>> {
        let mut result = Vec::new();
        if self.current_to_door_index > self.current_target_door_index {
            return Some(result);
        }
        let from_info = self.backtrack[(self.current_to_door_index - 1) as usize];
        let Some(from_room) = from_info.next_room else {
            log::warn!("FoundConnectionLocator45Degree.calculate_next_trace_corners: nextRoom is null");
            return Some(result);
        };
        let room_shape = self.room_shape(from_room);
        let trace_halfwidth = self.ctrl.compensated_trace_half_width[self.current_trace_layer as usize];
        let shrink_offset = if self.eng.room(from_room).is_obstacle() {
            trace_halfwidth
        } else {
            self.ctrl.get_trace_clearance_offset(self.current_trace_layer)
        };
        let trace_halfwidth_add = shrink_offset as f64;
        let mut shrinked_room_shape = room_shape.offset(-(shrink_offset as f64));
        if !shrinked_room_shape.is_empty() {
            let nearest = shrinked_room_shape.nearest_point_approx(&self.current_from_point.p)?;
            let horizontal_first = self.horizontal_first_door(from_info.door, &self.current_from_point.p, &nearest, false);
            let nearest = nearest.round().to_float();
            let add = calculate_additional_corner(&self.current_from_point.p, &nearest, horizontal_first, self.angle_restriction);
            result.push(self.fresh(add));
            let np = self.fresh(nearest);
            result.push(np);
            self.current_from_point = np;
        } else {
            shrinked_room_shape = room_shape.clone();
        }
        if self.current_to_door_index == self.current_target_door_index {
            let nearest = self.current_target_shape.nearest_point_approx(&self.current_from_point.p)?.round().to_float();
            let mut add = calculate_additional_corner(&self.current_from_point.p, &nearest, true, self.angle_restriction);
            if !shrinked_room_shape.contains_float(&add) {
                add = calculate_additional_corner(&self.current_from_point.p, &nearest, false, self.angle_restriction);
            }
            result.push(self.fresh(add));
            result.push(self.fresh(nearest));
            self.current_to_door_index += 1;
            return Some(result);
        }
        let to_info = self.backtrack[self.current_to_door_index as usize];
        let Expandable::Door(to_door) = to_info.door else {
            log::warn!("FoundConnectionLocator45Degree.calculate_next_trace_corners: ExpansionDoor expected");
            return Some(result);
        };
        let mut nearest_to_door_point;
        if self.eng.door(to_door).dimension == 2 {
            let shrinked = self.eng.door_shape(to_door).shrink(shrink_offset as f64);
            nearest_to_door_point = shrinked.nearest_point_approx(&self.current_from_point.p)?.round().to_float();
        } else {
            let sections = self.eng.door_section_segments(to_door, trace_halfwidth as f64);
            if to_info.section_no_of_door >= sections.len() {
                log::warn!("FoundConnectionLocator45Degree.calculate_next_trace_corners: lineSections inconsistent");
                return Some(result);
            }
            let section = sections[to_info.section_no_of_door];
            let len = section.b.distance(&section.a);
            if len <= 2.5 * trace_halfwidth_add {
                nearest_to_door_point = section.a.middle_point(&section.b);
            } else {
                let safe = section.shrink_segment(trace_halfwidth_add);
                nearest_to_door_point = safe.nearest_segment_point(&self.current_from_point.p);
            }
            let mut ok = true;
            if let Some(nr) = to_info.next_room {
                let s = TileShape::Simplex(self.room_shape(nr).to_simplex());
                let np = s.nearest_border_points_approx(&nearest_to_door_point, 2);
                if np.len() >= 2 {
                    ok = np[1].distance(&nearest_to_door_point) >= trace_halfwidth_add;
                }
            }
            if ok {
                let s = TileShape::Simplex(room_shape.to_simplex());
                let np = s.nearest_border_points_approx(&nearest_to_door_point, 2);
                if np.len() >= 2 {
                    ok = np[1].distance(&nearest_to_door_point) >= trace_halfwidth_add;
                }
            }
            if !ok {
                nearest_to_door_point = section.a.middle_point(&section.b);
            }
        }
        let nearest_to_door_point = nearest_to_door_point.round().to_float();
        let horizontal_first = self.horizontal_first_door(to_info.door, &self.current_from_point.p, &nearest_to_door_point, true);
        let add = calculate_additional_corner(&self.current_from_point.p, &nearest_to_door_point, horizontal_first, self.angle_restriction);
        result.push(self.fresh(add));
        result.push(self.fresh(nearest_to_door_point));
        self.current_to_door_index += 1;
        Some(result)
    }

    // ---------------------------------------------------------------- any angle

    fn door_corner(&self, info: &BacktrackElement, left: bool) -> Option<FloatPoint> {
        let from_room = self.eng.expandable_other_room(info.door, info.next_room)?;
        let pole = self.room_shape(from_room).centre_of_gravity();
        let door_shape = self.eng.expandable_shape(info.door);
        let no = if left {
            door_shape.index_of_left_most_corner(&pole)
        } else {
            door_shape.index_of_right_most_corner(&pole)
        };
        Some(door_shape.corner_approx(no))
    }

    fn next_trace_corners_any(&mut self) -> Option<Vec<P>> {
        let mut result = Vec::new();
        if self.current_to_door_index >= self.current_target_door_index {
            if self.current_to_door_index == self.current_target_door_index {
                let np = self.current_target_shape.nearest_point(&Point::Int(self.current_from_point.p.round()))?.to_float();
                self.current_to_door_index += 1;
                result.push(self.fresh(np));
            }
            return Some(result);
        }
        let hw_exact = self.ctrl.compensated_trace_half_width[self.current_trace_layer as usize] as f64;
        let hw_max = self.ctrl.get_trace_clearance_offset(self.current_trace_layer) as f64;
        let hw_middle = hw_exact + ANY_ANGLE_TOLERANCE;
        let cfp = self.current_from_point.p;
        let pfp = self.previous_from_point.p;
        let to_info = self.backtrack[self.current_to_door_index as usize];
        let mut door_left = self.door_corner(&to_info, true);
        let mut door_right = self.door_corner(&to_info, false);
        let (Some(dl), Some(dr)) = (door_left, door_right) else {
            return None;
        };
        if cfp.side_of(&dl, &dr) != Side::OnTheRight {
            if cfp.scalar_product(&pfp, &dl) >= 0.0 {
                door_left = None;
            }
            if cfp.scalar_product(&pfp, &dr) >= 0.0 {
                door_right = None;
            }
            if door_left.is_none() && door_right.is_none() {
                self.current_to_door_index += 1;
                result.push(self.current_from_point);
                return Some(result);
            }
        }
        let mut end_of_trace = false;
        let mut left_tangent_point;
        let mut right_tangent_point;
        let mut new_door_ind = self.current_to_door_index;
        let mut left_ind = new_door_ind;
        let mut right_ind = new_door_ind;
        let mut current_door_ind = self.current_to_door_index + 1;
        let mut result_corner: Option<P>;
        loop {
            left_tangent_point = door_left.and_then(|c| cfp.right_tangential_point(&c, hw_max));
            if door_left.is_some() && left_tangent_point.is_none() {
                left_tangent_point = door_left;
            }
            right_tangent_point = door_right.and_then(|c| cfp.left_tangential_point(&c, hw_max));
            if door_right.is_some() && right_tangent_point.is_none() {
                right_tangent_point = door_right;
            }
            if let (Some(lt), Some(rt)) = (left_tangent_point, right_tangent_point) {
                if rt.side_of(&cfp, &lt) != Side::OnTheRight {
                    let dl = door_left?;
                    let dr = door_right?;
                    if dl.distance(&cfp) <= dr.distance(&cfp) {
                        new_door_ind = left_ind;
                        result_corner = self.left_turn_next_corner(hw_max, &dl, &dr);
                    } else {
                        new_door_ind = right_ind;
                        result_corner = self.right_turn_next_corner(hw_max, &dr, &dl);
                    }
                    break;
                }
            }
            if current_door_ind >= self.current_target_door_index {
                end_of_trace = true;
                result_corner = None;
                break;
            }
            let next_info = self.backtrack[current_door_ind as usize];
            let mut next_left = self.door_corner(&next_info, true);
            let mut next_right = self.door_corner(&next_info, false);
            let (Some(nl), Some(nr)) = (next_left, next_right) else {
                return None;
            };
            if cfp.side_of(&nl, &nr) != Side::OnTheRight {
                if door_left.is_none() && cfp.scalar_product(&pfp, &nl) >= 0.0 {
                    next_left = None;
                }
                if door_right.is_none() && cfp.scalar_product(&pfp, &nr) >= 0.0 {
                    next_right = None;
                }
                if next_left.is_none() && next_right.is_none() {
                    self.current_to_door_index += 1;
                    result.push(self.current_from_point);
                    return Some(result);
                }
            }
            if let (Some(dl), Some(dr)) = (door_left, door_right) {
                let nl = next_left?;
                if nl.side_of(&cfp, &dr) == Side::OnTheRight {
                    new_door_ind = right_ind + 1;
                    result_corner = self.right_turn_next_corner(hw_max, &dr, &nl);
                    break;
                }
                let nr = next_right?;
                if nr.side_of(&cfp, &dl) == Side::OnTheLeft {
                    new_door_ind = left_ind + 1;
                    result_corner = self.left_turn_next_corner(hw_max, &dl, &nr);
                    break;
                }
            }
            let mut smaller_right = door_right.is_none();
            if let Some(dr) = door_right {
                let nr = next_right?;
                if nr.side_of(&cfp, &dr) != Side::OnTheRight {
                    if let Some(tp) = cfp.left_tangential_point(&nr, hw_max) {
                        let check_line = FloatLine::new(cfp, tp);
                        if check_line.segment_distance(&dr) >= hw_max {
                            smaller_right = true;
                        }
                    }
                }
            }
            if smaller_right {
                door_right = next_right;
                right_ind = current_door_ind;
            }
            let mut smaller_left = door_left.is_none();
            if let Some(dl) = door_left {
                let nl = next_left?;
                if nl.side_of(&cfp, &dl) != Side::OnTheLeft {
                    if let Some(tp) = cfp.right_tangential_point(&nl, hw_max) {
                        let check_line = FloatLine::new(cfp, tp);
                        if check_line.segment_distance(&dl) >= hw_max {
                            smaller_left = true;
                        }
                    }
                }
            }
            if smaller_left {
                door_left = next_left;
                left_ind = current_door_ind;
            }
            current_door_ind += 1;
        }
        if end_of_trace {
            let np = self.current_target_shape.nearest_point(&Point::Int(cfp.round()))?.to_float();
            result_corner = Some(self.fresh(np));
            if left_tangent_point.is_some() && np.side_of(&cfp, &left_tangent_point.unwrap()) == Side::OnTheLeft {
                new_door_ind = left_ind + 1;
                let ts = &self.current_target_shape;
                let target_right = ts.corner_approx(ts.index_of_right_most_corner(&cfp));
                if let Some(c) = door_left.and_then(|dl| right_left_tangential_point(&cfp, &target_right, &dl, hw_max)) {
                    result_corner = Some(self.fresh(c));
                    end_of_trace = false;
                }
            } else if right_tangent_point.is_some() && np.side_of(&cfp, &right_tangent_point.unwrap()) == Side::OnTheRight {
                let ts = &self.current_target_shape;
                let target_left = ts.corner_approx(ts.index_of_left_most_corner(&cfp));
                new_door_ind = right_ind + 1;
                if let Some(c) = door_right.and_then(|dr| left_right_tangential_point(&cfp, &target_left, &dr, hw_max)) {
                    result_corner = Some(self.fresh(c));
                    end_of_trace = false;
                }
            }
        }
        if end_of_trace {
            new_door_ind = self.current_target_door_index;
        }
        // A null result corner raises an exception in the original.
        let rc = result_corner?;
        let check_line = FloatLine::new(cfp, rc.p);
        let check_from = (self.current_to_door_index - 5).max(self.current_from_door_index + 1);
        let mut corrected: Option<FloatPoint> = None;
        let mut corrected_ind = 0;
        for i in check_from..new_door_ind {
            let info = self.backtrack[i as usize];
            let lc = self.door_corner(&info, true)?;
            let d = check_line.segment_distance(&lc);
            if d.abs() < hw_middle {
                if let Some(c) = right_left_tangential_point(&check_line.a, &check_line.b, &lc, hw_max) {
                    if corrected.is_none() || c.side_of(&cfp, &corrected.unwrap()) == Side::OnTheRight {
                        corrected_ind = i;
                        corrected = Some(c);
                    }
                }
            }
            let rcn = self.door_corner(&info, false)?;
            let d = check_line.segment_distance(&rcn);
            if d.abs() < hw_middle {
                if let Some(c) = left_right_tangential_point(&check_line.a, &check_line.b, &rcn, hw_max) {
                    if corrected.is_none() || c.side_of(&cfp, &corrected.unwrap()) == Side::OnTheLeft {
                        corrected_ind = i;
                        corrected = Some(c);
                    }
                }
            }
        }
        let mut rc = rc;
        if let Some(c) = corrected {
            rc = self.fresh(c);
            new_door_ind = corrected_ind.max(self.current_to_door_index);
        }
        self.current_to_door_index = new_door_ind;
        if rc != self.current_from_point {
            result.push(rc);
        }
        Some(result)
    }

    /// Returns the from corner itself (identity) if no tangent exists, None if the lines are parallel.
    fn right_turn_next_corner(&mut self, dist: f64, to: &FloatPoint, next: &FloatPoint) -> Option<P> {
        let from = self.current_from_point;
        let Some(t1) = from.p.left_tangential_point(to, dist) else {
            return Some(from);
        };
        let first = FloatLine::new(from.p, t1);
        let Some(t2) = to.right_tangential_point(next, 2.0 * dist + ANY_ANGLE_TOLERANCE) else {
            return Some(from);
        };
        let second = FloatLine::new(*to, t2).translate(dist);
        first.intersection(&second).map(|p| self.fresh(p))
    }

    fn left_turn_next_corner(&mut self, dist: f64, to: &FloatPoint, next: &FloatPoint) -> Option<P> {
        let from = self.current_from_point;
        let Some(t1) = from.p.right_tangential_point(to, dist) else {
            return Some(from);
        };
        let first = FloatLine::new(from.p, t1);
        let Some(t2) = to.left_tangential_point(next, 2.0 * dist + ANY_ANGLE_TOLERANCE) else {
            return Some(from);
        };
        let second = FloatLine::new(*to, t2).translate(-dist);
        first.intersection(&second).map(|p| self.fresh(p))
    }
}

fn right_left_tangential_point(from: &FloatPoint, to: &FloatPoint, center: &FloatPoint, dist: f64) -> Option<FloatPoint> {
    let t1 = from.right_tangential_point(center, dist)?;
    let first = FloatLine::new(*from, t1);
    let t2 = to.left_tangential_point(center, dist)?;
    let second = FloatLine::new(*to, t2);
    first.intersection(&second)
}

fn left_right_tangential_point(from: &FloatPoint, to: &FloatPoint, center: &FloatPoint, dist: f64) -> Option<FloatPoint> {
    let t1 = from.left_tangential_point(center, dist)?;
    let first = FloatLine::new(*from, t1);
    let t2 = to.right_tangential_point(center, dist)?;
    let second = FloatLine::new(*to, t2);
    first.intersection(&second)
}
