//! Pull tight algorithms for traces (port of `TraceTightener`, `TraceTightener45`,
//! `TraceTightener90` and `TraceTightenerAnyAngle`).

use crate::board::item::ItemId;
use crate::board::{Board, FixedState};
use crate::datastructures::{StopFlag, TimeLimit};
use crate::geometry::{
    Direction, FloatPoint, IntOctagon, IntPoint, Line, Point, Polyline, Side, Signum, Vector, SQRT2,
};
use crate::rules::AngleRestriction;
use std::collections::BTreeSet;

const C_MAX_COS_ANGLE: f64 = 0.999;
const C_MIN_CORNER_DIST_SQUARE: f64 = 0.9;
const SKIP_LENGTH: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TightenerKind {
    Ninety,
    FortyFive,
    AnyAngle,
}

/// Cost factors for traces on a layer used by the autorouter and the via optimizer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpansionCostFactor {
    pub horizontal: f64,
    pub vertical: f64,
}

pub struct TraceTightener {
    pub kind: TightenerKind,
    pub only_net_no_arr: Vec<i32>,
    stop: Option<StopFlag>,
    time_limit: Option<TimeLimit>,
    keep_point: Option<Point>,
    keep_point_layer: i32,
    current_layer: i32,
    current_half_width: i32,
    current_net_numbers: Vec<i32>,
    current_clearance_class: i32,
    pub current_clip_shape: Option<IntOctagon>,
    contact_pins: Option<BTreeSet<ItemId>>,
    min_translate_dist: i32,
}

/// The pieces of the original polyline API used here.
#[inline]
fn lines_of(p: &Polyline) -> &[Line] {
    p.lines()
}

impl TraceTightener {
    /// `TraceTightener.getInstance`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        board: &Board,
        only_net_no_arr: &[i32],
        clip_shape: Option<IntOctagon>,
        min_translate_dist: i32,
        stop: Option<StopFlag>,
        time_limit: i32,
        keep_point: Option<Point>,
        keep_point_layer: i32,
    ) -> TraceTightener {
        let kind = match board.rules.get_trace_angle_restriction() {
            AngleRestriction::NinetyDegree => TightenerKind::Ninety,
            AngleRestriction::FortyfiveDegree => TightenerKind::FortyFive,
            AngleRestriction::None => TightenerKind::AnyAngle,
        };
        TraceTightener {
            kind,
            only_net_no_arr: only_net_no_arr.to_vec(),
            stop,
            time_limit: if time_limit > 0 {
                Some(TimeLimit::new(time_limit as u64))
            } else {
                None
            },
            keep_point,
            keep_point_layer,
            current_layer: 0,
            current_half_width: 0,
            current_net_numbers: Vec::new(),
            current_clearance_class: 0,
            current_clip_shape: clip_shape,
            contact_pins: None,
            min_translate_dist: min_translate_dist.max(100),
        }
    }

    fn min_contact_chamfer_room(&self) -> f64 {
        (0.25 * (SQRT2 - 1.0) * self.current_half_width as f64).max(2.0)
    }

    pub fn is_stop_requested(&self) -> bool {
        if let Some(s) = &self.stop {
            if s.is_stop_requested() {
                return true;
            }
        }
        match &self.time_limit {
            Some(t) => t.limit_exceeded(),
            None => false,
        }
    }

    fn clip_contains(&self, p: &FloatPoint) -> bool {
        match &self.current_clip_shape {
            None => true,
            Some(c) => c.contains_float(p),
        }
    }

    fn clip_is_outside(&self, p: &Point) -> bool {
        match &self.current_clip_shape {
            None => false,
            Some(c) => crate::geometry::TileShape::Octagon(*c).is_outside(p),
        }
    }

    fn check(&self, board: &Board, polyline: &Polyline, index: usize) -> bool {
        match polyline.offset_shape(self.current_half_width, index) {
            Some(shape) => board.check_trace_shape(
                &shape,
                self.current_layer,
                &self.current_net_numbers,
                self.current_clearance_class,
                self.contact_pins.as_ref(),
            ),
            None => false,
        }
    }

    fn join_changed(&self, board: &mut Board, p: &FloatPoint) {
        board.join_changed_area(p, self.current_layer);
    }

    // ---------------------------------------------------------------- driver

    /// Optimizes the traces in the changed area of the board (`optChangedArea`).
    pub fn opt_changed_area(&mut self, board: &mut Board, trace_costs: Option<&[ExpansionCostFactor]>) {
        if board.changed_area.is_none() {
            return;
        }
        let mut something_changed = true;
        while something_changed {
            something_changed = false;
            for i in 0..board.layer_count() {
                let Some(ca) = board.changed_area.as_mut() else { return };
                let region = ca.get_area(i);
                if region.is_empty() {
                    continue;
                }
                ca.set_empty(i);
                let offset = 1.5
                    * (board.rules.clearance_matrix.max_value_on_layer(i) as f64
                        + 2.0 * board.rules.get_max_trace_half_width() as f64);
                let region = region.enlarge(offset);
                let objects = board.overlapping_objects(&crate::geometry::TileShape::Octagon(region), i);
                for obj in objects {
                    if self.is_stop_requested() {
                        return;
                    }
                    let Some(id) = obj.item() else { continue };
                    let Some(item) = board.try_item(id) else { continue };
                    if item.is_trace() {
                        if self.pull_tight_trace(board, id) {
                            something_changed = true;
                            if self.split_traces_at_keep_point(board) {
                                break;
                            }
                        } else if board.try_item(id).is_some() && self.smoothen_end_corners_at_trace(board, id) {
                            something_changed = true;
                            break;
                        }
                    } else if item.is_via() {
                        if let Some(costs) = trace_costs {
                            if super::via_optimizer::opt_via_location(board, id, costs, self.min_translate_dist, 10) {
                                something_changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Pulls a polyline tight with the trace properties given.
    #[allow(clippy::too_many_arguments)]
    pub fn pull_tight_polyline(
        &mut self,
        board: &mut Board,
        polyline: &Polyline,
        layer: i32,
        half_width: i32,
        nets: &[i32],
        clearance_class: i32,
        contact_pins: Option<BTreeSet<ItemId>>,
    ) -> Polyline {
        self.current_layer = layer;
        let tree = board.search_trees.default_tree();
        self.current_half_width = half_width + tree.clearance_compensation_value(&board.rules, clearance_class, layer);
        self.current_net_numbers = nets.to_vec();
        self.current_clearance_class = clearance_class;
        self.contact_pins = contact_pins;
        self.pull_tight(board, polyline)
    }

    fn pull_tight(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        let mut new_result = polyline.clone();
        let mut prev_result: Option<Polyline> = None;
        while prev_result.as_ref().map(|p| !p.ptr_eq(&new_result)).unwrap_or(true) && !self.is_stop_requested() {
            prev_result = Some(new_result.clone());
            let prev = new_result;
            new_result = match self.kind {
                TightenerKind::Ninety => {
                    let t1 = self.try_skip_second_corner(board, &prev);
                    let t2 = self.try_skip_corners(board, &t1);
                    self.reposition_lines(board, &t2)
                }
                TightenerKind::FortyFive => {
                    let t1 = self.reduce_corners_45(board, &prev);
                    let t2 = self.smoothen_corners_45(board, &t1);
                    self.reposition_lines(board, &t2)
                }
                TightenerKind::AnyAngle => {
                    let t = self.skip_segments_of_length_0(board, &prev);
                    let t0 = self.reduce_lines_any(board, &t);
                    let t1 = self.skip_lines_any(board, &t0);
                    let t2 = self.reduce_corners_any(board, &t1);
                    let t3 = self.reposition_lines_any(board, &t2);
                    self.smoothen_corners_any(board, &t3)
                }
            };
        }
        new_result
    }

    // ---------------------------------------------------------------- common

    fn reposition_lines(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        if self.kind == TightenerKind::AnyAngle {
            return self.reposition_lines_any(board, polyline);
        }
        let lines = lines_of(polyline);
        if lines.len() < 5 {
            return polyline.clone();
        }
        for i in 2..lines.len() - 2 {
            if let Some(nl) = self.reposition_line(board, lines, i) {
                let mut new_lines = lines.to_vec();
                new_lines[i] = nl;
                let r = Polyline::from_lines(&new_lines);
                return self.skip_segments_of_length_0(board, &r);
            }
        }
        polyline.clone()
    }

    /// `TraceTightener.repositionLine` (base version, used by the 45 and 90 degree tighteners).
    fn reposition_line(&mut self, board: &mut Board, lines: &[Line], no: usize) -> Option<Line> {
        if lines.len() - no < 3 {
            return None;
        }
        if self.current_clip_shape.is_some() {
            for i in -1i32..1 {
                let k = (no as i32 + i) as usize;
                let c = lines[k].intersection(&lines[k + 1]);
                if self.clip_is_outside(&c) {
                    return None;
                }
            }
        }
        let translate_line = lines[no];
        let prev_corner = lines[no - 2].intersection(&lines[no - 1]);
        let next_corner = lines[no + 1].intersection(&lines[no + 2]);
        let prev_dist = translate_line.signed_distance(&prev_corner.to_float());
        let next_dist = translate_line.signed_distance(&next_corner.to_float());
        if Signum::of(prev_dist) != Signum::of(next_dist) {
            return None;
        }
        let (nearest_point, mut max_translate_dist) = if prev_dist.abs() < next_dist.abs() {
            (prev_corner, prev_dist)
        } else {
            (next_corner, next_dist)
        };
        let mut translate_dist = max_translate_dist;
        let mut delta_dist = max_translate_dist;
        let side_of_nearest = translate_line.side_of(&nearest_point);
        let sign = Signum::as_int(max_translate_dist) as f64;
        let mut new_line: Option<Line> = None;
        let mut check_lines = [lines[no - 1], lines[no], lines[no + 1]];
        let mut first_time = true;
        while first_time || delta_dist.abs() > self.min_translate_dist as f64 {
            if first_time && nearest_point.is_int() {
                check_lines[1] = Line::from_point_dir(nearest_point.to_int(), &translate_line.direction());
            } else {
                check_lines[1] = translate_line.translate(-translate_dist);
            }
            if check_lines[1] == translate_line {
                return None;
            }
            let ns = check_lines[1].side_of(&nearest_point);
            if ns != side_of_nearest && ns != Side::Collinear {
                let sv = sign * 0.5;
                max_translate_dist -= sv;
                translate_dist -= sv;
                delta_dist -= sv;
                continue;
            }
            let tmp = Polyline::from_lines(&check_lines);
            let mut ok = false;
            if tmp.line_count() == 3 {
                ok = self.check(board, &tmp, 0);
            }
            delta_dist /= 2.0;
            if ok {
                new_line = Some(check_lines[1]);
                if first_time {
                    break;
                }
                translate_dist += delta_dist;
            } else {
                translate_dist -= delta_dist;
            }
            first_time = false;
        }
        let _ = max_translate_dist;
        if let Some(nl) = new_line {
            if board.changed_area.is_some() {
                let p1 = check_lines[0].intersection_approx(&nl);
                let p2 = check_lines[2].intersection_approx(&nl);
                let p3 = lines[no - 1].intersection_approx(&lines[no]);
                let p4 = lines[no].intersection_approx(&lines[no + 1]);
                for p in [p1, p2, p3, p4] {
                    self.join_changed(board, &p);
                }
            }
        }
        new_line
    }

    fn skip_segments_of_length_0(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        let mut changed = false;
        let mut current = polyline.clone();
        let mut i: isize = 1;
        while (i as usize) < current.line_count().saturating_sub(1) {
            let iu = i as usize;
            let try_skip = if iu == 1 || iu == current.line_count() - 2 {
                current.corner_ref(iu) == current.corner_ref(iu - 1)
            } else {
                current.corner_approx(iu).distance_square(&current.corner_approx(iu - 1)) < C_MIN_CORNER_DIST_SQUARE
            };
            if try_skip {
                let mut cl: Vec<Line> = current.lines().to_vec();
                cl.remove(iu);
                let tmp = Polyline::from_lines(&cl);
                let mut ok = tmp.line_count() == cl.len();
                if ok && !current.line(iu).is_multiple_of_45_degree() {
                    if iu > 1 {
                        ok = self.check(board, &tmp, iu - 2);
                    }
                    if ok && iu < current.line_count() - 2 {
                        ok = self.check(board, &tmp, iu - 1);
                    }
                }
                if ok {
                    changed = true;
                    current = tmp;
                    i -= 1;
                }
            }
            i += 1;
        }
        if !changed {
            return polyline.clone();
        }
        current
    }

    /// `smoothenEndCornersAtTrace`.
    pub fn smoothen_end_corners_at_trace(&mut self, board: &mut Board, trace_id: ItemId) -> bool {
        let item = board.item(trace_id);
        if !self.only_net_no_arr.is_empty() && !item.nets_equal(&self.only_net_no_arr) {
            return false;
        }
        let t = item.as_trace().unwrap();
        self.current_layer = t.layer;
        self.current_half_width = t.half_width;
        self.current_net_numbers = item.net_numbers.to_vec();
        self.current_clearance_class = item.clearance_class;
        self.smoothen_end_corners_at_trace_1(board, trace_id)
    }

    fn smoothen_end_corners_at_trace_1(&mut self, board: &mut Board, trace_id: ItemId) -> bool {
        if board.is_shove_fixed(board.item(trace_id)) {
            return false;
        }
        let saved = self.contact_pins.take();
        let mut result = false;
        let mut improved = true;
        let mut current = trace_id;
        while improved {
            improved = false;
            let Some(adjusted) = self.smoothen_end_corners_at_trace_2(board, current) else {
                continue;
            };
            let cur_item = board.item(current).clone();
            let layer = cur_item.first_layer();
            let cl = cur_item.clearance_class;
            let fixed: FixedState = cur_item.fixed_state;
            board.remove_item(current);
            if let Some(new_id) = board.insert_trace_without_cleaning(
                adjusted.clone(),
                layer,
                self.current_half_width,
                &cur_item.net_numbers,
                cl,
                fixed,
            ) {
                result = true;
                improved = true;
                current = new_id;
                for n in cur_item.net_numbers.iter() {
                    board.split_traces(&adjusted.first_corner(), layer, *n);
                    board.split_traces(&adjusted.last_corner(), layer, *n);
                    board.normalize_traces(*n);
                    if self.split_traces_at_keep_point(board) {
                        return true;
                    }
                }
                if !board.items.contains(current) {
                    break;
                }
            }
        }
        self.contact_pins = saved;
        result
    }

    pub fn split_traces_at_keep_point(&self, board: &mut Board) -> bool {
        let Some(kp) = &self.keep_point else { return false };
        for id in board.pick_traces(kp, self.keep_point_layer) {
            if board.items.contains(id) && board.split_trace_at_point(id, kp).is_some() {
                return true;
            }
        }
        false
    }

    fn smoothen_end_corners_at_trace_2(&mut self, board: &mut Board, trace_id: ItemId) -> Option<Polyline> {
        let item = board.try_item(trace_id)?;
        if !item.on_the_board {
            return None;
        }
        let mut result = self.smoothen_start_corner_at_trace(board, trace_id);
        if result.is_none() {
            result = self.smoothen_end_corner_at_trace(board, trace_id);
            if let Some(r) = &result {
                let p = r.corner_approx(r.corner_count() - 1);
                self.join_changed(board, &p);
            }
        } else if let Some(r) = &result {
            let p = r.corner_approx(0);
            self.join_changed(board, &p);
        }
        if let Some(r) = result {
            self.contact_pins = Some(board.touching_pins_at_end_corners(board.item(trace_id)));
            return Some(self.skip_segments_of_length_0(board, &r));
        }
        None
    }

    fn smoothen_start_corner_at_trace(&mut self, board: &mut Board, trace_id: ItemId) -> Option<Polyline> {
        match self.kind {
            TightenerKind::Ninety => None,
            TightenerKind::FortyFive => self.smoothen_end_corner_generic(board, trace_id, true, false),
            TightenerKind::AnyAngle => self.smoothen_end_corner_generic(board, trace_id, true, true),
        }
    }

    fn smoothen_end_corner_at_trace(&mut self, board: &mut Board, trace_id: ItemId) -> Option<Polyline> {
        match self.kind {
            TightenerKind::Ninety => None,
            TightenerKind::FortyFive => self.smoothen_end_corner_generic(board, trace_id, false, false),
            TightenerKind::AnyAngle => self.smoothen_end_corner_generic(board, trace_id, false, true),
        }
    }

    /// `smoothenStartCornerAtTrace` / `smoothenEndCornerAtTrace` of the 45 degree and the
    /// any angle tightener.
    fn smoothen_end_corner_generic(&mut self, board: &mut Board, trace_id: ItemId, at_start: bool, any_angle: bool) -> Option<Polyline> {
        let trace_polyline = board.item(trace_id).as_trace().unwrap().polyline.clone();
        let tl = trace_polyline.lines();
        let n = tl.len();
        let current_end_corner = if at_start {
            trace_polyline.corner(0)
        } else {
            trace_polyline.last_corner()
        };
        if self.clip_is_outside(&current_end_corner) {
            return None;
        }
        let mut current_prev_end_corner = if at_start {
            trace_polyline.corner(1)
        } else {
            trace_polyline.corner(trace_polyline.corner_count() - 2)
        };
        let mut skip_short_segment = false;
        let mut start_line_no = 1usize;
        let mut end_line_no = n - 2;
        if any_angle {
            skip_short_segment = !current_end_corner.is_int()
                && current_end_corner.to_float().distance_square(&current_prev_end_corner.to_float()) < SKIP_LENGTH;
            if skip_short_segment {
                if trace_polyline.corner_count() < 3 {
                    return None;
                }
                if at_start {
                    current_prev_end_corner = trace_polyline.corner(2);
                    start_line_no += 1;
                } else {
                    current_prev_end_corner = trace_polyline.corner(trace_polyline.corner_count() - 3);
                    end_line_no -= 1;
                }
            }
        }
        let (line_direction, prev_line_direction) = if at_start {
            (Direction::Int(tl[start_line_no].direction()), Direction::Int(tl[start_line_no + 1].direction()))
        } else if any_angle {
            // the original uses the same line for both directions here
            let d = Direction::Int(tl[end_line_no].direction().opposite());
            (d.clone(), d)
        } else {
            (
                Direction::Int(tl[n - 2].direction().opposite()),
                Direction::Int(tl[n - 3].direction().opposite()),
            )
        };
        let contacts = if at_start {
            board.trace_start_contacts(trace_id)
        } else {
            board.trace_end_contacts(trace_id)
        };
        let mut acute_angle = false;
        let mut bend = false;
        let mut other_corner_approx: Option<FloatPoint> = None;
        let mut other_line: Option<Line> = None;
        let mut other_prev_line: Option<Line> = None;
        let mut prev_corner_side: Option<Side> = None;
        for c in contacts {
            let ci = board.item(c);
            let Some(ct) = ci.as_trace() else { return None };
            if board.is_shove_fixed(ci) {
                return None;
            }
            let cp = &ct.polyline;
            if !at_start && any_angle && cp.corner_count() <= 2 {
                continue;
            }
            let (coa, col, copl) = if *cp.corner_ref(0) == current_end_corner {
                (cp.corner_approx(1), cp.line(1), cp.line(2))
            } else if cp.last_corner() == current_end_corner {
                let ccn = cp.corner_count() - 2;
                (cp.corner_approx(ccn), cp.line(ccn + 1).opposite(), cp.line(ccn))
            } else {
                return None;
            };
            let cur_side = current_prev_end_corner.side_of_line(&col);
            let col_dir = Direction::Int(col.direction());
            let proj = line_direction.projection(&col_dir);
            let mut found = false;
            if proj == Signum::Positive && cur_side != Side::Collinear {
                if any_angle || col.direction().is_orthogonal() {
                    acute_angle = true;
                    found = true;
                }
            } else if proj == Signum::Zero && trace_polyline.corner_count() > 2 && prev_line_direction.projection(&col_dir) == Signum::Positive {
                bend = true;
                found = true;
            }
            if found {
                other_corner_approx = Some(coa);
                other_line = Some(col);
                prev_corner_side = Some(cur_side);
                other_prev_line = Some(copl);
            }
        }
        let (mut new_line_count, diff): (usize, isize) = if any_angle {
            if at_start {
                if skip_short_segment {
                    (n, 0)
                } else {
                    (n + 1, 1)
                }
            } else if skip_short_segment {
                (n, 1)
            } else {
                (n + 1, 0)
            }
        } else {
            (n + 1, if at_start { 1 } else { 0 })
        };
        if acute_angle {
            let ol = other_line.unwrap();
            let left = prev_corner_side == Some(Side::OnTheLeft);
            let turn = if at_start == left { 2 } else { 6 };
            let new_dir = ol.direction().turn_45_degree(turn);
            let translate_line = Line::from_point_dir(current_end_corner.to_float().round(), &new_dir);
            let mut translate_dist = (SQRT2 - 1.0) * self.current_half_width as f64;
            let prev_corner_dist = translate_line.signed_distance(&current_prev_end_corner.to_float()).abs();
            let other_dist = translate_line.signed_distance(&other_corner_approx.unwrap()).abs();
            translate_dist = translate_dist.min(prev_corner_dist).min(other_dist);
            if translate_dist >= self.min_contact_chamfer_room() {
                translate_dist = (translate_dist - 1.0).max(1.0);
                if translate_line.side_of(&current_prev_end_corner) == Side::OnTheLeft {
                    translate_dist = -translate_dist;
                }
                let add_line = translate_line.translate(translate_dist);
                let mut new_lines = Vec::with_capacity(new_line_count);
                if at_start {
                    new_lines.push(ol);
                    new_lines.push(add_line);
                    let from = (2 - diff) as usize;
                    new_lines.extend_from_slice(&tl[from..from + new_line_count - 2]);
                } else {
                    let copy_len = if any_angle { n - 1 } else { n - 1 };
                    new_lines.extend_from_slice(&tl[..copy_len]);
                    new_lines.truncate(new_line_count - 2);
                    while new_lines.len() < new_line_count - 2 {
                        new_lines.push(tl[new_lines.len()]);
                    }
                    new_lines.push(add_line);
                    new_lines.push(ol);
                }
                return Some(Polyline::from_lines(&new_lines));
            }
        } else if bend {
            let ol = other_line.unwrap();
            let opl = other_prev_line.unwrap();
            if !any_angle {
                new_line_count = n + 1;
            }
            let mut check = Vec::with_capacity(new_line_count);
            if at_start {
                check.push(opl);
                check.push(ol);
                let from = (2 - diff) as usize;
                check.extend_from_slice(&tl[from..from + new_line_count - 2]);
            } else {
                let from = diff as usize;
                check.extend_from_slice(&tl[from..from + new_line_count - 2]);
                check.push(ol);
                check.push(opl);
            }
            let new_line = if any_angle {
                let start = if at_start { 0 } else { check.len() - 5 };
                self.reposition_line_any(board, &check, start)
            } else {
                let no = if at_start { 2 } else { n - 2 };
                self.reposition_line(board, &check, no)
            };
            if let Some(nl) = new_line {
                let mut new_lines = Vec::with_capacity(n);
                if at_start {
                    new_lines.push(ol);
                    new_lines.push(nl);
                    new_lines.extend_from_slice(&tl[2..]);
                } else {
                    new_lines.extend_from_slice(&tl[..n - 2]);
                    new_lines.push(nl);
                    new_lines.push(ol);
                }
                return Some(Polyline::from_lines(&new_lines));
            }
        }
        None
    }

    // ---------------------------------------------------------------- 90 degree

    fn try_skip_second_corner(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        let lines = lines_of(polyline);
        if lines.len() < 5 {
            return polyline.clone();
        }
        let check = Polyline::from_lines(&[lines[1], lines[0], lines[3], lines[4]]);
        if check.line_count() != 4 || !self.clip_contains(&check.corner_approx(1)) {
            return polyline.clone();
        }
        for i in 0..2 {
            if !self.check(board, &check, i) {
                return polyline.clone();
            }
        }
        let mut nl = vec![lines[1], lines[0]];
        nl.extend_from_slice(&lines[3..]);
        Polyline::from_lines(&nl)
    }

    fn try_skip_corners(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        let lines = lines_of(polyline);
        let n = lines.len();
        let mut new_lines: Vec<Line> = vec![lines[0]; n.max(2)];
        new_lines[0] = lines[0];
        new_lines[1] = lines[1];
        let mut new_index = 1usize;
        let mut changed = false;
        let mut second_last_skipped = false;
        let mut i = 5usize;
        while i <= n {
            let mut skip = false;
            let in_clip = self.clip_contains(&polyline.corner_approx(i - 3));
            let mut check_lines = [lines[0]; 4];
            if in_clip {
                check_lines[0] = new_lines[new_index - 1];
                check_lines[1] = new_lines[new_index];
                check_lines[2] = lines[i - 1];
                check_lines[3] = if i < n { lines[i] } else { lines[i - 2] };
                let cp = Polyline::from_lines(&check_lines);
                skip = cp.line_count() == 4 && self.clip_contains(&cp.corner_approx(1));
                if skip {
                    skip = self.check(board, &cp, 0);
                }
                if skip {
                    skip = self.check(board, &cp, 1);
                }
            }
            if skip {
                if i == n {
                    second_last_skipped = true;
                }
                if board.changed_area.is_some() {
                    let nc = check_lines[1].intersection_approx(&check_lines[2]);
                    self.join_changed(board, &nc);
                    let sc = lines[i - 2].intersection_approx(&lines[i - 3]);
                    self.join_changed(board, &sc);
                }
                changed = true;
                i += 1;
            } else {
                new_index += 1;
                new_lines[new_index] = lines[i - 3];
            }
            i += 1;
        }
        if !changed {
            return polyline.clone();
        }
        if second_last_skipped {
            new_index += 1;
            new_lines[new_index] = lines[n - 1];
            new_index += 1;
            new_lines[new_index] = lines[n - 2];
        } else {
            for k in (1..=3).rev() {
                new_index += 1;
                new_lines[new_index] = lines[n - k];
            }
        }
        new_lines.truncate(new_index + 1);
        Polyline::from_lines(&new_lines)
    }

    // ---------------------------------------------------------------- 45 degree

    fn reduce_corners_45(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        let n = polyline.line_count();
        if n <= 4 {
            return polyline.clone();
        }
        let mut cc: [Point; 4] = [Point::ZERO, Point::ZERO, Point::ZERO, Point::ZERO];
        for (i, c) in cc.iter_mut().enumerate() {
            *c = polyline.corner(i);
            if !c.is_int() {
                return polyline.clone();
            }
        }
        let mut in_clip = [true; 4];
        for i in 0..4 {
            in_clip[i] = !self.clip_is_outside(&cc[i]);
        }
        let mut changed = false;
        let mut new_corners: Vec<Point> = Vec::with_capacity(n - 3);
        new_corners.push(cc[0].clone());
        let mut new_corner: Point = Point::ZERO;
        let mut corner_index = 3usize;
        while corner_index < n - 1 {
            cc[3] = polyline.corner(corner_index);
            if !cc[3].is_int() {
                return polyline.clone();
            }
            if cc[1] == cc[2] || corner_index < n - 2 && cc[3].side_of(&cc[1], &cc[2]) == Side::Collinear {
                corner_index += 1;
                cc[2] = cc[3].clone();
                in_clip[2] = in_clip[3];
                if corner_index < n - 1 {
                    cc[3] = polyline.corner(corner_index);
                    if !cc[3].is_int() {
                        return polyline.clone();
                    }
                }
                changed = true;
            }
            in_clip[3] = !self.clip_is_outside(&cc[3]);
            let mut corner_removed = false;
            if in_clip[1] && in_clip[2] && in_clip[3] {
                let delta: Vector = cc[3].difference_by(&cc[2]);
                new_corner = cc[1].translate_by(&delta);
                if cc[3] == cc[2] {
                    corner_removed = true;
                } else if new_corner.side_of(&cc[0], &cc[1]) == Side::Collinear {
                    corner_removed = self.check_two_point_segments(board, &new_corner, &cc[1], &cc[3], true);
                }
            }
            if !corner_removed && in_clip[0] && in_clip[1] && in_clip[2] {
                let delta: Vector = cc[0].difference_by(&cc[1]);
                new_corner = cc[2].translate_by(&delta);
                if cc[0] == cc[1] {
                    corner_removed = true;
                } else if new_corner.side_of(&cc[2], &cc[3]) == Side::Collinear {
                    corner_removed = self.check_two_point_segments(board, &new_corner, &cc[0], &cc[2], false);
                }
            }
            if corner_removed {
                changed = true;
                cc[1] = new_corner.clone();
                in_clip[1] = !self.clip_is_outside(&cc[1]);
                if board.changed_area.is_some() {
                    let a = new_corner.to_float();
                    let b = cc[1].to_float();
                    let c = cc[2].to_float();
                    self.join_changed(board, &a);
                    self.join_changed(board, &b);
                    self.join_changed(board, &c);
                }
            } else {
                new_corners.push(cc[1].clone());
                cc[0] = cc[1].clone();
                cc[1] = cc[2].clone();
                in_clip[0] = in_clip[1];
                in_clip[1] = in_clip[2];
            }
            cc[2] = cc[3].clone();
            in_clip[2] = in_clip[3];
            corner_index += 1;
        }
        if !changed {
            return polyline.clone();
        }
        new_corners.push(cc[1].clone());
        new_corners.push(cc[2].clone());
        Polyline::from_points(&new_corners)
    }

    /// The two segment checks of `reduceCorners` of the 45 degree tightener.
    /// `first_case` distinguishes the equality test of the original.
    fn check_two_point_segments(&mut self, board: &mut Board, new_corner: &Point, p1: &Point, p2: &Point, first_case: bool) -> bool {
        let cp = Polyline::from_points(&[new_corner.clone(), p1.clone()]);
        if cp.line_count() != 3 {
            return true;
        }
        if !self.check(board, &cp, 0) {
            return false;
        }
        if first_case && new_corner == p2 {
            return true;
        }
        let cp = Polyline::from_points(&[new_corner.clone(), p2.clone()]);
        if cp.line_count() == 3 {
            self.check(board, &cp, 0)
        } else {
            true
        }
    }

    fn smoothen_corners_45(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        let mut result = polyline.clone();
        let mut changed = true;
        while changed {
            if result.line_count() < 4 {
                return result;
            }
            changed = false;
            let mut lines: Vec<Line> = result.lines().to_vec();
            let mut i = 1usize;
            while i + 2 < lines.len() {
                let d1 = lines[i].direction();
                let d2 = lines[i + 1].direction();
                if Direction::Int(d1).is_multiple_of_45_degree()
                    && Direction::Int(d2).is_multiple_of_45_degree()
                    && Direction::Int(d1).projection(&Direction::Int(d2)) != Signum::Positive
                {
                    let mut new_line = self.smoothen_corner_45(board, &lines, i);
                    if new_line.is_none() {
                        new_line = self.smoothen_sharp_corner_45(board, &lines, i);
                    }
                    if let Some(nl) = new_line {
                        changed = true;
                        lines.insert(i + 1, nl);
                        i += 1;
                    }
                }
                i += 1;
            }
            if changed {
                result = Polyline::from_lines(&lines);
            }
        }
        result
    }

    fn smoothen_sharp_corner_45(&mut self, board: &mut Board, lines: &[Line], no: usize) -> Option<Line> {
        let current_corner = lines[no].intersection_approx(&lines[no + 1]);
        if current_corner.x != (current_corner.x as i32) as f64 {
            if let Some(r) = self.smoothen_non_integer_corner(lines, no) {
                return Some(r);
            }
        }
        let prev_corner = lines[no].intersection_approx(&lines[no - 1]);
        let next_corner = lines[no + 1].intersection_approx(&lines[no + 2]);
        let prev_dir = lines[no].direction();
        let next_dir = lines[no + 1].direction();
        let new_dir = Direction::from_vector(&Vector::Int(prev_dir.get_vector()).add(&Vector::Int(next_dir.get_vector())));
        let translate_line = Line::from_point_direction(&Point::Int(current_corner.round()), &new_dir);
        let mut translate_dist = (SQRT2 - 1.0) * self.current_half_width as f64;
        let prev_dist = translate_line.signed_distance(&prev_corner).abs();
        let next_dist = translate_line.signed_distance(&next_corner).abs();
        translate_dist = translate_dist.min(prev_dist).min(next_dist);
        if translate_dist < 0.99 {
            return None;
        }
        translate_dist = (translate_dist - 1.0).max(1.0);
        if translate_line.side_of_float(&next_corner) == Side::OnTheLeft {
            translate_dist = -translate_dist;
        }
        let r = translate_line.translate(translate_dist);
        self.join_changed(board, &current_corner);
        Some(r)
    }

    fn smoothen_non_integer_corner(&self, lines: &[Line], no: usize) -> Option<Line> {
        let prev_line = lines[no];
        let next_line = lines[no + 1];
        if prev_line.is_equal_or_opposite(&next_line) {
            return None;
        }
        if !(prev_line.is_diagonal() && next_line.is_diagonal()) {
            return None;
        }
        let cc = prev_line.intersection_approx(&next_line);
        let pc = prev_line.intersection_approx(&lines[no - 1]);
        let nc = next_line.intersection_approx(&lines[no + 2]);
        let (mut nx, mut ny) = (0, 0);
        let mut vertical = false;
        let mut horizontal = false;
        if pc.x > cc.x && nc.x > cc.x {
            nx = cc.x.ceil() as i32;
            ny = cc.y.ceil() as i32;
            vertical = true;
        } else if pc.x < cc.x && nc.x < cc.x {
            nx = cc.x.floor() as i32;
            ny = cc.y.floor() as i32;
            vertical = true;
        } else if pc.y > cc.y && nc.y > cc.y {
            nx = cc.x.ceil() as i32;
            ny = cc.y.ceil() as i32;
            horizontal = true;
        } else if pc.y < cc.y && nc.y < cc.y {
            nx = cc.x.floor() as i32;
            ny = cc.y.floor() as i32;
            horizontal = true;
        }
        let dir = if vertical {
            if pc.y < nc.y {
                crate::geometry::IntDirection::UP
            } else {
                crate::geometry::IntDirection::DOWN
            }
        } else if horizontal {
            if pc.x < nc.x {
                crate::geometry::IntDirection::RIGHT
            } else {
                crate::geometry::IntDirection::LEFT
            }
        } else {
            return None;
        };
        Some(Line::from_point_dir(IntPoint::new(nx, ny), &dir))
    }

    fn smoothen_corner_45(&mut self, board: &mut Board, lines: &[Line], no: usize) -> Option<Line> {
        let prev_corner = lines[no].intersection_approx(&lines[no - 1]);
        let current_corner = lines[no].intersection_approx(&lines[no + 1]);
        let next_corner = lines[no + 1].intersection_approx(&lines[no + 2]);
        let prev_dir = lines[no].direction();
        let next_dir = lines[no + 1].direction();
        let new_dir = Direction::from_vector(&Vector::Int(prev_dir.get_vector()).add(&Vector::Int(next_dir.get_vector())));
        let translate_line = Line::from_point_direction(&Point::Int(current_corner.round()), &new_dir);
        let prev_dist = translate_line.signed_distance(&prev_corner).abs();
        let next_dist = translate_line.signed_distance(&next_corner).abs();
        if prev_dist == 0.0 || next_dist == 0.0 {
            return None;
        }
        let (mut max_translate_dist, nearest_corner) = if prev_dist <= next_dist {
            (prev_dist, prev_corner)
        } else {
            (next_dist, next_corner)
        };
        if max_translate_dist < 1.0 {
            return None;
        }
        max_translate_dist = (max_translate_dist - 1.0).max(1.0);
        if translate_line.side_of_float(&next_corner) == Side::OnTheLeft {
            max_translate_dist = -max_translate_dist;
        }
        let mut check_lines = [lines[no], lines[no], lines[no + 1]];
        let mut translate_dist = max_translate_dist;
        let mut delta_dist = max_translate_dist;
        let side_of_nearest = translate_line.side_of_float(&nearest_corner);
        let sign = Signum::as_int(max_translate_dist) as f64;
        let mut result: Option<Line> = None;
        while delta_dist.abs() > self.min_translate_dist as f64 {
            let mut ok = false;
            let new_line = translate_line.translate(translate_dist);
            let ns = new_line.side_of_float(&nearest_corner);
            if ns == side_of_nearest || ns == Side::Collinear {
                check_lines[1] = new_line;
                let tmp = Polyline::from_lines(&check_lines);
                if tmp.line_count() == 3 {
                    ok = self.check(board, &tmp, 0);
                }
                delta_dist /= 2.0;
                if ok {
                    result = Some(check_lines[1]);
                    if translate_dist == max_translate_dist {
                        break;
                    }
                    translate_dist += delta_dist;
                } else {
                    translate_dist -= delta_dist;
                }
            } else {
                let sv = sign * 0.5;
                max_translate_dist -= sv;
                translate_dist -= sv;
                delta_dist -= sv;
            }
        }
        if let Some(r) = result {
            if board.changed_area.is_some() {
                let a = check_lines[0].intersection_approx(&r);
                let b = check_lines[2].intersection_approx(&r);
                self.join_changed(board, &a);
                self.join_changed(board, &b);
                self.join_changed(board, &current_corner);
            }
        }
        result
    }

    // ---------------------------------------------------------------- any angle

    fn reduce_corners_any(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        let lines = lines_of(polyline);
        let n = lines.len();
        if n < 4 {
            return polyline.clone();
        }
        let last_index = n - 4;
        let mut new_lines: Vec<Line> = vec![lines[0]; n];
        new_lines[1] = lines[1];
        let mut new_index = 1usize;
        let mut changed = false;
        let mut current_lines = [lines[0]; 3];
        for i in 0..=last_index {
            let mut skip = false;
            let new_a = new_lines[new_index - 1].intersection_approx(&new_lines[new_index]);
            let new_b = polyline.corner_approx(i + 2);
            let in_clip = self.current_clip_shape.is_none()
                || self.clip_contains(&new_a) && self.clip_contains(&new_b) && self.clip_contains(&polyline.corner_approx(new_index));
            if in_clip {
                current_lines[1] = Line::new(new_a.round(), new_b.round());
                let mut ok = true;
                if new_index == 1 {
                    let fc = polyline.first_corner();
                    if !fc.is_int() {
                        ok = false;
                    } else {
                        let d = current_lines[1].direction();
                        current_lines[0] = Line::from_point_dir(fc.to_int(), &d.turn_45_degree(2));
                    }
                } else {
                    current_lines[0] = new_lines[new_index - 1];
                }
                if i == last_index {
                    let lc = polyline.last_corner();
                    if !lc.is_int() {
                        ok = false;
                    } else {
                        let d = current_lines[1].direction();
                        current_lines[2] = Line::from_point_dir(lc.to_int(), &d.turn_45_degree(2));
                    }
                } else {
                    current_lines[2] = lines[i + 3];
                }
                let check_dist = 100.0;
                if ok {
                    let is = current_lines[0].intersection_approx(&current_lines[1]);
                    if is.distance_square(&new_a) > check_dist {
                        ok = false;
                    }
                }
                if ok {
                    let is = current_lines[1].intersection_approx(&current_lines[2]);
                    if is.distance_square(&new_b) > check_dist {
                        ok = false;
                    }
                }
                if ok && i == 1 && !polyline.first_corner().is_int() {
                    let nc = current_lines[0].intersection(&current_lines[1]);
                    if nc.side_of_line(&new_lines[0]) != polyline.corner(1).side_of_line(&new_lines[0]) {
                        ok = false;
                    }
                }
                if ok && i + 1 == last_index && !polyline.last_corner().is_int() {
                    let nc = current_lines[1].intersection(&current_lines[2]);
                    if nc.side_of_line(&new_lines[0]) != polyline.corner(polyline.corner_count() - 2).side_of_line(&new_lines[0]) {
                        ok = false;
                    }
                }
                let mut current_polyline: Option<Polyline> = None;
                if ok {
                    let skip_corner = new_lines[new_index].intersection_approx(&lines[i + 2]);
                    let cp = Polyline::from_lines(&current_lines);
                    if cp.line_count() != 3 {
                        ok = false;
                    }
                    let length_before = skip_corner.distance(&new_a) + skip_corner.distance(&new_b);
                    let length_after = cp.length_approx() + 1.5;
                    if length_after >= length_before {
                        ok = false;
                    }
                    current_polyline = Some(cp);
                }
                if ok {
                    skip = self.check(board, current_polyline.as_ref().unwrap(), 0);
                }
            }
            if skip {
                changed = true;
                new_lines[new_index] = current_lines[1];
                if new_index == 1 {
                    new_lines[0] = current_lines[0];
                }
                if i == last_index {
                    new_index += 1;
                    new_lines[new_index] = current_lines[2];
                }
                if board.changed_area.is_some() {
                    self.join_changed(board, &new_a);
                    self.join_changed(board, &new_b);
                }
            } else {
                new_index += 1;
                new_lines[new_index] = lines[i + 2];
                if i == last_index {
                    new_index += 1;
                    new_lines[new_index] = lines[i + 3];
                }
            }
            if new_lines[new_index].is_parallel(&new_lines[new_index - 1]) {
                new_index -= 1;
            }
        }
        if !changed {
            return polyline.clone();
        }
        new_lines.truncate(new_index + 1);
        Polyline::from_lines(&new_lines)
    }

    fn smoothen_corners_any(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        if polyline.line_count() < 4 {
            return polyline.clone();
        }
        let mut changed = false;
        let mut lines: Vec<Line> = polyline.lines().to_vec();
        let mut i = 0usize;
        while i + 3 < lines.len() {
            if let Some(nl) = self.smoothen_corner_any(board, &lines, i) {
                changed = true;
                lines.insert(i + 2, nl);
                i += 1;
            }
            i += 1;
        }
        if !changed {
            return polyline.clone();
        }
        Polyline::from_lines(&lines)
    }

    fn reposition_lines_any(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        if polyline.line_count() < 5 {
            return polyline.clone();
        }
        let mut changed = false;
        let mut lines: Vec<Line> = polyline.lines().to_vec();
        let mut i = 0usize;
        while i + 4 < lines.len() {
            if let Some(nl) = self.reposition_line_any(board, &lines, i) {
                changed = true;
                lines[i + 2] = nl;
                if lines[i + 2].is_parallel(&lines[i + 1]) || lines[i + 2].is_parallel(&lines[i + 3]) {
                    break;
                }
            }
            i += 1;
        }
        if !changed {
            return polyline.clone();
        }
        Polyline::from_lines(&lines)
    }

    fn reduce_lines_any(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        if polyline.line_count() < 6 {
            return polyline.clone();
        }
        let mut changed = false;
        let mut lines: Vec<Line> = polyline.lines().to_vec();
        let mut i: isize = 2;
        while (i as usize) + 2 < lines.len() {
            let iu = i as usize;
            let prev_corner = lines[iu - 2].intersection_approx(&lines[iu - 1]);
            let next_corner = lines[iu + 1].intersection_approx(&lines[iu + 2]);
            if !(self.clip_contains(&prev_corner) && self.clip_contains(&next_corner)) {
                i += 1;
                continue;
            }
            let translate_line = lines[iu];
            let prev_dist = translate_line.signed_distance(&prev_corner);
            let next_dist = translate_line.signed_distance(&next_corner);
            if Signum::of(prev_dist) != Signum::of(next_dist) {
                i += 1;
                continue;
            }
            let mut translate_dist = if prev_dist.abs() < next_dist.abs() { prev_dist } else { next_dist };
            if translate_dist == 0.0 {
                i += 1;
                continue;
            }
            let line_side = translate_line.side_of_float(&prev_corner);
            let mut new_line = translate_line.translate(-translate_dist);
            let sign = Signum::as_int(translate_dist) as f64;
            let mut s_prev = new_line.side_of_float(&prev_corner);
            let mut s_next = new_line.side_of_float(&next_corner);
            while s_prev == line_side && s_next == line_side {
                translate_dist += sign * 0.5;
                new_line = translate_line.translate(-translate_dist);
                s_prev = new_line.side_of_float(&prev_corner);
                s_next = new_line.side_of_float(&next_corner);
            }
            let crossed_before = if s_prev != line_side { 1usize } else { 0 };
            let crossed_after = if s_next != line_side { 1usize } else { 0 };
            if crossed_before > 0 {
                if iu < 3 {
                    i += 1;
                    continue;
                }
                let ppc = lines[iu - 3].intersection_approx(&lines[iu - 2]);
                if new_line.side_of_float(&ppc) != line_side {
                    i += 1;
                    continue;
                }
            }
            if crossed_after > 0 {
                if iu >= lines.len() - 3 {
                    i += 1;
                    continue;
                }
                let nnc = lines[iu + 2].intersection_approx(&lines[iu + 3]);
                if new_line.side_of_float(&nnc) != line_side {
                    i += 1;
                    continue;
                }
            }
            let keep_before = iu - crossed_before;
            let mut current_lines: Vec<Line> = Vec::with_capacity(lines.len() - crossed_before - crossed_after);
            current_lines.extend_from_slice(&lines[..keep_before]);
            current_lines.push(new_line);
            current_lines.extend_from_slice(&lines[iu + 1 + crossed_after..]);
            let tmp = Polyline::from_lines(&current_lines);
            let mut ok = false;
            if tmp.line_count() == current_lines.len() {
                ok = self.check(board, &tmp, keep_before - 1);
            }
            if ok {
                self.join_changed(board, &prev_corner);
                self.join_changed(board, &next_corner);
                changed = true;
                lines = current_lines;
                i -= 1;
            }
            i += 1;
        }
        if !changed {
            return polyline.clone();
        }
        Polyline::from_lines(&lines)
    }

    fn smoothen_corner_any(&mut self, board: &mut Board, lines: &[Line], start_no: usize) -> Option<Line> {
        if lines.len() - start_no < 4 {
            return None;
        }
        let current_corner = lines[start_no + 1].intersection_approx(&lines[start_no + 2]);
        if !self.clip_contains(&current_corner) {
            return None;
        }
        let cos = lines[start_no + 1].cos_angle(&lines[start_no + 2]);
        if cos > C_MAX_COS_ANGLE {
            return None;
        }
        let prev_corner = lines[start_no].intersection_approx(&lines[start_no + 1]);
        let next_corner = lines[start_no + 2].intersection_approx(&lines[start_no + 3]);
        let prev_dir = Direction::Int(lines[start_no + 1].direction());
        let next_dir = Direction::Int(lines[start_no + 2].direction());
        let middle = prev_dir.middle_approx(&next_dir);
        let translate_line = Line::from_point_direction(&Point::Int(current_corner.round()), &middle);
        let prev_dist = translate_line.signed_distance(&prev_corner);
        let next_dist = translate_line.signed_distance(&next_corner);
        let (nearest_point, mut max_translate_dist) = if prev_dist.abs() < next_dist.abs() {
            (prev_corner, prev_dist)
        } else {
            (next_corner, next_dist)
        };
        if max_translate_dist.abs() < 1.0 {
            return None;
        }
        let mut current_lines: Vec<Line> = Vec::with_capacity(lines.len() + 1);
        current_lines.extend_from_slice(&lines[..start_no + 2]);
        current_lines.push(lines[start_no + 2]);
        current_lines.extend_from_slice(&lines[start_no + 2..]);
        let mut translate_dist = max_translate_dist;
        let mut delta_dist = max_translate_dist;
        let side_of_nearest = translate_line.side_of_float(&nearest_point);
        let sign = Signum::as_int(max_translate_dist) as f64;
        let mut result: Option<Line> = None;
        while delta_dist.abs() > self.min_translate_dist as f64 {
            let mut ok = false;
            let new_line = translate_line.translate(-translate_dist);
            let ns = new_line.side_of_float(&nearest_point);
            if ns == side_of_nearest || ns == Side::Collinear {
                current_lines[start_no + 2] = new_line;
                let tmp = Polyline::from_lines(&current_lines);
                if tmp.line_count() == current_lines.len() {
                    ok = self.check(board, &tmp, start_no + 1);
                }
                delta_dist /= 2.0;
                if ok {
                    result = Some(current_lines[start_no + 2]);
                    if translate_dist == max_translate_dist {
                        break;
                    }
                    translate_dist += delta_dist;
                } else {
                    translate_dist -= delta_dist;
                }
            } else {
                let sv = sign * 0.5;
                max_translate_dist -= sv;
                translate_dist -= sv;
                delta_dist -= sv;
            }
        }
        let r = result?;
        if board.changed_area.is_some() {
            let a = current_lines[start_no].intersection_approx(&current_lines[start_no + 1]);
            let b = current_lines[start_no + 3].intersection_approx(&current_lines[start_no + 4]);
            self.join_changed(board, &a);
            self.join_changed(board, &b);
        }
        Some(r)
    }

    /// `TraceTightenerAnyAngle.repositionLine` (different index semantics than the base).
    fn reposition_line_any(&mut self, board: &mut Board, lines: &[Line], start_no: usize) -> Option<Line> {
        if lines.len() - start_no < 5 {
            return None;
        }
        if self.current_clip_shape.is_some() {
            for i in 1..3 {
                let c = lines[start_no + i].intersection_approx(&lines[start_no + i + 1]);
                if !self.clip_contains(&c) {
                    return None;
                }
            }
        }
        let translate_line = lines[start_no + 2];
        let mut prev_corner = lines[start_no].intersection_approx(&lines[start_no + 1]);
        let mut next_corner = lines[start_no + 3].intersection_approx(&lines[start_no + 4]);
        let mut prev_dist = translate_line.signed_distance(&prev_corner);
        let mut skipped_before = 0usize;
        let mut skipped_after = 0usize;
        let eps = 0.001;
        while prev_dist.abs() < eps {
            skipped_before += 1;
            let cur = start_no as isize - skipped_before as isize;
            if cur < 0 {
                return None;
            }
            let cur = cur as usize;
            prev_corner = lines[cur].intersection_approx(&lines[cur + 1]);
            prev_dist = translate_line.signed_distance(&prev_corner);
        }
        let mut next_dist = translate_line.signed_distance(&next_corner);
        while next_dist.abs() < eps {
            skipped_after += 1;
            let cur = start_no + 3 + skipped_after;
            if cur >= lines.len() - 2 {
                return None;
            }
            next_corner = lines[cur].intersection_approx(&lines[cur + 1]);
            next_dist = translate_line.signed_distance(&next_corner);
        }
        if Signum::of(prev_dist) != Signum::of(next_dist) {
            return None;
        }
        let (nearest_point, mut max_translate_dist) = if prev_dist.abs() < next_dist.abs() {
            (prev_corner, prev_dist)
        } else {
            (next_corner, next_dist)
        };
        let mut current_lines: Vec<Line> = lines.to_vec();
        let mut translate_dist = max_translate_dist;
        let mut delta_dist = max_translate_dist;
        let side_of_nearest = translate_line.side_of_float(&nearest_point);
        let sign = Signum::as_int(max_translate_dist) as f64;
        let mut result: Option<Line> = None;
        let mut first_time = true;
        while first_time || delta_dist.abs() > self.min_translate_dist as f64 {
            let mut ok = false;
            let mut new_line = translate_line.translate(-translate_dist);
            if first_time && translate_dist.abs() < 1.0 {
                if new_line == translate_line {
                    let rounded = nearest_point.round();
                    if nearest_point.distance(&rounded.to_float()) < translate_dist.abs() {
                        new_line = Line::from_point_dir(rounded, &translate_line.direction());
                    }
                    first_time = false;
                }
                if new_line == translate_line {
                    return None;
                }
            }
            let ns = new_line.side_of_float(&nearest_point);
            if ns == side_of_nearest || ns == Side::Collinear {
                first_time = false;
                current_lines[start_no + 2] = new_line;
                let mut prev_translated = new_line;
                for i in 0..skipped_before {
                    let prev_line_no = start_no + 1 - skipped_before;
                    let cpc = prev_translated.intersection_approx(&current_lines[prev_line_no]);
                    let ctl = lines[start_no + 1 - i];
                    let ctd = ctl.signed_distance(&cpc);
                    prev_translated = ctl.translate(-ctd);
                    current_lines[start_no + 1 - i] = prev_translated;
                }
                prev_translated = new_line;
                for i in 0..skipped_after {
                    let next_line_no = start_no + 3 + skipped_after;
                    let cnc = prev_translated.intersection_approx(&current_lines[next_line_no]);
                    let ctl = lines[start_no + 3 + i];
                    let ctd = ctl.signed_distance(&cnc);
                    prev_translated = ctl.translate(-ctd);
                    current_lines[start_no + 3 + i] = prev_translated;
                }
                let tmp = Polyline::from_lines(&current_lines);
                if tmp.line_count() == current_lines.len() {
                    ok = self.check(board, &tmp, start_no + 1);
                }
                delta_dist /= 2.0;
                if ok {
                    result = Some(current_lines[start_no + 2]);
                    if translate_dist == max_translate_dist {
                        break;
                    }
                    translate_dist += delta_dist;
                } else {
                    translate_dist -= delta_dist;
                }
            } else {
                let sv = sign * 0.5;
                max_translate_dist -= sv;
                translate_dist -= sv;
                delta_dist -= sv;
            }
        }
        let r = result?;
        if board.changed_area.is_some() {
            let a = current_lines[start_no].intersection_approx(&current_lines[start_no + 1]);
            let b = current_lines[start_no + 3].intersection_approx(&current_lines[start_no + 4]);
            self.join_changed(board, &a);
            self.join_changed(board, &b);
        }
        Some(r)
    }

    fn skip_lines_any(&mut self, board: &mut Board, polyline: &Polyline) -> Polyline {
        let lines = lines_of(polyline);
        let n = lines.len();
        let mut i = 1usize;
        while i + 3 < n {
            for j in 0..=1 {
                let (current_line, c1, c2) = if j == 0 {
                    (lines[i + 2], polyline.corner_approx(i), polyline.corner_approx(i - 1))
                } else {
                    (lines[i], polyline.corner_approx(i + 1), polyline.corner_approx(i + 2))
                };
                if !(self.clip_contains(&c1) && self.clip_contains(&c2)) {
                    continue;
                }
                let mut s1 = current_line.side_of_float(&c1);
                let mut s2 = current_line.side_of_float(&c2);
                if s1 != s2 {
                    let reduced = polyline.skip_lines(i + 1, i + 1);
                    if reduced.line_count() == n - 1 {
                        let shape_index = if j == 0 { i } else { i - 1 };
                        if self.check(board, &reduced, shape_index) {
                            self.join_changed(board, &c1);
                            self.join_changed(board, &c2);
                            return reduced;
                        }
                    }
                }
                if i + 4 >= n {
                    break;
                }
                let c3 = if j == 1 {
                    polyline.corner_approx(i + 3)
                } else {
                    polyline.corner_approx(i + 1)
                };
                if !self.clip_contains(&c3) {
                    continue;
                }
                if j == 0 {
                    let cl = lines[i + 3];
                    s1 = cl.side_of_float(&c1);
                    s2 = cl.side_of_float(&c2);
                } else {
                    s1 = current_line.side_of_float(&c3);
                }
                if s1 != s2 {
                    let reduced = polyline.skip_lines(i + 1, i + 2);
                    if reduced.line_count() == n - 2 {
                        let shape_index = if j == 0 { i } else { i - 1 };
                        if self.check(board, &reduced, shape_index) {
                            self.join_changed(board, &c1);
                            self.join_changed(board, &c2);
                            self.join_changed(board, &c3);
                            return reduced;
                        }
                    }
                }
            }
            i += 1;
        }
        polyline.clone()
    }

    // ---------------------------------------------------------------- trace level

    /// `PolylineTrace.pullTight(TraceTightener)`.
    pub fn pull_tight_trace(&mut self, board: &mut Board, id: ItemId) -> bool {
        let Some(item) = board.try_item(id) else { return false };
        if !item.on_the_board || board.is_shove_fixed(item) || !item.nets_normal() {
            return false;
        }
        if !self.only_net_no_arr.is_empty() && !item.nets_equal(&self.only_net_no_arr) {
            return false;
        }
        if let Some(n) = item.net_numbers.first() {
            if let Some(c) = board.rules.net_class_of(*n) {
                if !c.get_pull_tight() {
                    return false;
                }
            }
        }
        let t = item.as_trace().unwrap().clone();
        let nets = item.net_numbers.to_vec();
        let cl = item.clearance_class;
        let contact_pins = board.touching_pins_at_end_corners(item);
        let new_lines = self.pull_tight_polyline(board, &t.polyline, t.layer, t.half_width, &nets, cl, Some(contact_pins));
        if !new_lines.ptr_eq(&t.polyline) {
            board.change_trace(id, new_lines);
            return true;
        }
        let ar = board.rules.get_trace_angle_restriction();
        if ar != AngleRestriction::NinetyDegree && board.rules.get_pin_edge_to_turn_dist() > 0.0 {
            if board.swap_connection_to_pin(id, true) {
                self.pull_tight_trace(board, id);
                return true;
            }
            if board.items.contains(id) && board.swap_connection_to_pin(id, false) {
                self.pull_tight_trace(board, id);
                return true;
            }
            if board.items.contains(id) && board.correct_connection_to_pin(id, true, ar) {
                self.pull_tight_trace(board, id);
                return true;
            }
            if board.items.contains(id) && board.correct_connection_to_pin(id, false, ar) {
                self.pull_tight_trace(board, id);
                return true;
            }
        }
        false
    }
}
