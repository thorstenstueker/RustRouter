//! `AutorouteControl`: the parameters of one autoroute connection.

use super::engine::TRACE_WIDTH_TOLERANCE;
use crate::board::{Board, ItemId};
use crate::geometry::Point;
use crate::optimize::ExpansionCostFactor;
use crate::library::PadstackId;
use crate::rules::{ViaInfo, ViaRuleId};

/// The via rule of a connection as list of via infos (a copy of the net class rule,
/// which the fanout may extend with board vias).
#[derive(Clone, Debug, Default)]
pub struct CtrlViaRule {
    pub name: String,
    pub vias: Vec<ViaInfo>,
}

impl CtrlViaRule {
    pub fn via_count(&self) -> usize {
        self.vias.len()
    }

    pub fn get_via(&self, i: usize) -> &ViaInfo {
        &self.vias[i]
    }

    pub fn contains_padstack(&self, ps: PadstackId) -> bool {
        self.vias.iter().any(|v| v.padstack == ps)
    }

    pub fn contains(&self, v: &ViaInfo) -> bool {
        self.vias.contains(v)
    }
}
use crate::settings::RouterSettings;

#[derive(Clone, Copy, Debug)]
pub struct ViaMask {
    pub from_layer: i32,
    pub to_layer: i32,
    pub attach_smd_allowed: bool,
}

#[derive(Clone, Debug)]
pub struct AutorouteControl {
    pub settings: RouterSettings,
    pub trace_costs: Vec<ExpansionCostFactor>,
    pub bend_costs: Vec<f64>,
    pub with_neckdown: bool,
    pub layer_active: Vec<bool>,
    pub layer_count: usize,
    pub trace_half_width: Vec<i32>,
    pub compensated_trace_half_width: Vec<i32>,
    pub trace_clearance_offset: Vec<i32>,
    pub via_radii: Vec<f64>,
    /// `addViaCosts[from].toLayer[to]`
    pub add_via_costs: Vec<Vec<i32>>,
    pub trace_clearance_class: i32,
    pub vias_allowed: bool,
    pub attach_smd_allowed: bool,
    pub min_normal_via_cost: f64,
    pub ripup_allowed: bool,
    pub ripup_costs: i32,
    pub ripup_pass_no: i32,
    pub is_fanout: bool,
    pub fanout_start_pin_name: Option<String>,
    pub fanout_start_pin_center: Option<Point>,
    pub fanout_start_pin_layer: i32,
    pub ignore_escape_window: bool,
    pub remove_unconnected_vias: bool,
    /// The via rule of the net (a copy, so fanout can extend it).
    pub via_rule: CtrlViaRule,
    pub via_rule_id: Option<ViaRuleId>,
    pub net_no: i32,
    pub via_clearance_class: i32,
    pub via_infos: Vec<ViaMask>,
    pub via_lower_bound: i32,
    pub via_upper_bound: i32,
    pub max_via_radius: f64,
    pub tidy_region_width: i32,
    pub pull_tight_accuracy: i32,
    pub max_shove_trace_recursion_depth: i32,
    pub max_shove_via_recursion_depth: i32,
    pub max_spring_over_recursion_depth: i32,
    pub min_cheap_via_cost: f64,
}

impl AutorouteControl {
    /// `new AutorouteControl(board, netNumber, settings)`
    pub fn new(board: &Board, net_no: i32, settings: &RouterSettings) -> AutorouteControl {
        let costs = settings.get_trace_costs();
        let via_costs = settings.get_via_costs();
        Self::with_costs(board, net_no, settings, via_costs, costs)
    }

    /// `new AutorouteControl(board, netNumber, settings, viaCosts, traceCosts)`
    pub fn with_costs(
        board: &Board,
        net_no: i32,
        settings: &RouterSettings,
        via_costs: i32,
        trace_costs: Vec<ExpansionCostFactor>,
    ) -> AutorouteControl {
        let layer_count = board.layer_count() as usize;
        let mut layer_active = vec![false; layer_count];
        let bend_costs = (0..layer_count).map(|i| settings.get_bend_cost(i)).collect();
        for (i, la) in layer_active.iter_mut().enumerate() {
            let active = settings.get_layer_active(i);
            if !board.layer_structure.layers[i].is_signal && active {
                log::warn!(
                    "Layer '{}' is a dedicated power plane and cannot be routed. Forcing active state to false.",
                    board.layer_structure.layers[i].name
                );
                *la = false;
            } else {
                *la = active;
            }
        }
        let mut ctrl = AutorouteControl {
            settings: settings.clone(),
            trace_costs,
            bend_costs,
            with_neckdown: settings.get_automatic_neckdown(),
            layer_active,
            layer_count,
            trace_half_width: vec![0; layer_count],
            compensated_trace_half_width: vec![0; layer_count],
            trace_clearance_offset: vec![0; layer_count],
            via_radii: vec![0.0; layer_count],
            add_via_costs: vec![vec![0; layer_count]; layer_count],
            trace_clearance_class: 1,
            vias_allowed: settings.vias_allowed,
            attach_smd_allowed: false,
            min_normal_via_cost: 0.0,
            ripup_allowed: false,
            ripup_costs: 1000,
            ripup_pass_no: 1,
            is_fanout: false,
            fanout_start_pin_name: None,
            fanout_start_pin_center: None,
            fanout_start_pin_layer: -1,
            ignore_escape_window: false,
            remove_unconnected_vias: true,
            via_rule: CtrlViaRule::default(),
            via_rule_id: None,
            net_no,
            via_clearance_class: 1,
            via_infos: Vec::new(),
            via_lower_bound: 0,
            via_upper_bound: layer_count as i32,
            max_via_radius: 0.0,
            tidy_region_width: i32::MAX,
            pull_tight_accuracy: 500,
            max_shove_trace_recursion_depth: 20,
            max_shove_via_recursion_depth: 8,
            max_spring_over_recursion_depth: 8,
            min_cheap_via_cost: 0.0,
        };
        ctrl.init_net(net_no, board, via_costs);
        ctrl
    }

    fn init_net(&mut self, net_no: i32, board: &Board, via_costs: i32) {
        self.net_no = net_no;
        let net_class = board.rules.net_class_of(net_no).cloned();
        match &net_class {
            Some(c) => {
                self.trace_clearance_class = c.get_trace_clearance_class();
                self.via_rule_id = c.get_via_rule();
            }
            None => {
                self.trace_clearance_class = 1;
                self.via_rule_id = if board.rules.via_rules.is_empty() { None } else { Some(ViaRuleId(0)) };
            }
        }
        self.via_rule = match self.via_rule_id {
            Some(id) => {
                let r = board.rules.via_rule(id);
                CtrlViaRule {
                    name: r.name.clone(),
                    vias: r.vias().iter().map(|v| board.rules.via_infos.get(*v).clone()).collect(),
                }
            }
            None => CtrlViaRule::default(),
        };
        let cm = &board.rules.clearance_matrix;
        for i in 0..self.layer_count {
            self.trace_half_width[i] = if net_no > 0 {
                board.rules.get_trace_half_width(net_no, i)
            } else {
                board.rules.get_trace_half_width(1, i)
            };
            self.compensated_trace_half_width[i] =
                self.trace_half_width[i] + cm.clearance_compensation_value(self.trace_clearance_class, i as i32);
            let safety_margin = cm.get_value(self.trace_clearance_class, self.trace_clearance_class, i as i32, true)
                - cm.get_value(self.trace_clearance_class, self.trace_clearance_class, i as i32, false);
            self.trace_clearance_offset[i] = self.compensated_trace_half_width[i] + TRACE_WIDTH_TOLERANCE + safety_margin;
            if let Some(c) = &net_class {
                if !c.is_active_routing_layer(i as i32) {
                    self.layer_active[i] = false;
                }
            }
        }
        self.rebuild_via_info(board, via_costs, net_no);
    }

    pub fn get_trace_clearance_offset(&self, layer: i32) -> i32 {
        if layer < 0 || layer as usize >= self.layer_count {
            return 0;
        }
        self.trace_clearance_offset[layer as usize]
    }

    pub fn update_trace_clearance_offset(&mut self, layer: i32, board: &Board) {
        if layer >= 0 && (layer as usize) < self.layer_count {
            let cm = &board.rules.clearance_matrix;
            let safety_margin = cm.get_value(self.trace_clearance_class, self.trace_clearance_class, layer, true)
                - cm.get_value(self.trace_clearance_class, self.trace_clearance_class, layer, false);
            self.trace_clearance_offset[layer as usize] =
                self.compensated_trace_half_width[layer as usize] + TRACE_WIDTH_TOLERANCE + safety_margin;
        }
    }

    fn is_pure_smd_net(board: &Board, net_no: i32) -> bool {
        let items: Vec<ItemId> = board.get_connectable_items(net_no);
        if items.is_empty() {
            return false;
        }
        let lc = board.layer_count();
        items.iter().all(|id| {
            let it = board.item(*id);
            it.is_pin() && it.first_layer() == it.last_layer_with(lc)
        })
    }

    fn has_smd_pin(board: &Board, net_no: i32) -> bool {
        let lc = board.layer_count();
        board.get_connectable_items(net_no).iter().any(|id| {
            let it = board.item(*id);
            it.is_pin() && it.first_layer() == it.last_layer_with(lc)
        })
    }

    pub fn rebuild_via_info(&mut self, board: &Board, via_costs: i32, net_no: i32) {
        self.via_clearance_class = if self.via_rule.via_count() > 0 {
            self.via_rule.get_via(0).clearance_class
        } else {
            1
        };
        self.via_infos = Vec::with_capacity(self.via_rule.via_count());
        self.attach_smd_allowed = false;
        for i in 0..self.via_rule.via_count() {
            let v = self.via_rule.get_via(i).clone();
            if v.attach_smd_allowed {
                self.attach_smd_allowed = true;
            }
            let ps = board.library.padstacks.get(v.padstack);
            let from = ps.from_layer();
            let to = ps.to_layer();
            for j in from..=to {
                let r = ps.get_shape(j).map(|s| 0.5 * s.max_width()).unwrap_or(0.0);
                self.via_radii[j as usize] = self.via_radii[j as usize].max(r);
            }
            self.via_infos.push(ViaMask {
                from_layer: from,
                to_layer: to,
                attach_smd_allowed: v.attach_smd_allowed,
            });
        }
        let pure_smd_net = Self::is_pure_smd_net(board, net_no);
        let has_smd = Self::has_smd_pin(board, net_no);
        if !self.attach_smd_allowed && self.layer_count > 1 && has_smd {
            self.attach_smd_allowed = true;
            for m in self.via_infos.iter_mut() {
                m.attach_smd_allowed = true;
            }
        }
        for j in 0..self.layer_count {
            self.via_radii[j] = self.via_radii[j].max(self.trace_half_width[j] as f64);
            self.max_via_radius = self.max_via_radius.max(self.via_radii[j]);
        }
        let mut via_cost_factor = self.max_via_radius.max(1.0);
        if pure_smd_net {
            via_cost_factor *= self.settings.autorouter.smd_net_via_cost_factor;
        }
        self.min_normal_via_cost = via_costs as f64 * via_cost_factor;
        self.min_cheap_via_cost = 0.8 * self.min_normal_via_cost;
    }

    /// `AutorouteControl.neckHalfWidth(board, settings)`
    pub fn neck_half_width(board: &Board, settings: &RouterSettings) -> i32 {
        let neck_width_um = settings.get_neck_width_um();
        if neck_width_um <= 0.0 {
            return board.rules.get_min_allowed_trace_half_width();
        }
        let resolution = board.communication.resolution.max(1);
        let neck_width = crate::geometry::basics::java_round(crate::board::structure::Unit::scale(
            neck_width_um * resolution as f64,
            crate::board::structure::Unit::Um,
            board.communication.unit,
        ));
        (1.max(neck_width / 2)).max(board.rules.get_min_allowed_trace_half_width())
    }
}
