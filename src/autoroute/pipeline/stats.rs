//! Adapter between the pipeline and the design rule checker / board statistics
//! (`drc` and `scoring` modules). The pipeline only uses the functions of this module.

use crate::board::{Board, ItemId};
use crate::scoring::{BoardStatistics, OptimizerMetrics, StatisticsOptions};
use crate::settings::RouterSettings;
use std::collections::BTreeSet;

pub use crate::drc::AirLine;
pub use crate::drc::ClearanceBaseline as Baseline;
pub use crate::drc::ClearanceViolation as Violation;

/// The values of `BoardStatistics` used by the pipeline.
#[derive(Clone, Debug, Default)]
pub struct BoardStats {
    pub incomplete_count: i32,
    pub clearance_violation_count: i32,
    pub via_count: i32,
    pub trace_total_length_mm: Option<f64>,
    pub min_trace_length_mm: Option<f64>,
    pub min_via_count: Option<i32>,
    pub total_weighted_length: f64,
    pub fanout_total_smd_pins: i32,
    pub fanout_escaped_count: i32,
    pub router_score: f32,
    pub optimizer_score: f32,
}

/// `board.routingLedger().incompleteCount()`
pub fn incomplete_count(board: &Board) -> i32 {
    crate::drc::incomplete_count(board) as i32
}

/// `board.routingLedger().incompleteNetNumbers()`
pub fn incomplete_net_numbers(board: &Board) -> BTreeSet<i32> {
    crate::drc::incomplete_net_numbers(board)
}

/// `new BoardStatistics(board)` with the router and optimizer scores for `settings`.
pub fn board_stats(board: &Board, settings: &RouterSettings) -> BoardStats {
    let s = BoardStatistics::with_options(
        board,
        StatisticsOptions {
            include_clearance_violations: true,
            include_connections: true,
            clearance_baseline: board.clearance_baseline.as_deref(),
            ..Default::default()
        },
    );
    BoardStats {
        incomplete_count: s.connections.incomplete_count.unwrap_or(0),
        clearance_violation_count: s.clearance_violations.total_count.unwrap_or(0),
        via_count: s.vias.total_count,
        trace_total_length_mm: s.traces.total_length_mm.map(|v| v as f64),
        min_trace_length_mm: s.bounds.min_trace_length_mm.map(|v| v as f64),
        min_via_count: s.bounds.min_via_count,
        total_weighted_length: s.traces.total_weighted_length as f64,
        fanout_total_smd_pins: s.fanout.total_smd_pins,
        fanout_escaped_count: s.fanout.escaped_count,
        router_score: s.get_router_score(settings),
        optimizer_score: s.get_optimizer_score(settings),
    }
}

/// `BoardStatistics.OptimizerMetrics.of(board)`: (via count, total weighted trace length).
pub fn optimizer_metrics(board: &Board) -> (i32, f64) {
    let m = OptimizerMetrics::of(board);
    (m.via_count, m.total_weighted_length as f64)
}

/// `!item.clearanceViolations().isEmpty()`
pub fn has_clearance_violations(board: &Board, id: ItemId) -> bool {
    !crate::drc::clearance_violations(board, id).is_empty()
}

/// `item.clearanceViolations()`
pub fn item_clearance_violations(board: &Board, id: ItemId) -> Vec<Violation> {
    crate::drc::clearance_violations(board, id)
}

/// `new DesignRulesChecker(board, null).getAllAirlines()`
pub fn all_airlines(board: &Board) -> Vec<AirLine> {
    crate::drc::all_airlines(board)
}

/// `new DesignRulesChecker(board, null).getAllClearanceViolations()`
pub fn all_clearance_violations(board: &Board) -> Vec<Violation> {
    crate::drc::all_clearance_violations(board)
}

/// `AutorouteUnroutedReport.build(board)` (simplified: one line per airline).
pub fn unrouted_report(board: &Board) -> String {
    let mut out = String::new();
    for a in all_airlines(board) {
        let name = board.rules.nets.get(a.net_no).map(|n| n.name.clone()).unwrap_or_else(|| format!("net#{}", a.net_no));
        out.push_str(&format!(
            "  - net '{}': from ({:.0}, {:.0}) to ({:.0}, {:.0})\n",
            name, a.from_corner.x, a.from_corner.y, a.to_corner.x, a.to_corner.y
        ));
    }
    out
}
