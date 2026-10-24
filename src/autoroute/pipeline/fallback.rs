//! Routing fallbacks and exploration (port of `RoutingFallback`, `RoutingFallbackRunner`,
//! `RoutingSupervisor`, `RetryNeighbourhood` and `LastMileBlockerRipup`).

use super::batch::{design_blocks, BatchAutorouter, StopReason};
use super::{stats, RoutingJob};
use crate::board::structure::Unit;
use crate::board::{Board, ItemId, TreeObject};
use crate::geometry::basics::java_round;
use crate::geometry::{FloatPoint, IntBox, TileShape};
use crate::settings::RouterSettings;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

/// `RoutingFallback.Strategy`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strategy {
    pub name: &'static str,
    pub needs_fanout: bool,
    pub continue_best: bool,
    pub fanout_only: bool,
}

pub const KNOWN: [Strategy; 9] = [
    Strategy { name: "no-neck", needs_fanout: false, continue_best: false, fanout_only: false },
    Strategy { name: "continue-fine-neck", needs_fanout: false, continue_best: true, fanout_only: false },
    Strategy { name: "fanout-retry", needs_fanout: true, continue_best: false, fanout_only: true },
    Strategy { name: "short-escape", needs_fanout: true, continue_best: false, fanout_only: true },
    Strategy { name: "open-escape", needs_fanout: true, continue_best: false, fanout_only: false },
    Strategy { name: "direct-first", needs_fanout: true, continue_best: false, fanout_only: false },
    Strategy { name: "no-fanout", needs_fanout: true, continue_best: false, fanout_only: false },
    Strategy { name: "continue", needs_fanout: false, continue_best: false, fanout_only: false },
    Strategy { name: "continue-no-neck", needs_fanout: false, continue_best: false, fanout_only: false },
];

impl Strategy {
    pub fn apply(&self, s: &mut RouterSettings) {
        match self.name {
            "no-neck" => s.neck_width_um = 0.0,
            "continue-fine-neck" => s.neck_width_um = s.get_neck_width_um().min(100.0),
            "fanout-retry" => s.fanout.retry_without_escape_window = true,
            "short-escape" => s.fanout.min_escape_length_mm = 0.0,
            "open-escape" => {
                s.fanout.min_escape_length_mm = 0.0;
                s.fanout.max_escape_length_mm = 1000.0;
                s.autorouter.smd_net_via_cost_factor = 1.0;
            }
            "direct-first" => s.fanout.strategy = "direct_first".to_string(),
            "no-fanout" => s.fanout.enabled = false,
            "continue-no-neck" => s.neck_width_um = 0.0,
            _ => {}
        }
    }
}

pub fn find(name: &str) -> Option<Strategy> {
    KNOWN.iter().copied().find(|s| s.name.eq_ignore_ascii_case(name))
}

pub fn known_strategy_names() -> String {
    KNOWN.iter().map(|s| s.name).collect::<Vec<_>>().join(", ")
}

/// `RoutingFallback.parse`
pub fn parse(settings: &RouterSettings, mut unknown: impl FnMut(&str)) -> Vec<Strategy> {
    let mut result = Vec::new();
    let spec = &settings.autorouter.fallback_strategies;
    if spec.trim().is_empty() {
        return result;
    }
    for raw in spec.split(',') {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        if name.eq_ignore_ascii_case("none") {
            return Vec::new();
        }
        match find(name) {
            None => unknown(name),
            Some(k) => {
                if !result.contains(&k) {
                    result.push(k);
                }
            }
        }
    }
    result
}

pub fn applicable(strategies: &[Strategy], settings: &RouterSettings, fanout_runs: bool) -> Vec<Strategy> {
    let necking = settings.get_neck_width_um() > 0.0;
    strategies
        .iter()
        .copied()
        .filter(|s| {
            !(s.needs_fanout && !fanout_runs
                || s.name == "no-neck" && !necking
                || s.name == "continue-fine-neck" && settings.get_neck_width_um() <= 100.0
                || s.name == "continue-no-neck" && !necking)
        })
        .collect()
}

pub fn is_retryable(reason: Option<StopReason>) -> bool {
    matches!(reason, Some(StopReason::Stagnation) | Some(StopReason::NoImprovement) | Some(StopReason::Completed))
}

pub fn describe(reason: Option<StopReason>) -> &'static str {
    match reason {
        None => "an unknown stop",
        Some(StopReason::Completed) => "completed",
        Some(StopReason::MaxPasses) => "pass limit",
        Some(StopReason::Stagnation) => "stagnation",
        Some(StopReason::NoImprovement) => "no improvement",
        Some(StopReason::TimedOut) => "timeout",
        Some(StopReason::Cancelled) => "cancelled",
        Some(StopReason::Repeated) => "a repeat of an earlier pass loop",
    }
}

/// `RoutingFallback.Outcome`
#[derive(Clone, Debug)]
pub struct Outcome {
    pub strategy: String,
    pub unrouted: i32,
    pub new_or_changed: i32,
    pub violations: i32,
    pub score: f32,
    pub design_blocked: i32,
}

impl Outcome {
    pub fn is_complete_and_clean(&self) -> bool {
        self.unrouted - self.design_blocked == 0 && self.new_or_changed == 0
    }

    pub fn of(strategy: &str, board: &Board, scoring: &RouterSettings, incumbent: Option<&Arc<stats::Baseline>>) -> Outcome {
        let s = stats::board_stats(board, scoring);
        let incomplete = s.incomplete_count;
        let mut blocked = 0;
        if incomplete > 0 {
            blocked = design_blocks::blocked_airlines(board, &stats::all_airlines(board)) as i32;
        }
        let violation_count = s.clearance_violation_count;
        let baseline = incumbent.or(board.clearance_baseline.as_ref());
        let changed = new_or_changed_count(board, violation_count, baseline);
        Outcome {
            strategy: strategy.to_string(),
            unrouted: incomplete,
            new_or_changed: changed,
            violations: violation_count,
            score: s.router_score,
            design_blocked: blocked,
        }
    }

    pub fn can_replace(candidate: &Outcome, incumbent: &Outcome, changed_since_incumbent: i32) -> bool {
        changed_since_incumbent == 0 && candidate.new_or_changed <= incumbent.new_or_changed && Self::compare(candidate, incumbent).is_lt()
    }

    pub fn compare(a: &Outcome, b: &Outcome) -> std::cmp::Ordering {
        a.unrouted
            .cmp(&b.unrouted)
            .then(a.new_or_changed.cmp(&b.new_or_changed))
            .then(a.violations.cmp(&b.violations))
            .then(super::batch::java_float_compare(b.score, a.score))
    }
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let blocked = if self.design_blocked > 0 {
            format!(" ({} blocked by the design)", self.design_blocked)
        } else {
            String::new()
        };
        write!(
            f,
            "{} unrouted{}, {} new violation{} ({} total), score {:.2}",
            self.unrouted,
            blocked,
            self.new_or_changed,
            if self.new_or_changed == 1 { "" } else { "s" },
            self.violations,
            self.score
        )
    }
}

fn new_or_changed_count(board: &Board, violation_count: i32, baseline: Option<&Arc<stats::Baseline>>) -> i32 {
    if violation_count == 0 {
        return 0;
    }
    match baseline {
        None => violation_count,
        Some(b) => b.count_new_or_changed(board, &stats::all_clearance_violations(board)) as i32,
    }
}

pub fn capture_clearance(board: &Board, violations: i32) -> Option<Arc<stats::Baseline>> {
    if violations == 0 {
        return None;
    }
    Some(Arc::new(stats::Baseline::new(board, &stats::all_clearance_violations(board))))
}

// ==================================================================== RoutingSupervisor

pub fn explores(settings: &RouterSettings) -> bool {
    settings.autorouter.explore_patience > 0
}

pub fn residual(settings: &RouterSettings, open: i32) -> bool {
    open > 0 && open <= settings.autorouter.residual_repair_max_open
}

struct SupervisorAttempt {
    kind: String,
    name: String,
    seed: i32,
    try_number: i32,
}

#[derive(Default)]
struct RoutingSupervisor {
    stats: Vec<(String, i32, i32)>,
    attempts_without_gain: i32,
}

impl RoutingSupervisor {
    fn get(&self, kind: &str) -> Option<(i32, i32)> {
        self.stats.iter().find(|s| s.0 == kind).map(|s| (s.1, s.2))
    }

    fn exploration_exhausted(&self, settings: &RouterSettings) -> bool {
        self.attempts_without_gain >= settings.autorouter.explore_patience
    }

    fn via_factor(attempt: i32) -> i32 {
        1 << attempt.clamp(1, 3)
    }

    fn exploration_fallback(job_settings: &RouterSettings) -> String {
        let keep = &job_settings.autorouter.explore_fallback_strategies;
        if keep.trim().is_empty() {
            return job_settings.autorouter.fallback_strategies.clone();
        }
        let names: Vec<&str> = keep.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        let mut kept = Vec::new();
        for s in parse(job_settings, |_| {}) {
            if names.iter().any(|n| s.name.eq_ignore_ascii_case(n)) {
                kept.push(s.name);
            }
        }
        if kept.is_empty() {
            "none".to_string()
        } else {
            kept.join(",")
        }
    }

    fn neck_applies(job_settings: &RouterSettings, board: &Board) -> bool {
        if job_settings.get_neck_width_um() <= 100.0 {
            return false;
        }
        let half_um = board.rules.get_min_allowed_trace_half_width() as f64;
        half_um <= 50.0 * board.communication.get_resolution(Unit::Um)
    }

    fn next_attempt(&self, round: i32, job_settings: &RouterSettings, board: &Board) -> SupervisorAttempt {
        let mut applicable = vec!["seed", "retry-plan", "neighbourhood"];
        if Self::neck_applies(job_settings, board) {
            applicable.push("neck-100um");
        }
        applicable.push("vias-cheaper");
        applicable.push("vias-dearer");
        let mut choice: Option<&str> = applicable.iter().copied().find(|k| self.get(k).is_none());
        if choice.is_none() {
            let total: i32 = self.stats.iter().map(|s| s.1).sum();
            let total_tries = 2.max(total) as f64;
            let mut best = f64::NEG_INFINITY;
            for k in &applicable {
                let (tries, closed) = self.get(k).unwrap();
                let score = (closed as f64 + 0.5) / tries as f64 + (2.0 * total_tries.ln() / tries as f64).sqrt();
                if score > best {
                    best = score;
                    choice = Some(k);
                }
            }
        }
        let choice = choice.unwrap_or("seed").to_string();
        let attempt = self.get(&choice).map(|s| s.0).unwrap_or(0) + 1;
        let name = format!("{} (seed {}{})", choice, round, if attempt > 1 { format!(", try {attempt}") } else { String::new() });
        SupervisorAttempt {
            kind: choice,
            name,
            seed: round,
            try_number: attempt,
        }
    }

    fn apply(attempt: &SupervisorAttempt, job_settings: &RouterSettings, settings: &mut RouterSettings) {
        settings.autorouter.item_order_seed = Some(attempt.seed as i64);
        settings.autorouter.explore_patience = 0;
        settings.autorouter.fallback_strategies = Self::exploration_fallback(job_settings);
        match attempt.kind.as_str() {
            "seed" => settings.fanout.enabled = attempt.seed % 2 == 1 && job_settings.is_fanout_enabled(),
            "neck-100um" => settings.neck_width_um = 100.0,
            "vias-cheaper" => settings.set_via_costs(1.max(job_settings.get_via_costs() / Self::via_factor(attempt.try_number))),
            "vias-dearer" => settings.set_via_costs(job_settings.get_via_costs() * Self::via_factor(attempt.try_number)),
            _ => {}
        }
    }

    fn attempt_done(&mut self, attempt: &SupervisorAttempt, open_before: i32, open_after: i32, kept: bool) {
        let (tries, closed) = self.get(&attempt.kind).unwrap_or((0, 0));
        let gain = if kept { 0.max(open_before - open_after) } else { 0 };
        match self.stats.iter_mut().find(|s| s.0 == attempt.kind) {
            Some(s) => {
                s.1 = tries + 1;
                s.2 = closed + gain;
            }
            None => self.stats.push((attempt.kind.clone(), tries + 1, closed + gain)),
        }
        self.attempts_without_gain = if gain > 0 { 0 } else { self.attempts_without_gain + 1 };
    }
}

// ==================================================================== RetryNeighbourhood

const RADIUS_MM: f64 = 1.5;
const MAX_RADIUS_MM: f64 = 12.0;

fn reach(attempt: i32) -> (f64, bool) {
    if attempt <= 1 {
        (RADIUS_MM, false)
    } else {
        ((RADIUS_MM * attempt as f64).min(MAX_RADIUS_MM), true)
    }
}

/// `RetryNeighbourhood.ripNearOpenPins`
pub fn rip_near_open_pins(board: &mut Board, attempt: i32) -> i32 {
    let (radius_mm, any_net) = reach(attempt);
    if !any_net {
        return 0;
    }
    let radius = (radius_mm * 1000.0 * board.communication.get_resolution(Unit::Um)).ceil() as i32;
    let mut found: Vec<ItemId> = Vec::new();
    let mut seen: HashSet<ItemId> = HashSet::new();
    for pin in board.get_pins() {
        let it = board.item(pin);
        if it.net_count() != 1 || board.has_ignored_nets(it) {
            continue;
        }
        let net = it.get_net_number(0);
        if board.unconnected_set(pin, net).is_empty() {
            continue;
        }
        let b = board.item(pin).bounding_box().offset(radius as f64);
        for layer in 0..board.layer_count() {
            for o in board.overlapping_objects(&TileShape::Box(b), layer) {
                let TreeObject::Item(id) = o else { continue };
                let oi = board.item(id);
                if !(oi.is_trace() || oi.is_via()) || oi.net_count() != 1 || oi.contains_net(net) {
                    continue;
                }
                if oi.is_user_fixed() || board.is_deletion_forbidden(oi) {
                    continue;
                }
                if seen.insert(id) {
                    found.push(id);
                }
            }
        }
    }
    found.sort_by_key(|i| i.0);
    let mut removed = 0;
    for id in found {
        board.remove_item(id);
        removed += 1;
    }
    removed
}

// ==================================================================== LastMileBlockerRipup

const LAST_MILE_MAX_ITEMS: usize = 24;

fn corridor(from: &FloatPoint, to: &FloatPoint) -> IntBox {
    let (x1, y1, x2, y2) = (java_round(from.x), java_round(from.y), java_round(to.x), java_round(to.y));
    let (min_x, max_x, min_y, max_y) = (x1.min(x2), x1.max(x2), y1.min(y2), y1.max(y2));
    let span = (max_x - min_x).max(max_y - min_y);
    let margin = (span / 4).max(1);
    IntBox::new(min_x - margin, min_y - margin, max_x + margin, max_y + margin)
}

/// `LastMileBlockerRipup.ripBlockers`
pub fn rip_last_mile_blockers(board: &mut Board) -> i32 {
    let airlines = stats::all_airlines(board);
    if airlines.is_empty() {
        return 0;
    }
    let mut blockers: Vec<ItemId> = Vec::new();
    for a in &airlines {
        let c = corridor(&a.from_corner, &a.to_corner);
        for o in board.overlapping_objects(&TileShape::Box(c), -1) {
            let TreeObject::Item(id) = o else { continue };
            if blockers.len() >= LAST_MILE_MAX_ITEMS {
                break;
            }
            let it = board.item(id);
            let removable = (it.is_trace() || it.is_via())
                && !board.is_deletion_forbidden(it)
                && !it.is_user_fixed()
                && !it.net_numbers.contains(&a.net_no)
                && it.net_count() > 0;
            if removable && !blockers.contains(&id) {
                blockers.push(id);
            }
        }
        if blockers.len() >= LAST_MILE_MAX_ITEMS {
            break;
        }
    }
    if blockers.is_empty() {
        return 0;
    }
    board.remove_items(&blockers);
    blockers.len() as i32
}

// ==================================================================== RoutingFallbackRunner

struct Attempt {
    outcome: Outcome,
    board: Board,
    router: BatchAutorouter,
    baseline: Option<Arc<stats::Baseline>>,
    id: u64,
}

pub struct RoutingFallbackRunner {
    pass_loop_starts: BTreeMap<u64, Outcome>,
    next_id: u64,
}

impl Default for RoutingFallbackRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl RoutingFallbackRunner {
    pub fn new() -> Self {
        RoutingFallbackRunner {
            pass_loop_starts: BTreeMap::new(),
            next_id: 1,
        }
    }

    fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn stopped(job: &RoutingJob) -> bool {
        job.thread.is_stop_requested() || job.is_timed_out()
    }

    /// `RoutingFallbackRunner.run`; `input` is the board before routing.
    pub fn run(&mut self, job: &mut RoutingJob, default_router: BatchAutorouter, strategies: &[Strategy], input: Board) -> BatchAutorouter {
        let input_baseline = job.board.clearance_baseline.clone();
        let best = Outcome::of("default", &job.board, &job.settings, None);
        if best.is_complete_and_clean() {
            return default_router;
        }
        let reason = default_router.last_stop_reason;
        if !is_retryable(reason) {
            job.log_info(&format!("Routing fallback skipped: the pass loop stopped at its {} with {}.", describe(reason), best));
            return default_router;
        }
        let fanout_runs = job.settings.is_fanout_enabled() && !job.board.get_smd_pins().is_empty();
        let appl = applicable(strategies, &job.settings, fanout_runs);
        let continuations: Vec<Strategy> = appl.iter().copied().filter(|s| s.continue_best).collect();
        let alternatives: Vec<Strategy> = appl.iter().copied().filter(|s| !s.continue_best).collect();
        let is_residual = residual(&job.settings, best.unrouted);
        if alternatives.is_empty() && continuations.is_empty() && !is_residual {
            return default_router;
        }
        job.log_info(&format!(
            "Routing fallback: the default configuration stopped at {} with {}; trying {} alternative {}{}.",
            describe(reason),
            best,
            alternatives.len(),
            if alternatives.len() == 1 { "strategy" } else { "strategies" },
            if is_residual { ", then continuing the best board" } else { "" }
        ));
        if let Some(h) = default_router.pass_loop_start_hash {
            self.pass_loop_starts.insert(h, best.clone());
        }
        let default_clearance = capture_clearance(&job.board, best.violations);
        let id = self.new_id();
        let mut best_attempt = Attempt {
            outcome: best,
            board: job.board.clone(),
            router: default_router,
            baseline: default_clearance,
            id,
        };
        let mut continued_from: Option<u64> = None;
        if is_residual && !alternatives.is_empty() {
            best_attempt = self.run_continuations(job, best_attempt, &automatic_continuations(&job.settings));
            continued_from = Some(best_attempt.id);
        }
        for strategy in &alternatives {
            if Self::stopped(job) || best_attempt.outcome.is_complete_and_clean() {
                break;
            }
            let Some(attempt) = self.run_attempt(job, &input, strategy, false, input_baseline.clone(), &best_attempt) else {
                continue;
            };
            let stop_reason = attempt.router.last_stop_reason;
            if stop_reason == Some(StopReason::Repeated) {
                let repeated = attempt.router.pass_loop_start_hash.and_then(|h| self.pass_loop_starts.get(&h).cloned());
                job.log_info(&format!(
                    "Routing fallback '{}': its fanout left the board an earlier pass loop started on; stopped after the fanout ({}).",
                    strategy.name,
                    repeated.unwrap_or_else(|| attempt.outcome.clone())
                ));
                if stop_alternatives(&best_attempt.outcome, stop_reason) {
                    break;
                }
                continue;
            }
            if strategy.fanout_only && stop_reason != Some(StopReason::TimedOut) {
                if let Some(h) = attempt.router.pass_loop_start_hash {
                    self.pass_loop_starts.entry(h).or_insert_with(|| attempt.outcome.clone());
                }
            }
            job.log_info(&format!("Routing fallback '{}': {} after {}.", strategy.name, attempt.outcome, describe(stop_reason)));
            if replaces(&attempt, &best_attempt) {
                best_attempt = attempt;
            } else {
                job.board = best_attempt.board.clone();
            }
            if stop_alternatives(&best_attempt.outcome, stop_reason) {
                break;
            }
        }
        let mut after = continuations.clone();
        if continued_from.is_some() && continued_from == Some(best_attempt.id) {
            if !best_attempt.outcome.is_complete_and_clean() {
                job.log_info("Routing fallback: no alternative replaced the board the continuations ran on; not continuing it again.");
            }
        } else {
            after.extend(automatic_continuations(&job.settings));
        }
        if !after.is_empty() {
            best_attempt = self.run_continuations(job, best_attempt, &after);
        }
        job.log_info(&format!("Routing fallback chose '{}': {}.", best_attempt.outcome.strategy, best_attempt.outcome));
        job.board = best_attempt.board;
        best_attempt.router
    }

    /// `RoutingFallbackRunner.explore`
    pub fn explore(&mut self, job: &mut RoutingJob, incumbent: BatchAutorouter) -> BatchAutorouter {
        if !explores(&job.settings) || Self::stopped(job) {
            return incumbent;
        }
        let mut supervisor = RoutingSupervisor::default();
        let best = Outcome::of("incumbent", &job.board, &job.settings, None);
        let id = self.new_id();
        let mut best_attempt = Attempt {
            outcome: best,
            board: job.board.clone(),
            router: incumbent,
            baseline: None,
            id,
        };
        let job_settings = job.settings.clone();
        let mut round = 0;
        while !best_attempt.outcome.is_complete_and_clean() && !supervisor.exploration_exhausted(&job_settings) && !Self::stopped(job) {
            round += 1;
            let planned = supervisor.next_attempt(round, &job_settings, &best_attempt.board);
            let mut settings = job_settings.clone();
            RoutingSupervisor::apply(&planned, &job_settings, &mut settings);
            let mut copy = Board::deserialize(&best_attempt.board.serialize());
            if copy.clearance_baseline.is_none() {
                copy.clearance_baseline = best_attempt.board.clearance_baseline.clone();
            }
            if planned.kind == "neighbourhood" {
                let ripped = rip_near_open_pins(&mut copy, planned.try_number);
                job.log_info(&format!(
                    "Exploration '{}' ripped {} item(s) within {} mm.",
                    planned.name,
                    ripped,
                    reach(planned.try_number).0
                ));
            }
            job.log_info(&format!("Exploration attempt '{}' from {}.", planned.name, best_attempt.outcome));
            let open_before = best_attempt.outcome.unrouted;
            let attempt = self.run_prepared(job, copy, settings, false, &format!("explore-{}", planned.kind));
            let mut kept = false;
            if let Some(a) = attempt {
                if replaces(&a, &best_attempt) {
                    best_attempt = a;
                    kept = true;
                }
            }
            let open_after = best_attempt.outcome.unrouted;
            supervisor.attempt_done(&planned, open_before, open_after, kept);
        }
        job.board = best_attempt.board;
        job.settings = job_settings;
        best_attempt.router
    }

    fn run_continuations(&mut self, job: &mut RoutingJob, mut best_attempt: Attempt, continuations: &[Strategy]) -> Attempt {
        for strategy in continuations {
            if best_attempt.outcome.is_complete_and_clean() || Self::stopped(job) {
                break;
            }
            if !strategy.continue_best && !residual(&job.settings, best_attempt.outcome.unrouted) {
                break;
            }
            let copy = best_attempt.board.deep_copy();
            let mut settings = job.settings.clone();
            strategy.apply(&mut settings);
            settings.fanout.enabled = false;
            let Some(attempt) = self.run_prepared(job, copy, settings, true, strategy.name) else {
                break;
            };
            job.log_info(&format!(
                "Routing fallback continuation '{}': {} after {}.",
                strategy.name,
                attempt.outcome,
                describe(attempt.router.last_stop_reason)
            ));
            if replaces(&attempt, &best_attempt) {
                best_attempt = attempt;
            } else if !strategy.continue_best {
                job.board = best_attempt.board.clone();
                job.log_info(&format!(
                    "Routing fallback: stopping residual continuations after '{}' did not improve the incumbent.",
                    strategy.name
                ));
                break;
            } else {
                job.board = best_attempt.board.clone();
            }
        }
        best_attempt
    }

    fn run_attempt(
        &mut self,
        job: &mut RoutingJob,
        input: &Board,
        strategy: &Strategy,
        keep_fanout_vias: bool,
        input_baseline: Option<Arc<stats::Baseline>>,
        _best: &Attempt,
    ) -> Option<Attempt> {
        let mut board = Board::deserialize(input);
        if board.clearance_baseline.is_none() {
            board.clearance_baseline = input_baseline;
        }
        let mut settings = job.settings.clone();
        strategy.apply(&mut settings);
        let attempt = self.run_prepared(job, board, settings, keep_fanout_vias, strategy.name)?;
        if strategy.fanout_only && attempt.router.last_stop_reason == Some(StopReason::Repeated) {
            if let Some(rep) = attempt.router.pass_loop_start_hash.and_then(|h| self.pass_loop_starts.get(&h).cloned()) {
                let id = self.new_id();
                return Some(Attempt {
                    outcome: Outcome {
                        strategy: strategy.name.to_string(),
                        ..rep
                    },
                    board: job.board.clone(),
                    router: attempt.router,
                    baseline: attempt.baseline,
                    id,
                });
            }
        }
        Some(attempt)
    }

    fn run_prepared(&mut self, job: &mut RoutingJob, board: Board, settings: RouterSettings, keep_fanout_vias: bool, strategy_name: &str) -> Option<Attempt> {
        let saved_settings = std::mem::replace(&mut job.settings, settings);
        job.thread.reset_autorouter_stop_request();
        job.board = board;
        let remove_unconnected_vias = !keep_fanout_vias && !job.settings.is_fanout_enabled();
        let mut router = BatchAutorouter::create_attempt(&job.board, &job.settings, remove_unconnected_vias);
        if find(strategy_name).map(|s| s.fanout_only).unwrap_or(false) {
            router.repeats_pass_loop = Some(self.pass_loop_starts.keys().copied().collect());
        }
        router.run_batch_loop(job);
        job.settings = saved_settings;
        let outcome = Outcome::of(strategy_name, &job.board, &job.settings, None);
        let baseline = capture_clearance(&job.board, outcome.violations);
        let id = self.new_id();
        Some(Attempt {
            outcome,
            board: job.board.clone(),
            router,
            baseline,
            id,
        })
    }
}

fn replaces(candidate: &Attempt, incumbent: &Attempt) -> bool {
    let changed = if candidate.outcome.violations == 0 {
        0
    } else {
        match &incumbent.baseline {
            None => candidate.outcome.violations,
            Some(b) => b.count_new_or_changed(&candidate.board, &stats::all_clearance_violations(&candidate.board)) as i32,
        }
    };
    Outcome::can_replace(&candidate.outcome, &incumbent.outcome, changed)
}

fn stop_alternatives(best: &Outcome, reason: Option<StopReason>) -> bool {
    best.is_complete_and_clean() || reason == Some(StopReason::TimedOut) || reason == Some(StopReason::Cancelled)
}

fn automatic_continuations(settings: &RouterSettings) -> Vec<Strategy> {
    let mut r = vec![find("continue").unwrap()];
    if settings.get_neck_width_um() > 0.0 {
        r.push(find("continue-no-neck").unwrap());
    }
    r
}
