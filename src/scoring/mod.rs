//! Board statistics and scores (port of `app.freerouting.core.scoring`, headless parts).
//!
//! Java's boxed nullable fields (`Integer`, `Float`, `Double`) which are read with null
//! checks by the score functions are `Option`s; the others are plain values.
//! Not ported: the constructors from SES/DSN/JSON file contents, the JSON based
//! `BenchmarkScoreCalculator.run/calculateScores` (only the score rounding is provided)
//! and the report string formatting of `ScoringWeightComparison`.

pub mod bounds;
pub mod breakdown;

pub use bounds::calculate_bounds;
pub use breakdown::{BoardScoreBreakdown, ScoringWeightComparison, ScoringWeightComparisonResult};

use crate::board::{Board, FixedState, ItemId, ItemKind, Unit};
use crate::drc::{ClearanceBaseline, ClearanceChecker, NetRoutingLedger};
use crate::rules::BoardRules;
use crate::settings::{OptimizerScoreSettings, RouterScoreSettings, RouterSettings, RoutingCostSettings};

/// `RouterScoringVersion`. The Rust settings have no version field; the default of
/// the original (`V2_CONTINUOUS`) is used by [`BoardStatistics::get_router_score`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouterScoringVersion {
    V1Legacy,
    V2Continuous,
}

/// `OptimizerScoringVersion`; default of the original: `V2_LOWER_BOUND`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptimizerScoringVersion {
    V1Legacy,
    V2LowerBound,
}

pub const DEFAULT_ROUTER_SCORING_VERSION: RouterScoringVersion = RouterScoringVersion::V2Continuous;
pub const DEFAULT_OPTIMIZER_SCORING_VERSION: OptimizerScoringVersion = OptimizerScoringVersion::V2LowerBound;

/// `java.awt.geom.Rectangle2D.Float`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsBoard {
    pub bounding_box: RectF,
    pub size: RectF,
    pub area_cm2: f32,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsLayers {
    pub total_count: i32,
    pub signal_count: i32,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsItems {
    pub total_count: i32,
    pub trace_count: i32,
    pub via_count: i32,
    pub conduction_area_count: i32,
    pub drill_item_count: i32,
    pub pin_count: i32,
    pub component_outline_count: i32,
    pub other_count: i32,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsComponents {
    pub total_count: i32,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsPads {
    pub total_count: i32,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsNets {
    pub total_count: i32,
    pub class_count: i32,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsConnections {
    pub maximum_count: Option<i32>,
    pub incomplete_count: Option<i32>,
    pub design_blocked_count: Option<i32>,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsTraces {
    pub total_count: i32,
    pub total_segment_count: i32,
    pub total_length: f32,
    pub total_length_mm: Option<f32>,
    pub total_weighted_length: f32,
    pub average_length: f32,
    pub total_vertical_length: f32,
    pub total_horizontal_length: f32,
    pub total_angled_length: f32,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsBends {
    pub total_count: i32,
    pub ninety_degree_count: i32,
    pub forty_five_degree_count: i32,
    pub other_angle_count: i32,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsVias {
    pub total_count: i32,
    pub through_hole_count: i32,
    pub blind_count: i32,
    pub buried_count: i32,
}

#[derive(Clone, Debug)]
pub struct BoardStatisticsClearanceViolations {
    pub total_count: Option<i32>,
    pub pre_existing_count: i32,
    pub unfixable_count: i32,
    pub router_introduced_count: i32,
    pub new_or_changed_count: Option<i32>,
    pub total_violation_um: Option<f64>,
    pub min_violation_um: Option<f64>,
    pub max_violation_um: Option<f64>,
    pub avg_violation_um: Option<f64>,
}

impl Default for BoardStatisticsClearanceViolations {
    fn default() -> Self {
        BoardStatisticsClearanceViolations {
            total_count: None,
            pre_existing_count: 0,
            unfixable_count: 0,
            router_introduced_count: 0,
            new_or_changed_count: None,
            total_violation_um: None,
            min_violation_um: None,
            max_violation_um: None,
            avg_violation_um: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsDifficulty {
    pub pin_count: Option<i32>,
    pub signal_layer_count: Option<i32>,
    pub complexity_c: Option<i32>,
    pub difficulty_d: Option<f32>,
    pub board_area_cm2: Option<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsBounds {
    pub min_trace_length_mm: Option<f32>,
    pub min_via_count: Option<i32>,
    pub min_bend_count: Option<i32>,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatisticsFanout {
    pub total_smd_pins: i32,
    pub pins_to_escape: i32,
    pub escaped_count: i32,
}

/// Options of the `BoardStatistics` constructor.
pub struct StatisticsOptions<'a> {
    /// Unit of the lengths (default mm).
    pub unit: Option<Unit>,
    pub include_clearance_violations: bool,
    pub include_connections: bool,
    /// The board's routing ledger (`board.routingLedger()`); a fresh one is used if `None`.
    pub ledger: Option<&'a mut NetRoutingLedger>,
    /// `board.clearanceBaseline`.
    pub clearance_baseline: Option<&'a ClearanceBaseline>,
}

impl Default for StatisticsOptions<'_> {
    fn default() -> Self {
        StatisticsOptions {
            unit: None,
            include_clearance_violations: true,
            include_connections: true,
            ledger: None,
            clearance_baseline: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatistics {
    pub host: String,
    pub unit: String,
    pub board: BoardStatisticsBoard,
    pub layers: BoardStatisticsLayers,
    pub items: BoardStatisticsItems,
    pub components: BoardStatisticsComponents,
    pub pads: BoardStatisticsPads,
    pub nets: BoardStatisticsNets,
    pub connections: BoardStatisticsConnections,
    pub traces: BoardStatisticsTraces,
    pub bends: BoardStatisticsBends,
    pub vias: BoardStatisticsVias,
    pub clearance_violations: BoardStatisticsClearanceViolations,
    pub difficulty: BoardStatisticsDifficulty,
    pub bounds: BoardStatisticsBounds,
    pub fanout: BoardStatisticsFanout,
}

/// `DoubleStream.sum()` of the JDK (compensated Kahan summation, JDK 18+ final step).
pub fn java_double_stream_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut s0 = 0.0f64;
    let mut s1 = 0.0f64;
    let mut simple = 0.0f64;
    for v in values {
        let tmp = v - s1;
        let sum = s0;
        let velvel = sum + tmp;
        s1 = (velvel - sum) - tmp;
        s0 = velvel;
        simple += v;
    }
    let tmp = s0 - s1;
    if tmp.is_nan() && simple.is_infinite() {
        simple
    } else {
        tmp
    }
}

/// `length * (halfWidth + clearance)`, halved for shove fixed traces; `None` for other
/// fixed states (the weighted length of `BoardStatistics` and `OptimizerMetrics`).
fn weighted_trace_length(board: &Board, id: ItemId) -> Option<f64> {
    let item = board.item(id);
    let t = item.as_trace()?;
    if item.fixed_state != FixedState::Unfixed && item.fixed_state != FixedState::ShoveFixed {
        return None;
    }
    let mut w = t.get_length()
        * (t.half_width + board.clearance_value(item.clearance_class, BoardRules::default_clearance_class(), t.layer))
            as f64;
    if item.fixed_state == FixedState::ShoveFixed {
        w /= 2.0;
    }
    Some(w)
}

impl BoardStatistics {
    /// `new BoardStatistics(board)`: with clearance violations and connections.
    pub fn new(board: &Board) -> BoardStatistics {
        Self::with_options(board, StatisticsOptions::default())
    }

    /// `new BoardStatistics(board, unit, includeClearanceViolations, includeConnections)`.
    pub fn with_flags(
        board: &Board,
        unit: Option<Unit>,
        include_clearance_violations: bool,
        include_connections: bool,
    ) -> BoardStatistics {
        Self::with_options(
            board,
            StatisticsOptions {
                unit,
                include_clearance_violations,
                include_connections,
                ..Default::default()
            },
        )
    }

    pub fn with_options(board: &Board, options: StatisticsOptions<'_>) -> BoardStatistics {
        let mut s = BoardStatistics::default();
        let bb = board.bounding_box;
        let info = &board.communication.specctra_parser_info;
        s.host = format!(
            "{},{}",
            info.host_cad.as_deref().unwrap_or("null"),
            info.host_version.as_deref().unwrap_or("null")
        );
        let board_unit = board.communication.unit;
        s.unit = board_unit.as_str().to_string();
        s.board.bounding_box = RectF {
            x: bb.ll.x as f32,
            y: bb.ll.y as f32,
            width: bb.ur.x.wrapping_sub(bb.ll.x) as f32,
            height: bb.ur.y.wrapping_sub(bb.ll.y) as f32,
        };
        s.board.size = RectF {
            x: 0.0,
            y: 0.0,
            width: (bb.ll.x as f32 - bb.ur.x as f32).abs(),
            height: (bb.ll.y as f32 - bb.ur.y as f32).abs(),
        };
        s.layers.total_count = board.layer_count();
        s.layers.signal_count = board.layer_structure.signal_layer_count() as i32;

        for item in board.items.iter() {
            s.items.total_count += 1;
            match &item.kind {
                ItemKind::Trace(_) => s.items.trace_count += 1,
                ItemKind::Via(_) => s.items.via_count += 1,
                ItemKind::Obstacle(_) if item.is_conduction_area() => s.items.conduction_area_count += 1,
                ItemKind::Pin(_) => s.items.pin_count += 1,
                ItemKind::ComponentOutline(_) => s.items.component_outline_count += 1,
                _ => s.items.other_count += 1,
            }
        }
        s.components.total_count = board.components.count() as i32;
        s.pads.total_count = board.get_pin_count() as i32;
        s.nets.total_count = board.rules.nets.max_net_number();
        s.nets.class_count = board.rules.net_classes.count() as i32;
        let traces = board.get_traces();
        s.traces.total_count = traces.len() as i32;
        s.traces.total_length =
            java_double_stream_sum(traces.iter().map(|id| board.item(*id).as_trace().unwrap().get_length())) as f32;
        let resolution = if board.communication.resolution > 0 { board.communication.resolution } else { 1 } as f64;
        let board_unit_to_mm = Unit::scale(1.0, board_unit, Unit::Mm) / resolution;
        let board_unit_to_um = Unit::scale(1.0, board_unit, Unit::Um) / resolution;
        s.board.area_cm2 = (s.board.size.width as f64 * board_unit_to_mm * s.board.size.height as f64 * board_unit_to_mm
            / 100.0) as f32;
        s.difficulty.pin_count = Some(s.items.pin_count);
        s.difficulty.signal_layer_count = Some(s.layers.signal_count);
        s.difficulty.complexity_c = Some(1.max(s.items.pin_count.wrapping_mul(s.layers.signal_count)));
        s.difficulty.board_area_cm2 = Some(s.board.area_cm2);
        s.difficulty.difficulty_d = Some(s.difficulty.complexity_c.unwrap() as f32);
        s.bounds = calculate_bounds(board);
        s.traces.total_length_mm = Some((s.traces.total_length as f64 * board_unit_to_mm) as f32);
        s.traces.average_length = if s.traces.total_count > 0 {
            s.traces.total_length / s.traces.total_count as f32
        } else {
            0.0
        };
        for id in &traces {
            let t = board.item(*id).as_trace().unwrap();
            let polyline = &t.polyline;
            let corner_count = polyline.corner_count() as i32;
            if corner_count > 1 {
                s.traces.total_segment_count += corner_count - 1;
            }
            for line in polyline.lines() {
                let a = line.a.to_float();
                let b = line.b.to_float();
                let length = ((a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)).sqrt() as f32;
                if a.x == b.x {
                    s.traces.total_vertical_length += length;
                } else if a.y == b.y {
                    s.traces.total_horizontal_length += length;
                } else {
                    s.traces.total_angled_length += length;
                }
            }
        }
        s.traces.total_weighted_length = 0.0;
        for item in board.items.iter() {
            if let Some(w) = weighted_trace_length(board, item.id) {
                s.traces.total_weighted_length += w as f32;
            }
        }
        if options.include_connections {
            let mut fresh;
            let ledger = match options.ledger {
                Some(l) => l,
                None => {
                    fresh = NetRoutingLedger::new();
                    &mut fresh
                }
            };
            s.connections.maximum_count = Some(ledger.maximum_connections(board) as i32);
            s.connections.incomplete_count = Some(ledger.incomplete_count(board) as i32);
        }
        for id in &traces {
            let polyline = &board.item(*id).as_trace().unwrap().polyline;
            let corner_count = polyline.corner_count();
            if corner_count >= 3 {
                s.bends.total_count += corner_count as i32 - 2;
                for i in 1..corner_count - 1 {
                    let prev = polyline.corner(i - 1).to_float();
                    let current = polyline.corner(i).to_float();
                    let next = polyline.corner(i + 1).to_float();
                    let dx1 = current.x - prev.x;
                    let dy1 = current.y - prev.y;
                    let dx2 = next.x - current.x;
                    let dy2 = next.y - current.y;
                    let mut angle = (dy2.atan2(dx2) - dy1.atan2(dx1)).to_degrees().abs();
                    angle = angle.min(360.0 - angle);
                    angle = if angle > 180.0 { 360.0 - angle } else { angle };
                    if (angle - 90.0).abs() < 1.0 {
                        s.bends.ninety_degree_count += 1;
                    } else if (angle - 45.0).abs() < 1.0 || (angle - 135.0).abs() < 1.0 {
                        s.bends.forty_five_degree_count += 1;
                    } else {
                        s.bends.other_angle_count += 1;
                    }
                }
            }
        }
        s.vias.total_count = board.get_via_count() as i32;
        for id in board.get_vias() {
            let d = board.item(id).as_drill().unwrap();
            if d.first_layer == 0 && d.last_layer == s.layers.total_count - 1 {
                s.vias.through_hole_count += 1;
            } else if d.first_layer == 0 || d.last_layer == s.layers.total_count - 1 {
                s.vias.blind_count += 1;
            } else {
                s.vias.buried_count += 1;
            }
        }
        let checker = if options.include_clearance_violations { Some(ClearanceChecker::new(board)) } else { None };
        if let Some(checker) = &checker {
            let violations = checker.all_clearance_violations();
            s.clearance_violations.total_count = Some(violations.len() as i32);
            if !violations.is_empty() {
                let mut min_v = f64::MAX;
                let mut max_v = 0.0f64;
                let mut sum = 0.0f64;
                for cv in &violations {
                    let shortfall = 0.0f64.max(cv.expected_clearance - cv.actual_clearance);
                    let shortfall_um = shortfall * board_unit_to_um;
                    min_v = java_min(min_v, shortfall_um);
                    max_v = java_max(max_v, shortfall_um);
                    sum += shortfall_um;
                }
                s.clearance_violations.total_violation_um = Some(sum);
                s.clearance_violations.min_violation_um = Some(min_v);
                s.clearance_violations.max_violation_um = Some(max_v);
                s.clearance_violations.avg_violation_um = Some(sum / violations.len() as f64);
            } else {
                s.clearance_violations.total_violation_um = Some(0.0);
                s.clearance_violations.min_violation_um = Some(0.0);
                s.clearance_violations.max_violation_um = Some(0.0);
                s.clearance_violations.avg_violation_um = Some(0.0);
            }
            let pre_existing = board.pre_existing_clearance_violations_count as i32;
            s.clearance_violations.pre_existing_count = pre_existing;
            s.clearance_violations.unfixable_count = violations.iter().filter(|v| v.is_unfixable(board)).count() as i32;
            s.clearance_violations.new_or_changed_count = options
                .clearance_baseline
                .map(|b| b.count_new_or_changed(board, &violations) as i32);
            s.clearance_violations.router_introduced_count = 0.max(violations.len() as i32 - pre_existing);
        } else {
            s.clearance_violations.total_count = Some(0);
            s.clearance_violations.pre_existing_count = 0;
            s.clearance_violations.unfixable_count = 0;
            s.clearance_violations.new_or_changed_count = Some(0);
            s.clearance_violations.router_introduced_count = 0;
            s.clearance_violations.total_violation_um = Some(0.0);
            s.clearance_violations.min_violation_um = Some(0.0);
            s.clearance_violations.max_violation_um = Some(0.0);
            s.clearance_violations.avg_violation_um = Some(0.0);
        }
        let unit = options.unit.unwrap_or(Unit::Mm);
        if unit != board_unit {
            let (from, to) = (board_unit, unit);
            let sc = |v: f32| Unit::scale(v as f64, from, to) as f32;
            s.unit = unit.as_str().to_string();
            s.board.bounding_box = RectF {
                x: sc(s.board.bounding_box.x),
                y: sc(s.board.bounding_box.y),
                width: sc(s.board.bounding_box.width),
                height: sc(s.board.bounding_box.height),
            };
            s.board.size = RectF {
                x: 0.0,
                y: 0.0,
                width: sc(s.board.size.width),
                height: sc(s.board.size.height),
            };
            s.traces.total_length = sc(s.traces.total_length);
            s.traces.total_weighted_length = sc(s.traces.total_weighted_length);
            s.traces.average_length = sc(s.traces.average_length);
            s.traces.total_horizontal_length = sc(s.traces.total_horizontal_length);
            s.traces.total_vertical_length = sc(s.traces.total_vertical_length);
            s.traces.total_angled_length = sc(s.traces.total_angled_length);
        }
        // Fanout statistics. Needs clearance checks of the pin contacts; uses the DRC
        // tree built above if available.
        let fanout_checker;
        let checker = match &checker {
            Some(c) => c,
            None => {
                fanout_checker = ClearanceChecker::new(board);
                &fanout_checker
            }
        };
        let mut total = 0;
        let mut escaped = 0;
        let mut already_connected = 0;
        for item in board.items.iter() {
            let Some(pin) = item.as_pin() else { continue };
            if pin.drill.first_layer != pin.drill.last_layer {
                continue;
            }
            if item.net_count() > 0 {
                total += 1;
                let net_no = item.get_net_number(0);
                if board.unconnected_set(item.id, net_no).is_empty() {
                    already_connected += 1;
                }
                if is_pin_escaped(checker, item.id) {
                    escaped += 1;
                }
            }
        }
        s.fanout.total_smd_pins = total;
        s.fanout.pins_to_escape = total - already_connected;
        s.fanout.escaped_count = escaped;
        s
    }

    /// `calculateScore` (legacy score).
    pub fn calculate_score(&self, w: &RoutingCostSettings) -> f32 {
        let maximum_score = self.get_maximum_score(w);
        let penalties = self.connections.incomplete_count.unwrap_or(0) as f32 * w.unrouted_net_penalty
            + self.clearance_violations.total_count.unwrap_or(0) as f32 * w.clearance_violation_penalty
            + self.bends.total_count as f32 * w.bend_penalty;
        let trace_length_for_cost = self.traces.total_length_mm.unwrap_or(self.traces.total_length);
        let costs = (trace_length_for_cost as f64 * w.default_preferred_direction_trace_cost
            + self.vias.total_count.wrapping_mul(w.via_costs) as f64) as f32;
        maximum_score - penalties - costs
    }

    pub fn get_maximum_score(&self, w: &RoutingCostSettings) -> f32 {
        self.connections.maximum_count.unwrap_or(0) as f32 * w.unrouted_net_penalty
    }

    fn get_legacy_normalized_score(&self, w: &RoutingCostSettings) -> f32 {
        let maximum_score = self.get_maximum_score(w);
        if maximum_score <= 0.0 {
            return 0.0;
        }
        java_max_f32(0.0, self.calculate_score(w) / maximum_score) * 1000.0
    }

    /// `getRouterScore(RoutingCostSettings)` (legacy).
    pub fn get_router_score_legacy(&self, w: &RoutingCostSettings) -> f32 {
        self.get_legacy_normalized_score(w)
    }

    /// `getRouterScore(RouterSettings)` with the default scoring version (V2).
    pub fn get_router_score(&self, settings: &RouterSettings) -> f32 {
        self.get_router_score_with_version(settings, DEFAULT_ROUTER_SCORING_VERSION)
    }

    pub fn get_router_score_with_version(&self, settings: &RouterSettings, version: RouterScoringVersion) -> f32 {
        if version != RouterScoringVersion::V2Continuous {
            return self.get_legacy_normalized_score(&settings.scoring);
        }
        self.get_v2_router_score(&settings.router_scoring)
    }

    /// The V2 (continuous) router score. Takes `&self` (the original's
    /// `ensureDifficulty()` side effect is applied on a copy if needed).
    pub fn get_v2_router_score(&self, settings: &RouterScoreSettings) -> f32 {
        let mut diff_owner;
        let stats = if self.difficulty.difficulty_d.is_none() {
            diff_owner = self.clone();
            diff_owner.ensure_difficulty();
            &diff_owner
        } else {
            self
        };
        let difficulty = stats.difficulty.difficulty_d.map(|d| 1.0f64.max(d as f64)).unwrap_or(1.0);
        let connections = stats.connections.maximum_count.map(|c| 0.max(c) as f64).unwrap_or(0.0);
        let incomplete = stats.connections.incomplete_count.map(|c| 0.max(c) as f64).unwrap_or(0.0);
        let violation_count = stats.clearance_violations.total_count.map(|c| 0.max(c) as f64).unwrap_or(0.0);
        let violation_depth = stats.clearance_violations.total_violation_um.map(|d| java_max(0.0, d)).unwrap_or(0.0);
        let split = java_min(1.0, java_max(0.0, settings.unrouted_free_fraction as f64));
        let first_half_weight = settings.unrouted_first_half_weight as f64;
        let second_half_weight = settings.unrouted_second_half_weight as f64;
        let open_fraction = if connections > 0.0 { incomplete / connections } else { 0.0 };
        let (first_half_open, second_half_open) = if connections <= 0.0 {
            (0.0, 0.0)
        } else if split <= 0.0 {
            (0.0, open_fraction)
        } else if split >= 1.0 {
            (open_fraction, 0.0)
        } else {
            (
                java_min(1.0, java_max(0.0, (open_fraction - split) / (1.0 - split))),
                java_min(1.0, open_fraction / split),
            )
        };
        let unrouted_penalty = first_half_weight * first_half_open + second_half_weight * second_half_open;
        let mut drc_penalty = settings.clearance_violation_count_weight as f64 * violation_count / difficulty;
        let depth_scale = java_max(1.0, settings.clearance_violation_depth_scale as f64);
        drc_penalty += settings.clearance_violation_depth_weight as f64 * violation_depth / depth_scale / difficulty;
        java_max(0.0, 1000.0 - unrouted_penalty - drc_penalty) as f32
    }

    /// `ensureDifficulty()`.
    pub fn ensure_difficulty(&mut self) {
        if self.difficulty.difficulty_d.is_some() {
            return;
        }
        if self.difficulty.pin_count.map(|p| p <= 0).unwrap_or(true) {
            self.difficulty.pin_count = Some(if self.items.pin_count > 0 {
                self.items.pin_count
            } else if self.pads.total_count > 0 {
                self.pads.total_count
            } else {
                0
            });
        }
        if self.difficulty.signal_layer_count.map(|p| p <= 0).unwrap_or(true) {
            self.difficulty.signal_layer_count = Some(if self.layers.signal_count > 0 {
                self.layers.signal_count
            } else if self.layers.total_count > 0 {
                self.layers.total_count
            } else {
                0
            });
        }
        if self.difficulty.complexity_c.map(|c| c <= 0).unwrap_or(true) {
            let pins = self.difficulty.pin_count.unwrap_or(0);
            let layers = self.difficulty.signal_layer_count.unwrap_or(0);
            self.difficulty.complexity_c = Some(1.max(pins.wrapping_mul(layers)));
        }
        if self.difficulty.difficulty_d.is_none() {
            self.difficulty.difficulty_d = Some(self.difficulty.complexity_c.unwrap() as f32);
        }
    }

    /// `getOptimizerScore(RoutingCostSettings)` (legacy).
    pub fn get_optimizer_score_legacy(&self, w: &RoutingCostSettings) -> f32 {
        self.get_legacy_normalized_score(w)
    }

    /// `getOptimizerScore(RouterSettings)` with the default scoring version (V2).
    pub fn get_optimizer_score(&self, settings: &RouterSettings) -> f32 {
        self.get_optimizer_score_with_version(settings, DEFAULT_OPTIMIZER_SCORING_VERSION)
    }

    pub fn get_optimizer_score_with_version(&self, settings: &RouterSettings, version: OptimizerScoringVersion) -> f32 {
        if version == OptimizerScoringVersion::V2LowerBound {
            return self.get_v2_optimizer_score(&settings.optimizer_scoring);
        }
        self.get_legacy_normalized_score(&settings.scoring)
    }

    pub fn get_v2_optimizer_score(&self, settings: &OptimizerScoreSettings) -> f32 {
        let difficulty = self.difficulty.difficulty_d.map(|d| 1.0f64.max(d as f64)).unwrap_or(1.0);
        let min_trace_length = self.bounds.min_trace_length_mm.map(|v| java_max(0.0, v as f64)).unwrap_or(0.0);
        let min_via_count = self.bounds.min_via_count.map(|v| 0.max(v) as f64).unwrap_or(0.0);
        let min_bend_count = self.bounds.min_bend_count.map(|v| 0.max(v) as f64).unwrap_or(0.0);
        let actual_trace_length = self.traces.total_length_mm.map(|v| java_max(0.0, v as f64)).unwrap_or(0.0);
        let actual_via_count = 0.max(self.vias.total_count) as f64;
        let actual_bend_count = 0.max(self.bends.total_count) as f64;
        let length_floor = java_max(0.0, settings.length_floor as f64);
        let difficulty_floor = java_max(1.0, settings.difficulty_scale_floor as f64);
        let length_penalty = settings.excess_wire_length_weight as f64
            * java_max(0.0, actual_trace_length - min_trace_length)
            / java_max(min_trace_length, length_floor);
        let via_penalty = settings.excess_via_weight as f64 * java_max(0.0, actual_via_count - min_via_count)
            / java_max(difficulty, difficulty_floor);
        let bend_penalty = settings.excess_bend_weight as f64 * java_max(0.0, actual_bend_count - min_bend_count)
            / java_max(difficulty, difficulty_floor);
        java_max(0.0, 1000.0 - length_penalty - via_penalty - bend_penalty) as f32
    }

    /// Deprecated `getNormalizedScore(RoutingCostSettings)` = legacy router score.
    pub fn get_normalized_score(&self, w: &RoutingCostSettings) -> f32 {
        self.get_router_score_legacy(w)
    }
}

/// `BoardStatistics.isPinEscaped(pin)`.
pub fn is_pin_escaped(checker: &ClearanceChecker<'_>, pin: ItemId) -> bool {
    let board = checker.board();
    for c in board.normal_contacts(pin) {
        let ci = board.item(c);
        if ci.is_trace() {
            if checker.clearance_violations(c).is_empty() {
                return true;
            }
        } else if ci.is_via() {
            if checker.clearance_violations(c).is_empty() {
                for vc in board.normal_contacts(c) {
                    let vci = board.item(vc);
                    if vci.is_trace() || vci.is_conduction_area() {
                        return true;
                    }
                }
            }
        } else if ci.is_conduction_area() {
            return true;
        }
    }
    false
}

/// `BoardStatistics.OptimizerMetrics`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OptimizerMetrics {
    pub via_count: i32,
    pub total_length: f32,
    pub total_weighted_length: f32,
}

impl OptimizerMetrics {
    pub fn of(board: &Board) -> OptimizerMetrics {
        let mut via_count = 0;
        let mut total_length = 0.0f32;
        let mut total_weighted_length = 0.0f32;
        for item in board.items.iter() {
            if item.is_via() {
                via_count += 1;
            } else if let Some(t) = item.as_trace() {
                total_length += t.get_length() as f32;
                if let Some(w) = weighted_trace_length(board, item.id) {
                    total_weighted_length += w as f32;
                }
            }
        }
        let unit = board.communication.unit;
        if unit != Unit::Mm {
            total_length = Unit::scale(total_length as f64, unit, Unit::Mm) as f32;
            total_weighted_length = Unit::scale(total_weighted_length as f64, unit, Unit::Mm) as f32;
        }
        OptimizerMetrics {
            via_count,
            total_length,
            total_weighted_length,
        }
    }
}

/// `BenchmarkScoreCalculator.BenchmarkScoreResult`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BenchmarkScoreResult {
    pub router_score: f32,
    pub is_failed: bool,
}

/// The benchmark score of a run: `max(0, round(score * 100) / 100)` of the V2 router
/// score with default settings (`BenchmarkScoreCalculator.processRunsArray`).
pub fn benchmark_score(stats: Option<&BoardStatistics>, is_failed: bool) -> BenchmarkScoreResult {
    let mut failed = is_failed;
    let mut router_score = 0.0f32;
    if !failed {
        match stats {
            Some(st) => {
                let mut st = st.clone();
                st.ensure_difficulty();
                router_score = st.get_router_score(&RouterSettings::default());
            }
            None => failed = true,
        }
    }
    // Math.round(float) -> int
    let rounded = ((router_score * 100.0f32) + 0.5f32).floor() as i32;
    let rounded_score = java_max_f32(0.0, rounded as f32 / 100.0f32);
    BenchmarkScoreResult {
        router_score: rounded_score,
        is_failed: failed,
    }
}

/// `Math.max(double, double)` (NaN propagating, -0.0 < 0.0).
#[inline]
pub(crate) fn java_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() && b.is_sign_negative() {
            return -0.0;
        }
        return 0.0;
    }
    if a >= b {
        a
    } else {
        b
    }
}

/// `Math.min(double, double)`.
#[inline]
pub(crate) fn java_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() || b.is_sign_negative() {
            return -0.0;
        }
        return 0.0;
    }
    if a <= b {
        a
    } else {
        b
    }
}

#[inline]
pub(crate) fn java_max_f32(a: f32, b: f32) -> f32 {
    java_max(a as f64, b as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kahan_sum_matches_simple_for_small_values() {
        assert_eq!(java_double_stream_sum([1.0, 2.0, 3.5]), 6.5);
        // compensated: 1e16 + 1 + 1 - 1e16 == 2 with compensation
        assert_eq!(java_double_stream_sum([1e16, 1.0, 1.0, -1e16]), 2.0);
    }

    #[test]
    fn v2_router_score_fully_routed() {
        let mut s = BoardStatistics::default();
        s.connections.maximum_count = Some(10);
        s.connections.incomplete_count = Some(0);
        s.clearance_violations.total_count = Some(0);
        s.clearance_violations.total_violation_um = Some(0.0);
        s.items.pin_count = 20;
        s.layers.signal_count = 2;
        let settings = RouterSettings::default();
        assert_eq!(s.get_router_score(&settings), 1000.0);
        // half open: first half weight not reached, second half fully open
        s.connections.incomplete_count = Some(5);
        let v = s.get_router_score(&settings);
        assert!((v - (1000.0 - 2000.0 / 3.0)).abs() < 0.01, "{v}");
    }

    #[test]
    fn legacy_score() {
        let mut s = BoardStatistics::default();
        s.connections.maximum_count = Some(2);
        s.connections.incomplete_count = Some(1);
        s.clearance_violations.total_count = Some(0);
        s.traces.total_length_mm = Some(0.0);
        let w = RouterSettings::default().scoring;
        assert_eq!(s.get_router_score_legacy(&w), 500.0);
    }

    #[test]
    fn benchmark_rounding() {
        let r = benchmark_score(None, false);
        assert!(r.is_failed);
        assert_eq!(r.router_score, 0.0);
    }
}
