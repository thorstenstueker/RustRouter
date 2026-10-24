//! Port of `core.scoring.BoardScoreBreakdown` and `core.scoring.ScoringWeightComparison`
//! (legacy weighted score decomposition).

use super::{java_max_f32, BoardStatistics};
use crate::settings::RoutingCostSettings;

#[derive(Clone, Debug)]
pub struct BoardScoreBreakdown {
    pub maximum_score: f32,
    pub unrouted_connections_penalty: f32,
    pub clearance_violations_penalty: f32,
    pub bends_penalty: f32,
    pub trace_length_cost: f32,
    pub vias_cost: f32,
    pub total_penalties: f32,
    pub total_costs: f32,
    pub raw_score: f32,
    pub normalized_score: f32,
    pub weights: RoutingCostSettings,
    pub max_connections: i32,
    pub incomplete_connections: i32,
    pub clearance_violations: i32,
    pub bend_count: i32,
    pub total_trace_length_mm: f32,
    pub via_count: i32,
}

impl BoardScoreBreakdown {
    fn new(
        weights: &RoutingCostSettings,
        max_connections: i32,
        incomplete_connections: i32,
        clearance_violations: i32,
        bend_count: i32,
        total_trace_length_mm: f32,
        via_count: i32,
    ) -> BoardScoreBreakdown {
        let maximum_score = max_connections as f32 * weights.unrouted_net_penalty;
        let unrouted_connections_penalty = incomplete_connections as f32 * weights.unrouted_net_penalty;
        let clearance_violations_penalty = clearance_violations as f32 * weights.clearance_violation_penalty;
        let bends_penalty = bend_count as f32 * weights.bend_penalty;
        let trace_length_cost = (total_trace_length_mm as f64 * weights.default_preferred_direction_trace_cost) as f32;
        // int * Integer = int, then widened to float
        let vias_cost = via_count.wrapping_mul(weights.via_costs) as f32;
        let total_penalties = unrouted_connections_penalty + clearance_violations_penalty + bends_penalty;
        let total_costs = trace_length_cost + vias_cost;
        let raw_score = maximum_score - total_penalties - total_costs;
        let normalized_score = if maximum_score > 0.0 {
            java_max_f32(0.0, raw_score / maximum_score) * 1000.0
        } else {
            0.0
        };
        BoardScoreBreakdown {
            maximum_score,
            unrouted_connections_penalty,
            clearance_violations_penalty,
            bends_penalty,
            trace_length_cost,
            vias_cost,
            total_penalties,
            total_costs,
            raw_score,
            normalized_score,
            weights: weights.clone(),
            max_connections,
            incomplete_connections,
            clearance_violations,
            bend_count,
            total_trace_length_mm,
            via_count,
        }
    }

    /// `BoardScoreBreakdown.of(stats, weights)`. Note: like the original this uses
    /// `traces.totalLength` (which is in mm for statistics created with the default unit).
    pub fn of(stats: &BoardStatistics, weights: &RoutingCostSettings) -> BoardScoreBreakdown {
        Self::new(
            weights,
            stats.connections.maximum_count.unwrap_or(0),
            stats.connections.incomplete_count.unwrap_or(0),
            stats.clearance_violations.total_count.unwrap_or(0),
            stats.bends.total_count,
            stats.traces.total_length,
            stats.vias.total_count,
        )
    }

    pub fn to_summary_string(&self) -> String {
        format!(
            "score={:.1}/1000 (raw={:.0}/{:.0}) | unrouted={}×{:.0}={:.0} | violations={}×{:.0}={:.0} | bends={}×{:.1}={:.0} | length={:.1}mm×{:.2}={:.0} | vias={}×{:.0}={:.0}",
            self.normalized_score,
            self.raw_score,
            self.maximum_score,
            self.incomplete_connections,
            self.weights.unrouted_net_penalty,
            self.unrouted_connections_penalty,
            self.clearance_violations,
            self.weights.clearance_violation_penalty,
            self.clearance_violations_penalty,
            self.bend_count,
            self.weights.bend_penalty,
            self.bends_penalty,
            self.total_trace_length_mm,
            self.weights.default_preferred_direction_trace_cost,
            self.trace_length_cost,
            self.via_count,
            self.weights.via_costs as f64,
            self.vias_cost
        )
    }
}

pub struct ScoringWeightComparison;

#[derive(Clone, Debug)]
pub struct ScoringWeightComparisonResult {
    pub score_a: BoardScoreBreakdown,
    pub score_b: BoardScoreBreakdown,
    pub raw_score_delta: f32,
    pub normalized_score_delta: f32,
    pub unrouted_penalty_delta: f32,
    pub clearance_penalty_delta: f32,
    pub bends_penalty_delta: f32,
    pub trace_length_cost_delta: f32,
    pub vias_cost_delta: f32,
}

impl ScoringWeightComparison {
    pub fn compare(
        stats: &BoardStatistics,
        weights_a: &RoutingCostSettings,
        weights_b: &RoutingCostSettings,
    ) -> ScoringWeightComparisonResult {
        let a = BoardScoreBreakdown::of(stats, weights_a);
        let b = BoardScoreBreakdown::of(stats, weights_b);
        ScoringWeightComparisonResult {
            raw_score_delta: b.raw_score - a.raw_score,
            normalized_score_delta: b.normalized_score - a.normalized_score,
            unrouted_penalty_delta: b.unrouted_connections_penalty - a.unrouted_connections_penalty,
            clearance_penalty_delta: b.clearance_violations_penalty - a.clearance_violations_penalty,
            bends_penalty_delta: b.bends_penalty - a.bends_penalty,
            trace_length_cost_delta: b.trace_length_cost - a.trace_length_cost,
            vias_cost_delta: b.vias_cost - a.vias_cost,
            score_a: a,
            score_b: b,
        }
    }
}

impl ScoringWeightComparisonResult {
    pub fn is_candidate_better(&self) -> bool {
        self.normalized_score_delta > 0.0
    }
}
