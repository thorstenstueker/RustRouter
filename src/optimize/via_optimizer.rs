//! Optimization of via locations (port of `ViaOptimizer`).

use super::shove::{drill_item_check, drill_item_insert};
use super::trace_tightener::{ExpansionCostFactor, TraceTightener};
use crate::board::item::ItemId;
use crate::board::Board;
use crate::geometry::{FloatLine, FloatPoint, IntPoint, Point, Side, Vector};
use crate::rules::AngleRestriction;

fn is_within_tolerance(p1: &Point, p2: &Point, tolerance: i32) -> bool {
    let a = p1.to_float();
    let b = p2.to_float();
    (a.x - b.x).abs() + (a.y - b.y).abs() <= tolerance as f64
}

fn pull_tight_traces_at(board: &mut Board, location: &Point, layer: i32, accuracy: i32) {
    for id in board.pick_traces(location, layer) {
        if !board.items.contains(id) {
            continue;
        }
        let nets = board.item(id).net_numbers.to_vec();
        let mut tt = TraceTightener::new(board, &nets, None, accuracy, None, -1, None, -1);
        tt.pull_tight_trace(board, id);
    }
}

/// `ViaOptimizer.optViaLocation`.
pub fn opt_via_location(
    board: &mut Board,
    via: ItemId,
    trace_costs: &[ExpansionCostFactor],
    accuracy: i32,
    max_recursion_depth: i32,
) -> bool {
    let Some(item) = board.try_item(via) else { return false };
    if board.is_shove_fixed(item) {
        return false;
    }
    if max_recursion_depth <= 0 {
        return false;
    }
    let contacts: Vec<ItemId> = board.normal_contacts(via).into_iter().collect();
    let mut plane_or_fanout = contacts.len() == 1;
    let mut first_trace: Option<ItemId> = None;
    let mut second_trace: Option<ItemId> = None;
    if !plane_or_fanout {
        if contacts.len() != 2 {
            return false;
        }
        for (k, c) in contacts.iter().enumerate() {
            let ci = board.item(*c);
            if board.is_shove_fixed(ci) || !ci.is_trace() {
                if ci.is_conduction_area() {
                    plane_or_fanout = true;
                } else {
                    return false;
                }
            } else if k == 0 {
                first_trace = Some(*c);
            } else {
                second_trace = Some(*c);
            }
        }
    }
    if plane_or_fanout {
        return opt_plane_or_fanout_via(board, via, accuracy, max_recursion_depth);
    }
    let (Some(ft), Some(st)) = (first_trace, second_trace) else {
        return false;
    };
    let via_center = board.item(via).as_drill().unwrap().center.clone();
    let signal: Vec<bool> = board.layer_structure.layers.iter().map(|l| l.is_signal).collect();
    let tolerance = (board.item(via).as_drill().unwrap().min_width(&signal) / 2.0) as i32 + 1;
    let fti = board.item(ft).clone();
    let sti = board.item(st).clone();
    let ftt = fti.as_trace().unwrap();
    let stt = sti.as_trace().unwrap();
    let ft_from = if is_within_tolerance(&ftt.first_corner(), &via_center, tolerance) {
        ftt.polyline.corner(1)
    } else if is_within_tolerance(&ftt.last_corner(), &via_center, tolerance) {
        ftt.polyline.corner(ftt.polyline.corner_count() - 2)
    } else {
        return false;
    };
    let st_from = if is_within_tolerance(&stt.first_corner(), &via_center, tolerance) {
        stt.polyline.corner(1)
    } else if is_within_tolerance(&stt.last_corner(), &via_center, tolerance) {
        stt.polyline.corner(stt.polyline.corner_count() - 2)
    } else {
        return false;
    };
    let fc = trace_costs.get(ftt.layer as usize).copied().unwrap_or(ExpansionCostFactor { horizontal: 1.0, vertical: 1.0 });
    let sc = trace_costs.get(stt.layer as usize).copied().unwrap_or(fc);
    let new_location = reposition_via_two(
        board,
        via,
        ftt.half_width,
        fti.clearance_class,
        ftt.layer,
        fc,
        &ft_from,
        stt.half_width,
        sti.clearance_class,
        stt.layer,
        sc,
        &st_from,
    );
    let Some(new_location) = new_location else { return false };
    if new_location == via_center {
        return false;
    }
    let delta = new_location.difference_by(&via_center);
    if !drill_item_insert(board, via, &delta, 9, 9, None) {
        log::warn!("OptViaAlgo.opt_via_location: move via failed");
        return false;
    }
    pull_tight_traces_at(board, &new_location, ftt.layer, accuracy);
    pull_tight_traces_at(board, &new_location, stt.layer, accuracy);
    let vias: Vec<ItemId> = board
        .pick_items(&new_location, ftt.layer)
        .into_iter()
        .filter(|i| board.item(*i).is_via())
        .collect();
    if let Some(v) = vias.first() {
        opt_via_location(board, *v, trace_costs, accuracy, max_recursion_depth - 1);
    }
    true
}

fn opt_plane_or_fanout_via(board: &mut Board, via: ItemId, accuracy: i32, max_recursion_depth: i32) -> bool {
    if max_recursion_depth <= 0 {
        return false;
    }
    let contacts: Vec<ItemId> = board.normal_contacts(via).into_iter().collect();
    if contacts.is_empty() {
        return false;
    }
    let mut plane: Option<ItemId> = None;
    let mut trace: Option<ItemId> = None;
    for c in &contacts {
        let ci = board.item(*c);
        if ci.is_conduction_area() {
            if plane.is_some() {
                return false;
            }
            plane = Some(*c);
        } else if ci.is_trace() {
            if board.is_shove_fixed(ci) || trace.is_some() {
                return false;
            }
            trace = Some(*c);
        } else {
            return false;
        }
    }
    let Some(trace) = trace else { return false };
    let via_center = board.item(via).as_drill().unwrap().center.clone();
    let signal: Vec<bool> = board.layer_structure.layers.iter().map(|l| l.is_signal).collect();
    let tolerance = (board.item(via).as_drill().unwrap().min_width(&signal) / 2.0) as i32 + 1;
    let ti = board.item(trace).clone();
    let t = ti.as_trace().unwrap();
    let at_first = if is_within_tolerance(&t.first_corner(), &via_center, tolerance) {
        true
    } else if is_within_tolerance(&t.last_corner(), &via_center, tolerance) {
        false
    } else {
        return false;
    };
    let pl = &t.polyline;
    let check_corner = if at_first {
        pl.corner(1)
    } else {
        pl.corner(pl.corner_count() - 2)
    };
    let rounded = check_corner.to_float().round();
    let (hw, layer, cl) = (t.half_width, t.layer, ti.clearance_class);
    let mut new_loc = reposition_via_to(board, via, rounded, hw, layer, cl);
    if new_loc.is_none() && pl.corner_count() >= 3 {
        let prev_corner = if at_first {
            pl.corner(2)
        } else {
            pl.corner(pl.corner_count() - 3)
        };
        let fcc = check_corner.to_float();
        let fvc = via_center.to_float();
        let fpc = prev_corner.to_float();
        if fcc.scalar_product(&fvc, &fpc) != 0.0 {
            let line = FloatLine::new(fcc, fpc);
            let projection = Point::Int(line.perpendicular_projection(&fvc).round());
            let diff = projection.difference_by(&via_center);
            let ar = board.rules.get_trace_angle_restriction();
            let ok = !(projection == via_center
                || ar == AngleRestriction::NinetyDegree && !diff.is_orthogonal()
                || ar == AngleRestriction::FortyfiveDegree && !diff.is_multiple_of_45_degree());
            if ok && drill_item_check(board, via, &diff, 0, 0, None, None) {
                let nets = board.item(via).net_numbers.to_vec();
                let ok_len = board.check_trace_segment_points(&via_center, &projection, layer, &nets, hw, cl, false);
                if ok_len >= i32::MAX as f64 {
                    new_loc = Some(projection);
                }
            }
        }
    }
    let Some(new_loc) = new_loc else { return false };
    if let Some(p) = plane {
        let player = board.item(p).first_layer();
        let ok = board.pick_items(&new_loc, player).contains(&p);
        if !ok {
            return false;
        }
    }
    let diff = new_loc.difference_by(&via_center);
    if !drill_item_insert(board, via, &diff, 9, 9, None) {
        log::warn!("OptViaAlgo.opt_plane_or_fanout_via: move via failed");
        return false;
    }
    pull_tight_traces_at(board, &new_loc, layer, accuracy);
    if new_loc == check_corner {
        opt_plane_or_fanout_via(board, via, accuracy, max_recursion_depth - 1);
    }
    true
}

fn reposition_via_to(board: &mut Board, via: ItemId, to: IntPoint, hw: i32, layer: i32, cl: i32) -> Option<Point> {
    let from = board.item(via).as_drill().unwrap().center.clone();
    let to_p = Point::Int(to);
    if from == to_p {
        return None;
    }
    let nets = board.item(via).net_numbers.to_vec();
    let mut ok_length = board.check_trace_segment_points(&from, &to_p, layer, &nets, hw, cl, false);
    if ok_length <= 0.0 {
        return None;
    }
    let ff = from.to_float();
    let ft = to.to_float();
    let nft = if ok_length >= i32::MAX as f64 { ft } else { ff.change_length(&ft, ok_length) };
    let new_to = Point::Int(nft.round());
    let delta = new_to.difference_by(&from);
    if drill_item_check(board, via, &delta, 0, 0, None, None) {
        return Some(new_to);
    }
    let min_length = 0.3 * hw as f64 + 1.0;
    ok_length = ok_length.min(ff.distance(&ft));
    let mut current_length = ok_length / 2.0;
    ok_length = 0.0;
    let mut result = None;
    while current_length >= min_length {
        let cp = Point::Int(ff.change_length(&ft, ok_length + current_length).round());
        let delta = cp.difference_by(&from);
        if drill_item_check(board, via, &delta, 0, 0, None, None) {
            ok_length += current_length;
            result = Some(cp);
        }
        current_length /= 2.0;
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn reposition_via_connect(
    board: &mut Board,
    via: ItemId,
    to: IntPoint,
    hw1: i32,
    layer1: i32,
    cl1: i32,
    connect: IntPoint,
    hw2: i32,
    layer2: i32,
    cl2: i32,
) -> bool {
    let from = board.item(via).as_drill().unwrap().center.clone();
    let to_p = Point::Int(to);
    if from == to_p {
        return false;
    }
    let delta = to_p.difference_by(&from);
    if board.rules.get_trace_angle_restriction() == AngleRestriction::None && delta.length_approx() <= 1.5 {
        return false;
    }
    let nets = board.item(via).net_numbers.to_vec();
    if board.check_trace_segment_points(&from, &to_p, layer1, &nets, hw1, cl1, false) < i32::MAX as f64 {
        return false;
    }
    if board.check_trace_segment_points(&to_p, &Point::Int(connect), layer2, &nets, hw2, cl2, false) < i32::MAX as f64 {
        return false;
    }
    drill_item_check(board, via, &delta, 0, 0, None, None)
}

#[allow(clippy::too_many_arguments)]
fn reposition_via_two(
    board: &mut Board,
    via: ItemId,
    fhw: i32,
    fcl: i32,
    fl: i32,
    fc: ExpansionCostFactor,
    ff_corner: &Point,
    shw: i32,
    scl: i32,
    sl: i32,
    sc: ExpansionCostFactor,
    sf_corner: &Point,
) -> Option<Point> {
    let via_location = board.item(via).as_drill().unwrap().center.clone();
    let first_delta = ff_corner.difference_by(&via_location);
    let second_delta = sf_corner.difference_by(&via_location);
    let scalar = first_delta.scalar_product(&second_delta);
    let fvl = via_location.to_float();
    let ffc = ff_corner.to_float();
    let fsc = sf_corner.to_float();
    let f_dist = fvl.distance(&ffc);
    let s_dist = fvl.distance(&fsc);
    let rf = ffc.round();
    let rs = fsc.round();
    if via_location.side_of(ff_corner, sf_corner) == Side::Collinear && scalar > 0.0 {
        if s_dist < f_dist {
            return reposition_via_to(board, via, rs, fhw, fl, fcl);
        }
        return reposition_via_to(board, via, rf, shw, sl, scl);
    }
    let mut w1 = fvl.weighted_distance(&ffc, fc.horizontal, fc.vertical);
    let mut w2 = fvl.weighted_distance(&ffc, sc.horizontal, sc.vertical);
    if w1 > w2 {
        if let Some(r) = reposition_via_to(board, via, rf, shw, sl, scl) {
            return Some(r);
        }
    }
    w1 = fvl.weighted_distance(&fsc, sc.horizontal, sc.vertical);
    w2 = fvl.weighted_distance(&fsc, fc.horizontal, fc.vertical);
    if w1 > w2 {
        if let Some(r) = reposition_via_to(board, via, rs, fhw, fl, fcl) {
            return Some(r);
        }
    }
    if scalar > 0.0 && board.rules.get_trace_angle_restriction() != AngleRestriction::NinetyDegree {
        let (tp1, ftp1, tp2, ftp2);
        if f_dist < s_dist {
            tp1 = rf;
            ftp1 = ffc;
            ftp2 = fvl.change_length(&fsc, f_dist);
            tp2 = ftp2.round();
        } else {
            ftp1 = fvl.change_length(&ffc, s_dist);
            tp1 = ftp1.round();
            tp2 = rs;
            ftp2 = fsc;
        }
        let w1 = ftp1.weighted_distance(&ftp2, fc.horizontal, fc.vertical);
        let w2 = ftp1.weighted_distance(&ftp2, sc.horizontal, sc.vertical);
        let result = if w1 > w2 {
            reposition_via_to(board, via, tp1, shw, sl, scl).or_else(|| reposition_via_to(board, via, tp2, fhw, fl, fcl))
        } else {
            reposition_via_to(board, via, tp2, fhw, fl, fcl).or_else(|| reposition_via_to(board, via, tp1, shw, sl, scl))
        };
        if result.is_some() {
            return result;
        }
    }
    if !first_delta.is_orthogonal() {
        let w1 = fvl.weighted_distance(&ffc, fc.horizontal, fc.vertical);
        for check in [FloatPoint::new(fvl.x, ffc.y), FloatPoint::new(ffc.x, fvl.y)] {
            let w2 = fvl.weighted_distance(&check, sc.horizontal, sc.vertical);
            let w3 = check.weighted_distance(&ffc, fc.horizontal, fc.vertical);
            if w1 > w2 + w3 {
                let cl = check.round();
                if reposition_via_connect(board, via, cl, shw, sl, scl, rf, fhw, fl, fcl) {
                    return Some(Point::Int(cl));
                }
            }
        }
    }
    if !second_delta.is_orthogonal() {
        let w1 = fvl.weighted_distance(&fsc, sc.horizontal, sc.vertical);
        for check in [FloatPoint::new(fvl.x, fsc.y), FloatPoint::new(fsc.x, fvl.y)] {
            let w2 = fvl.weighted_distance(&check, fc.horizontal, fc.vertical);
            let w3 = check.weighted_distance(&fsc, sc.horizontal, sc.vertical);
            if w1 > w2 + w3 {
                let cl = check.round();
                if reposition_via_connect(board, via, cl, fhw, fl, fcl, rs, shw, sl, scl) {
                    return Some(Point::Int(cl));
                }
            }
        }
    }
    None
}

#[allow(dead_code)]
fn _unused(_: Vector) {}
