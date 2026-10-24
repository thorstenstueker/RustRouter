//! The fanout stage (port of `BatchFanout`, `DirectPinRouting` and `RoutingBoard.fanout`).

use super::{stats, RoutingJob};
use crate::autoroute::control::CtrlViaRule;
use crate::autoroute::{AutorouteAttemptResult, AutorouteAttemptState, AutorouteControl};
use crate::board::structure::{FixedState, Unit};
use crate::board::{Board, ItemId};
use crate::datastructures::{StopFlag, TimeLimit};
use crate::geometry::basics::java_round;
use crate::geometry::FloatPoint;
use crate::rules::{BoardRules, ItemClass, ViaInfo};
use crate::settings::RouterSettings;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct FanoutRunSummary {
    pub completed_pass_count: i32,
    pub total_duration_millis: u128,
    pub is_timed_out: bool,
}

/// `RoutingBoard.fanoutFallbackViaRule`
pub fn fanout_fallback_via_rule(
    net_rule: &CtrlViaRule,
    board_rule: Option<CtrlViaRule>,
    net_via_clearance: Option<i32>,
    _rules: &BoardRules,
) -> CtrlViaRule {
    let mut combined = CtrlViaRule {
        name: format!("{}_fallback", net_rule.name),
        vias: net_rule.vias.clone(),
    };
    let Some(board_rule) = board_rule else {
        return combined;
    };
    for bv in &board_rule.vias {
        match net_via_clearance {
            Some(cl) if cl != bv.clearance_class => {
                let present = combined.vias.iter().any(|e| e.padstack == bv.padstack && e.clearance_class == cl);
                if !present {
                    combined.vias.push(ViaInfo {
                        name: bv.name.clone(),
                        padstack: bv.padstack,
                        clearance_class: cl,
                        attach_smd_allowed: bv.attach_smd_allowed,
                    });
                }
            }
            _ => {
                if !combined.contains(bv) {
                    combined.vias.push(bv.clone());
                }
            }
        }
    }
    combined
}

fn board_default_via_rule(board: &Board) -> Option<CtrlViaRule> {
    let r = board.rules.via_rules.first()?;
    Some(CtrlViaRule {
        name: r.name.clone(),
        vias: r.vias().iter().map(|v| board.rules.via_infos.get(*v).clone()).collect(),
    })
}

impl Board {
    /// `RoutingBoard.fanout(pin, settings, ripupCosts, ...)`
    pub fn fanout(
        &mut self,
        pin: ItemId,
        settings: &RouterSettings,
        ripup_costs: i32,
        stop: Option<StopFlag>,
        time_limit: Option<TimeLimit>,
    ) -> AutorouteAttemptResult {
        self.clear_removed_items();
        let lc = self.layer_count();
        let p = self.item(pin);
        if p.first_layer() != p.last_layer_with(lc) || p.net_count() != 1 {
            return AutorouteAttemptResult::with(AutorouteAttemptState::AlreadyConnected, format!("The pin '#{}' is already connected.", pin.0));
        }
        if self.has_ignored_nets(p) {
            return AutorouteAttemptResult::with(AutorouteAttemptState::Skipped, format!("The pin '#{}' belongs to an ignored net class.", pin.0));
        }
        let net_no = p.get_net_number(0);
        let pin_layer = p.first_layer();
        let connected = self.connected_set(pin, net_no, false);
        for c in &connected {
            let ci = self.item(*c);
            if ci.first_layer() != pin_layer || ci.last_layer_with(lc) != pin_layer {
                return AutorouteAttemptResult::with(AutorouteAttemptState::AlreadyConnected, format!("The pin '#{}' is already connected.", pin.0));
            }
        }
        let unconnected = self.unconnected_set(pin, net_no);
        if unconnected.is_empty() {
            return AutorouteAttemptResult::with(AutorouteAttemptState::NoUnconnectedNets, format!("The pin '#{}' is already connected.", pin.0));
        }
        let pin_center = self.item(pin).as_drill().unwrap().center.to_float();
        let mut sorted: Vec<ItemId> = unconnected.iter().copied().collect();
        let dist = |b: &Board, id: ItemId| {
            let bx = b.item(id).bounding_box();
            let cx = (bx.ll.x as f64 + bx.ur.x as f64) / 2.0;
            let cy = (bx.ll.y as f64 + bx.ur.y as f64) / 2.0;
            let dx = cx - pin_center.x;
            let dy = cy - pin_center.y;
            dx * dx + dy * dy
        };
        sorted.sort_by(|a, b| dist(self, *a).partial_cmp(&dist(self, *b)).unwrap_or(Ordering::Equal));
        let mut ctrl = AutorouteControl::new(self, net_no, settings);
        ctrl.is_fanout = true;
        if settings.fanout.fallback_to_board_vias && ctrl.via_rule_id.is_some() {
            let net_via_clearance = self
                .rules
                .net_class_of(net_no)
                .map(|c| c.default_item_clearance_classes.get(ItemClass::Via));
            ctrl.via_rule = fanout_fallback_via_rule(&ctrl.via_rule, board_default_via_rule(self), net_via_clearance, &self.rules);
            ctrl.rebuild_via_info(self, settings.get_via_costs(), net_no);
        }
        let comp_id = self.item(pin).component_id;
        let pin_name = self.pin_name(pin);
        ctrl.fanout_start_pin_name = match (self.components.try_get(comp_id), pin_name) {
            (Some(c), Some(n)) => Some(format!("{}-{}", c.name, n)),
            _ => Some(format!("#{}", pin.0)),
        };
        ctrl.fanout_start_pin_center = Some(self.item(pin).as_drill().unwrap().center.clone());
        ctrl.fanout_start_pin_layer = pin_layer;
        ctrl.remove_unconnected_vias = false;
        if ripup_costs >= 0 {
            ctrl.ripup_allowed = true;
            ctrl.ripup_costs = ripup_costs;
        }
        let mut ripped = BTreeSet::new();
        self.init_autoroute(net_no, ctrl.trace_clearance_class, stop.clone(), time_limit, false);
        let mut result: Option<AutorouteAttemptResult> = None;
        if sorted.len() <= 4 {
            if let Some(closest) = sorted.first() {
                let dest: BTreeSet<ItemId> = [*closest].into_iter().collect();
                let r = self.autoroute_connection(&connected, &dest, &ctrl, &mut ripped, None);
                let r = if r.state != AutorouteAttemptState::Routed
                    && r.state != AutorouteAttemptState::AlreadyConnected
                    && sorted.len() > 1
                {
                    self.autoroute_connection(&connected, &unconnected, &ctrl, &mut ripped, None)
                } else {
                    r
                };
                result = Some(r);
            }
        } else {
            result = Some(self.autoroute_connection(&connected, &unconnected, &ctrl, &mut ripped, None));
        }
        if let Some(r) = &result {
            if r.state == AutorouteAttemptState::Failed && settings.fanout.retry_without_escape_window {
                ctrl.ignore_escape_window = true;
                result = Some(self.autoroute_connection(&connected, &unconnected, &ctrl, &mut ripped, None));
            }
        }
        let result = result.unwrap_or_else(|| AutorouteAttemptResult::with(AutorouteAttemptState::Failed, "No target items to route connection."));
        if result.state == AutorouteAttemptState::Routed {
            self.opt_changed_area(&[net_no], None, settings.trace_pull_tight_accuracy, Some(&ctrl.trace_costs), stop, 1000, None, -1);
        }
        result
    }
}

struct FanoutPin {
    pin: ItemId,
    pin_index: i32,
    distance_to_component_center: f64,
    distance_to_closest_on_net: f64,
    surroundings_density: i32,
}

struct FanoutComponent {
    component_id: i32,
    smd_pin_count: usize,
    smd_pins: Vec<FanoutPin>,
}

fn compare_pins(order: &str, a: &FanoutPin, b: &FanoutPin) -> Ordering {
    let mut r = 0;
    match order {
        "inner_first" => {
            let d = a.distance_to_component_center - b.distance_to_component_center;
            r = if d > 0.0 { 1 } else if d < 0.0 { -1 } else { 0 };
        }
        "outer_first" => {
            let d = a.distance_to_component_center - b.distance_to_component_center;
            r = if d > 0.0 { -1 } else if d < 0.0 { 1 } else { 0 };
        }
        "distanceToClosestOnNet" => {
            let d = a.distance_to_closest_on_net - b.distance_to_closest_on_net;
            r = if d > 0.0 { 1 } else if d < 0.0 { -1 } else { 0 };
        }
        "surroundingsDensity" => {
            let d = b.surroundings_density - a.surroundings_density;
            r = d.signum();
        }
        _ => {}
    }
    if r == 0 {
        r = a.pin_index.wrapping_sub(b.pin_index);
    }
    r.cmp(&0)
}

fn build_components(board: &Board, sorting_order: &str) -> Vec<FanoutComponent> {
    let smd_with_nets: Vec<ItemId> = board
        .get_smd_pins()
        .into_iter()
        .filter(|p| {
            let it = board.item(*p);
            it.net_count() > 0 && !board.has_ignored_nets(it)
        })
        .collect();
    let all_pins = board.get_pins();
    let resolution = board.communication.get_resolution(Unit::Um);
    let mut comps: Vec<FanoutComponent> = Vec::new();
    for ci in 1..=board.components.count() as i32 {
        let pins: Vec<ItemId> = smd_with_nets.iter().copied().filter(|p| board.item(*p).component_id == ci).collect();
        let n = pins.len();
        let (mut x, mut y) = (0.0, 0.0);
        for p in &pins {
            let c = board.item(*p).as_drill().unwrap().center.to_float();
            x += c.x;
            y += c.y;
        }
        if n > 0 {
            x /= n as f64;
            y /= n as f64;
        }
        let gravity = FloatPoint::new(x, y);
        let mut smd_pins: Vec<FanoutPin> = Vec::new();
        for p in &pins {
            let it = board.item(*p);
            let loc = it.as_drill().unwrap().center.to_float();
            let mut min_d = f64::MAX;
            let net_no = if it.net_count() > 0 { it.get_net_number(0) } else { 0 };
            if net_no > 0 {
                for o in &all_pins {
                    if o != p && board.item(*o).contains_net(net_no) {
                        let d = loc.distance(&board.item(*o).as_drill().unwrap().center.to_float());
                        if d < min_d {
                            min_d = d;
                        }
                    }
                }
            }
            let max_dist = 20000.0 * resolution;
            let density = smd_with_nets
                .iter()
                .filter(|o| *o != p && loc.distance(&board.item(**o).as_drill().unwrap().center.to_float()) <= max_dist)
                .count() as i32;
            let fp = FanoutPin {
                pin: *p,
                pin_index: it.as_pin().map(|x| x.pin_index).unwrap_or(0),
                distance_to_component_center: loc.distance(&gravity),
                distance_to_closest_on_net: min_d,
                surroundings_density: density,
            };
            // TreeSet semantics: equal elements are dropped
            match smd_pins.binary_search_by(|e| compare_pins(sorting_order, e, &fp)) {
                Ok(_) => {}
                Err(pos) => smd_pins.insert(pos, fp),
            }
        }
        if n > 0 {
            let c = FanoutComponent {
                component_id: ci,
                smd_pin_count: n,
                smd_pins,
            };
            let cmp = |a: &FanoutComponent, b: &FanoutComponent| {
                if a.smd_pin_count > b.smd_pin_count {
                    Ordering::Less
                } else if a.smd_pin_count < b.smd_pin_count {
                    Ordering::Greater
                } else {
                    a.component_id.cmp(&b.component_id)
                }
            };
            match comps.binary_search_by(|e| cmp(e, &c)) {
                Ok(_) => {}
                Err(pos) => comps.insert(pos, c),
            }
        }
    }
    comps
}

/// `BatchFanout.fanoutBoard`
pub fn fanout_board(job: &mut RoutingJob) -> FanoutRunSummary {
    if job.settings.uses_direct_pin_routing() {
        return direct_pin_routing(job);
    }
    let start = Instant::now();
    let sorting_order = job.settings.fanout.pin_sorting_order.clone();
    let comps = build_components(&job.board, &sorting_order);
    let mut state = FanoutState {
        total_items_fanouted: 0,
        deadline: job
            .settings
            .fanout
            .timeout_string
            .as_deref()
            .and_then(super::parse_timespan_string)
            .map(|s| start + std::time::Duration::from_secs(s.max(0) as u64)),
        is_timed_out: false,
        failed_pin_generation: HashMap::new(),
        component_generation: HashMap::new(),
    };
    let max_passes = job.settings.fanout.max_passes;
    let stagnation_pass_limit = 3;
    let mut completed = 0;
    let mut previous_board_state = i64::MIN;
    let mut identical = 0;
    let mut last_hash = job.board.content_hash();
    for i in 0..max_passes {
        if let Some(d) = state.deadline {
            if Instant::now() >= d {
                state.is_timed_out = true;
                log::info!("Fanout stage timed out before starting pass #{}", i + 1);
                break;
            }
        }
        let max_items = job.settings.fanout.max_items;
        if max_items > 0 && state.total_items_fanouted >= max_items {
            break;
        }
        let routed = fanout_pass(job, &comps, &mut state, i);
        completed += 1;
        if routed == 0 {
            break;
        }
        let board_state = ((routed as i64) << 32) ^ job.board.get_via_count() as i64;
        if board_state == previous_board_state {
            identical += 1;
            if identical >= stagnation_pass_limit {
                log::info!("Fanout stopped after {completed} passes: no progress for {stagnation_pass_limit} consecutive passes.");
                break;
            }
        } else {
            identical = 0;
            previous_board_state = board_state;
        }
        if state.is_timed_out {
            break;
        }
        let h = job.board.content_hash();
        if h == last_hash {
            break;
        }
        last_hash = h;
    }
    FanoutRunSummary {
        completed_pass_count: completed,
        total_duration_millis: start.elapsed().as_millis(),
        is_timed_out: state.is_timed_out,
    }
}

struct FanoutState {
    total_items_fanouted: i32,
    deadline: Option<Instant>,
    is_timed_out: bool,
    failed_pin_generation: HashMap<i32, i32>,
    component_generation: HashMap<i32, i32>,
}

fn fanout_pass(job: &mut RoutingJob, comps: &[FanoutComponent], st: &mut FanoutState, pass_no: i32) -> i32 {
    let pass_start = Instant::now();
    let mut routed = 0;
    let mut not_routed = 0;
    let mut insert_errors = 0;
    let vias_before = job.board.get_via_count();
    let ripup_costs = job.settings.get_start_ripup_costs() * (pass_no + 1);
    let base_ms = job.settings.fanout.max_milliseconds_per_pin;
    let effective_ripup = if job.settings.fanout.ripup_allowed { ripup_costs } else { -1 };
    'comps: for c in comps {
        for p in &c.smd_pins {
            let max_items = job.settings.fanout.max_items;
            if max_items > 0 && st.total_items_fanouted >= max_items {
                log::info!("Max items limit reached ({max_items}). Stopping fanout.");
                break 'comps;
            }
            let max_ms = (base_ms as f64 * (pass_no + 1) as f64) as i32;
            let time_limit = TimeLimit::new(max_ms.max(0) as u64);
            let pin = p.pin;
            if job.board.has_ignored_nets(job.board.item(pin)) {
                continue;
            }
            let net_no = job.board.item(pin).get_net_number(0);
            if let Some(nc) = job.board.rules.net_class_of(net_no) {
                let via_rule_ok = nc.get_via_rule().map(|r| job.board.rules.via_rule(r).via_count() > 0).unwrap_or(false);
                let has_board_vias = job.board.rules.via_rules.first().map(|r| r.via_count() > 0).unwrap_or(false);
                let fallback_allowed = job.settings.fanout.fallback_to_board_vias && has_board_vias;
                if !(via_rule_ok || fallback_allowed) {
                    log::debug!("BatchFanout: skipping pin #{} because its net class has no vias defined and fallback is disabled/unavailable.", pin.0);
                    continue;
                }
            }
            let comp_gen = *st.component_generation.get(&c.component_id).unwrap_or(&0);
            if let Some(g) = st.failed_pin_generation.get(&pin.0) {
                if *g == comp_gen {
                    continue;
                }
            }
            let max_id_before = job.board.communication.id_generator.max_generated_id();
            job.board.start_marking_changed_area();
            let settings = job.settings.clone();
            let mut result = job.board.fanout(pin, &settings, effective_ripup, Some(job.thread.clone()), Some(time_limit));
            if result.state == AutorouteAttemptState::Routed {
                if let Some(rej) = super::BatchAutorouter::enforce_strict_drc(&mut job.board, net_no, max_id_before) {
                    result = rej;
                }
            }
            match result.state {
                AutorouteAttemptState::Routed => {
                    routed += 1;
                    st.total_items_fanouted += 1;
                    *st.component_generation.entry(c.component_id).or_insert(0) += 1;
                }
                AutorouteAttemptState::Failed => {
                    not_routed += 1;
                    st.total_items_fanouted += 1;
                    st.failed_pin_generation.insert(pin.0, comp_gen);
                }
                AutorouteAttemptState::InsertError => {
                    insert_errors += 1;
                    st.total_items_fanouted += 1;
                    st.failed_pin_generation.insert(pin.0, comp_gen);
                }
                _ => {}
            }
            if let Some(d) = st.deadline {
                if Instant::now() >= d {
                    log::info!("Fanout stage timed out.");
                    st.is_timed_out = true;
                    return routed;
                }
            }
            if job.thread.is_stop_autorouter_requested() {
                return routed;
            }
        }
    }
    let extra = job.board.get_via_count().saturating_sub(vias_before);
    log::info!(
        "Fanout pass #{} completed in {:.2} seconds with {} SMD pin{} fanouted, {} not routed, {} insert error{}, +{} extra via{} (ripup costs={}).",
        pass_no + 1,
        pass_start.elapsed().as_secs_f64(),
        routed,
        if routed == 1 { "" } else { "s" },
        not_routed,
        insert_errors,
        if insert_errors == 1 { "" } else { "s" },
        extra,
        if extra == 1 { "" } else { "s" },
        ripup_costs
    );
    routed
}

/// `DirectPinRouting.run`
fn direct_pin_routing(job: &mut RoutingJob) -> FanoutRunSummary {
    let start = Instant::now();
    let lc = job.board.layer_count();
    let mut pins: Vec<ItemId> = job
        .board
        .get_smd_pins()
        .into_iter()
        .filter(|p| {
            let it = job.board.item(*p);
            it.net_count() == 1 && !job.board.has_ignored_nets(it)
        })
        .collect();
    pins.sort_by_key(|p| {
        let it = job.board.item(*p);
        (it.component_id, it.as_pin().map(|x| x.pin_index).unwrap_or(0))
    });
    let mut fixed: Vec<(ItemId, FixedState)> = Vec::new();
    for id in job.board.items.ids() {
        let it = job.board.item(id);
        if it.is_trace() || it.is_via() {
            fixed.push((id, it.fixed_state));
            job.board.item_mut(id).fixed_state = FixedState::SystemFixed;
        }
    }
    let mut attempts = 0;
    let mut routed = 0;
    let mut deferred = 0;
    let max_items = if job.settings.fanout.max_items > 0 { job.settings.fanout.max_items } else { i32::MAX };
    let fanout_disabled = job.settings.fanout.max_passes <= 0;
    let stopped = |j: &RoutingJob| j.thread.is_stop_requested() || j.thread.is_stop_autorouter_requested();
    for pin in pins {
        if stopped(job) || attempts >= max_items || fanout_disabled {
            break;
        }
        let net = job.board.item(pin).get_net_number(0);
        let layer = job.board.item(pin).first_layer();
        let source = job.board.connected_set(pin, net, false);
        let dest = job.board.unconnected_set(pin, net);
        let touches_plane = source.iter().any(|s| job.board.item(*s).is_conduction_area());
        let on_layer = dest.iter().any(|d| {
            let it = job.board.item(*d);
            it.first_layer() <= layer && it.last_layer_with(lc) >= layer
        });
        if dest.is_empty() || touches_plane || !on_layer {
            continue;
        }
        attempts += 1;
        let mut r = route_on_layer(job, pin, net, layer, false);
        if r.state != AutorouteAttemptState::Routed && job.settings.get_neck_width_um() > 0.0 && !stopped(job) {
            r = route_on_layer(job, pin, net, layer, true);
        }
        if r.state == AutorouteAttemptState::Routed {
            routed += 1;
        } else {
            deferred += 1;
        }
    }
    job.board.clear_transient_autoroute_state();
    for (id, state) in fixed {
        if let Some(it) = job.board.items.get_mut(id) {
            it.fixed_state = state;
        }
    }
    log::info!("Direct pin routing: {routed} complete surface connections, {deferred} deferred.");
    let _ = stats::incomplete_count(&job.board);
    FanoutRunSummary {
        completed_pass_count: 1,
        total_duration_millis: start.elapsed().as_millis(),
        is_timed_out: false,
    }
}

fn route_on_layer(job: &mut RoutingJob, pin: ItemId, net: i32, layer: i32, neck: bool) -> AutorouteAttemptResult {
    let mut c = AutorouteControl::new(&job.board, net, &job.settings);
    c.vias_allowed = false;
    c.ripup_allowed = false;
    c.remove_unconnected_vias = false;
    for i in 0..c.layer_count {
        c.layer_active[i] = c.layer_active[i] && i as i32 == layer;
    }
    let l = layer as usize;
    if !c.layer_active[l] {
        return AutorouteAttemptResult::with(AutorouteAttemptState::Failed, "Pin layer is inactive.");
    }
    if neck {
        let resolution = job.board.communication.resolution.max(1);
        let w = java_round(Unit::scale(job.settings.get_neck_width_um() * resolution as f64, Unit::Um, job.board.communication.unit));
        let neck_hw = 1.max(w / 2).max(job.board.rules.get_min_allowed_trace_half_width());
        if neck_hw >= c.trace_half_width[l] {
            return AutorouteAttemptResult::with(AutorouteAttemptState::Failed, "Neck width is not narrower than the class width.");
        }
        let comp = c.compensated_trace_half_width[l] - c.trace_half_width[l];
        c.trace_half_width[l] = neck_hw;
        c.compensated_trace_half_width[l] = neck_hw + comp;
    }
    let per_pin_ms = job.settings.fanout.max_milliseconds_per_pin.min(i32::MAX as i64).max(0) as u64;
    job.board
        .init_autoroute(net, c.trace_clearance_class, Some(job.thread.clone()), Some(TimeLimit::new(per_pin_ms)), false);
    let src = job.board.connected_set(pin, net, false);
    let dst = job.board.unconnected_set(pin, net);
    job.board.autoroute_connection(&src, &dst, &c, &mut BTreeSet::new(), None)
}
