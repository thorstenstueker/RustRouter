//! Board design rules (port of `app.freerouting.rules`).

mod clearance_matrix;
mod nets;
mod vias;

pub use clearance_matrix::{ClearanceMatrix, CLEARANCE_SAFETY_MARGIN};
pub use nets::{Net, NetClass, NetClassId, NetClasses, Nets, HIDDEN_NET_NUMBER, MAX_LEGAL_NET_NUMBER};
pub use vias::{ViaInfo, ViaInfoId, ViaInfos, ViaRule, ViaRuleId};

use crate::board::LayerStructure;
use crate::library::Padstacks;

/// Restriction of trace angles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AngleRestriction {
    None,
    #[default]
    FortyfiveDegree,
    NinetyDegree,
}

/// Item classes for default clearance class assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemClass {
    None = 0,
    Trace = 1,
    Via = 2,
    Pin = 3,
    Smd = 4,
    Area = 5,
}

impl ItemClass {
    pub const ALL: [ItemClass; 6] = [
        ItemClass::None,
        ItemClass::Trace,
        ItemClass::Via,
        ItemClass::Pin,
        ItemClass::Smd,
        ItemClass::Area,
    ];
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultItemClearanceClasses {
    classes: [i32; 6],
}

impl Default for DefaultItemClearanceClasses {
    fn default() -> Self {
        let mut r = DefaultItemClearanceClasses { classes: [0; 6] };
        r.set_all(1);
        r
    }
}

impl DefaultItemClearanceClasses {
    pub fn get(&self, c: ItemClass) -> i32 {
        self.classes[c as usize]
    }

    pub fn set(&mut self, c: ItemClass, index: i32) {
        self.classes[c as usize] = index;
    }

    pub fn set_all(&mut self, index: i32) {
        for i in 1..self.classes.len() {
            self.classes[i] = index;
        }
    }
}

/// The rules of a board.
#[derive(Clone, Debug)]
pub struct BoardRules {
    pub clearance_matrix: ClearanceMatrix,
    pub nets: Nets,
    pub via_infos: ViaInfos,
    pub via_rules: Vec<ViaRule>,
    pub net_classes: NetClasses,
    layer_count: usize,
    signal_layers: Vec<bool>,
    trace_angle_restriction: AngleRestriction,
    ignore_conduction: bool,
    min_trace_half_width: i32,
    max_trace_half_width: i32,
    min_allowed_trace_half_width: i32,
    pin_edge_to_turn_dist: f64,
    use_slow_autoroute_algorithm: bool,
    hole_clearance: i32,
    pub clearance_tolerance_um: f64,
}

impl BoardRules {
    pub fn new(layer_structure: &LayerStructure, clearance_matrix: ClearanceMatrix) -> BoardRules {
        let mut r = BoardRules {
            clearance_matrix,
            nets: Nets::default(),
            via_infos: ViaInfos::default(),
            via_rules: Vec::new(),
            net_classes: NetClasses::default(),
            layer_count: layer_structure.layers.len(),
            signal_layers: layer_structure.layers.iter().map(|l| l.is_signal).collect(),
            trace_angle_restriction: AngleRestriction::FortyfiveDegree,
            ignore_conduction: true,
            min_trace_half_width: 100000,
            max_trace_half_width: 100,
            min_allowed_trace_half_width: 0,
            pin_edge_to_turn_dist: 0.0,
            use_slow_autoroute_algorithm: false,
            hole_clearance: 0,
            clearance_tolerance_um: 1.0,
        };
        // In the original the default net class is created lazily on first access;
        // the parser accesses it before appending other classes, so it is always class 0.
        r.create_default_net_class();
        r
    }

    pub const fn default_clearance_class() -> i32 {
        1
    }

    pub const fn clearance_class_none() -> i32 {
        0
    }

    pub fn layer_count(&self) -> usize {
        self.layer_count
    }

    pub fn net_class_of(&self, net_no: i32) -> Option<&NetClass> {
        let net = self.nets.get(net_no)?;
        Some(self.net_classes.get(net.net_class))
    }

    pub fn get_trace_half_width(&self, net_no: i32, layer: usize) -> i32 {
        match self.net_class_of(net_no) {
            Some(c) => c.get_trace_half_width(layer),
            None => self.get_default_net_class().get_trace_half_width(layer),
        }
    }

    pub fn trace_widths_are_layer_dependent(&self, net_no: i32) -> bool {
        let compare = self.get_trace_half_width(net_no, 0);
        (1..self.layer_count).any(|i| self.get_trace_half_width(net_no, i) != compare)
    }

    pub fn get_min_trace_half_width(&self) -> i32 {
        self.min_trace_half_width
    }

    pub fn get_max_trace_half_width(&self) -> i32 {
        self.max_trace_half_width
    }

    pub fn get_min_allowed_trace_half_width(&self) -> i32 {
        self.min_allowed_trace_half_width
    }

    pub fn set_min_allowed_trace_half_width(&mut self, v: i32) {
        self.min_allowed_trace_half_width = v.max(0);
    }

    pub fn get_hole_clearance(&self) -> i32 {
        self.hole_clearance
    }

    pub fn set_hole_clearance(&mut self, v: i32) {
        self.hole_clearance = v.max(0);
    }

    pub fn set_default_trace_half_width(&mut self, layer: usize, value: i32) {
        self.get_default_net_class_mut().set_trace_half_width_on_layer(layer, value);
        self.min_trace_half_width = self.min_trace_half_width.min(value);
        self.max_trace_half_width = self.max_trace_half_width.max(value);
    }

    pub fn get_default_trace_half_width(&self, layer: usize) -> i32 {
        self.get_default_net_class().get_trace_half_width(layer)
    }

    pub fn set_default_trace_half_widths(&mut self, value: i32) {
        if value <= 0 {
            log::warn!("BoardRules.set_trace_half_widths: value out of range");
            return;
        }
        self.get_default_net_class_mut().set_trace_half_width(value);
        self.min_trace_half_width = self.min_trace_half_width.min(value);
        self.max_trace_half_width = self.max_trace_half_width.max(value);
    }

    fn ensure_default_net_class(&mut self) {
        if self.net_classes.count() == 0 {
            self.create_default_net_class();
        }
    }

    pub fn get_default_net_class(&self) -> &NetClass {
        // The default net class is created when the rules are built.
        self.net_classes.get(NetClassId(0))
    }

    pub fn get_default_net_class_mut(&mut self) -> &mut NetClass {
        self.ensure_default_net_class();
        self.net_classes.get_mut(NetClassId(0))
    }

    pub fn create_default_net_class(&mut self) {
        let id = self.net_classes.append("default", &self.signal_layers, false);
        let c = self.net_classes.get_mut(id);
        c.set_trace_half_width(1500);
        c.set_trace_clearance_class(1);
    }

    pub fn get_new_net_class(&mut self, name: Option<&str>) -> NetClassId {
        self.ensure_default_net_class();
        let def_cl = self.get_default_net_class().get_trace_clearance_class();
        let def_hw = self.get_default_net_class().get_trace_half_width(0);
        let def_via = self.get_default_via_rule();
        let id = match name {
            Some(n) => self.net_classes.append(n, &self.signal_layers, false),
            None => self.net_classes.append_unnamed(&self.signal_layers),
        };
        let c = self.net_classes.get_mut(id);
        c.set_trace_clearance_class(def_cl);
        c.set_via_rule(def_via);
        c.set_trace_half_width(def_hw);
        id
    }

    /// Appends a net class with the given name, or returns the existing one.
    pub fn append_net_class(&mut self, name: &str) -> NetClassId {
        if let Some(id) = self.net_classes.find_by_name(name) {
            return id;
        }
        self.ensure_default_net_class();
        let default_class = self.net_classes.get(NetClassId(0)).clone();
        let id = self.net_classes.append(name, &self.signal_layers, false);
        let c = self.net_classes.get_mut(id);
        c.default_item_clearance_classes = default_class.default_item_clearance_classes.clone();
        c.set_via_rule(default_class.get_via_rule());
        c.set_trace_half_width(default_class.get_trace_half_width(0));
        c.set_trace_clearance_class(default_class.get_trace_clearance_class());
        id
    }

    /// Creates a via rule from all vias with the default via clearance class of the net class.
    pub fn create_default_via_rule(&mut self, net_class: NetClassId, name: &str, padstacks: &Padstacks) {
        if self.via_infos.count() == 0 {
            return;
        }
        let mut rule = ViaRule::new(name);
        let default_via_cl = self
            .net_classes
            .get(net_class)
            .default_item_clearance_classes
            .get(ItemClass::Via);
        for i in 0..self.via_infos.count() {
            let vid = ViaInfoId(i);
            let info = self.via_infos.get(vid);
            if info.clearance_class == default_via_cl {
                let ps = padstacks.get(info.padstack);
                let (from, to) = (ps.from_layer(), ps.to_layer());
                match rule.get_layer_range(&self.via_infos, padstacks, from, to) {
                    Some(existing) => {
                        let new_shape = ps.get_shape(from);
                        let ex_shape = padstacks
                            .get(self.via_infos.get(existing).padstack)
                            .get_shape(from);
                        if let (Some(n), Some(e)) = (new_shape, ex_shape) {
                            if n.max_width() < e.max_width() {
                                rule.remove_via(existing);
                                rule.append_via(vid);
                            }
                        }
                    }
                    None => rule.append_via(vid),
                }
            }
        }
        self.via_rules.push(rule);
        let rid = ViaRuleId(self.via_rules.len() - 1);
        self.net_classes.get_mut(net_class).set_via_rule(Some(rid));
    }

    pub fn get_default_via_rule(&self) -> Option<ViaRuleId> {
        if self.via_rules.is_empty() {
            None
        } else {
            Some(ViaRuleId(0))
        }
    }

    pub fn get_via_rule_by_name(&self, name: &str) -> Option<ViaRuleId> {
        self.via_rules.iter().position(|r| r.name == name).map(ViaRuleId)
    }

    pub fn via_rule(&self, id: ViaRuleId) -> &ViaRule {
        &self.via_rules[id.0]
    }

    pub fn get_pin_edge_to_turn_dist(&self) -> f64 {
        self.pin_edge_to_turn_dist
    }

    pub fn set_pin_edge_to_turn_dist(&mut self, v: f64) {
        self.pin_edge_to_turn_dist = v;
    }

    pub fn get_ignore_conduction(&self) -> bool {
        self.ignore_conduction
    }

    pub fn set_ignore_conduction(&mut self, v: bool) {
        self.ignore_conduction = v;
    }

    pub fn get_trace_angle_restriction(&self) -> AngleRestriction {
        self.trace_angle_restriction
    }

    pub fn set_trace_angle_restriction(&mut self, a: AngleRestriction) {
        self.trace_angle_restriction = a;
    }

    pub fn get_use_slow_autoroute_algorithm(&self) -> bool {
        self.use_slow_autoroute_algorithm
    }

    pub fn set_use_slow_autoroute_algorithm(&mut self, v: bool) {
        self.use_slow_autoroute_algorithm = v;
    }

    pub fn get_default_via_diameter(&self, padstacks: &Padstacks) -> f64 {
        let Some(rule) = self.get_default_via_rule() else {
            return 0.0;
        };
        let rule = self.via_rule(rule);
        if rule.via_count() == 0 {
            return 0.0;
        }
        let ps = padstacks.get(self.via_infos.get(rule.get_via(0)).padstack);
        let mut result = ps.get_shape(ps.from_layer()).map(|s| s.max_width()).unwrap_or(0.0);
        if let Some(s) = ps.get_shape(ps.to_layer()) {
            result = result.max(s.max_width());
        }
        result
    }
}
