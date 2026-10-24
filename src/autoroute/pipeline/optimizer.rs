//! The route optimizer (port of `BatchOptimizer` and `ItemRouteResult`).
//!
//! Each candidate item is rerouted on a fresh copy of the pass baseline board (as the
//! worker boards of the original), so the candidates of a chunk are evaluated in parallel
//! with rayon and the result does not depend on the number of threads.

use super::batch::BatchAutorouter;
use super::{stats, RoutingJob};
use crate::board::connectivity::StopConnectionOption;
use crate::board::{Board, ItemId};
use crate::geometry::basics::java_round;
use crate::geometry::FloatPoint;
use crate::settings::ItemSelectionStrategy;
use rayon::prelude::*;
use std::collections::{BTreeSet, HashMap};
use std::time::Instant;

/// `ItemRouteResult`
#[derive(Clone, Debug)]
pub struct ItemRouteResult {
    pub item_id: i32,
    pub improvement_percentage: f32,
    pub via_count_before: i32,
    pub via_count_after: i32,
    pub trace_length_before: f64,
    pub trace_length_after: f64,
    pub incomplete_count_before: i32,
    pub incomplete_count_after: i32,
    pub improved: bool,
}

impl ItemRouteResult {
    pub fn empty(item_id: i32) -> Self {
        let mut r = Self::new(item_id, 0, 0, 0.0, 0.0, 0, 1);
        r.improved = false;
        r
    }

    pub fn new(item_id: i32, vb: i32, va: i32, tb: f64, ta: f64, ib: i32, ia: i32) -> Self {
        let improved = if ia < ib {
            true
        } else if ia > ib {
            false
        } else if va < vb {
            true
        } else if va > vb {
            false
        } else {
            ta < tb
        };
        let improvement_percentage = if vb != 0 && tb != 0.0 {
            (1.0 - ((va as f64 / vb as f64) + (ta / tb)) / 2.0) as f32
        } else {
            0.0
        };
        ItemRouteResult {
            item_id,
            improvement_percentage,
            via_count_before: vb,
            via_count_after: va,
            trace_length_before: tb,
            trace_length_after: ta,
            incomplete_count_before: ib,
            incomplete_count_after: ia,
            improved,
        }
    }

    /// `compareTo`
    pub fn compare(&self, r: &ItemRouteResult) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        if self.incomplete_count_after != r.incomplete_count_after {
            return self.incomplete_count_after.cmp(&r.incomplete_count_after);
        }
        if self.via_count_after != r.via_count_after {
            return self.via_count_after.cmp(&r.via_count_after);
        }
        if self.trace_length_after < r.trace_length_after {
            return Less;
        }
        if self.trace_length_after > r.trace_length_after {
            return Greater;
        }
        self.item_id.cmp(&r.item_id)
    }

    pub fn improved_over(&self, r: &ItemRouteResult) -> bool {
        self.compare(r).is_lt()
    }
}

pub struct BatchOptimizer {
    use_increased_ripup_costs: bool,
    min_cumulative_trace_length: f64,
    total_items_optimized: i32,
    deadline: Option<Instant>,
    pub is_timed_out: bool,
    best_board: Option<Board>,
    best_score: f32,
    best_incomplete_count: i32,
    best_clearance_violation_count: i32,
    result_map: HashMap<i32, ItemRouteResult>,
}

struct CandidateResult {
    result: ItemRouteResult,
    board: Option<Board>,
}

impl BatchOptimizer {
    pub fn new(job: &mut RoutingJob) -> BatchOptimizer {
        if job.settings.optimizer.algorithm != "freerouting-optimizer" {
            job.log_warning(&format!(
                "The algorithm '{}' is not supported by the batch autorouter. The default algorithm 'freerouting-optimizer' will be used instead.",
                job.settings.optimizer.algorithm
            ));
            job.settings.optimizer.algorithm = "freerouting-optimizer".to_string();
        }
        BatchOptimizer {
            use_increased_ripup_costs: false,
            min_cumulative_trace_length: 0.0,
            total_items_optimized: 0,
            deadline: None,
            is_timed_out: false,
            best_board: None,
            best_score: 0.0,
            best_incomplete_count: 0,
            best_clearance_violation_count: 0,
            result_map: HashMap::new(),
        }
    }

    fn evaluate_pre_flight_guards(&self, job: &RoutingJob, s: &stats::BoardStats) -> Option<String> {
        if !job.settings.optimizer.enable_preflight_guards {
            return None;
        }
        if s.incomplete_count > 0 {
            return Some(format!(
                "the board has {} unrouted connection(s) (optimizer only runs on completely routed boards)",
                s.incomplete_count
            ));
        }
        let initial = s.optimizer_score;
        if s.via_count == 0 {
            if initial >= 950.0 {
                return Some(format!("the board has no vias to eliminate and initial optimizer score ({initial:.2}) is already >= 950.00"));
            }
            if let (Some(min), Some(total)) = (s.min_trace_length_mm, s.trace_total_length_mm) {
                if min > 0.0 && (total as f32) <= (min as f32) * 1.05 {
                    return Some(format!(
                        "the board has no vias to eliminate and trace length ({total:.2} mm) is within 5% of theoretical minimum ({min:.2} mm)"
                    ));
                }
            }
        }
        if initial >= 995.0 {
            return Some(format!("the initial optimizer score ({initial:.2}) is already at or near theoretical maximum (995.00)"));
        }
        if let (Some(min), Some(total)) = (s.min_trace_length_mm, s.trace_total_length_mm) {
            if min > 0.0 && (total as f32) <= (min as f32) * 1.02 && s.min_via_count.map(|m| s.via_count <= m).unwrap_or(true) {
                return Some(format!(
                    "total trace length ({total:.2} mm) is already within 2% of the theoretical minimum ({min:.2} mm)"
                ));
            }
        }
        if are_all_vias_mandatory_layer_transitions(&job.board) {
            return Some("all vias on the board are mandatory layer transitions between SMD pins that cannot be eliminated".to_string());
        }
        None
    }

    /// `BatchOptimizer.runBatchLoop`
    pub fn run_batch_loop(&mut self, job: &mut RoutingJob) {
        job.thread.reset_autorouter_stop_request();
        job.log_debug(&format!(
            "Before optimization: Via count: {}, trace length: {}",
            job.board.get_via_count(),
            java_round(job.board.cumulative_trace_length())
        ));
        self.use_increased_ripup_costs = true;
        let initial = stats::board_stats(&job.board, &job.settings);
        if let Some(reason) = self.evaluate_pre_flight_guards(job, &initial) {
            job.log_info(&format!("Skipping optimization stage: {reason}."));
            return;
        }
        self.best_board = Some(job.board.deep_copy());
        self.best_score = initial.optimizer_score;
        self.best_incomplete_count = initial.incomplete_count;
        self.best_clearance_violation_count = initial.clearance_violation_count;
        job.log_info(&format!(
            "Optimization stage started on board '{}'. Baseline router score: {:.2}, optimizer score: {:.2}, incomplete connections: {}, clearance violations: {}.",
            job.board.get_hash(),
            initial.router_score,
            initial.optimizer_score,
            initial.incomplete_count,
            initial.clearance_violation_count
        ));
        let session_start = Instant::now();
        if let Some(t) = job.settings.optimizer.timeout_string.as_deref().and_then(super::parse_timespan_string) {
            self.deadline = Some(session_start + std::time::Duration::from_secs(t.max(0) as u64));
        }
        let threshold = job.settings.optimizer.optimization_improvement_threshold;
        if threshold.is_nan() || threshold.is_infinite() || threshold < 0.0 {
            job.settings.optimizer.optimization_improvement_threshold = 2.5;
        } else if threshold > 0.0 && threshold < 0.1 {
            job.settings.optimizer.optimization_improvement_threshold = threshold * 100.0;
        }
        let mut current_pass = 0;
        while current_pass < job.settings.optimizer.max_passes
            && self.total_items_optimized < job.settings.optimizer.max_items
            && !job.thread.is_stop_requested()
        {
            if let Some(d) = self.deadline {
                if Instant::now() >= d {
                    self.is_timed_out = true;
                    job.log_info(&format!("Optimizer stage timed out before starting pass #{}", current_pass + 1));
                    break;
                }
            }
            current_pass += 1;
            job.progress_pass = current_pass;
            let score_before = stats::board_stats(&job.board, &job.settings).optimizer_score;
            let with_preferred = current_pass % 2 != 0;
            self.opt_route_pass(job, current_pass, with_preferred);
            job.notify(true);
            if self.is_timed_out {
                break;
            }
            let pass_stats = stats::board_stats(&job.board, &job.settings);
            let score_after = pass_stats.optimizer_score;
            let rejection = if pass_stats.incomplete_count > self.best_incomplete_count {
                Some("CONNECTIVITY_REGRESSION")
            } else if pass_stats.clearance_violation_count > self.best_clearance_violation_count {
                Some("DRC_COUNT_REGRESSION")
            } else if !(score_after > self.best_score) {
                Some("OPTIMIZER_SCORE_NOT_IMPROVED")
            } else {
                None
            };
            match rejection {
                None => {
                    self.best_score = score_after;
                    self.best_incomplete_count = pass_stats.incomplete_count;
                    self.best_clearance_violation_count = pass_stats.clearance_violation_count;
                    self.best_board = Some(job.board.deep_copy());
                }
                Some(r) => {
                    job.log_info(&format!(
                        "Optimizer pass #{} candidate rejected: {}. Restoring incumbent (optimizer score {:.2}, incomplete connections: {}, clearance violations: {}).",
                        current_pass, r, self.best_score, self.best_incomplete_count, self.best_clearance_violation_count
                    ));
                    self.restore_incumbent_board(job);
                }
            }
            let fraction = if score_before > 0.0 {
                ((score_after - score_before) / score_before) as f64
            } else {
                0.0
            };
            let percent = fraction * 100.0;
            job.log_info(&format!(
                "Optimizer pass #{}: optimizer score {:.2} -> {:.2}, router score: {:.2}, incomplete connections: {}, clearance violations: {}.",
                current_pass, score_before, score_after, pass_stats.router_score, pass_stats.incomplete_count, pass_stats.clearance_violation_count
            ));
            let improvement = if self.use_increased_ripup_costs && score_after <= score_before {
                self.use_increased_ripup_costs = false;
                -1.0
            } else {
                percent
            };
            let threshold = job.settings.optimizer.optimization_improvement_threshold as f64;
            if improvement != -1.0 && improvement < threshold {
                job.log_info(&format!(
                    "Stopping optimizer because the improvement in this pass ({improvement:.4}%) is below the threshold ({threshold:.2}%)."
                ));
                break;
            }
        }
        let end = stats::board_stats(&job.board, &job.settings);
        let regressed = end.optimizer_score < self.best_score
            || end.incomplete_count > self.best_incomplete_count
            || end.clearance_violation_count > self.best_clearance_violation_count;
        if regressed && self.best_board.is_some() {
            job.log_info(&format!(
                "Restoring best board achieved (score {:.2} vs final {:.2}, incomplete connections {} vs {}, clearance violations {} vs {}).",
                self.best_score,
                end.optimizer_score,
                self.best_incomplete_count,
                end.incomplete_count,
                self.best_clearance_violation_count,
                end.clearance_violation_count
            ));
            self.restore_incumbent_board(job);
        }
        self.best_board = None;
        let fin = stats::board_stats(&job.board, &job.settings);
        job.log_info(&format!(
            "Optimization stage {} Baseline router score: {:.2}, baseline optimizer score: {:.2}, final router score: {:.2}, final optimizer score: {:.2}, completed in {:.2} seconds.",
            if self.is_timed_out {
                "completed with timeout:"
            } else if job.thread.is_stop_requested() {
                "interrupted:"
            } else {
                "completed:"
            },
            initial.router_score,
            initial.optimizer_score,
            fin.router_score,
            fin.optimizer_score,
            session_start.elapsed().as_secs_f64()
        ));
    }

    fn restore_incumbent_board(&mut self, job: &mut RoutingJob) {
        if let Some(b) = &self.best_board {
            job.board = b.deep_copy();
        }
    }

    fn prepare_candidate_items(&mut self, board: &Board, strategy: &ItemSelectionStrategy) -> Vec<i32> {
        let mut ids = Vec::new();
        let mut sorted = ReadSortedRouteItems::new();
        if *strategy == ItemSelectionStrategy::Prioritized && !self.result_map.is_empty() {
            let mut prioritized: Vec<ItemRouteResult> = Vec::new();
            let mut non_prioritized = Vec::new();
            while let Some(item) = sorted.next(board) {
                match self.result_map.get(&item.0) {
                    Some(r) => prioritized.push(r.clone()),
                    None => non_prioritized.push(item.0),
                }
            }
            prioritized.sort_by(|a, b| a.compare(b));
            ids.extend(prioritized.iter().map(|r| r.item_id));
            ids.extend(non_prioritized);
        } else {
            while let Some(item) = sorted.next(board) {
                ids.push(item.0);
            }
        }
        self.result_map.clear();
        ids
    }

    fn opt_route_pass(&mut self, job: &mut RoutingJob, pass_no: i32, with_preferred: bool) -> f32 {
        let before = stats::optimizer_metrics(&job.board);
        self.min_cumulative_trace_length = before.1;
        let strategy = job.settings.optimizer.item_selection_strategy.clone();
        let mut candidates = self.prepare_candidate_items(&job.board, &strategy);
        let max_items = job.settings.optimizer.max_items;
        if max_items > 0 {
            let remaining = max_items - self.total_items_optimized;
            if remaining <= 0 {
                job.log_info(&format!("Max items limit reached ({max_items}). Stopping optimizer."));
                return 0.0;
            }
            if candidates.len() > remaining as usize {
                candidates.truncate(remaining as usize);
            }
        }
        if candidates.is_empty() {
            return 0.0;
        }
        let threads = job.settings.optimizer.max_threads.max(1) as usize;
        let chunk_size = (threads * 4).max(8);
        let mut winning: Option<CandidateResult> = None;
        let mut stopped_or_timed_out = false;
        let mut consecutive_failures = 0;
        let max_consecutive_failures = if pass_no == 1 {
            job.settings.optimizer.max_consecutive_failures_pass1
        } else {
            job.settings.optimizer.max_consecutive_failures
        };
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("thread pool");
        let start_pass = Instant::now();
        let mut i = 0;
        'chunks: while i < candidates.len() {
            if let Some(d) = self.deadline {
                if Instant::now() >= d {
                    job.log_info("Optimizer stage timed out.");
                    self.is_timed_out = true;
                    stopped_or_timed_out = true;
                    break;
                }
            }
            if job.thread.is_stop_requested() {
                stopped_or_timed_out = true;
                break;
            }
            job.notify(false);
            let end = (i + chunk_size).min(candidates.len());
            let chunk = &candidates[i..end];
            let baseline_metrics = stats::optimizer_metrics(&job.board);
            let baseline_board = &job.board;
            let settings = &job.settings;
            let thread = job.thread.clone();
            let deadline = self.deadline;
            let min_len = self.min_cumulative_trace_length;
            let increased = self.use_increased_ripup_costs;
            let results: Vec<CandidateResult> = pool.install(|| {
                chunk
                    .par_iter()
                    .map(|item_id| {
                        if thread.is_stop_requested() || deadline.map(|d| Instant::now() >= d).unwrap_or(false) {
                            return CandidateResult {
                                result: ItemRouteResult::empty(*item_id),
                                board: None,
                            };
                        }
                        let worker = baseline_board.deep_copy();
                        if worker.try_item(ItemId(*item_id)).is_none() {
                            return CandidateResult {
                                result: ItemRouteResult::empty(*item_id),
                                board: None,
                            };
                        }
                        let mut cjob = RoutingJob::new(worker, settings.clone());
                        cjob.thread = thread.clone();
                        let result = opt_route_item_on_board(&mut cjob, ItemId(*item_id), min_len, with_preferred, increased, deadline, baseline_metrics);
                        let improved = result.improved;
                        let mut board = cjob.board;
                        if improved {
                            board.clear_transient_autoroute_state();
                            CandidateResult { result, board: Some(board) }
                        } else {
                            CandidateResult { result, board: None }
                        }
                    })
                    .collect()
            });
            for res in results {
                self.total_items_optimized += 1;
                self.result_map.insert(res.result.item_id, res.result.clone());
                if res.result.improved {
                    consecutive_failures = 0;
                    let better = match &winning {
                        None => true,
                        Some(w) => res.result.improved_over(&w.result),
                    };
                    if better {
                        winning = Some(res);
                    }
                } else {
                    consecutive_failures += 1;
                    if consecutive_failures >= max_consecutive_failures {
                        job.log_info(&format!(
                            "Stopping optimization pass #{pass_no} early after {consecutive_failures} consecutive items could not be improved."
                        ));
                        break 'chunks;
                    }
                }
            }
            i = end;
        }
        let mut route_improved = 0.0;
        if !stopped_or_timed_out {
            if let Some(w) = winning {
                if w.result.improved {
                    if let Some(b) = w.board {
                        job.board = b;
                        self.min_cumulative_trace_length = stats::optimizer_metrics(&job.board).1;
                        route_improved = w.result.improvement_percentage;
                    }
                }
            }
        }
        let after = stats::board_stats(&job.board, &job.settings);
        job.log_info(&format!(
            "Optimizer pass #{} on board '{}' was completed in {:.2} seconds with the score of {:.2} ({} unrouted, {} violations).",
            pass_no,
            job.board.get_hash(),
            start_pass.elapsed().as_secs_f64(),
            after.optimizer_score,
            after.incomplete_count,
            after.clearance_violation_count
        ));
        route_improved
    }
}

fn contains_only_unfixed_traces(board: &Board, items: &BTreeSet<ItemId>) -> bool {
    items.iter().all(|i| {
        let it = board.item(*i);
        !it.is_user_fixed() && it.is_trace()
    })
}

/// `BatchOptimizer.optRouteItemOnBoard`
fn opt_route_item_on_board(
    job: &mut RoutingJob,
    item: ItemId,
    baseline_trace_length: f64,
    with_preferred: bool,
    use_increased_ripup_costs: bool,
    deadline: Option<Instant>,
    metrics_before: (i32, f64),
) -> ItemRouteResult {
    let baseline = if baseline_trace_length > 0.0 { baseline_trace_length } else { metrics_before.1 };
    let board = &mut job.board;
    let it = board.item(item);
    let was_trace = it.is_trace();
    if board.has_ignored_nets(it) {
        return ItemRouteResult::empty(item.0);
    }
    let incomplete_before = stats::incomplete_count(board);
    let mut ripped: BTreeSet<ItemId> = BTreeSet::new();
    ripped.insert(item);
    if it.is_trace() {
        let start = board.trace_start_contacts(item);
        if contains_only_unfixed_traces(board, &start) {
            ripped.extend(start);
        }
        let end = board.trace_end_contacts(item);
        if contains_only_unfixed_traces(board, &end) {
            ripped.extend(end);
        }
    }
    let mut ripped_connections: BTreeSet<ItemId> = BTreeSet::new();
    for r in &ripped {
        ripped_connections.extend(board.connection_items(*r, StopConnectionOption::None));
    }
    for r in &ripped_connections {
        let ri = board.item(*r);
        if ri.is_user_fixed() || board.has_ignored_nets(ri) {
            return ItemRouteResult::empty(item.0);
        }
    }
    let nets: Vec<i32> = board.item(item).net_numbers.to_vec();
    let rc: Vec<ItemId> = ripped_connections.into_iter().collect();
    board.remove_items(&rc);
    for n in nets {
        board.combine_traces(n);
    }
    let mut ripup_costs = job.settings.get_start_ripup_costs();
    if use_increased_ripup_costs {
        ripup_costs = ripup_costs.wrapping_mul(job.settings.optimizer.additional_ripup_cost_factor_at_start);
    }
    if was_trace {
        ripup_costs = java_round(job.settings.optimizer.trace_ripup_cost_factor as f64 * ripup_costs as f64);
    }
    let max_passes = job.settings.optimizer.max_autoroute_passes;
    let accuracy = job.settings.trace_pull_tight_accuracy;
    BatchAutorouter::autoroute_passes_for_optimizing_item(job, max_passes, ripup_costs, accuracy, with_preferred);
    let after = stats::optimizer_metrics(&job.board);
    let incomplete_after = stats::incomplete_count(&job.board);
    let mut result = ItemRouteResult::new(item.0, metrics_before.0, after.0, baseline, after.1, incomplete_before, incomplete_after);
    let route_improved = !job.thread.is_stop_requested() && deadline.map(|d| Instant::now() < d).unwrap_or(true) && result.improved;
    result.improved = route_improved;
    result
}

/// `BatchOptimizer.areAllViasMandatoryLayerTransitions`
pub fn are_all_vias_mandatory_layer_transitions(board: &Board) -> bool {
    let vias = board.get_vias();
    if vias.is_empty() {
        return false;
    }
    let lc = board.layer_count();
    for via in vias {
        let v = board.item(via);
        if v.is_user_fixed() {
            continue;
        }
        if v.net_count() == 0 {
            return false;
        }
        let connected = board.connected_set(via, v.get_net_number(0), true);
        let mut smd_pins = Vec::new();
        let mut only = true;
        for c in &connected {
            let ci = board.item(*c);
            if ci.is_pin() {
                if ci.first_layer() == ci.last_layer_with(lc) {
                    smd_pins.push(*c);
                } else {
                    only = false;
                    break;
                }
            } else if ci.is_via() {
                if *c != via {
                    only = false;
                    break;
                }
            } else if !ci.is_trace() {
                only = false;
                break;
            }
        }
        if !only || smd_pins.len() < 2 {
            return false;
        }
        let first = board.item(smd_pins[0]).first_layer();
        if !smd_pins.iter().any(|p| board.item(*p).first_layer() != first) {
            return false;
        }
    }
    true
}

/// `ReadSortedRouteItems`: vias sorted by location, then traces not connected to vias.
struct ReadSortedRouteItems {
    min_item_coor: FloatPoint,
    min_item_layer: i32,
}

impl ReadSortedRouteItems {
    fn new() -> Self {
        ReadSortedRouteItems {
            min_item_coor: FloatPoint::new(i32::MIN as f64, i32::MIN as f64),
            min_item_layer: -1,
        }
    }

    fn after_min(&self, c: &FloatPoint, layer: i32) -> bool {
        c.x > self.min_item_coor.x
            || c.x == self.min_item_coor.x && (c.y > self.min_item_coor.y || c.y == self.min_item_coor.y && layer > self.min_item_layer)
    }

    fn next(&mut self, board: &Board) -> Option<ItemId> {
        let mut result = None;
        let mut cur_min = FloatPoint::new(i32::MAX as f64, i32::MAX as f64);
        let mut cur_min_layer = i32::MAX;
        let before = |c: &FloatPoint, l: i32, m: &FloatPoint, ml: i32| c.x < m.x || c.x == m.x && (c.y < m.y || c.y == m.y && l < ml);
        for item in board.items.iter() {
            if !item.is_via() || item.is_user_fixed() || board.has_ignored_nets(item) {
                continue;
            }
            let c = item.as_drill().unwrap().center.to_float();
            let l = item.first_layer();
            if self.after_min(&c, l) && before(&c, l, &cur_min, cur_min_layer) {
                cur_min = c;
                cur_min_layer = l;
                result = Some(item.id);
            }
        }
        for item in board.items.iter() {
            let Some(t) = item.as_trace() else { continue };
            if board.is_shove_fixed(item) || board.has_ignored_nets(item) {
                continue;
            }
            let f = t.first_corner().to_float();
            let l2 = t.last_corner().to_float();
            let c = if f.x < l2.x || f.x == l2.x && f.y < l2.y { l2 } else { f };
            let layer = t.layer;
            if self.after_min(&c, layer) && before(&c, layer, &cur_min, cur_min_layer) {
                let connected_to_via = board.normal_contacts(item.id).iter().any(|ct| {
                    let ci = board.item(*ct);
                    ci.is_via() && !ci.is_user_fixed()
                });
                if !connected_to_via {
                    cur_min = c;
                    cur_min_layer = layer;
                    result = Some(item.id);
                }
            }
        }
        self.min_item_coor = cur_min;
        self.min_item_layer = cur_min_layer;
        result
    }
}
