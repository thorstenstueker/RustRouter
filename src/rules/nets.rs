//! Nets and net classes.

use super::{DefaultItemClearanceClasses, ViaRuleId};

pub const MAX_LEGAL_NET_NUMBER: i32 = 9_999_999;
pub const HIDDEN_NET_NUMBER: i32 = 10_000_001;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NetClassId(pub usize);

#[derive(Clone, Debug)]
pub struct NetClass {
    name: String,
    trace_half_width: Vec<i32>,
    active_routing_layer: Vec<bool>,
    signal_layers: Vec<bool>,
    pub default_item_clearance_classes: DefaultItemClearanceClasses,
    pub is_ignored_by_autorouter: bool,
    via_rule: Option<ViaRuleId>,
    trace_clearance_class: i32,
    shove_fixed: bool,
    pull_tight: bool,
    ignore_cycles_with_areas: bool,
    minimum_trace_length: f64,
    maximum_trace_length: f64,
}

impl NetClass {
    pub fn new(name: &str, signal_layers: &[bool], ignored_by_autorouter: bool) -> NetClass {
        NetClass {
            name: name.to_string(),
            trace_half_width: vec![0; signal_layers.len()],
            active_routing_layer: signal_layers.to_vec(),
            signal_layers: signal_layers.to_vec(),
            default_item_clearance_classes: DefaultItemClearanceClasses::default(),
            is_ignored_by_autorouter: ignored_by_autorouter,
            via_rule: None,
            trace_clearance_class: 0,
            shove_fixed: false,
            pull_tight: true,
            ignore_cycles_with_areas: false,
            minimum_trace_length: 0.0,
            maximum_trace_length: 0.0,
        }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    pub fn set_trace_half_width(&mut self, value: i32) {
        self.trace_half_width.iter_mut().for_each(|v| *v = value);
    }

    pub fn set_trace_half_width_on_layer(&mut self, layer: usize, value: i32) {
        self.trace_half_width[layer] = value;
    }

    pub fn set_trace_half_width_on_inner(&mut self, value: i32) {
        let n = self.trace_half_width.len();
        for i in 1..n.saturating_sub(1) {
            self.trace_half_width[i] = value;
        }
    }

    pub fn layer_count(&self) -> usize {
        self.trace_half_width.len()
    }

    pub fn get_trace_half_width(&self, layer: usize) -> i32 {
        match self.trace_half_width.get(layer) {
            Some(v) => *v,
            None => {
                log::warn!(" NetClass.get_trace_half_width: layer out of range");
                0
            }
        }
    }

    pub fn get_trace_clearance_class(&self) -> i32 {
        self.trace_clearance_class
    }

    pub fn set_trace_clearance_class(&mut self, c: i32) {
        self.trace_clearance_class = c;
    }

    pub fn get_via_rule(&self) -> Option<ViaRuleId> {
        self.via_rule
    }

    pub fn set_via_rule(&mut self, r: Option<ViaRuleId>) {
        self.via_rule = r;
    }

    pub fn is_shove_fixed(&self) -> bool {
        self.shove_fixed
    }

    pub fn set_shove_fixed(&mut self, v: bool) {
        self.shove_fixed = v;
    }

    pub fn get_pull_tight(&self) -> bool {
        self.pull_tight
    }

    pub fn set_pull_tight(&mut self, v: bool) {
        self.pull_tight = v;
    }

    pub fn get_ignore_cycles_with_areas(&self) -> bool {
        self.ignore_cycles_with_areas
    }

    pub fn set_ignore_cycles_with_areas(&mut self, v: bool) {
        self.ignore_cycles_with_areas = v;
    }

    pub fn get_minimum_trace_length(&self) -> f64 {
        self.minimum_trace_length
    }

    pub fn set_minimum_trace_length(&mut self, v: f64) {
        self.minimum_trace_length = v;
    }

    pub fn get_maximum_trace_length(&self) -> f64 {
        self.maximum_trace_length
    }

    pub fn set_maximum_trace_length(&mut self, v: f64) {
        self.maximum_trace_length = v;
    }

    pub fn is_active_routing_layer(&self, layer: i32) -> bool {
        layer >= 0 && (layer as usize) < self.active_routing_layer.len() && self.active_routing_layer[layer as usize]
    }

    pub fn set_active_routing_layer(&mut self, layer: i32, active: bool) {
        if layer >= 0 && (layer as usize) < self.active_routing_layer.len() {
            self.active_routing_layer[layer as usize] = active;
        }
    }

    pub fn set_all_layers_active(&mut self, v: bool) {
        self.active_routing_layer.iter_mut().for_each(|a| *a = v);
    }

    pub fn set_all_inner_layers_active(&mut self, v: bool) {
        let n = self.active_routing_layer.len();
        for i in 1..n.saturating_sub(1) {
            self.active_routing_layer[i] = v;
        }
    }

    pub fn trace_width_is_layer_dependent(&self) -> bool {
        let c = self.trace_half_width[0];
        (1..self.trace_half_width.len()).any(|i| self.signal_layers[i] && self.trace_half_width[i] != c)
    }

    pub fn trace_width_is_inner_layer_dependent(&self) -> bool {
        let n = self.trace_half_width.len();
        if n <= 3 {
            return false;
        }
        let mut first = 1;
        while first < n - 1 && !self.signal_layers[first] {
            first += 1;
        }
        if first >= n - 1 {
            return false;
        }
        let c = self.trace_half_width[first];
        (first + 1..n - 1).any(|i| self.signal_layers[i] && self.trace_half_width[i] != c)
    }
}

#[derive(Clone, Debug, Default)]
pub struct NetClasses {
    classes: Vec<NetClass>,
}

impl NetClasses {
    pub fn count(&self) -> usize {
        self.classes.len()
    }

    pub fn get(&self, id: NetClassId) -> &NetClass {
        &self.classes[id.0]
    }

    pub fn get_mut(&mut self, id: NetClassId) -> &mut NetClass {
        &mut self.classes[id.0]
    }

    pub fn find_by_name(&self, name: &str) -> Option<NetClassId> {
        self.classes.iter().position(|c| c.name == name).map(NetClassId)
    }

    pub fn append(&mut self, name: &str, signal_layers: &[bool], ignored: bool) -> NetClassId {
        self.classes.push(NetClass::new(name, signal_layers, ignored));
        NetClassId(self.classes.len() - 1)
    }

    pub fn append_unnamed(&mut self, signal_layers: &[bool]) -> NetClassId {
        let mut index = 0;
        let name = loop {
            index += 1;
            let n = format!("class{index}");
            if self.find_by_name(&n).is_none() {
                break n;
            }
        };
        self.append(&name, signal_layers, false)
    }

    pub fn find(&self, trace_half_width: i32, trace_clearance_class: i32, via_rule: Option<ViaRuleId>) -> Option<NetClassId> {
        self.classes
            .iter()
            .position(|c| {
                c.trace_clearance_class == trace_clearance_class
                    && c.via_rule == via_rule
                    && c.trace_half_width.iter().all(|w| *w == trace_half_width)
            })
            .map(NetClassId)
    }

    pub fn find_by_widths(&self, widths: &[i32], trace_clearance_class: i32, via_rule: Option<ViaRuleId>) -> Option<NetClassId> {
        self.classes
            .iter()
            .position(|c| {
                c.trace_clearance_class == trace_clearance_class
                    && c.via_rule == via_rule
                    && c.trace_half_width.as_slice() == widths
            })
            .map(NetClassId)
    }

    pub fn iter(&self) -> impl Iterator<Item = (NetClassId, &NetClass)> {
        self.classes.iter().enumerate().map(|(i, c)| (NetClassId(i), c))
    }
}

#[derive(Clone, Debug)]
pub struct Net {
    pub name: String,
    pub subnet_number: i32,
    pub net_number: i32,
    contains_plane: bool,
    pub net_class: NetClassId,
    min_length: Option<f64>,
    max_length: Option<f64>,
}

impl Net {
    pub fn contains_plane(&self) -> bool {
        self.contains_plane
    }

    pub fn set_contains_plane(&mut self, v: bool) {
        self.contains_plane = v;
    }

    pub fn set_class(&mut self, c: NetClassId) {
        self.net_class = c;
    }

    pub fn set_minimum_trace_length(&mut self, v: f64) {
        self.min_length = Some(v);
    }

    pub fn set_maximum_trace_length(&mut self, v: f64) {
        self.max_length = Some(v);
    }

    pub fn explicit_length_constraint(&self) -> (Option<f64>, Option<f64>) {
        (self.min_length, self.max_length)
    }
}

impl std::fmt::Display for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Net #{} ({})", self.net_number, self.name)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Nets {
    nets: Vec<Net>,
}

impl Nets {
    #[inline]
    pub fn is_normal_net_number(net_no: i32) -> bool {
        net_no > 0 && net_no <= MAX_LEGAL_NET_NUMBER
    }

    pub fn max_net_number(&self) -> i32 {
        self.nets.len() as i32
    }

    pub fn get_by_name_subnet(&self, name: &str, subnet: i32) -> Option<&Net> {
        self.nets
            .iter()
            .find(|n| n.name.eq_ignore_ascii_case(name) && n.subnet_number == subnet)
    }

    pub fn get_by_name(&self, name: &str) -> Vec<&Net> {
        self.nets.iter().filter(|n| n.name.eq_ignore_ascii_case(name)).collect()
    }

    #[inline]
    pub fn get(&self, net_no: i32) -> Option<&Net> {
        if net_no < 1 || net_no as usize > self.nets.len() {
            return None;
        }
        Some(&self.nets[net_no as usize - 1])
    }

    pub fn get_mut(&mut self, net_no: i32) -> Option<&mut Net> {
        if net_no < 1 || net_no as usize > self.nets.len() {
            return None;
        }
        Some(&mut self.nets[net_no as usize - 1])
    }

    /// Adds a new net with the default net class.
    pub fn add(&mut self, name: &str, subnet_number: i32, contains_plane: bool) -> i32 {
        let no = self.nets.len() as i32 + 1;
        if no >= MAX_LEGAL_NET_NUMBER {
            log::warn!("Nets.add_net: maxNetNo out of range");
        }
        self.nets.push(Net {
            name: name.to_string(),
            subnet_number,
            net_number: no,
            contains_plane,
            net_class: NetClassId(0),
            min_length: None,
            max_length: None,
        });
        no
    }

    pub fn iter(&self) -> impl Iterator<Item = &Net> {
        self.nets.iter()
    }
}
