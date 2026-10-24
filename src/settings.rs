//! Router settings (port of `app.freerouting.settings.RouterSettings` and its parts,
//! with the defaults of `DefaultSettings`).

use crate::board::Board;
use crate::optimize::ExpansionCostFactor;

pub const ALGORITHM_CURRENT: &str = "freerouting-router";
pub const MIN_BEND_COST: f64 = 0.0;
pub const MAX_BEND_COST: f64 = 9.9;

pub const DEFAULT_FALLBACK_STRATEGIES: &str = "direct-first,fanout-retry,short-escape,open-escape,no-fanout";
pub const DEFAULT_EXPLORE_FALLBACK_STRATEGIES: &str = "short-escape,open-escape";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardUpdateStrategy {
    GreedySequential,
    GlobalOptimal,
    HybridOptimal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemSelectionStrategy {
    Sequential,
    Random,
    Prioritized,
}

#[derive(Clone, Debug)]
pub struct AutorouterSettings {
    pub enabled: bool,
    pub algorithm: String,
    pub max_passes: i32,
    pub max_items: i32,
    pub max_threads: Option<i32>,
    pub save_intermediate_stages: bool,
    pub ignore_net_classes: Vec<String>,
    pub start_necked: bool,
    pub fallback_strategies: String,
    pub explore_fallback_strategies: String,
    pub smd_net_via_cost_factor: f64,
    pub residual_repair_max_open: i32,
    pub explore_patience: i32,
    pub item_order_seed: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct OptimizerSettings {
    pub enabled: bool,
    pub algorithm: String,
    pub max_passes: i32,
    pub max_items: i32,
    pub max_threads: i32,
    pub optimization_improvement_threshold: f32,
    pub enable_preflight_guards: bool,
    pub max_consecutive_failures: i32,
    pub max_consecutive_failures_pass1: i32,
    pub additional_ripup_cost_factor_at_start: i32,
    pub trace_ripup_cost_factor: f32,
    pub max_autoroute_passes: i32,
    pub board_update_strategy: BoardUpdateStrategy,
    pub item_selection_strategy: ItemSelectionStrategy,
    pub timeout_string: Option<String>,
}

#[derive(Clone, Debug)]
pub struct FanoutSettings {
    pub enabled: bool,
    pub max_passes: i32,
    pub max_items: i32,
    pub max_milliseconds_per_pin: i64,
    pub ripup_allowed: bool,
    pub min_escape_length_mm: f64,
    pub max_escape_length_mm: f64,
    pub start_via_diameter_mm: f64,
    pub end_via_diameter_mm: f64,
    pub pin_sorting_order: String,
    pub fallback_to_board_vias: bool,
    pub retry_without_escape_window: bool,
    pub strategy: String,
    pub timeout_string: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerSettings {
    pub routable: Option<bool>,
    pub preferred_direction_horizontal: Option<bool>,
    pub bend_cost: Option<f64>,
    pub preferred_direction_trace_cost: Option<f64>,
    pub undesired_direction_trace_cost: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct RoutingCostSettings {
    pub preferred_direction_trace_cost: Vec<f64>,
    pub undesired_direction_trace_cost: Vec<f64>,
    pub default_preferred_direction_trace_cost: f64,
    pub default_undesired_direction_trace_cost: f64,
    pub via_costs: i32,
    pub plane_via_costs: i32,
    pub start_ripup_costs: i32,
    pub default_bend_cost: f64,
    pub unrouted_net_penalty: f32,
    pub clearance_violation_penalty: f32,
    pub bend_penalty: f32,
}

#[derive(Clone, Debug)]
pub struct RouterScoreSettings {
    pub unrouted_connection_weight: f32,
    pub unrouted_free_fraction: f32,
    pub unrouted_first_half_weight: f32,
    pub unrouted_second_half_weight: f32,
    pub clearance_violation_count_weight: f32,
    pub clearance_violation_depth_weight: f32,
    pub clearance_violation_depth_scale: f32,
}

#[derive(Clone, Debug)]
pub struct OptimizerScoreSettings {
    pub excess_wire_length_weight: f32,
    pub excess_via_weight: f32,
    pub excess_bend_weight: f32,
    pub length_floor: f32,
    pub difficulty_scale_floor: f32,
}

#[derive(Clone, Debug)]
pub struct RouterSettings {
    pub autorouter: AutorouterSettings,
    pub fanout: FanoutSettings,
    pub optimizer: OptimizerSettings,
    pub scoring: RoutingCostSettings,
    pub router_scoring: RouterScoreSettings,
    pub optimizer_scoring: OptimizerScoreSettings,
    pub copper_to_edge_clearance_um: f64,
    pub hole_clearance_um: f64,
    pub clearance_tolerance_um: f64,
    pub plane_nets: Vec<String>,
    pub plane_as_obstacle: bool,
    pub neck_width_um: f64,
    pub strict_drc: bool,
    pub fix_kicad_footprints: bool,
    pub job_timeout_string: String,
    pub layers: Vec<LayerSettings>,
    pub trace_pull_tight_accuracy: i32,
    pub vias_allowed: bool,
    pub automatic_neckdown: bool,
    pub max_threads: i32,
    pub board_specific_trace_costs_applied: bool,
}

fn available_processors() -> i32 {
    std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(1)
}

fn default_max_threads() -> i32 {
    (available_processors() - 1).max(1)
}

impl Default for RouterSettings {
    /// The defaults of `DefaultSettings`.
    fn default() -> Self {
        let threads = default_max_threads();
        RouterSettings {
            autorouter: AutorouterSettings {
                enabled: true,
                algorithm: ALGORITHM_CURRENT.to_string(),
                max_passes: 0,
                max_items: i32::MAX,
                max_threads: Some(threads),
                save_intermediate_stages: false,
                ignore_net_classes: Vec::new(),
                start_necked: true,
                fallback_strategies: DEFAULT_FALLBACK_STRATEGIES.to_string(),
                explore_fallback_strategies: DEFAULT_EXPLORE_FALLBACK_STRATEGIES.to_string(),
                smd_net_via_cost_factor: 0.1,
                residual_repair_max_open: 4,
                explore_patience: 0,
                item_order_seed: None,
            },
            fanout: FanoutSettings {
                enabled: true,
                max_passes: 20,
                max_items: i32::MAX,
                max_milliseconds_per_pin: 10000,
                ripup_allowed: true,
                min_escape_length_mm: 2.5,
                max_escape_length_mm: 4.5,
                start_via_diameter_mm: 0.25,
                end_via_diameter_mm: 0.25,
                pin_sorting_order: "outer_first".to_string(),
                fallback_to_board_vias: true,
                retry_without_escape_window: false,
                strategy: "eager".to_string(),
                timeout_string: None,
            },
            optimizer: OptimizerSettings {
                enabled: true,
                algorithm: "freerouting-optimizer".to_string(),
                max_passes: 100,
                max_items: i32::MAX,
                max_threads: threads,
                optimization_improvement_threshold: 2.5,
                enable_preflight_guards: true,
                max_consecutive_failures: 50,
                max_consecutive_failures_pass1: 12,
                additional_ripup_cost_factor_at_start: 10,
                trace_ripup_cost_factor: 0.6,
                max_autoroute_passes: 6,
                board_update_strategy: BoardUpdateStrategy::GlobalOptimal,
                item_selection_strategy: ItemSelectionStrategy::Sequential,
                timeout_string: None,
            },
            scoring: RoutingCostSettings {
                preferred_direction_trace_cost: Vec::new(),
                undesired_direction_trace_cost: Vec::new(),
                default_preferred_direction_trace_cost: 1.0,
                default_undesired_direction_trace_cost: 1.0,
                via_costs: 50,
                plane_via_costs: 5,
                start_ripup_costs: 100,
                default_bend_cost: 0.0,
                unrouted_net_penalty: 5_000_000.0,
                clearance_violation_penalty: 1_000_000.0,
                bend_penalty: 10.0,
            },
            router_scoring: RouterScoreSettings {
                unrouted_connection_weight: 1000.0,
                unrouted_free_fraction: 0.5,
                unrouted_first_half_weight: 1000.0 / 3.0,
                unrouted_second_half_weight: 2000.0 / 3.0,
                clearance_violation_count_weight: 25.0,
                clearance_violation_depth_weight: 300.0,
                clearance_violation_depth_scale: 1000.0,
            },
            optimizer_scoring: OptimizerScoreSettings {
                excess_wire_length_weight: 1000.0,
                excess_via_weight: 2000.0,
                excess_bend_weight: 500.0,
                length_floor: 1.0,
                difficulty_scale_floor: 1.0,
            },
            copper_to_edge_clearance_um: 250.0,
            hole_clearance_um: 0.0,
            clearance_tolerance_um: 1.0,
            plane_nets: Vec::new(),
            plane_as_obstacle: false,
            neck_width_um: 0.0,
            strict_drc: false,
            fix_kicad_footprints: false,
            job_timeout_string: "12:00:00".to_string(),
            layers: Vec::new(),
            trace_pull_tight_accuracy: 500,
            vias_allowed: true,
            automatic_neckdown: true,
            max_threads: threads,
            board_specific_trace_costs_applied: false,
        }
    }
}

impl RouterSettings {
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    pub fn set_layer_count(&mut self, layer_count: usize) {
        if self.layers.len() != layer_count {
            self.board_specific_trace_costs_applied = false;
            self.layers = vec![LayerSettings::default(); layer_count];
        }
        self.scoring.preferred_direction_trace_cost = vec![1.0; layer_count];
        self.scoring.undesired_direction_trace_cost = vec![1.0; layer_count];
        for l in self.layers.iter_mut() {
            l.routable = Some(true);
            l.preferred_direction_horizontal = None;
            l.bend_cost = None;
            l.preferred_direction_trace_cost = None;
            l.undesired_direction_trace_cost = None;
        }
    }

    pub fn get_autorouter_max_threads(&self) -> i32 {
        let configured = self.autorouter.max_threads.unwrap_or(self.max_threads);
        normalize_max_threads(Some(configured))
    }

    pub fn set_max_threads(&mut self, value: Option<i32>) {
        self.max_threads = normalize_max_threads(value);
        self.autorouter.max_threads = Some(self.max_threads);
        self.optimizer.max_threads = self.max_threads;
    }

    pub fn apply_board_specific_optimizations_if_needed(&mut self, board: &Board) {
        if self.layer_count() != board.layer_count() as usize || !self.board_specific_trace_costs_applied {
            self.apply_board_specific_optimizations(board);
        }
    }

    /// Derives preferred directions and trace costs from the board shape.
    pub fn apply_board_specific_optimizations(&mut self, board: &Board) {
        let hw = board.bounding_box.width() as f64;
        let vw = board.bounding_box.height() as f64;
        let n = board.layer_count() as usize;
        let h_add = 0.1 * crate::geometry::basics::java_round(10.0 * hw / vw) as f64;
        let v_add = 0.1 * crate::geometry::basics::java_round(10.0 * vw / hw) as f64;
        if self.layers.len() != n {
            self.board_specific_trace_costs_applied = false;
            let old = std::mem::take(&mut self.layers);
            self.layers = (0..n).map(|i| old.get(i).cloned().unwrap_or_default()).collect();
        }
        if self.scoring.preferred_direction_trace_cost.len() != n {
            self.scoring.preferred_direction_trace_cost = vec![0.0; n];
            self.board_specific_trace_costs_applied = false;
        }
        if self.scoring.undesired_direction_trace_cost.len() != n {
            self.scoring.undesired_direction_trace_cost = vec![0.0; n];
            self.board_specific_trace_costs_applied = false;
        }
        let mut pref_horizontal = hw < vw;
        let init = !self.board_specific_trace_costs_applied;
        for i in 0..n {
            let is_signal = board.layer_structure.layers[i].is_signal;
            if is_signal {
                pref_horizontal = !pref_horizontal;
            }
            let l = &mut self.layers[i];
            if !is_signal {
                l.routable = Some(false);
            } else if l.routable.is_none() {
                l.routable = Some(true);
            }
            if l.bend_cost.is_none() {
                l.bend_cost = Some(self.scoring.default_bend_cost);
            }
            if l.preferred_direction_horizontal.is_none() {
                l.preferred_direction_horizontal = Some(pref_horizontal);
            }
            if let Some(c) = l.preferred_direction_trace_cost {
                self.scoring.preferred_direction_trace_cost[i] = c;
            } else if init {
                self.scoring.preferred_direction_trace_cost[i] = self.scoring.default_preferred_direction_trace_cost;
            }
            if let Some(c) = l.undesired_direction_trace_cost {
                self.scoring.undesired_direction_trace_cost[i] = c;
            } else if init {
                self.scoring.undesired_direction_trace_cost[i] = self.scoring.default_undesired_direction_trace_cost
                    + if pref_horizontal { h_add } else { v_add };
            }
        }
        if init {
            let signal_count = board.layer_structure.signal_layer_count();
            if signal_count > 2 {
                let outer = 0.2 * signal_count as f64;
                if self.layers[0].preferred_direction_trace_cost.is_none() {
                    self.scoring.preferred_direction_trace_cost[0] += outer;
                }
                if self.layers[n - 1].preferred_direction_trace_cost.is_none() {
                    self.scoring.preferred_direction_trace_cost[n - 1] += outer;
                }
                if self.layers[0].undesired_direction_trace_cost.is_none() {
                    self.scoring.undesired_direction_trace_cost[0] += outer;
                }
                if self.layers[n - 1].undesired_direction_trace_cost.is_none() {
                    self.scoring.undesired_direction_trace_cost[n - 1] += outer;
                }
            }
            self.board_specific_trace_costs_applied = true;
        }
    }

    pub fn apply_net_class_exclusions(&self, board: &mut Board) {
        for name in &self.autorouter.ignore_net_classes {
            if name.trim().is_empty() {
                continue;
            }
            let ids: Vec<_> = board
                .rules
                .net_classes
                .iter()
                .filter(|(_, c)| c.get_name().eq_ignore_ascii_case(name))
                .map(|(i, _)| i)
                .collect();
            for i in ids {
                board.rules.net_classes.get_mut(i).is_ignored_by_autorouter = true;
            }
        }
    }

    pub fn uses_direct_pin_routing(&self) -> bool {
        let s = &self.fanout.strategy;
        if s.is_empty() || s.eq_ignore_ascii_case("eager") {
            return false;
        }
        s.eq_ignore_ascii_case("direct_first")
    }

    pub fn get_start_ripup_costs(&self) -> i32 {
        self.scoring.start_ripup_costs
    }

    pub fn set_start_ripup_costs(&mut self, v: i32) {
        self.scoring.start_ripup_costs = v.max(1);
    }

    pub fn get_run_router(&self) -> bool {
        self.autorouter.enabled
    }

    pub fn get_run_optimizer(&self) -> bool {
        self.optimizer.enabled
    }

    pub fn is_fanout_enabled(&self) -> bool {
        self.fanout.enabled
    }

    pub fn get_via_costs(&self) -> i32 {
        self.scoring.via_costs
    }

    pub fn set_via_costs(&mut self, v: i32) {
        self.scoring.via_costs = v.max(1);
    }

    pub fn get_plane_via_costs(&self) -> i32 {
        self.scoring.plane_via_costs
    }

    pub fn set_plane_via_costs(&mut self, v: i32) {
        self.scoring.plane_via_costs = v.max(1);
    }

    pub fn get_layer_active(&self, layer: usize) -> bool {
        match self.layers.get(layer) {
            Some(l) => l.routable.unwrap_or(true),
            None => {
                log::warn!("AutorouteSettings.get_layer_active: layer={layer} out of range");
                false
            }
        }
    }

    pub fn set_layer_active(&mut self, layer: usize, value: bool) {
        if let Some(l) = self.layers.get_mut(layer) {
            l.routable = Some(value);
        }
    }

    pub fn get_bend_cost(&self, layer: usize) -> f64 {
        match self.layers.get(layer) {
            Some(l) => match l.bend_cost {
                Some(c) => c,
                None => self.scoring.default_bend_cost.clamp(MIN_BEND_COST, MAX_BEND_COST),
            },
            None => 0.0,
        }
    }

    pub fn get_preferred_direction_is_horizontal(&self, layer: usize) -> bool {
        match self.layers.get(layer) {
            Some(l) => l.preferred_direction_horizontal.unwrap_or(layer % 2 == 1),
            None => false,
        }
    }

    pub fn set_preferred_direction_is_horizontal(&mut self, layer: usize, value: bool) {
        if let Some(l) = self.layers.get_mut(layer) {
            l.preferred_direction_horizontal = Some(value);
        }
    }

    pub fn get_preferred_direction_trace_costs(&self, layer: usize) -> f64 {
        if layer >= self.layers.len() {
            return 0.0;
        }
        if let Some(c) = self.layers[layer].preferred_direction_trace_cost {
            return c;
        }
        self.scoring.preferred_direction_trace_cost.get(layer).copied().unwrap_or(1.0)
    }

    pub fn get_against_preferred_direction_trace_costs(&self, layer: usize) -> f64 {
        if layer >= self.layers.len() {
            return 0.0;
        }
        if let Some(c) = self.layers[layer].undesired_direction_trace_cost {
            return c;
        }
        self.scoring.undesired_direction_trace_cost.get(layer).copied().unwrap_or(1.0)
    }

    pub fn set_preferred_direction_trace_costs(&mut self, layer: usize, value: f64) {
        let n = self.layers.len();
        if layer >= n {
            return;
        }
        if self.scoring.preferred_direction_trace_cost.len() != n {
            self.scoring.preferred_direction_trace_cost = vec![0.0; n];
        }
        self.scoring.preferred_direction_trace_cost[layer] = value.max(0.1);
        self.layers[layer].preferred_direction_trace_cost = Some(self.scoring.preferred_direction_trace_cost[layer]);
        self.board_specific_trace_costs_applied = true;
    }

    pub fn set_against_preferred_direction_trace_costs(&mut self, layer: usize, value: f64) {
        let n = self.layers.len();
        if layer >= n {
            return;
        }
        if self.scoring.undesired_direction_trace_cost.len() != n {
            self.scoring.undesired_direction_trace_cost = vec![0.0; n];
        }
        self.scoring.undesired_direction_trace_cost[layer] = value.max(0.1);
        self.layers[layer].undesired_direction_trace_cost = Some(self.scoring.undesired_direction_trace_cost[layer]);
        self.board_specific_trace_costs_applied = true;
    }

    pub fn get_horizontal_trace_costs(&self, layer: usize) -> f64 {
        if self.get_preferred_direction_is_horizontal(layer) {
            self.get_preferred_direction_trace_costs(layer)
        } else {
            self.get_against_preferred_direction_trace_costs(layer)
        }
    }

    pub fn get_vertical_trace_costs(&self, layer: usize) -> f64 {
        if self.get_preferred_direction_is_horizontal(layer) {
            self.get_against_preferred_direction_trace_costs(layer)
        } else {
            self.get_preferred_direction_trace_costs(layer)
        }
    }

    pub fn get_trace_costs(&self) -> Vec<ExpansionCostFactor> {
        (0..self.scoring.preferred_direction_trace_cost.len())
            .map(|i| ExpansionCostFactor {
                horizontal: self.get_horizontal_trace_costs(i),
                vertical: self.get_vertical_trace_costs(i),
            })
            .collect()
    }

    pub fn get_automatic_neckdown(&self) -> bool {
        self.automatic_neckdown
    }

    pub fn get_neck_width_um(&self) -> f64 {
        if self.neck_width_um > 0.0 {
            self.neck_width_um
        } else {
            0.0
        }
    }

    /// Validates the settings (`validateAgainstBoard`), returns the warnings.
    pub fn validate_against_board(&mut self, board: Option<&Board>) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.autorouter.max_passes < 0 || (self.autorouter.max_passes > 9999 && self.autorouter.max_passes != i32::MAX) {
            warnings.push(format!("Invalid maxPasses value: {}, using default 0 (no limit)", self.autorouter.max_passes));
            self.autorouter.max_passes = 0;
        }
        let ap = available_processors();
        if self.max_threads < 0 {
            warnings.push(format!("Invalid maxThreads value: {}, using {}", self.max_threads, default_max_threads()));
            self.max_threads = default_max_threads();
        } else if self.max_threads > ap {
            warnings.push(format!("Invalid maxThreads value: {}, capping at {}", self.max_threads, ap));
            self.max_threads = ap;
        }
        if self.trace_pull_tight_accuracy < 1 {
            warnings.push(format!("Invalid tracePullTightAccuracy value: {}, using default 500", self.trace_pull_tight_accuracy));
            self.trace_pull_tight_accuracy = 500;
        }
        for (i, l) in self.layers.iter_mut().enumerate() {
            if let Some(b) = l.bend_cost {
                if !(MIN_BEND_COST..=MAX_BEND_COST).contains(&b) {
                    let c = b.clamp(MIN_BEND_COST, MAX_BEND_COST);
                    warnings.push(format!("Layer {i} bendCost {b} out of range, clamped to {c}"));
                    l.bend_cost = Some(c);
                }
            }
            if let Some(c) = l.preferred_direction_trace_cost {
                if c < 0.1 {
                    warnings.push(format!("Layer {i} preferredDirectionTraceCost {c} below 0.1, clamped to 0.1"));
                    l.preferred_direction_trace_cost = Some(0.1);
                }
            }
            if let Some(c) = l.undesired_direction_trace_cost {
                if c < 0.1 {
                    warnings.push(format!("Layer {i} undesiredDirectionTraceCost {c} below 0.1, clamped to 0.1"));
                    l.undesired_direction_trace_cost = Some(0.1);
                }
            }
        }
        if let Some(b) = board {
            for name in &self.autorouter.ignore_net_classes {
                if name.trim().is_empty() {
                    continue;
                }
                if !b.rules.net_classes.iter().any(|(_, c)| c.get_name().eq_ignore_ascii_case(name)) {
                    warnings.push(format!("Unknown net class '{name}' specified in ignoreNetClasses (not present on board)"));
                }
            }
        }
        for w in &warnings {
            log::warn!("{w}");
        }
        warnings
    }
}

fn normalize_max_threads(value: Option<i32>) -> i32 {
    let ap = available_processors();
    match value {
        None => default_max_threads(),
        Some(v) if v < 0 => default_max_threads(),
        Some(0) => ap,
        Some(v) => v.min(ap),
    }
}
