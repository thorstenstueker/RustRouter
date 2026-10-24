//! Routing operations of the board (the interactive/automatic routing methods of
//! `RoutingBoard` and `RoutingBoardOperations`).

use super::connectivity::StopConnectionOption;
use super::item::ItemId;
use super::{Board, FixedState};
use crate::datastructures::{StopFlag, TimeLimit};
use crate::geometry::{IntOctagon, Point, Polyline, TileShape};
use crate::optimize::shove::{self, ShapeEntrySide};
use crate::optimize::{ExpansionCostFactor, TraceTightener};
use crate::rules::{AngleRestriction, ViaInfo};
use std::collections::BTreeSet;

const PULL_TIGHT_TIME_LIMIT: i32 = 2000;

impl Board {
    /// Optimizes the changed area of the board and stops marking it (`optChangedArea`).
    #[allow(clippy::too_many_arguments)]
    pub fn opt_changed_area(
        &mut self,
        only_nets: &[i32],
        clip_shape: Option<IntOctagon>,
        accuracy: i32,
        trace_costs: Option<&[ExpansionCostFactor]>,
        stop: Option<StopFlag>,
        time_limit: i32,
        keep_point: Option<Point>,
        keep_point_layer: i32,
    ) {
        if self.changed_area.is_none() {
            return;
        }
        if clip_shape != Some(IntOctagon::EMPTY) {
            let mut tt = TraceTightener::new(self, only_nets, clip_shape, accuracy, stop, time_limit, keep_point, keep_point_layer);
            tt.opt_changed_area(self, trace_costs);
        }
        self.changed_area = None;
    }

    /// Removes items and pulls the remaining traces tight (`removeItemsAndPullTight`).
    pub fn remove_items_and_pull_tight(&mut self, items: &[ItemId], tidy_width: i32, accuracy: i32) -> bool {
        let mut result = true;
        let mut tidy_region: Option<IntOctagon> = if tidy_width < i32::MAX { Some(IntOctagon::EMPTY) } else { None };
        let calculate_tidy = tidy_width < i32::MAX && tidy_width > 0;
        self.start_marking_changed_area();
        let mut changed_nets: BTreeSet<i32> = BTreeSet::new();
        for id in items {
            let Some(item) = self.try_item(*id) else { continue };
            if self.is_deletion_forbidden(item) || item.is_user_fixed() {
                result = false;
                continue;
            }
            for i in 0..self.tile_shape_count(*id) {
                let shape = self.tile_shape(*id, i).unwrap();
                let layer = self.shape_layer(*id, i);
                if let Some(c) = self.changed_area.as_mut() {
                    c.join_shape(&shape, layer);
                }
                if calculate_tidy {
                    tidy_region = Some(tidy_region.unwrap().union(&shape.bounding_octagon()));
                }
            }
            let nets: Vec<i32> = self.item(*id).net_numbers.to_vec();
            self.remove_item(*id);
            changed_nets.extend(nets);
        }
        for n in changed_nets {
            self.combine_traces(n);
        }
        if calculate_tidy {
            tidy_region = Some(tidy_region.unwrap().enlarge(tidy_width as f64));
        }
        self.opt_changed_area(&[], tidy_region, accuracy, None, None, PULL_TIGHT_TIME_LIMIT, None, -1);
        result
    }

    /// Checks, if a trace polyline can be inserted by shoving obstacles away.
    #[allow(clippy::too_many_arguments)]
    pub fn check_forced_trace_polyline(
        &mut self,
        polyline: &Polyline,
        half_width: i32,
        layer: i32,
        nets: &[i32],
        clearance_class: i32,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        max_spring_over_recursion_depth: i32,
    ) -> bool {
        let comp_hw = half_width + self.search_trees.default_tree().clearance_compensation_value(&self.rules, clearance_class, layer);
        let shapes = polyline.offset_shapes_range(comp_hw, 0, polyline.line_count() as i32 - 1);
        let ortho = self.rules.get_trace_angle_restriction() == AngleRestriction::NinetyDegree;
        for (i, s) in shapes.into_iter().enumerate() {
            let s = if ortho { TileShape::Box(s.bounding_box()) } else { s };
            let from_side = ShapeEntrySide::from_polyline(polyline, i + 1, &s);
            if !shove::shove_check(
                self,
                &s,
                Some(&from_side),
                None,
                layer,
                nets,
                clearance_class,
                max_recursion_depth,
                max_via_recursion_depth,
                max_spring_over_recursion_depth,
                None,
            ) {
                return false;
            }
        }
        true
    }

    /// Inserts a trace segment by shoving obstacles (`insertForcedTraceSegment`).
    /// Returns the end point of the inserted part, `None` if the board was damaged.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_forced_trace_segment(
        &mut self,
        from: &Point,
        to: &Point,
        half_width: i32,
        layer: i32,
        nets: &[i32],
        clearance_class: i32,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        max_spring_over_recursion_depth: i32,
        tidy_width: i32,
        accuracy: i32,
        with_check: bool,
        time_limit: Option<&TimeLimit>,
    ) -> Option<Point> {
        if from == to {
            return Some(to.clone());
        }
        let pl = Polyline::from_corners(from, to);
        self.insert_forced_trace_polyline(
            &pl,
            half_width,
            layer,
            nets,
            clearance_class,
            max_recursion_depth,
            max_via_recursion_depth,
            max_spring_over_recursion_depth,
            tidy_width,
            accuracy,
            with_check,
            time_limit,
        )
    }

    /// Inserts a trace polyline by shoving obstacles (`insertForcedTracePolyline`).
    /// Returns the end point of the inserted part or the from point, if nothing was inserted;
    /// `None` if the insertion failed after the board was changed.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_forced_trace_polyline(
        &mut self,
        polyline: &Polyline,
        half_width: i32,
        layer: i32,
        nets: &[i32],
        clearance_class: i32,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        max_spring_over_recursion_depth: i32,
        tidy_width: i32,
        accuracy: i32,
        with_check: bool,
        time_limit: Option<&TimeLimit>,
    ) -> Option<Point> {
        self.clear_shove_failing_obstacle();
        if polyline.line_count() < 2 {
            return None;
        }
        let from_corner = polyline.first_corner();
        let to_corner = polyline.last_corner();
        if from_corner == to_corner {
            return Some(to_corner);
        }
        if !(from_corner.is_int() && to_corner.is_int()) {
            log::warn!("RoutingBoard.insert_forced_trace_segment: only implemented for IntPoints");
            return Some(from_corner);
        }
        self.start_marking_changed_area();
        let mut picked_trace: Option<ItemId> = None;
        let picked = self.pick_traces(&from_corner, layer);
        if picked.len() == 1 {
            let t = self.item(picked[0]);
            let tt = t.as_trace().unwrap();
            if t.nets_equal(nets) && tt.half_width == half_width && t.clearance_class == clearance_class {
                picked_trace = Some(picked[0]);
            }
        }
        let comp_hw = half_width + self.search_trees.default_tree().clearance_compensation_value(&self.rules, clearance_class, layer);
        let Some(new_polyline) = shove::spring_over_obstacles(self, polyline, comp_hw, layer, nets, clearance_class, None) else {
            if std::env::var_os("RR_DEBUG").is_some() {
                eprintln!("insert_forced_trace_polyline: spring_over_obstacles failed");
            }
            return Some(from_corner);
        };
        let new_polyline = new_polyline;
        let mut combined = match picked_trace {
            None => new_polyline.clone(),
            Some(pt) => new_polyline.combine(&self.any_item(pt).expect("item exists").as_trace().unwrap().polyline.clone()),
        };
        if combined.line_count() < 3 {
            return Some(from_corner);
        }
        let start_shape_no = combined.line_count() - new_polyline.line_count();
        let trace_shapes = combined.offset_shapes_range(comp_hw, start_shape_no as i32, combined.line_count() as i32 - 1);
        let mut last_shape_no = trace_shapes.len();
        let ortho = self.rules.get_trace_angle_restriction() == AngleRestriction::NinetyDegree;
        for (i, s) in trace_shapes.iter().enumerate() {
            let s = if ortho { TileShape::Box(s.bounding_box()) } else { s.clone() };
            let fs_no = combined.corner_count() - trace_shapes.len() - 1 + i;
            let from_side = ShapeEntrySide::from_polyline(&combined, fs_no, &s);
            if with_check
                && !shove::shove_check(
                    self,
                    &s,
                    Some(&from_side),
                    None,
                    layer,
                    nets,
                    clearance_class,
                    max_recursion_depth,
                    max_via_recursion_depth,
                    max_spring_over_recursion_depth,
                    time_limit,
                )
            {
                last_shape_no = i;
                break;
            }
            if !shove::shove_insert(
                self,
                &s,
                Some(&from_side),
                layer,
                nets,
                clearance_class,
                None,
                max_recursion_depth,
                max_via_recursion_depth,
                max_spring_over_recursion_depth,
            ) {
                if std::env::var_os("RR_DEBUG").is_some() {
                    eprintln!("insert_forced_trace_polyline: shove_insert failed at shape {i}");
                }
                return None;
            }
        }
        let mut new_polyline = new_polyline;
        let mut new_corner = to_corner.clone();
        if last_shape_no < trace_shapes.len() {
            let mut last_trace_shape = trace_shapes[last_shape_no].clone();
            if ortho {
                last_trace_shape = TileShape::Box(last_trace_shape.bounding_box());
            }
            let sample_width = 2 * self.get_min_trace_half_width();
            let last_corner = new_polyline.corner_approx(last_shape_no + 1);
            let prev_last_corner = new_polyline.corner_approx(last_shape_no);
            let last_segment_length = last_corner.distance(&prev_last_corner);
            if last_segment_length > 100.0 * sample_width as f64 {
                return Some(from_corner);
            }
            let mut shape_index = combined.corner_count() - trace_shapes.len() - 1 + last_shape_no;
            if last_segment_length > sample_width as f64 {
                new_polyline = new_polyline.shorten(
                    new_polyline.line_count() - (trace_shapes.len() - last_shape_no - 1),
                    sample_width as f64,
                );
                let cur_last = new_polyline.last_corner();
                if !cur_last.is_int() {
                    return Some(from_corner);
                }
                new_corner = cur_last;
                combined = match picked_trace {
                    None => new_polyline.clone(),
                    Some(pt) => new_polyline.combine(&self.any_item(pt).expect("item exists").as_trace().unwrap().polyline.clone()),
                };
                if combined.line_count() < 3 {
                    return Some(new_corner);
                }
                shape_index = combined.line_count() - 3;
                last_trace_shape = combined.offset_shape(comp_hw, shape_index).unwrap_or_else(TileShape::empty);
                if ortho {
                    last_trace_shape = TileShape::Box(last_trace_shape.bounding_box());
                }
            }
            let from_side = ShapeEntrySide::from_polyline(&combined, shape_index, &last_trace_shape);
            if !shove::shove_check(
                self,
                &last_trace_shape,
                Some(&from_side),
                None,
                layer,
                nets,
                clearance_class,
                max_recursion_depth,
                max_via_recursion_depth,
                max_spring_over_recursion_depth,
                time_limit,
            ) {
                return Some(from_corner);
            }
            if !shove::shove_insert(
                self,
                &last_trace_shape,
                Some(&from_side),
                layer,
                nets,
                clearance_class,
                None,
                max_recursion_depth,
                max_via_recursion_depth,
                max_spring_over_recursion_depth,
            ) {
                return None;
            }
        }
        for i in 0..new_polyline.corner_count() {
            self.join_changed_area(&new_polyline.corner_approx(i), layer);
        }
        let new_trace = self.insert_trace_without_cleaning(new_polyline, layer, half_width, nets, clearance_class, FixedState::Unfixed);
        if let Some(nt) = new_trace {
            self.combine_trace(nt);
        }
        let tidy_region = if tidy_width < i32::MAX {
            Some(new_corner.surrounding_octagon().enlarge(tidy_width as f64))
        } else {
            None
        };
        let opt_nets: Vec<i32> = if max_recursion_depth <= 0 { nets.to_vec() } else { Vec::new() };
        let mut tt = TraceTightener::new(self, &opt_nets, tidy_region, accuracy, None, -1, Some(new_corner.clone()), layer);
        let mut new_trace = new_trace.filter(|t| self.items.contains(*t));
        if let Some(nt) = new_trace {
            let clip = self.changed_area.as_ref().map(|c| c.get_area(layer));
            if self.normalize_trace(nt, clip.as_ref()) {
                tt.split_traces_at_keep_point(self);
                new_trace = self.pick_traces(&new_corner, layer).first().copied();
            }
        }
        if tidy_width > 0 {
            if let Some(nt) = new_trace {
                if self.items.contains(nt) {
                    tt.pull_tight_trace(self, nt);
                }
            }
        }
        Some(new_corner)
    }

    /// Inserts a via by shoving obstacles (`forcedVia`).
    #[allow(clippy::too_many_arguments)]
    pub fn forced_via(
        &mut self,
        via_info: &ViaInfo,
        location: &Point,
        nets: &[i32],
        trace_clearance_class: i32,
        trace_pen_half_widths: &[i32],
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        tidy_width: i32,
        accuracy: i32,
        time_limit: i32,
    ) -> bool {
        self.clear_shove_failing_obstacle();
        self.start_marking_changed_area();
        let result = shove::forced_via_insert(
            self,
            via_info,
            location,
            nets,
            trace_clearance_class,
            trace_pen_half_widths,
            max_recursion_depth,
            max_via_recursion_depth,
        );
        if result {
            let tidy = if tidy_width < i32::MAX {
                Some(location.surrounding_octagon().enlarge(tidy_width as f64))
            } else {
                None
            };
            let opt_nets: Vec<i32> = if max_recursion_depth <= 0 { nets.to_vec() } else { Vec::new() };
            self.opt_changed_area(&opt_nets, tidy, accuracy, None, None, time_limit, None, -1);
        }
        result
    }

    /// Connects a point to a trace with a perpendicular segment (`connectToTrace`).
    pub fn connect_to_trace(&mut self, from: crate::geometry::IntPoint, to_trace: ItemId, half_width: i32, clearance_class: i32) -> bool {
        let item = self.item(to_trace).clone();
        self.connect_to_trace_item(from, &item, half_width, clearance_class)
    }

    /// `connectToTrace` with the trace object itself (it may already be removed from the board).
    pub fn connect_to_trace_item(&mut self, from: crate::geometry::IntPoint, item: &crate::board::Item, half_width: i32, clearance_class: i32) -> bool {
        let item = item.clone();
        let Some(t) = item.as_trace() else { return false };
        let fp = Point::Int(from);
        if t.polyline.contains(&fp) {
            return true;
        }
        let Some(pl) = t.polyline.projection_line(&fp) else {
            return false;
        };
        let connection = pl.to_polyline();
        if connection.line_count() != 3 {
            return false;
        }
        if !self.check_polyline_trace(&connection, t.layer, half_width, &item.net_numbers, clearance_class) {
            return false;
        }
        if self.changed_area.is_some() {
            for i in 0..connection.corner_count() {
                self.join_changed_area(&connection.corner_approx(i), t.layer);
            }
        }
        self.insert_trace(connection, t.layer, half_width, &item.net_numbers, clearance_class, FixedState::Unfixed);
        let (first, last) = (t.first_corner(), t.last_corner());
        for c in [first, last] {
            if fp != c {
                if let Some(tail) = self.get_trace_tail(&c, t.layer, &item.net_numbers) {
                    if !self.item(tail).is_user_fixed() {
                        self.remove_item(tail);
                    }
                }
            }
        }
        true
    }

    /// Removes trace tails of a net (all nets if `net_no <= 0`) (`removeTraceTails`).
    pub fn remove_trace_tails(&mut self, net_no: i32, stop: StopConnectionOption) -> bool {
        let mut stubs: BTreeSet<ItemId> = BTreeSet::new();
        for id in self.items.ids() {
            let item = self.item(id);
            if !item.is_routable() || item.net_count() != 1 {
                continue;
            }
            if net_no > 0 && item.get_net_number(0) != net_no {
                continue;
            }
            if self.is_tail(id) {
                if item.is_via() {
                    if stop == StopConnectionOption::Via {
                        continue;
                    }
                    if stop == StopConnectionOption::FanoutVia && self.is_fanout_via(id, None) {
                        continue;
                    }
                }
                stubs.insert(id);
            }
        }
        let mut stub_connections: BTreeSet<ItemId> = BTreeSet::new();
        for id in stubs {
            if self.normal_contacts(id).len() == 1 {
                stub_connections.extend(self.connection_items(id, stop));
            } else {
                stub_connections.insert(id);
            }
        }
        if stub_connections.is_empty() {
            return false;
        }
        let v: Vec<ItemId> = stub_connections.into_iter().collect();
        self.remove_items(&v);
        self.combine_traces(net_no);
        true
    }

    /// Changes whether conduction areas on signal layers are obstacles (`changePlaneAsObstacle`).
    pub fn change_plane_as_obstacle(&mut self, value: bool) {
        let target_ignore = !value;
        let mut changed = false;
        for id in self.items.ids() {
            let layer_is_signal;
            {
                let item = self.item(id);
                let Some(o) = item.as_obstacle() else { continue };
                if !item.is_conduction_area() {
                    continue;
                }
                layer_is_signal = self.layer_structure.layers[o.layer as usize].is_signal;
                if !(layer_is_signal && o.conduction_is_obstacle() != value) {
                    continue;
                }
            }
            if let Some(o) = self.item_mut(id).as_obstacle_mut() {
                if let super::ObstacleKind::Conduction { is_filled, .. } = o.kind {
                    o.kind = super::ObstacleKind::Conduction { is_obstacle: value, is_filled };
                    changed = true;
                }
            }
        }
        if self.rules.get_ignore_conduction() != target_ignore {
            self.rules.set_ignore_conduction(target_ignore);
            changed = true;
        }
        if changed {
            self.reinsert_tree_items();
        }
    }

    /// Removes net numbers from route items with more than 1 net (`reduceNetsOfRouteItems`).
    pub fn reduce_nets_of_route_items(&mut self) -> bool {
        let mut result = false;
        loop {
            let mut changed = false;
            'items: for id in self.items.ids() {
                let item = self.item(id).clone();
                if item.net_numbers.len() <= 1 || item.fixed_state == FixedState::SystemFixed {
                    continue;
                }
                if item.is_via() {
                    let contacts = self.normal_contacts(id);
                    for n in item.net_numbers.iter() {
                        for c in &contacts {
                            if !self.item(*c).contains_net(*n) {
                                self.remove_item_from_net(id, *n);
                                changed = true;
                                break;
                            }
                        }
                        if changed {
                            break;
                        }
                    }
                } else if item.is_trace() {
                    let mut contacts = self.trace_start_contacts(id);
                    for i in 0..2 {
                        for n in item.net_numbers.iter() {
                            let mut pin_found = false;
                            for c in &contacts {
                                if self.item(*c).is_pin() {
                                    pin_found = true;
                                    if !self.item(*c).contains_net(*n) {
                                        self.remove_item_from_net(id, *n);
                                        changed = true;
                                        break;
                                    }
                                }
                            }
                            if !pin_found {
                                for c in &contacts {
                                    let ci = self.item(*c);
                                    if !ci.is_pin() && !ci.contains_net(*n) {
                                        self.remove_item_from_net(id, *n);
                                        changed = true;
                                        break;
                                    }
                                }
                            }
                        }
                        if changed {
                            break;
                        }
                        if i == 0 {
                            contacts = self.trace_end_contacts(id);
                        }
                    }
                }
                if changed {
                    result = true;
                    break 'items;
                }
            }
            if !changed {
                break;
            }
        }
        result
    }

    /// Assigns net-less pads overlapping netted pads of their footprint to that net.
    /// Returns (assigned count, tie pad count).
    pub fn connect_netless_pads_to_their_footprints_net(&mut self) -> (usize, usize) {
        let mut assigned = 0;
        let mut tie_pads = 0;
        for pin in self.get_pins() {
            if self.item(pin).net_count() != 0 {
                continue;
            }
            let mut nets: BTreeSet<i32> = BTreeSet::new();
            let mut own_footprint = true;
            let comp = self.item(pin).component_id;
            for o in self.overlapping_netted_pins(pin) {
                let oi = self.item(o);
                own_footprint &= oi.component_id == comp;
                nets.extend(oi.net_numbers.iter().copied());
            }
            if nets.is_empty() {
                continue;
            }
            if nets.len() == 1 {
                self.assign_net_no(pin, *nets.iter().next().unwrap());
            } else if own_footprint {
                self.set_item_nets(pin, nets.iter().copied().collect());
                tie_pads += 1;
            } else {
                continue;
            }
            assigned += 1;
        }
        (assigned, tie_pads)
    }

    /// Like `connect_netless_pads_to_their_footprints_net`, with the KiCad specific variant.
    pub fn connect_netless_pads(&mut self, kicad_board: bool, fix_kicad_footprints: bool) -> (usize, usize) {
        if !kicad_board {
            return self.connect_netless_pads_to_their_footprints_net();
        }
        if !fix_kicad_footprints {
            return (0, 0);
        }
        let netless: Vec<ItemId> = self.get_pins().into_iter().filter(|p| self.item(*p).net_count() == 0).collect();
        let index: rustc_hash::FxHashMap<ItemId, usize> = netless.iter().enumerate().map(|(i, p)| (*p, i)).collect();
        let mut parent: Vec<usize> = (0..netless.len()).collect();
        fn find(parent: &mut [usize], i: usize) -> usize {
            if parent[i] == i {
                return i;
            }
            let r = find(parent, parent[i]);
            parent[i] = r;
            r
        }
        for (i, p) in netless.iter().enumerate() {
            for o in self.copper_overlapping_pins(*p, |o| o.net_count() == 0) {
                if let Some(oi) = index.get(&o) {
                    let ri = find(&mut parent, i);
                    let ro = find(&mut parent, *oi);
                    parent[ri] = ro;
                }
            }
        }
        // groups in order of first appearance of their root (HashMap order in the original)
        let mut groups: Vec<(usize, Vec<ItemId>)> = Vec::new();
        for (i, p) in netless.iter().enumerate() {
            let r = find(&mut parent, i);
            match groups.iter_mut().find(|(gr, _)| *gr == r) {
                Some((_, v)) => v.push(*p),
                None => groups.push((r, vec![*p])),
            }
        }
        let mut assignments: Vec<(ItemId, i32)> = Vec::new();
        for (_, members) in groups {
            let comp = self.item(members[0]).component_id;
            if members.iter().any(|m| self.item(*m).component_id != comp) {
                continue;
            }
            let mut netted = Vec::new();
            for m in &members {
                netted.extend(self.copper_overlapping_pins(*m, |o| o.net_count() != 0));
            }
            if netted.is_empty() {
                continue;
            }
            let first_net = self.item(netted[0]).get_net_number(0);
            let valid = netted.iter().all(|o| {
                let oi = self.item(*o);
                oi.component_id == comp && oi.net_count() == 1 && oi.get_net_number(0) == first_net
            });
            if valid {
                for m in members {
                    assignments.push((m, first_net));
                }
            }
        }
        for (p, n) in &assignments {
            self.assign_net_no(*p, *n);
        }
        (assignments.len(), 0)
    }
}
