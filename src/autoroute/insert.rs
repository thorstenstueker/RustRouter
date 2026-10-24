//! Insertion of a found connection into the board (port of `FoundConnectionInserter`).

use super::control::AutorouteControl;
use super::locate::{calculate_additional_corner, FoundConnection, ResultItem};
use crate::board::{Board, Item, ItemId};
use crate::geometry::{IntPoint, Point, Polyline};
use crate::optimize::shove;

struct Inserter<'a> {
    board: &'a mut Board,
    ctrl: &'a AutorouteControl,
    last_corner: Option<IntPoint>,
    first_corner: Option<IntPoint>,
}

/// `FoundConnectionInserter.getInstance`: false corresponds to a null result.
pub fn insert_found_connection(
    board: &mut Board,
    ctrl: &AutorouteControl,
    connection: &FoundConnection,
    start_item: Option<Item>,
    target_item: Option<Item>,
) -> bool {
    let _ = (connection.start_item, connection.target_item);
    insert_segments(
        board,
        ctrl,
        start_item,
        target_item,
        connection.start_layer,
        connection.target_layer,
        &connection.connection_items,
    )
}

/// `FoundConnectionInserter.insertSegments`
pub fn insert_segments(
    board: &mut Board,
    ctrl: &AutorouteControl,
    start_item: Option<Item>,
    target_item: Option<Item>,
    start_layer: i32,
    target_layer: i32,
    segments: &[ResultItem],
) -> bool {
    let mut current_layer = target_layer;
    let mut ins = Inserter {
        board,
        ctrl,
        last_corner: None,
        first_corner: None,
    };
    for seg in segments {
        let Some(first) = seg.corners.first() else {
            return false;
        };
        if !ins.insert_via(Some(*first), current_layer, seg.layer) {
            return false;
        }
        current_layer = seg.layer;
        if !ins.insert_trace(seg) {
            return false;
        }
    }
    if !ins.insert_via(ins.last_corner, current_layer, start_layer) {
        return false;
    }
    if let Some(t) = target_item.filter(|t| t.is_trace()) {
        match ins.first_corner {
            Some(fc) => {
                ins.board
                    .connect_to_trace_item(fc, &t, ctrl.trace_half_width[start_layer as usize], ctrl.trace_clearance_class);
            }
            None => log::warn!(
                "FoundConnectionInserter: firstCorner is null for net #{}, skipping connect_to_trace for target item. This may indicate a degenerate route segment.",
                ctrl.net_no
            ),
        }
    }
    if let Some(t) = start_item.filter(|t| t.is_trace()) {
        match ins.last_corner {
            Some(lc) => {
                ins.board
                    .connect_to_trace_item(lc, &t, ctrl.trace_half_width[target_layer as usize], ctrl.trace_clearance_class);
            }
            None => log::warn!(
                "FoundConnectionInserter: lastCorner is null for net #{}, skipping connect_to_trace for start item. This may indicate a degenerate route segment.",
                ctrl.net_no
            ),
        }
    }
    ins.board.normalize_traces(ctrl.net_no);
    true
}

impl Inserter<'_> {
    fn forced_segment(&mut self, from: &Point, to: &Point, half_width: i32, layer: i32, nets: &[i32]) -> Option<Point> {
        let c = self.ctrl;
        self.board.insert_forced_trace_segment(
            from,
            to,
            half_width,
            layer,
            nets,
            c.trace_clearance_class,
            c.max_shove_trace_recursion_depth,
            c.max_shove_via_recursion_depth,
            c.max_spring_over_recursion_depth,
            i32::MAX,
            c.pull_tight_accuracy,
            true,
            None,
        )
    }

    fn insert_trace(&mut self, trace: &ResultItem) -> bool {
        let ctrl = self.ctrl;
        if trace.corners.len() == 1 {
            if self.first_corner.is_none() {
                self.first_corner = Some(trace.corners[0]);
            }
            self.last_corner = Some(trace.corners[0]);
            return true;
        }
        let saved_edge_to_turn_dist = self.board.rules.get_pin_edge_to_turn_dist();
        self.board.rules.set_pin_edge_to_turn_dist(-1.0);
        let mut start_pin: Option<ItemId> = None;
        let mut end_pin: Option<ItemId> = None;
        if ctrl.with_neckdown {
            let mut current_end_corner = Point::Int(trace.corners[0]);
            for i in 0..2 {
                for id in self.board.pick_items(&current_end_corner, trace.layer) {
                    let it = self.board.item(id);
                    if !it.is_pin() {
                        continue;
                    }
                    if it.contains_net(ctrl.net_no) && it.as_drill().unwrap().center == current_end_corner {
                        if i == 0 {
                            start_pin = Some(id);
                        } else {
                            end_pin = Some(id);
                        }
                    }
                }
                current_end_corner = Point::Int(*trace.corners.last().unwrap());
            }
        }
        let nets = [ctrl.net_no];
        let mut from_corner_no = 0usize;
        let mut result = true;
        let n = trace.corners.len();
        for i in 1..n {
            let arr: Vec<Point> = trace.corners[from_corner_no..=i].iter().map(|p| Point::Int(*p)).collect();
            let insert_polyline = Polyline::from_points(&arr);
            let ok_point = if insert_polyline.line_count() >= 2 {
                self.board.insert_forced_trace_polyline(
                    &insert_polyline,
                    ctrl.trace_half_width[trace.layer as usize],
                    trace.layer,
                    &nets,
                    ctrl.trace_clearance_class,
                    ctrl.max_shove_trace_recursion_depth,
                    ctrl.max_shove_via_recursion_depth,
                    ctrl.max_spring_over_recursion_depth,
                    i32::MAX,
                    ctrl.pull_tight_accuracy,
                    true,
                    None,
                )
            } else {
                None
            };
            let pl_last = if insert_polyline.line_count() >= 2 { Some(insert_polyline.last_corner()) } else { None };
            let pl_first = if insert_polyline.line_count() >= 2 { Some(insert_polyline.first_corner()) } else { None };
            let is_last = ok_point.is_some() && ok_point == pl_last;
            let mut neckdown_inserted = false;
            let mut micro_neckdown_inserted = false;
            if let Some(op) = &ok_point {
                if !is_last && ctrl.with_neckdown && arr.len() == 2 {
                    neckdown_inserted = self.insert_neckdown(op, &arr[1], trace.layer, start_pin, end_pin);
                }
            }
            if !neckdown_inserted && !is_last && ctrl.with_neckdown && arr.len() == 2 {
                micro_neckdown_inserted = self.insert_fanout_micro_neckdown(ok_point.as_ref(), &arr[1], trace.layer, &nets, start_pin, end_pin);
            }
            if is_last || neckdown_inserted || micro_neckdown_inserted {
                from_corner_no = i;
            } else if ok_point.is_some() && ok_point == pl_first && i != n - 1 {
                if from_corner_no > 0 && arr.len() < 3 {
                    from_corner_no -= 1;
                }
            } else {
                log::debug!(
                    "FoundConnectionInserter: insert trace failed for net #{} at corner {}/{} on layer {}, trace width: {}, from corner: {}, okPoint: {:?}, target: {:?}, corners {:?}",
                    ctrl.net_no,
                    i,
                    n - 1,
                    trace.layer,
                    ctrl.trace_half_width[trace.layer as usize],
                    from_corner_no,
                    ok_point,
                    pl_last,
                    trace.corners
                );
                result = false;
                break;
            }
        }
        for i in 0..n - 1 {
            if let Some(stub) = self.board.get_trace_tail(&Point::Int(trace.corners[i]), trace.layer, &nets) {
                self.board.remove_item(stub);
            }
        }
        self.board.rules.set_pin_edge_to_turn_dist(saved_edge_to_turn_dist);
        if self.first_corner.is_none() {
            self.first_corner = Some(trace.corners[0]);
        }
        self.last_corner = Some(*trace.corners.last().unwrap());
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_fanout_micro_neckdown(
        &mut self,
        ok_point: Option<&Point>,
        target: &Point,
        layer: i32,
        nets: &[i32],
        start_pin: Option<ItemId>,
        end_pin: Option<ItemId>,
    ) -> bool {
        let from = ok_point.unwrap_or(target).clone();
        if from == *target {
            return false;
        }
        let ctrl = self.ctrl;
        let base = ctrl.trace_half_width[layer as usize];
        let lc = self.board.layer_count();
        let mut candidates: Vec<i32> = Vec::new();
        let add = |v: i32, c: &mut Vec<i32>| {
            if !c.contains(&v) {
                c.push(v);
            }
        };
        if let Some(p) = start_pin {
            if self.board.item(p).is_on_layer(layer, lc) {
                add(self.board.pin_trace_neckdown_halfwidth(p, layer), &mut candidates);
            }
        }
        if let Some(p) = end_pin {
            if self.board.item(p).is_on_layer(layer, lc) {
                add(self.board.pin_trace_neckdown_halfwidth(p, layer), &mut candidates);
            }
        }
        let min_half_width = AutorouteControl::neck_half_width(self.board, &ctrl.settings);
        for pct in [1.max(base * 3 / 4), 1.max(base * 3 / 5), 1.max(base / 2)] {
            if pct >= min_half_width {
                add(pct, &mut candidates);
            }
        }
        for c in candidates {
            if c <= 0 || c >= base {
                continue;
            }
            let ok = self.forced_segment(&from, target, c, layer, nets);
            if ok.as_ref() == Some(target) {
                return true;
            }
        }
        false
    }

    fn insert_neckdown(&mut self, from: &Point, to: &Point, layer: i32, start_pin: Option<ItemId>, end_pin: Option<ItemId>) -> bool {
        if let Some(p) = start_pin {
            let ok = self.try_neck_down(to, from, layer, p);
            if ok.as_ref() == Some(from) {
                return true;
            }
        }
        if let Some(p) = end_pin {
            let ok = self.try_neck_down(from, to, layer, p);
            return ok.as_ref() == Some(to);
        }
        false
    }

    fn try_neck_down(&mut self, from: &Point, to: &Point, layer: i32, pin: ItemId) -> Option<Point> {
        let ctrl = self.ctrl;
        let lc = self.board.layer_count();
        if !self.board.item(pin).is_on_layer(layer, lc) {
            return None;
        }
        let pin_item = self.board.item(pin);
        let pin_center = pin_item.as_drill().unwrap().center.to_float();
        let clearance = self
            .board
            .rules
            .clearance_matrix
            .get_value(ctrl.trace_clearance_class, pin_item.clearance_class, layer, true) as f64;
        let neck_down_distance = 2.0 * (0.5 * self.board.pin_max_width(pin, layer) + clearance);
        if pin_center.distance(&to.to_float()) >= neck_down_distance {
            return None;
        }
        let neck_down_halfwidth = self.board.pin_trace_neckdown_halfwidth(pin, layer);
        if neck_down_halfwidth >= ctrl.trace_half_width[layer as usize] {
            return None;
        }
        let float_from = from.to_float();
        let float_to = to.to_float();
        let tolerance = 2.0;
        let nets = [ctrl.net_no];
        let mut ok_length = self.board.check_trace_segment_points(
            from,
            to,
            layer,
            &nets,
            ctrl.trace_half_width[layer as usize],
            ctrl.trace_clearance_class,
            true,
        );
        if ok_length >= i32::MAX as f64 {
            return Some(from.clone());
        }
        ok_length -= tolerance;
        let mut neck_down_end_point: Point;
        if ok_length <= tolerance {
            neck_down_end_point = from.clone();
        } else {
            let float_end = float_from.change_length(&float_to, ok_length);
            neck_down_end_point = Point::Int(float_end.round());
            let horizontal_first = (float_from.x - float_end.x).abs() >= (float_from.y - float_end.y).abs();
            let ar = self.board.rules.get_trace_angle_restriction();
            let add_corner = Point::Int(calculate_additional_corner(&float_from, &float_end, horizontal_first, ar).round());
            let hw = ctrl.trace_half_width[layer as usize];
            let ok = self.forced_segment(from, &add_corner, hw, layer, &nets);
            if ok.as_ref() != Some(&add_corner) {
                return Some(from.clone());
            }
            let ok = self.forced_segment(&add_corner, &neck_down_end_point, hw, layer, &nets);
            if ok.as_ref() != Some(&neck_down_end_point) {
                return Some(from.clone());
            }
            let add_corner = Point::Int(calculate_additional_corner(&float_end, &float_to, !horizontal_first, ar).round());
            if add_corner != *to {
                let ok = self.forced_segment(&neck_down_end_point, &add_corner, hw, layer, &nets);
                if ok.as_ref() != Some(&add_corner) {
                    return Some(from.clone());
                }
                neck_down_end_point = add_corner;
            }
        }
        self.forced_segment(&neck_down_end_point, to, neck_down_halfwidth, layer, &nets)
    }

    fn insert_via(&mut self, location: Option<IntPoint>, input_from_layer: i32, input_to_layer: i32) -> bool {
        if input_from_layer == input_to_layer {
            return true;
        }
        let Some(location) = location else {
            return false;
        };
        let location = Point::Int(location);
        let (from_layer, to_layer) = if input_from_layer < input_to_layer {
            (input_from_layer, input_to_layer)
        } else {
            (input_to_layer, input_from_layer)
        };
        let ctrl = self.ctrl;
        let nets = [ctrl.net_no];
        let mut via_info = None;
        let mut found_suitable_span = false;
        for i in 0..ctrl.via_rule.via_count() {
            let v = ctrl.via_rule.get_via(i).clone();
            let ps = self.board.library.padstacks.get(v.padstack);
            if ps.from_layer() > from_layer || ps.to_layer() < to_layer {
                continue;
            }
            found_suitable_span = true;
            if shove::forced_via_check(
                self.board,
                &v,
                &location,
                &nets,
                ctrl.max_shove_trace_recursion_depth,
                ctrl.max_shove_via_recursion_depth,
                Some(&ctrl.trace_half_width),
                ctrl.trace_clearance_class,
            ) {
                via_info = Some(v);
                break;
            }
        }
        let Some(via_info) = via_info else {
            if !found_suitable_span {
                log::debug!(
                    "FoundConnectionInserter: via mask not found for net #{} covering layers {} to {}",
                    ctrl.net_no,
                    from_layer,
                    to_layer
                );
            } else {
                log::debug!("FoundConnectionInserter: via placement blocked by clearance/shove limits for net #{}", ctrl.net_no);
            }
            return false;
        };
        if !shove::forced_via_insert(
            self.board,
            &via_info,
            &location,
            &nets,
            ctrl.trace_clearance_class,
            &ctrl.trace_half_width,
            ctrl.max_shove_trace_recursion_depth,
            ctrl.max_shove_via_recursion_depth,
        ) {
            log::debug!("FoundConnectionInserter: forced via failed for net #{}", ctrl.net_no);
            return false;
        }
        true
    }
}
