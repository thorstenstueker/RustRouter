//! `BatchAutorouter` with its pass loop (`AutorouteBatchLoop`), the pass runner
//! (`AutoroutePassRunner`), the connection router (`AutorouteConnectionRouter`),
//! `BoardHistory` and `DesignBlocks`.

use super::stats;
use super::RoutingJob;
use crate::autoroute::{AutorouteAttemptResult, AutorouteAttemptState, AutorouteControl};
use crate::board::connectivity::StopConnectionOption;
use crate::board::structure::Unit;
use crate::board::{Board, ItemId, ItemKind};
use crate::datastructures::TimeLimit;
use crate::geometry::basics::{java_round, JavaRandom};
use crate::optimize::ExpansionCostFactor;
use crate::settings::RouterSettings;
use std::collections::{BTreeSet, HashMap, HashSet};

pub const MAX_HISTORY_SIZE: usize = 30;
const BOARD_RANK_LIMIT: i32 = MAX_HISTORY_SIZE as i32;
const MAXIMUM_TRIES_ON_THE_SAME_BOARD: i32 = 3;
const TIME_LIMIT_TO_PREVENT_ENDLESS_LOOP: i32 = 1000;
const STOP_AT_PASS_MINIMUM: i32 = 8;
const STOP_AT_PASS_MODULO: i32 = 4;
const STAGNATION_PASS_LIMIT: i32 = 10;
const FANOUT_RECOVERY_STAGNATION_PASSES: i32 = 3;
const LAST_MILE_INCOMPLETE_LIMIT: i32 = 8;
const LAST_MILE_STAGNATION_PASSES: i32 = 3;
const LAST_MILE_MAX_ATTEMPTS: i32 = 2;
const STAGNATION_SCORE_THRESHOLD: f32 = 0.5;

/// `BatchAutorouter.StopReason`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Completed,
    MaxPasses,
    Stagnation,
    NoImprovement,
    TimedOut,
    Cancelled,
    Repeated,
}

// ==================================================================== BoardHistory

struct HistoryEntry {
    board: Board,
    hash: u64,
    score: f32,
    restore_count: i32,
}

/// `BoardHistory`: the best boards seen during routing.
pub struct BoardHistory {
    max_size: usize,
    boards: Vec<HistoryEntry>,
    settings: RouterSettings,
}

impl BoardHistory {
    pub fn new(settings: &RouterSettings) -> BoardHistory {
        BoardHistory {
            max_size: MAX_HISTORY_SIZE,
            boards: Vec::new(),
            settings: settings.clone(),
        }
    }

    pub fn add(&mut self, board: &Board) {
        let hash = board.content_hash();
        if self.boards.iter().any(|e| e.hash == hash) {
            return;
        }
        let score = stats::board_stats(board, &self.settings).router_score;
        if self.boards.len() >= self.max_size {
            let mut worst_index = 0;
            let mut worst_score = self.boards[0].score;
            for (i, e) in self.boards.iter().enumerate().skip(1) {
                if e.score < worst_score {
                    worst_score = e.score;
                    worst_index = i;
                }
            }
            if score <= worst_score {
                return;
            }
            self.boards.remove(worst_index);
        }
        self.boards.push(HistoryEntry {
            board: board.serialize(),
            hash,
            score,
            restore_count: 0,
        });
    }

    pub fn clear(&mut self) {
        self.boards.clear();
    }

    pub fn size(&self) -> usize {
        self.boards.len()
    }

    pub fn get_max_score(&self) -> f32 {
        let mut max = 0.0f32;
        for e in &self.boards {
            if e.score > max {
                max = e.score;
            }
        }
        max
    }

    pub fn restore_board(&mut self, max_allowed_restore_count: i32) -> Option<Board> {
        let max = if max_allowed_restore_count <= 0 { i32::MAX } else { max_allowed_restore_count };
        // List.sort is stable; Float.compare orders NaN last.
        self.boards.sort_by(|a, b| java_float_compare(b.score, a.score));
        for e in self.boards.iter_mut() {
            if e.restore_count <= max {
                e.restore_count += 1;
                return Some(Board::deserialize(&e.board));
            }
        }
        None
    }

    pub fn restore_best_board(&mut self) -> Option<Board> {
        self.restore_board(0)
    }

    pub fn get_rank(&self, board: &Board) -> i32 {
        let hash = board.content_hash();
        for (i, e) in self.boards.iter().enumerate() {
            if e.hash == hash {
                return i as i32 + 1;
            }
        }
        -1
    }
}

/// `Float.compare` (-0.0 < 0.0, NaN is the largest value).
pub fn java_float_compare(a: f32, b: f32) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.total_cmp(&b),
    }
}

// ==================================================================== DesignBlocks

/// `DesignBlocks`: connections the design blocks (pins covered by foreign fixed copper).
pub mod design_blocks {
    use super::*;

    pub fn is_blocked(board: &Board, pin: ItemId) -> bool {
        blocking_items(board, pin).map(|b| !b.is_empty()).unwrap_or(false)
    }

    pub fn blocking_items(board: &Board, pin: ItemId) -> Option<Vec<ItemId>> {
        let lc = board.layer_count();
        let p = board.item(pin);
        let count = p.as_drill().map(|d| d.tile_shape_count).unwrap_or(0);
        let mut pin_layers: BTreeSet<i32> = BTreeSet::new();
        for i in 0..count {
            pin_layers.insert(board.shape_layer(pin, i));
        }
        if pin_layers.is_empty() {
            return None;
        }
        let mut blocked_layers: BTreeSet<i32> = BTreeSet::new();
        let mut blockers: Vec<ItemId> = Vec::new();
        for v in stats::item_clearance_violations(board, pin) {
            let other = if v.first_item == pin { v.second_item } else { v.first_item };
            let oi = board.item(other);
            if v.actual_clearance > 0.0
                || oi.is_routable()
                || !is_copper(oi)
                || oi.shares_net(p)
                || !covers_centre(board, pin, other, v.layer, v.expected_clearance)
            {
                continue;
            }
            blocked_layers.insert(v.layer);
            if !blockers.contains(&other) {
                blockers.push(other);
            }
        }
        let _ = lc;
        if pin_layers.is_subset(&blocked_layers) {
            Some(blockers)
        } else {
            None
        }
    }

    pub fn is_blocked_set(board: &Board, set: &BTreeSet<ItemId>) -> bool {
        if set.is_empty() {
            return false;
        }
        set.iter().all(|id| board.item(*id).is_pin() && is_blocked(board, *id))
    }

    pub fn connections_blocked(board: &Board, item: ItemId, net_no: i32) -> bool {
        let own = board.connected_set(item, net_no, false);
        if is_blocked_set(board, &own) {
            return true;
        }
        let mut seen: HashSet<ItemId> = own.iter().copied().collect();
        let mut saw_other = false;
        for n in board.get_connectable_items(net_no) {
            if seen.contains(&n) {
                continue;
            }
            let other = board.connected_set(n, net_no, false);
            seen.extend(other.iter().copied());
            if !is_blocked_set(board, &other) {
                return false;
            }
            saw_other = true;
        }
        saw_other
    }

    pub fn blocked_airlines(board: &Board, airlines: &[stats::AirLine]) -> usize {
        airlines
            .iter()
            .filter(|a| end_blocked(board, a.from_item, a.net_no) || end_blocked(board, a.to_item, a.net_no))
            .count()
    }

    fn end_blocked(board: &Board, item: ItemId, net_no: i32) -> bool {
        board.item(item).is_pin() && is_blocked(board, item) && is_blocked_set(board, &board.connected_set(item, net_no, false))
    }

    fn covers_centre(board: &Board, pin: ItemId, other: ItemId, layer: i32, clearance: f64) -> bool {
        let p = board.item(pin);
        let centre = p.as_drill().unwrap().center.to_float();
        let mut half_width = 0;
        if p.net_count() > 0 {
            if let Some(c) = board.rules.net_class_of(p.get_net_number(0)) {
                half_width = c.get_trace_half_width(layer as usize);
            }
        }
        let n = board.tile_shape_count(other);
        for i in 0..n {
            if board.shape_layer(other, i) == layer {
                if let Some(s) = board.tile_shape(other, i) {
                    if s.distance(&centre) < clearance + half_width as f64 {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn is_copper(item: &crate::board::Item) -> bool {
        item.is_pin() || item.is_trace() || item.is_via() || item.is_conduction_area()
    }
}

// ==================================================================== BatchAutorouter

pub struct BatchAutorouter {
    pub remove_unconnected_vias: bool,
    pub trace_costs: Vec<ExpansionCostFactor>,
    pub retain_autoroute_database: bool,
    pub start_ripup_costs: i32,
    pub trace_pull_tight_accuracy: i32,
    pub total_items_routed: i32,
    pub fanout_timed_out: bool,
    pub initial_unrouted_count: i32,
    pub is_optimizer_autorouter: bool,
    pub necked_route_nets: BTreeSet<i32>,
    pub start_necked: bool,
    pub start_necked_routed: i32,
    pub start_necked_failed: i32,
    pub last_stop_reason: Option<StopReason>,
    pub pass_loop_start_hash: Option<u64>,
    /// `repeatsPassLoop`: hashes of earlier pass loop starts.
    pub repeats_pass_loop: Option<HashSet<u64>>,
    previous_incomplete_nets: BTreeSet<i32>,
    previous_incomplete_count: i32,
    stagnation_count: i32,
}

impl BatchAutorouter {
    /// `new BatchAutorouter(job)`
    pub fn from_job(job: &RoutingJob) -> BatchAutorouter {
        let s = &job.settings;
        Self::new(&job.board, s, !s.is_fanout_enabled(), true, s.get_start_ripup_costs(), s.trace_pull_tight_accuracy)
    }

    /// `BatchAutorouter.createAttempt(job, board, settings, removeUnconnectedVias)`
    pub fn create_attempt(board: &Board, settings: &RouterSettings, remove_unconnected_vias: bool) -> BatchAutorouter {
        Self::new(board, settings, remove_unconnected_vias, true, settings.get_start_ripup_costs(), settings.trace_pull_tight_accuracy)
    }

    pub fn new(
        board: &Board,
        settings: &RouterSettings,
        remove_unconnected_vias: bool,
        with_preferred_directions: bool,
        start_ripup_costs: i32,
        pull_tight_accuracy: i32,
    ) -> BatchAutorouter {
        let trace_costs = if with_preferred_directions {
            settings.get_trace_costs()
        } else {
            (0..board.layer_count() as usize)
                .map(|i| {
                    let c = settings.get_preferred_direction_trace_costs(i);
                    ExpansionCostFactor {
                        horizontal: c,
                        vertical: c,
                    }
                })
                .collect()
        };
        let start_necked = settings.autorouter.start_necked;
        let mut necked_route_nets = BTreeSet::new();
        if start_necked {
            for id in board.get_traces() {
                let it = board.item(id);
                let t = it.as_trace().unwrap();
                if it.net_count() == 1
                    && board.rules.nets.get(it.get_net_number(0)).is_some()
                    && t.half_width < board.rules.get_trace_half_width(it.get_net_number(0), t.layer as usize)
                {
                    necked_route_nets.insert(it.get_net_number(0));
                }
            }
        }
        BatchAutorouter {
            remove_unconnected_vias,
            trace_costs,
            retain_autoroute_database: false,
            start_ripup_costs,
            trace_pull_tight_accuracy: pull_tight_accuracy,
            total_items_routed: 0,
            fanout_timed_out: false,
            initial_unrouted_count: 0,
            is_optimizer_autorouter: false,
            necked_route_nets,
            start_necked,
            start_necked_routed: 0,
            start_necked_failed: 0,
            last_stop_reason: None,
            pass_loop_start_hash: None,
            repeats_pass_loop: None,
            previous_incomplete_nets: BTreeSet::new(),
            previous_incomplete_count: -1,
            stagnation_count: 0,
        }
    }

    fn repeats_earlier_pass_loop(&self, hash: u64) -> bool {
        self.repeats_pass_loop.as_ref().map(|s| s.contains(&hash)).unwrap_or(false)
    }

    pub fn reset_anti_oscillation_state(&mut self) {
        self.previous_incomplete_nets.clear();
        self.previous_incomplete_count = -1;
        self.stagnation_count = 0;
    }

    /// `autoroutePassesForOptimizingItem`: returns the number of passes used.
    #[allow(clippy::too_many_arguments)]
    pub fn autoroute_passes_for_optimizing_item(
        job: &mut RoutingJob,
        max_pass_count: i32,
        ripup_costs: i32,
        trace_pull_tight_accuracy: i32,
        with_preferred_directions: bool,
    ) -> i32 {
        let settings = job.settings.clone();
        let mut router = BatchAutorouter::new(&job.board, &settings, true, with_preferred_directions, ripup_costs, trace_pull_tight_accuracy);
        router.is_optimizer_autorouter = true;
        router.start_necked = false;
        let mut still_unrouted = true;
        let mut current_pass_no = 1;
        while still_unrouted && !job.thread.is_stop_autorouter_requested() && current_pass_no <= max_pass_count {
            still_unrouted = router.autoroute_pass(job, current_pass_no);
            current_pass_no += 1;
        }
        router.remove_tails(job, StopConnectionOption::None);
        if !still_unrouted {
            current_pass_no -= 1;
        }
        current_pass_no
    }

    /// `enforceStrictDrc`: removes the new items of a net, if they have clearance violations.
    pub fn enforce_strict_drc(board: &mut Board, route_net_no: i32, max_item_id_before: i32) -> Option<AutorouteAttemptResult> {
        let mut new_items = Vec::new();
        let mut has_violation = false;
        for id in board.get_connectable_items(route_net_no) {
            let it = board.item(id);
            if id.0 <= max_item_id_before || !(it.is_trace() || it.is_via()) {
                continue;
            }
            new_items.push(id);
            if !has_violation && stats::has_clearance_violations(board, id) {
                has_violation = true;
            }
        }
        if !has_violation {
            return None;
        }
        board.remove_items(&new_items);
        Some(AutorouteAttemptResult::with(
            AutorouteAttemptState::Failed,
            format!("strict_drc: connection ripped because {} new item(s) included clearance violations", new_items.len()),
        ))
    }

    /// `getAutorouteItems`: plane items first, then signal items.
    pub fn get_autoroute_items(board: &Board) -> Vec<ItemId> {
        let mut plane_items = Vec::new();
        let mut signal_items = Vec::new();
        let mut handled: BTreeSet<ItemId> = BTreeSet::new();
        for item in board.items.iter() {
            if !item.is_connectable_type() || item.is_routable() || handled.contains(&item.id) {
                continue;
            }
            let mut needs_routing = false;
            let mut has_plane_net = false;
            for i in 0..item.net_count() {
                let net_no = item.get_net_number(i);
                let connected = board.connected_set(item.id, net_no, false);
                for c in &connected {
                    if board.item(*c).net_count() <= 1 {
                        handled.insert(*c);
                    }
                }
                let net_item_count = board.connectable_item_count(net_no);
                if connected.len() < net_item_count && !board.has_ignored_nets(item) {
                    let is_plane = board.rules.nets.get(net_no).map(|n| n.contains_plane()).unwrap_or(false);
                    if is_plane {
                        if connected.iter().any(|c| board.item(*c).is_conduction_area()) {
                            continue;
                        }
                        has_plane_net = true;
                    }
                    needs_routing = true;
                }
            }
            if needs_routing {
                if has_plane_net {
                    plane_items.push(item.id);
                } else {
                    signal_items.push(item.id);
                }
            }
        }
        plane_items.extend(signal_items);
        plane_items
    }

    pub fn is_plane_item(board: &Board, item: ItemId) -> bool {
        let it = board.item(item);
        (0..it.net_count()).any(|i| board.rules.nets.get(it.get_net_number(i)).map(|n| n.contains_plane()).unwrap_or(false))
    }

    pub fn calculate_incomplete_count(board: &Board) -> i32 {
        stats::incomplete_count(board)
    }

    pub fn remove_tails(&self, job: &mut RoutingJob, stop: StopConnectionOption) {
        let board = &mut job.board;
        board.start_marking_changed_area();
        board.remove_trace_tails(-1, stop);
        board.opt_changed_area(
            &[],
            None,
            self.trace_pull_tight_accuracy,
            Some(&self.trace_costs),
            Some(job.thread.clone()),
            TIME_LIMIT_TO_PREVENT_ENDLESS_LOOP,
            None,
            -1,
        );
    }

    // ---------------------------------------------------------------- AutoroutePassRunner

    /// `autoroutePass(passNo)`: true, if there were items to route.
    pub fn autoroute_pass(&mut self, job: &mut RoutingJob, pass_no: i32) -> bool {
        let mut items = Self::get_autoroute_items(&job.board);
        self.drop_design_blocked_items(&job.board, &mut items);
        if items.is_empty() {
            return false;
        }
        if pass_no > 1 {
            self.reorder_single_thread_items(job, &mut items, pass_no);
        }
        let mut items_to_go = items.len() as i32;
        let mut ripped_item_count = 0;
        let mut not_routed = 0;
        let mut routed = 0;
        let mut _skipped = 0;
        'items: for current_item in items {
            if job.thread.is_stop_autorouter_requested() {
                break;
            }
            let net_count = job.board.try_item(current_item).map(|i| i.net_count()).unwrap_or(0);
            for i in 0..net_count {
                if job.thread.is_stop_autorouter_requested() {
                    break;
                }
                let max_items = job.settings.autorouter.max_items;
                if max_items > 0 && self.total_items_routed >= max_items {
                    job.log_info(&format!("Max items limit reached ({max_items}). Stopping auto-router."));
                    job.thread.request_stop();
                    break 'items;
                }
                self.total_items_routed += 1;
                job.board.start_marking_changed_area();
                let mut ripped: BTreeSet<ItemId> = BTreeSet::new();
                let mut ripped_costs: HashMap<ItemId, i32> = HashMap::new();
                let Some(item) = job.board.try_item(current_item) else {
                    continue;
                };
                let net_no = item.get_net_number(i);
                let result = self.autoroute_item(job, current_item, net_no, &mut ripped, &mut ripped_costs, pass_no);
                match result.state {
                    AutorouteAttemptState::Routed => routed += 1,
                    AutorouteAttemptState::AlreadyConnected
                    | AutorouteAttemptState::NoUnconnectedNets
                    | AutorouteAttemptState::ConnectedToPlane => _skipped += 1,
                    _ => {
                        job.board.failure_log.record_failure(current_item, net_no, pass_no, result.state, &result.details);
                        job.log_debug(&format!("Autorouter {}", result.details));
                        let failure_count = job.board.failure_log.get_failure_count(current_item);
                        if failure_count >= 2
                            && job.board.try_item(current_item).is_some()
                            && !design_blocked(&job.board, current_item, net_no)
                        {
                            let to_rip = failed_net_rip_items(&job.board, current_item, net_no);
                            if !to_rip.is_empty() {
                                job.board.remove_items(&to_rip);
                            }
                        }
                        not_routed += 1;
                    }
                }
                items_to_go -= 1;
                ripped_item_count += ripped.len();
                job.notify(false);
            }
        }
        let _ = (items_to_go, ripped_item_count);
        if self.remove_unconnected_vias {
            self.remove_tails(job, StopConnectionOption::None);
        } else {
            self.remove_tails(job, StopConnectionOption::FanoutVia);
        }
        let current_incomplete_nets = stats::incomplete_net_numbers(&job.board);
        let incomplete_count = Self::calculate_incomplete_count(&job.board);
        if self.previous_incomplete_count >= 0 && incomplete_count >= self.previous_incomplete_count {
            self.stagnation_count += 1;
        } else {
            self.stagnation_count = 0;
        }
        self.previous_incomplete_count = incomplete_count;
        self.previous_incomplete_nets = current_incomplete_nets;
        routed > 0 || not_routed > 0
    }

    fn reorder_single_thread_items(&self, job: &RoutingJob, items: &mut Vec<ItemId>, pass_no: i32) {
        let board = &job.board;
        let mut plane_items = Vec::new();
        let mut signal_items = Vec::new();
        for id in items.iter() {
            if Self::is_plane_item(board, *id) {
                plane_items.push(*id);
            } else {
                signal_items.push(*id);
            }
        }
        if signal_items.is_empty() {
            return;
        }
        let key = |id: &ItemId| {
            let it = board.item(*id);
            (if it.net_count() > 0 { it.get_net_number(0) } else { 0 }, id.0)
        };
        let order_seed = job.settings.autorouter.item_order_seed;
        if self.stagnation_count >= 2 || order_seed.is_some() {
            signal_items.sort_by_key(key);
            let mut seed = (pass_no as i64).wrapping_mul(1000003).wrapping_add(board.rules.nets.max_net_number() as i64);
            if let Some(s) = order_seed {
                seed = seed.wrapping_mul(31).wrapping_add(s);
            }
            java_shuffle(&mut signal_items, &mut JavaRandom::new(seed));
        } else if !self.previous_incomplete_nets.is_empty() {
            let mut persistent = Vec::new();
            let mut other = Vec::new();
            for id in signal_items {
                let it = board.item(id);
                if (0..it.net_count()).any(|i| self.previous_incomplete_nets.contains(&it.get_net_number(i))) {
                    persistent.push(id);
                } else {
                    other.push(id);
                }
            }
            persistent.sort_by_key(key);
            other.sort_by_key(key);
            signal_items = persistent;
            signal_items.extend(other);
        } else {
            signal_items.sort_by_key(key);
        }
        items.clear();
        items.extend(plane_items);
        items.extend(signal_items);
    }

    fn drop_design_blocked_items(&self, board: &Board, items: &mut Vec<ItemId>) {
        let before = items.len();
        items.retain(|id| {
            let blocked_candidate = board.failure_log.get_failure_count(*id) > 0;
            !(blocked_candidate && design_blocked(board, *id, board.item(*id).get_net_number(0)))
        });
        if items.len() < before {
            log::debug!(
                "Not retrying {} item(s) whose open connections the design blocks (pins on foreign copper).",
                before - items.len()
            );
        }
    }

    // ---------------------------------------------------------------- AutorouteConnectionRouter

    fn new_control(&self, job: &RoutingJob, net_no: i32, via_costs: i32, pass_no: i32) -> AutorouteControl {
        let mut c = AutorouteControl::with_costs(&job.board, net_no, &job.settings, via_costs, self.trace_costs.clone());
        c.ripup_allowed = true;
        c.ripup_costs = self.start_ripup_costs.wrapping_mul(pass_no);
        c.remove_unconnected_vias = self.remove_unconnected_vias;
        // Like the original, `ripup_pass_no` keeps its default (1): `AutorouteConnectionRouter`
        // only scales the ripup costs by the pass number, so ripup costs are never randomized.
        c
    }

    /// `AutorouteConnectionRouter.route`
    pub fn autoroute_item(
        &mut self,
        job: &mut RoutingJob,
        item: ItemId,
        net_no: i32,
        ripped: &mut BTreeSet<ItemId>,
        ripup_costs: &mut HashMap<ItemId, i32>,
        pass_no: i32,
    ) -> AutorouteAttemptResult {
        job.board.clear_removed_items();
        let contains_plane = job.board.rules.nets.get(net_no).map(|n| n.contains_plane()).unwrap_or(false);
        let via_costs = if contains_plane {
            job.settings.get_plane_via_costs()
        } else {
            job.settings.get_via_costs()
        };
        let ctrl = self.new_control(job, net_no, via_costs, pass_no);
        let unconnected = job.board.unconnected_set(item, net_no);
        if unconnected.is_empty() {
            return AutorouteAttemptResult::new(AutorouteAttemptState::NoUnconnectedNets);
        }
        let connected = job.board.connected_set(item, net_no, false);
        let (start_set, dest_set) = if contains_plane {
            if connected.iter().any(|c| job.board.item(*c).is_conduction_area()) {
                return AutorouteAttemptResult::new(AutorouteAttemptState::ConnectedToPlane);
            }
            (connected, unconnected)
        } else {
            (unconnected, connected)
        };
        let max_ms = (100000.0 * 2f64.powi(pass_no - 1)).min(i32::MAX as f64) as i32;
        let time_limit = TimeLimit::new(max_ms.max(0) as u64);
        let strict = job.settings.strict_drc;
        if self.start_necked && job.settings.get_neck_width_um() > 0.0 && self.necked_route_nets.contains(&net_no) {
            let before_start = job.board.communication.id_generator.max_generated_id();
            let start_snapshot = if strict { Some(job.board.serialize()) } else { None };
            if let Some(started) = self.retry_connection_necked(
                job, net_no, &ctrl, via_costs, &start_set, &dest_set, ripped, ripup_costs, pass_no, time_limit, true, true,
            ) {
                if started.state != AutorouteAttemptState::Routed {
                    self.start_necked_failed += 1;
                    return started;
                }
                if let Some(rej) = self.apply_strict_drc_after_route(job, net_no, before_start, start_snapshot.as_ref(), pass_no) {
                    return rej;
                }
                self.start_necked_routed += 1;
                return started;
            }
        }
        job.board
            .init_autoroute(net_no, ctrl.trace_clearance_class, Some(job.thread.clone()), Some(time_limit), self.retain_autoroute_database);
        let max_item_id_before_route = job.board.communication.id_generator.max_generated_id();
        let strict_snapshot = if strict { Some(job.board.serialize()) } else { None };
        let result = job.board.autoroute_connection(&start_set, &dest_set, &ctrl, ripped, Some(ripup_costs));
        if result.state == AutorouteAttemptState::Routed {
            job.board.opt_changed_area(
                &[],
                None,
                self.trace_pull_tight_accuracy,
                Some(&ctrl.trace_costs),
                Some(job.thread.clone()),
                TIME_LIMIT_TO_PREVENT_ENDLESS_LOOP,
                None,
                -1,
            );
        }
        if (result.state == AutorouteAttemptState::Failed || result.state == AutorouteAttemptState::InsertError)
            && job.settings.get_neck_width_um() > 0.0
        {
            if let Some(necked) = self.retry_connection_necked(
                job, net_no, &ctrl, via_costs, &start_set, &dest_set, ripped, ripup_costs, pass_no, time_limit, false, false,
            ) {
                if let Some(r) = self.apply_strict_drc_after_route(job, net_no, max_item_id_before_route, strict_snapshot.as_ref(), pass_no) {
                    return r;
                }
                return necked;
            }
        }
        if result.state == AutorouteAttemptState::Routed {
            if let Some(r) = self.apply_strict_drc_after_route(job, net_no, max_item_id_before_route, strict_snapshot.as_ref(), pass_no) {
                return r;
            }
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn retry_connection_necked(
        &mut self,
        job: &mut RoutingJob,
        net_no: i32,
        original: &AutorouteControl,
        via_costs: i32,
        start_set: &BTreeSet<ItemId>,
        dest_set: &BTreeSet<ItemId>,
        ripped: &mut BTreeSet<ItemId>,
        ripup_costs: &mut HashMap<ItemId, i32>,
        pass_no: i32,
        time_limit: TimeLimit,
        planned: bool,
        quiet: bool,
    ) -> Option<AutorouteAttemptResult> {
        let neck_half_width = AutorouteControl::neck_half_width(&job.board, &job.settings);
        let narrower = (0..original.layer_count).any(|i| original.layer_active[i] && original.trace_half_width[i] > neck_half_width);
        if !narrower {
            return None;
        }
        let mut neck = self.new_control(job, net_no, via_costs, pass_no);
        for i in 0..neck.layer_count {
            let compensation = neck.compensated_trace_half_width[i] - neck.trace_half_width[i];
            neck.trace_half_width[i] = neck.trace_half_width[i].min(neck_half_width);
            neck.compensated_trace_half_width[i] = neck.trace_half_width[i] + compensation;
            neck.update_trace_clearance_offset(i as i32, &job.board);
        }
        job.board
            .init_autoroute(net_no, neck.trace_clearance_class, Some(job.thread.clone()), Some(time_limit), self.retain_autoroute_database);
        let result = job.board.autoroute_connection(start_set, dest_set, &neck, ripped, Some(ripup_costs));
        if result.state != AutorouteAttemptState::Routed {
            return if planned { Some(result) } else { None };
        }
        self.necked_route_nets.insert(net_no);
        job.board.opt_changed_area(
            &[],
            None,
            self.trace_pull_tight_accuracy,
            Some(&neck.trace_costs),
            Some(job.thread.clone()),
            TIME_LIMIT_TO_PREVENT_ENDLESS_LOOP,
            None,
            -1,
        );
        if !quiet {
            let name = job.board.rules.nets.get(net_no).map(|n| n.name.clone()).unwrap_or_else(|| format!("#{net_no}"));
            log::info!(
                "{}{}' at {} um trace width.",
                if planned { "Retry plan routed net '" } else { "Necked retry routed net '" },
                name,
                job.settings.get_neck_width_um()
            );
        }
        Some(result)
    }

    fn apply_strict_drc_after_route(
        &self,
        job: &mut RoutingJob,
        net_no: i32,
        max_item_id_before: i32,
        snapshot: Option<&Board>,
        pass_no: i32,
    ) -> Option<AutorouteAttemptResult> {
        let strict = job.settings.strict_drc || pass_no >= 3;
        if !strict {
            return None;
        }
        let rejection = Self::enforce_strict_drc(&mut job.board, net_no, max_item_id_before);
        if rejection.is_some() {
            if let Some(s) = snapshot {
                job.board = Board::deserialize(s);
            }
        }
        rejection
    }

    // ---------------------------------------------------------------- AutorouteBatchLoop

    fn stop_for(&mut self, job: &RoutingJob, reason: StopReason) {
        if self.last_stop_reason.is_none() {
            self.last_stop_reason = Some(reason);
        }
        job.thread.request_stop_autorouter();
    }

    fn router_score(job: &RoutingJob) -> (stats::BoardStats, f32) {
        let s = stats::board_stats(&job.board, &job.settings);
        let score = s.router_score;
        (s, score)
    }

    /// `runBatchLoop()`: fanout stage and the autoroute pass loop. Returns false if stopped.
    pub fn run_batch_loop(&mut self, job: &mut RoutingJob) -> bool {
        let lc = job.settings.layer_count().min(job.board.layer_structure.count());
        let any_routable = (0..lc).any(|i| job.settings.get_layer_active(i) && job.board.layer_structure.layers[i].is_signal);
        if !any_routable {
            log::warn!("Cannot start autorouter: all layers are disabled.");
            return false;
        }
        self.initial_unrouted_count = Self::calculate_incomplete_count(&job.board);
        self.reset_anti_oscillation_state();
        self.last_stop_reason = None;
        let mut bh = BoardHistory::new(&job.settings);
        if job.settings.is_fanout_enabled() {
            if job.board.get_smd_pins().is_empty() {
                job.log_info("Fanout stage is enabled but skipped because the board has no SMD pins.");
            } else {
                let summary = super::fanout::fanout_board(job);
                self.fanout_timed_out = summary.is_timed_out;
            }
        }
        let current_unrouted = Self::calculate_incomplete_count(&job.board);
        let is_router_enabled = job.settings.get_run_router() && job.settings.autorouter.max_passes >= 0;
        if is_router_enabled {
            let (_, score) = Self::router_score(job);
            job.log_info(&format!(
                "Auto-routing stage started on board '{}' with baseline score {:.2} for {} unrouted item{}.",
                job.board.get_hash(),
                score,
                current_unrouted,
                if current_unrouted == 1 { "" } else { "s" }
            ));
        }
        let mut continue_autorouting = is_router_enabled;
        if is_router_enabled {
            let h = job.board.content_hash();
            self.pass_loop_start_hash = Some(h);
            if self.repeats_earlier_pass_loop(h) {
                self.last_stop_reason = Some(StopReason::Repeated);
                continue_autorouting = false;
                job.log_info("Pass loop skipped: fanout left the board an earlier pass loop with the same settings started on.");
            }
        }
        let mut current_pass = 1;
        let mut consecutive_no_improvement = 0;
        let mut fanout_recovery_applied = false;
        let mut last_mile_attempts = 0;
        let mut last_best_score = f32::NEG_INFINITY;
        let mut global_best_score = f32::NEG_INFINITY;
        let mut pass_of_best_score = 0;
        let mut incomplete_at_best = 0;
        while continue_autorouting && !job.thread.is_stop_autorouter_requested() {
            if job.is_timed_out() {
                self.stop_for(job, StopReason::TimedOut);
            }
            let max_passes = job.settings.autorouter.max_passes;
            if max_passes > 0 && current_pass > max_passes {
                self.stop_for(job, StopReason::MaxPasses);
                break;
            }
            job.current_pass = current_pass;
            job.progress_pass = current_pass;
            bh.add(&job.board);
            let pass_start = std::time::Instant::now();
            continue_autorouting = self.autoroute_pass(job, current_pass);
            job.notify(true);
            let (mut stats_after, mut score_after) = Self::router_score(job);
            if bh.size() as i32 >= STOP_AT_PASS_MINIMUM || job.thread.is_stop_autorouter_requested() {
                if (current_pass % STOP_AT_PASS_MODULO == 0 && current_pass >= STOP_AT_PASS_MINIMUM)
                    || job.thread.is_stop_autorouter_requested()
                {
                    if bh.get_max_score() > score_after {
                        let Some(restored) = bh.restore_board(MAXIMUM_TRIES_ON_THE_SAME_BOARD) else {
                            job.log_info("The router was not able to improve the board, stopping the auto-router.");
                            self.stop_for(job, StopReason::NoImprovement);
                            break;
                        };
                        let rank = bh.get_rank(&restored);
                        if rank > BOARD_RANK_LIMIT {
                            self.stop_for(job, StopReason::Stagnation);
                            break;
                        }
                        job.board = restored;
                        self.reset_anti_oscillation_state();
                        consecutive_no_improvement = 0;
                        let (s, sc) = Self::router_score(job);
                        stats_after = s;
                        score_after = sc;
                        last_best_score = score_after;
                    }
                }
            }
            if !self.is_optimizer_autorouter {
                job.log_info(&format!(
                    "Auto-routing pass #{} on board '{}' was completed in {:.2} seconds with score {:.2} ({} unrouted, {} violations).",
                    current_pass,
                    job.board.get_hash(),
                    pass_start.elapsed().as_secs_f64(),
                    score_after,
                    stats_after.incomplete_count,
                    stats_after.clearance_violation_count
                ));
            }
            if current_pass >= STOP_AT_PASS_MINIMUM && continue_autorouting {
                if score_after > last_best_score + STAGNATION_SCORE_THRESHOLD {
                    consecutive_no_improvement = 0;
                    last_best_score = score_after;
                } else {
                    consecutive_no_improvement += 1;
                    if job.settings.is_fanout_enabled()
                        && !fanout_recovery_applied
                        && stats_after.incomplete_count > 0
                        && consecutive_no_improvement >= FANOUT_RECOVERY_STAGNATION_PASSES
                    {
                        let before = stats_after.incomplete_count;
                        self.remove_tails(job, StopConnectionOption::None);
                        let (s, sc) = Self::router_score(job);
                        stats_after = s;
                        score_after = sc;
                        last_best_score = score_after;
                        consecutive_no_improvement = 0;
                        fanout_recovery_applied = true;
                        job.log_debug(&format!(
                            "Applied one-time fanout recovery cleanup (removed fanout tails/vias). Incompletes: {} -> {}.",
                            before, stats_after.incomplete_count
                        ));
                    }
                    let incomplete_now = stats_after.incomplete_count;
                    if incomplete_now > 0
                        && incomplete_now <= LAST_MILE_INCOMPLETE_LIMIT
                        && last_mile_attempts < LAST_MILE_MAX_ATTEMPTS
                        && consecutive_no_improvement >= LAST_MILE_STAGNATION_PASSES
                    {
                        let removed = super::fallback::rip_last_mile_blockers(&mut job.board);
                        last_mile_attempts += 1;
                        if removed > 0 {
                            let (s, sc) = Self::router_score(job);
                            stats_after = s;
                            score_after = sc;
                            last_best_score = score_after;
                            consecutive_no_improvement = 0;
                            job.log_info(&format!(
                                "Last-mile rip-up removed {removed} blocking trace(s) or via(s) around {incomplete_now} remaining connection(s)."
                            ));
                        }
                    }
                    if consecutive_no_improvement >= STAGNATION_PASS_LIMIT {
                        let report = stats::unrouted_report(&job.board);
                        job.log_info(&format!(
                            "The router's score ({:.2}) has not improved by more than {} points in the last {} passes ({} item{} still unconnected). Stopping the auto-router.\nThe following connections could not be routed -- please review your design (e.g. check pad clearances, trace width rules, and available routing space):\n{}",
                            score_after,
                            STAGNATION_SCORE_THRESHOLD,
                            STAGNATION_PASS_LIMIT,
                            stats_after.incomplete_count,
                            if stats_after.incomplete_count == 1 { "" } else { "s" },
                            report
                        ));
                        self.stop_for(job, StopReason::Stagnation);
                        break;
                    }
                }
                if score_after > global_best_score + STAGNATION_SCORE_THRESHOLD {
                    global_best_score = score_after;
                    pass_of_best_score = current_pass;
                    incomplete_at_best = stats_after.incomplete_count;
                } else if current_pass - pass_of_best_score >= STAGNATION_PASS_LIMIT {
                    let report = stats::unrouted_report(&job.board);
                    job.log_info(&format!(
                        "The router's best score ({:.2}) has not improved by more than {} points since pass #{}. Stopping the auto-router after {} passes ({} item{} still unconnected).\nThe following connections could not be routed -- please review your design (e.g. check pad clearances, trace width rules, and available routing space):\n{}",
                        global_best_score,
                        STAGNATION_SCORE_THRESHOLD,
                        pass_of_best_score,
                        current_pass,
                        incomplete_at_best,
                        if incomplete_at_best == 1 { "" } else { "s" },
                        report
                    ));
                    self.stop_for(job, StopReason::Stagnation);
                    break;
                }
            } else if stats_after.incomplete_count == 0 && score_after > STAGNATION_SCORE_THRESHOLD {
                consecutive_no_improvement = 0;
                last_best_score = score_after;
            }
            if continue_autorouting && !job.thread.is_stop_autorouter_requested() {
                current_pass += 1;
            }
        }
        let (_, current_final_score) = Self::router_score(job);
        let best_history_score = bh.get_max_score();
        if best_history_score > current_final_score {
            if let Some(best) = bh.restore_best_board() {
                job.board = best;
                job.log_debug(&format!(
                    "The final board state (score {current_final_score:.2}) is worse than the best board seen during routing (score {best_history_score:.2}). Restoring the best board as the final result."
                ));
            }
        }
        if self.last_stop_reason.is_none() {
            self.last_stop_reason = Some(if job.is_timed_out() {
                StopReason::TimedOut
            } else if job.thread.is_stop_requested() {
                StopReason::Cancelled
            } else if job.thread.is_stop_autorouter_requested() {
                StopReason::Stagnation
            } else {
                StopReason::Completed
            });
        }
        let was_router_run = job.settings.get_run_router() && job.settings.autorouter.max_passes >= 0;
        if was_router_run && !job.thread.is_stop_autorouter_requested() && self.last_stop_reason != Some(StopReason::Repeated) {
            self.remove_tails(job, StopConnectionOption::None);
        }
        bh.clear();
        if self.start_necked_routed + self.start_necked_failed > 0 {
            job.log_info(&format!(
                "Start necked: {} connection(s) of nets that needed the neck before routed at the neck width without a class-width search, {} did not route.",
                self.start_necked_routed, self.start_necked_failed
            ));
        }
        !job.thread.is_stop_autorouter_requested()
    }
}

/// `Collections.shuffle(list, rnd)`
pub fn java_shuffle<T>(list: &mut [T], rnd: &mut JavaRandom) {
    let size = list.len();
    let mut i = size;
    while i > 1 {
        let j = rnd.next_int(i as i32) as usize;
        list.swap(i - 1, j);
        i -= 1;
    }
}

/// `AutoroutePassRunner.designBlocked`
pub fn design_blocked(board: &Board, item: ItemId, net_no: i32) -> bool {
    board.item(item).net_count() == 1 && design_blocks::connections_blocked(board, item, net_no)
}

/// `AutoroutePassRunner.failedNetRipItems`
fn failed_net_rip_items(board: &Board, failed_item: ItemId, net_no: i32) -> Vec<ItemId> {
    let items = board.get_connectable_items(net_no);
    let pin_count = items.iter().filter(|i| board.item(**i).is_pin()).count();
    let branched = pin_count > 2;
    let ct = board.communication.coordinate_transform.as_ref();
    let um = Unit::scale(1500.0, Unit::Um, board.communication.unit);
    let offset = java_round(ct.map(|c| c.dsn_to_board(um)).unwrap_or(um));
    let region = board.item(failed_item).bounding_box().offset(offset as f64);
    let mut result = Vec::new();
    for id in items {
        let it = board.item(id);
        if (it.is_trace() || it.is_via())
            && !it.is_user_fixed()
            && it.net_count() == 1
            && (!branched || region.intersects(&it.bounding_box()))
        {
            result.push(id);
        }
    }
    result
}

#[allow(dead_code)]
fn _unused(_: ItemKind) {}
