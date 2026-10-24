//! The network scope (port of `parser/Network`, `Net`, `NetList` and `NetClass`).

use super::lexer::{Kw, LexState, Lexer, Tok};
use super::placement::ComponentLocation;
use super::reader::ReadCtx;
use super::scopes::{
    read_circuit_scope, read_layer_rule_scope, read_on_off_scope, read_rule_scope, read_string_list_scope,
    read_string_scope, CircuitResult, LayerRule, Rule,
};
use super::shapes::{dsn_to_board_point, round_i, DsnLayerStructure};
use super::structure::contains_wire_clearance_pair;
use crate::board::{Board, CoordinateTransform, FixedState, ObstacleKind};
use crate::geometry::{Area, PolylineArea, Shape, Vector};
use crate::library::{PadstackId, PartPin};
use crate::rules::{ClearanceMatrix, ItemClass, NetClassId, ViaInfo, ViaRule, ViaRuleId};
use std::collections::{BTreeMap, BTreeSet};

/// Removes all occurrences of `.` followed by digits (`replaceAll("\\.\\d+", "")`).
pub fn strip_dot_digits(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut r = String::with_capacity(s.len());
    let mut i = 0;
    while i < c.len() {
        if c[i] == '.' && i + 1 < c.len() && c[i + 1].is_ascii_digit() {
            i += 1;
            while i < c.len() && c[i].is_ascii_digit() {
                i += 1;
            }
            continue;
        }
        r.push(c[i]);
        i += 1;
    }
    r
}

/// `String.split(regex)` of Java for a literal separator (trailing empty strings removed).
pub fn java_split(s: &str, sep: &str) -> Vec<String> {
    if s.is_empty() {
        return vec![String::new()];
    }
    let mut parts: Vec<String> = s.split(sep).map(|x| x.to_string()).collect();
    while parts.last().map(|p| p.is_empty()).unwrap_or(false) {
        parts.pop();
    }
    parts
}

// ------------------------------------------------------------------ net list

/// `Net.Id`: sorted by name and subnet number.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NetId {
    pub name: String,
    pub subnet_number: i32,
}

impl NetId {
    pub fn new(name: &str, subnet_number: i32) -> NetId {
        NetId {
            name: name.to_string(),
            subnet_number,
        }
    }
}

/// `Net.Pin`: sorted tuple of component name and pin name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NetPin {
    pub component_name: String,
    pub pin_name: String,
}

/// `NetList`: the nets of the DSN file sorted by their ids.
#[derive(Default)]
pub struct NetList {
    nets: BTreeMap<NetId, Option<BTreeSet<NetPin>>>,
}

impl NetList {
    pub fn contains(&self, id: &NetId) -> bool {
        self.nets.contains_key(id)
    }

    /// Adds a net. Returns false, if it exists already.
    pub fn add_net(&mut self, id: NetId) -> bool {
        if self.nets.contains_key(&id) {
            return false;
        }
        self.nets.insert(id, None);
        true
    }

    pub fn set_pins(&mut self, id: &NetId, pins: impl IntoIterator<Item = NetPin>) {
        if let Some(p) = self.nets.get_mut(id) {
            *p = Some(pins.into_iter().collect());
        }
    }

    /// All nets containing the pin.
    pub fn get_nets(&self, component: &str, pin: &str) -> Vec<&NetId> {
        let search = NetPin {
            component_name: component.to_string(),
            pin_name: pin.to_string(),
        };
        self.nets
            .iter()
            .filter(|(_, p)| p.as_ref().map(|s| s.contains(&search)).unwrap_or(false))
            .map(|(id, _)| id)
            .collect()
    }
}

// ------------------------------------------------------------------ DSN net classes

/// The information of a DSN class scope (`parser/NetClass`).
pub struct DsnNetClass {
    pub name: String,
    pub trace_clearance_class: Option<String>,
    pub net_list: Vec<String>,
    pub rules: Vec<Rule>,
    pub layer_rules: Vec<LayerRule>,
    pub use_via: Vec<String>,
    pub use_layer: Vec<String>,
    pub via_rule: Option<String>,
    pub shove_fixed: bool,
    pub pull_tight: bool,
    pub min_trace_length: f64,
    pub max_trace_length: f64,
    pub circuit: Option<CircuitResult>,
}

pub(super) fn read_net_class_scope(lx: &mut Lexer) -> Option<DsnNetClass> {
    lx.yybegin(LexState::Name);
    let name = lx.next_string();
    let net_list = lx.next_string_list();
    let mut c = DsnNetClass {
        name,
        trace_clearance_class: None,
        net_list,
        rules: Vec::new(),
        layer_rules: Vec::new(),
        use_via: Vec::new(),
        use_layer: Vec::new(),
        via_rule: None,
        shove_fixed: false,
        pull_tight: true,
        min_trace_length: 0.0,
        max_trace_length: 0.0,
        circuit: None,
    };
    let mut next = lx.next_token();
    if next == Tok::Close {
        return Some(c);
    }
    let mut prev = next.clone();
    loop {
        next = lx.next_token();
        if next == Tok::Eof {
            log::warn!("NetClass.read_scope: unexpected end of file at '{}'", lx.scope_identifier);
            return None;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            match next {
                Tok::Kw(Kw::Rule) => c.rules.extend(read_rule_scope(lx)),
                Tok::Kw(Kw::LayerRule) => match read_layer_rule_scope(lx) {
                    Some(r) => c.layer_rules.push(r),
                    None => {
                        lx.set_fatal("NullPointerException in Network.insertNetClass");
                        return None;
                    }
                },
                Tok::Kw(Kw::ViaRule) => c.via_rule = Some(read_string_scope(lx)),
                Tok::Kw(Kw::Circuit) => {
                    if let Some(r) = read_circuit_scope(lx) {
                        c.max_trace_length = r.max_length;
                        c.min_trace_length = r.min_length;
                        c.use_via.extend(r.use_via.iter().cloned());
                        c.use_layer.extend(r.use_layer.iter().cloned());
                        c.circuit = Some(r);
                    }
                }
                Tok::Kw(Kw::ClearanceClass) => c.trace_clearance_class = Some(read_string_scope(lx)),
                Tok::Kw(Kw::ShoveFixed) => c.shove_fixed = read_on_off_scope(lx),
                Tok::Kw(Kw::PullTight) => c.pull_tight = read_on_off_scope(lx),
                _ => {
                    lx.skip_scope();
                }
            }
        }
        prev = next.clone();
    }
    Some(c)
}

/// `NetClass.ClassClass`.
pub struct ClassClass {
    pub class_names: Vec<String>,
    pub rules: Vec<Rule>,
    pub layer_rules: Vec<LayerRule>,
}

fn read_class_class_scope(lx: &mut Lexer) -> Option<ClassClass> {
    let mut cc = ClassClass {
        class_names: Vec::new(),
        rules: Vec::new(),
        layer_rules: Vec::new(),
    };
    let mut prev = Tok::Eof;
    loop {
        let next = lx.next_token();
        if next == Tok::Eof {
            log::warn!("ClassClass.read_scope: unexpected end of file at '{}'", lx.scope_identifier);
            return None;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            if next.is_kw(Kw::Classes) {
                match read_string_list_scope(lx) {
                    Some(l) => cc.class_names.extend(l),
                    None => {
                        lx.set_fatal("NullPointerException in NetClass.readClassClassScope");
                        return None;
                    }
                }
            } else if next.is_kw(Kw::Rule) {
                cc.rules.extend(read_rule_scope(lx));
            } else if next.is_kw(Kw::LayerRule) {
                match read_layer_rule_scope(lx) {
                    Some(r) => cc.layer_rules.push(r),
                    None => {
                        lx.set_fatal("NullPointerException in Network.insertClassPairs");
                        return None;
                    }
                }
            }
        }
        prev = next;
    }
    Some(cc)
}

/// `KiCadNetClassNames.isKiCadDefaultNetClassName`.
fn is_kicad_default_net_class_name(name: &str) -> bool {
    !name.is_empty() && (name.eq_ignore_ascii_case("default") || name.eq_ignore_ascii_case("kicad_default"))
}

// ------------------------------------------------------------------ reading

fn read_net_pins(lx: &mut Lexer, pins: &mut Vec<NetPin>) -> bool {
    loop {
        let component = lx.next_string_ext(true, '-');
        if component.is_empty() {
            break;
        }
        lx.yybegin(LexState::SpecChar);
        lx.next_token(); // overread the hyphen
        let pin = lx.next_string_nl(true);
        pins.push(NetPin {
            component_name: component,
            pin_name: pin,
        });
    }
    let t = lx.next_token();
    if t == Tok::Eof {
        log::warn!("Network.read_net_pins: unexpected end of file at '{}'", lx.scope_identifier);
        return false;
    }
    if t != Tok::Close {
        log::warn!("Network.read_net_pins: expected closed bracket is missing at '{}'", lx.scope_identifier);
    }
    true
}

pub(super) fn read_via_info(lx: &mut Lexer, board: &mut Board) -> Option<ViaInfo> {
    lx.yybegin(LexState::Name);
    let Tok::Str(name) = lx.next_token() else {
        log::warn!("Network.read_via_info: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    lx.yybegin(LexState::Name);
    let Tok::Str(padstack_name) = lx.next_token() else {
        log::warn!("Network.read_via_info: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    lx.scope_identifier = padstack_name.clone();
    let cleaned = strip_dot_digits(&padstack_name);
    let mut ps = board
        .library
        .get_via_padstack_by_name(&padstack_name)
        .or_else(|| board.library.get_via_padstack_by_name(&cleaned));
    if ps.is_none() {
        let found = board
            .library
            .padstacks
            .get_by_name(&padstack_name)
            .or_else(|| board.library.padstacks.get_by_name(&cleaned));
        let Some(found) = found else {
            log::warn!("Network.read_via_info: padstack not found at '{}'", lx.scope_identifier);
            return None;
        };
        board.library.add_via_padstack(found);
        ps = Some(found);
    }
    lx.yybegin(LexState::Name);
    let Tok::Str(cl_name) = lx.next_token() else {
        log::warn!("Network.read_via_info: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    let mut cl = board.rules.clearance_matrix.get_no(&cl_name);
    if cl < 0 {
        cl = crate::rules::BoardRules::default_clearance_class();
    }
    let mut attach = false;
    let t = lx.next_token();
    if t != Tok::Close {
        if !t.is_kw(Kw::Attach) {
            log::warn!("Network.read_via_info: Keyword.ATTACH expected at '{}'", lx.scope_identifier);
            return None;
        }
        attach = true;
        if lx.next_token() != Tok::Close {
            log::warn!("Network.read_via_info: closing bracket expected at '{}'", lx.scope_identifier);
            return None;
        }
    }
    Some(ViaInfo {
        name,
        padstack: ps.unwrap(),
        clearance_class: cl,
        attach_smd_allowed: attach,
    })
}

pub(super) fn read_via_rule(lx: &mut Lexer) -> Option<Vec<String>> {
    let mut result = Vec::new();
    loop {
        lx.yybegin(LexState::Name);
        match lx.next_token() {
            Tok::Close => break,
            Tok::Str(s) => result.push(s),
            _ => {
                log::warn!("Network.read_via_rule: string expected at '{}'", lx.scope_identifier);
                return None;
            }
        }
    }
    Some(result)
}

fn create_ordered_subnets(pins: &[NetPin]) -> Vec<Vec<NetPin>> {
    let mut result = Vec::new();
    for w in pins.windows(2) {
        let set: BTreeSet<NetPin> = w.iter().cloned().collect();
        result.push(set.into_iter().collect());
    }
    result
}

fn read_net_scope(ctx: &mut ReadCtx) -> bool {
    let net_name = ctx.lx.next_string();
    let mut subnet_number = 1;
    let mut next = ctx.lx.next_token();
    let scope_is_empty = next == Tok::Close;
    if let Tok::Int(i) = next {
        subnet_number = i;
    }
    let mut pin_order_found = false;
    let mut pin_list: Vec<NetPin> = Vec::new();
    let mut net_rules: Vec<Rule> = Vec::new();
    let mut circuit: Option<CircuitResult> = None;
    let mut subnet_pin_lists: Vec<Vec<NetPin>> = Vec::new();
    if !scope_is_empty {
        loop {
            let prev = std::mem::replace(&mut next, ctx.lx.next_token());
            if next == Tok::Eof {
                log::warn!("Network.read_net_scope: unexpected end of file at '{}'", ctx.lx.scope_identifier);
                return false;
            }
            if next == Tok::Close {
                break;
            }
            if prev == Tok::Open {
                match next {
                    Tok::Kw(Kw::Pins) => {
                        if !read_net_pins(&mut ctx.lx, &mut pin_list) {
                            return false;
                        }
                    }
                    Tok::Kw(Kw::Order) => {
                        pin_order_found = true;
                        if !read_net_pins(&mut ctx.lx, &mut pin_list) {
                            return false;
                        }
                    }
                    Tok::Kw(Kw::Fromto) => {
                        let mut l = Vec::new();
                        if !read_net_pins(&mut ctx.lx, &mut l) {
                            return false;
                        }
                        let set: BTreeSet<NetPin> = l.into_iter().collect();
                        subnet_pin_lists.push(set.into_iter().collect());
                    }
                    Tok::Kw(Kw::Rule) => net_rules.extend(read_rule_scope(&mut ctx.lx)),
                    Tok::Kw(Kw::Circuit) => {
                        if let Some(c) = read_circuit_scope(&mut ctx.lx) {
                            circuit = Some(c);
                        }
                    }
                    Tok::Kw(Kw::LayerRule) => {
                        log::warn!(
                            "Network.read_net_scope: layer_rule not yet implemented at '{}'",
                            ctx.lx.scope_identifier
                        );
                        ctx.lx.skip_scope();
                    }
                    _ => {
                        ctx.lx.skip_scope();
                    }
                }
            }
        }
    }
    if subnet_pin_lists.is_empty() {
        if pin_order_found {
            subnet_pin_lists = create_ordered_subnets(&pin_list);
        } else {
            subnet_pin_lists.push(pin_list);
        }
    }
    let ct = ctx.ct();
    for pins in subnet_pin_lists {
        let net_id = NetId::new(&net_name, subnet_number);
        if !ctx.netlist.contains(&net_id) && ctx.netlist.add_net(net_id.clone()) {
            let contains_plane = ctx.layer_structure.as_ref().map(|l| l.contains_plane(&net_name)).unwrap_or(false);
            ctx.board.as_mut().unwrap().rules.nets.add(&net_name, subnet_number, contains_plane);
        }
        ctx.netlist.set_pins(&net_id, pins);
        let board = ctx.board.as_mut().unwrap();
        let board_net = board.rules.nets.get_by_name_subnet(&net_name, subnet_number).map(|n| n.net_number);
        if let (Some(net_no), Some(c)) = (board_net, circuit.as_ref()) {
            let max = if c.max_length > 0.0 { ct.dsn_to_board(c.max_length) } else { 0.0 };
            let min = if c.min_length > 0.0 { ct.dsn_to_board(c.min_length) } else { 0.0 };
            if max > 0.0 || min > 0.0 {
                let n = board.rules.nets.get_mut(net_no).unwrap();
                n.set_minimum_trace_length(min);
                n.set_maximum_trace_length(max);
            }
        }
        if !net_rules.is_empty() {
            let Some(net_no) = board_net else {
                log::warn!("Network.read_net_scope: board net not found at '{}'", ctx.lx.scope_identifier);
                return false;
            };
            for r in &net_rules {
                match r {
                    Rule::Width(w) => {
                        let hw = round_i(ct.dsn_to_board(*w) / 2.0);
                        let def = board.rules.get_default_net_class();
                        let (def_cl, def_via) = (def.get_trace_clearance_class(), def.get_via_rule());
                        let class = match board.rules.net_classes.find(hw, def_cl, def_via) {
                            Some(c) => c,
                            None => board.rules.get_new_net_class(None),
                        };
                        board.rules.net_classes.get_mut(class).set_trace_half_width(hw);
                        board.rules.nets.get_mut(net_no).unwrap().set_class(class);
                    }
                    Rule::Length { max, min } => {
                        let max = if *max > 0.0 { ct.dsn_to_board(*max) } else { 0.0 };
                        let min = if *min > 0.0 { ct.dsn_to_board(*min) } else { 0.0 };
                        if max > 0.0 || min > 0.0 {
                            let n = board.rules.nets.get_mut(net_no).unwrap();
                            n.set_minimum_trace_length(min);
                            n.set_maximum_trace_length(max);
                        }
                    }
                    Rule::Meander(_) => {}
                    _ => log::warn!(
                        "Network.read_net_scope: Rule not yet implemented at '{}'",
                        ctx.lx.scope_identifier
                    ),
                }
            }
        }
        subnet_number += 1;
    }
    true
}

/// `Network.readScope`.
pub fn read_network_scope(ctx: &mut ReadCtx) -> bool {
    if ctx.board.is_none() {
        return ctx.fatal("NullPointerException in Network.readScope: board missing");
    }
    let mut classes: Vec<DsnNetClass> = Vec::new();
    let mut class_classes: Vec<ClassClass> = Vec::new();
    let mut via_infos: Vec<ViaInfo> = Vec::new();
    let mut via_rules: Vec<Vec<String>> = Vec::new();
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, ctx.lx.next_token());
        if next == Tok::Eof {
            log::warn!("Network.read_scope: unexpected end of file at '{}'", ctx.lx.scope_identifier);
            return false;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            match next {
                Tok::Kw(Kw::Net) => {
                    read_net_scope(ctx);
                }
                Tok::Kw(Kw::Via) => match read_via_info(&mut ctx.lx, ctx.board.as_mut().unwrap()) {
                    Some(v) => via_infos.push(v),
                    None => return false,
                },
                Tok::Kw(Kw::ViaRule) => match read_via_rule(&mut ctx.lx) {
                    Some(r) => via_rules.push(r),
                    None => return false,
                },
                Tok::Kw(Kw::Class) => match read_net_class_scope(&mut ctx.lx) {
                    Some(c) => classes.push(c),
                    None => return false,
                },
                Tok::Kw(Kw::ClassClass) => match read_class_class_scope(&mut ctx.lx) {
                    Some(c) => class_classes.push(c),
                    None => return false,
                },
                _ => {
                    ctx.lx.skip_scope();
                }
            }
        }
    }
    for c in &classes {
        match &mut ctx.via_padstack_names {
            Some(v) => v.extend(c.use_via.iter().cloned()),
            None => ctx.via_padstack_names = Some(c.use_via.clone()),
        }
    }
    let via_at_smd = ctx.via_at_smd_allowed;
    {
        let board = ctx.board.as_mut().unwrap();
        if let Some(names) = &ctx.via_padstack_names {
            let mut found: Vec<PadstackId> = Vec::new();
            for name in names {
                let ps = board
                    .library
                    .padstacks
                    .get_by_name(name)
                    .or_else(|| board.library.padstacks.get_by_name(&strip_dot_digits(name)));
                match ps {
                    Some(p) => found.push(p),
                    None => log::warn!(
                        "Library.read_scope: via padstack with name '{} not found at '{}'",
                        name,
                        ctx.lx.scope_identifier
                    ),
                }
            }
            board.library.set_via_padstacks(found);
        }
        insert_via_infos(via_infos, board, via_at_smd);
        insert_via_rules(&via_rules, board);
    }
    let ct = ctx.ct();
    for c in &classes {
        insert_net_class(c, ctx.layer_structure.as_ref().unwrap(), ctx.board.as_mut().unwrap(), &ct, via_at_smd);
    }
    insert_class_pairs(&class_classes, ctx.board.as_mut().unwrap(), &ct);
    insert_components(ctx);
    insert_logical_parts(ctx);
    true
}

fn insert_via_infos(infos: Vec<ViaInfo>, board: &mut Board, attach_allowed: bool) {
    if !infos.is_empty() {
        for i in infos {
            board.rules.via_infos.add(i);
        }
    } else {
        create_default_via_infos(board, NetClassId(0), attach_allowed);
    }
}

fn create_default_via_infos(board: &mut Board, net_class: NetClassId, attach_allowed: bool) {
    let class = board.rules.net_classes.get(net_class);
    let cl = class.default_item_clearance_classes.get(ItemClass::Via);
    let is_default = net_class == NetClassId(0);
    let class_name = class.get_name().to_string();
    for i in 0..board.library.via_padstack_count() {
        let ps_id = board.library.get_via_padstack(i).unwrap();
        let ps = board.library.padstacks.get(ps_id);
        let via_attach = attach_allowed && ps.attach_allowed;
        let name = if is_default {
            ps.name.clone()
        } else {
            format!("{}-{}", ps.name, class_name)
        };
        board.rules.via_infos.add(ViaInfo {
            name,
            padstack: ps_id,
            clearance_class: cl,
            attach_smd_allowed: via_attach,
        });
    }
}

fn insert_via_rules(rules: &[Vec<String>], board: &mut Board) {
    let mut found = false;
    for l in rules {
        if l.len() < 2 {
            continue;
        }
        if add_via_rule(l, board) {
            found = true;
        }
    }
    if !found {
        board.rules.create_default_via_rule(NetClassId(0), "default", &board.library.padstacks);
    }
    let def = board.rules.get_default_via_rule();
    for i in 0..board.rules.net_classes.count() {
        board.rules.net_classes.get_mut(NetClassId(i)).set_via_rule(def);
    }
}

/// `Network.addViaRule`: replaces an existing via rule with the same name.
pub fn add_via_rule(names: &[String], board: &mut Board) -> bool {
    let rule_name = &names[0];
    let existing = board.rules.get_via_rule_by_name(rule_name);
    let mut rule = ViaRule::new(rule_name);
    let mut ok = true;
    for n in &names[1..] {
        match board.rules.via_infos.get_by_name(n) {
            Some(v) => rule.append_via(v),
            None => {
                log::warn!("Network.insert_via_rules: viaInfo not found");
                ok = false;
            }
        }
    }
    if ok {
        if let Some(e) = existing {
            board.rules.via_rules.remove(e.0);
        }
        board.rules.via_rules.push(rule);
        if let Some(e) = existing {
            // Via rules are referenced by index: shift the references of the net classes.
            // A class referencing the replaced rule keeps the removed rule object in the
            // original; here it references the new rule with the same name.
            let new_id = ViaRuleId(board.rules.via_rules.len() - 1);
            for i in 0..board.rules.net_classes.count() {
                let nc = board.rules.net_classes.get_mut(NetClassId(i));
                if let Some(r) = nc.get_via_rule() {
                    if r == e {
                        nc.set_via_rule(Some(new_id));
                    } else if r.0 > e.0 {
                        nc.set_via_rule(Some(ViaRuleId(r.0 - 1)));
                    }
                }
            }
        }
    }
    ok
}

pub(super) fn insert_net_class(
    c: &DsnNetClass,
    ls: &DsnLayerStructure,
    board: &mut Board,
    ct: &CoordinateTransform,
    via_at_smd: bool,
) {
    let class = if is_kicad_default_net_class_name(&c.name) {
        NetClassId(0)
    } else {
        board.rules.append_net_class(&c.name)
    };
    if let Some(cl_name) = &c.trace_clearance_class {
        let cl = board.rules.clearance_matrix.get_no(cl_name);
        if cl >= 0 {
            board.rules.net_classes.get_mut(class).set_trace_clearance_class(cl);
        } else {
            log::warn!(
                "Network.insert_net_class: clearance class not found at '{}'",
                board.rules.net_classes.get(class).get_name()
            );
        }
    }
    if let Some(vr) = &c.via_rule {
        match board.rules.get_via_rule_by_name(vr) {
            Some(r) => board.rules.net_classes.get_mut(class).set_via_rule(Some(r)),
            None => log::warn!(
                "Network.insert_net_class: via rule not found at '{}'",
                board.rules.net_classes.get(class).get_name()
            ),
        }
    }
    if c.max_trace_length > 0.0 {
        board.rules.net_classes.get_mut(class).set_maximum_trace_length(ct.dsn_to_board(c.max_trace_length));
    }
    if c.min_trace_length > 0.0 {
        board.rules.net_classes.get_mut(class).set_minimum_trace_length(ct.dsn_to_board(c.min_trace_length));
    }
    for net_name in &c.net_list {
        let nos: Vec<i32> = board.rules.nets.get_by_name(net_name).iter().map(|n| n.net_number).collect();
        for no in nos {
            board.rules.nets.get_mut(no).unwrap().set_class(class);
        }
    }
    let mut clearance_rule_found = false;
    for r in &c.rules {
        match r {
            Rule::Width(w) => {
                let hw = round_i(ct.dsn_to_board(w / 2.0));
                board.rules.net_classes.get_mut(class).set_trace_half_width(hw);
            }
            Rule::Clearance { value, pairs } => {
                add_clearance_rule(board, class, *value, pairs, -1, ct);
                clearance_rule_found = true;
            }
            Rule::Length { max, min } => {
                if *max > 0.0 {
                    board.rules.net_classes.get_mut(class).set_maximum_trace_length(ct.dsn_to_board(*max));
                }
                if *min > 0.0 {
                    board.rules.net_classes.get_mut(class).set_minimum_trace_length(ct.dsn_to_board(*min));
                }
            }
            Rule::Meander(_) => {}
        }
    }
    for lr in &c.layer_rules {
        for layer_name in &lr.layer_names {
            let layer = board.layer_structure.get_no(layer_name);
            if layer < 0 {
                log::warn!(
                    "Network.insert_net_class: layer not found at '{}'",
                    board.rules.net_classes.get(class).get_name()
                );
                continue;
            }
            for r in &lr.rules {
                match r {
                    Rule::Width(w) => {
                        let hw = round_i(ct.dsn_to_board(w / 2.0));
                        board.rules.net_classes.get_mut(class).set_trace_half_width_on_layer(layer as usize, hw);
                    }
                    Rule::Clearance { value, pairs } => {
                        add_clearance_rule(board, class, *value, pairs, layer, ct);
                        clearance_rule_found = true;
                    }
                    _ => log::warn!(
                        "Network.insert_net_class: layer rule type not yet implemented at '{}'",
                        board.rules.net_classes.get(class).get_name()
                    ),
                }
            }
        }
    }
    {
        let nc = board.rules.net_classes.get_mut(class);
        nc.set_pull_tight(c.pull_tight);
        nc.set_shove_fixed(c.shove_fixed);
    }
    let mut via_infos_created = false;
    if clearance_rule_found && class != NetClassId(0) {
        create_default_via_infos(board, class, via_at_smd);
        via_infos_created = true;
    }
    if !c.use_via.is_empty() {
        create_via_rule(&c.use_via, class, board);
    } else if via_infos_created {
        let name = board.rules.net_classes.get(class).get_name().to_string();
        board.rules.create_default_via_rule(class, &name, &board.library.padstacks);
    }
    if !c.use_layer.is_empty() {
        let nc = board.rules.net_classes.get_mut(class);
        for i in 0..ls.layers.len() {
            nc.set_active_routing_layer(i as i32, false);
        }
        for name in &c.use_layer {
            nc.set_active_routing_layer(ls.get_no(name), true);
        }
        for i in 0..ls.layers.len() {
            if !nc.is_active_routing_layer(i as i32) {
                nc.set_trace_half_width_on_layer(i, 0);
            }
        }
    }
}

fn create_via_rule(use_via: &[String], class: NetClassId, board: &mut Board) {
    let nc = board.rules.net_classes.get(class);
    let mut rule = ViaRule::new(nc.get_name());
    let default_via_cl = nc.default_item_clearance_classes.get(ItemClass::Via);
    for via_name in use_via {
        let cleaned = strip_dot_digits(via_name);
        for i in 0..board.rules.via_infos.count() {
            let id = crate::rules::ViaInfoId(i);
            let info = board.rules.via_infos.get(id);
            if info.clearance_class == default_via_cl {
                let ps_name = &board.library.padstacks.get(info.padstack).name;
                if ps_name == via_name || *ps_name == cleaned {
                    rule.append_via(id);
                }
            }
        }
    }
    board.rules.via_rules.push(rule);
    let rid = ViaRuleId(board.rules.via_rules.len() - 1);
    board.rules.net_classes.get_mut(class).set_via_rule(Some(rid));
}

fn add_clearance_rule(board: &mut Board, class: NetClassId, value: f64, pairs: &[String], layer: i32, ct: &CoordinateTransform) {
    let clearance = round_i(ct.dsn_to_board(value));
    let class_name = board.rules.net_classes.get(class).get_name().to_string();
    let cm = &mut board.rules.clearance_matrix;
    let mut class_no = cm.get_no(&class_name);
    if class_no < 0 {
        cm.append_class(&class_name);
        class_no = cm.get_no(&class_name);
        for i in 1..cm.get_class_count() {
            for j in 0..cm.get_layer_count() {
                let v = cm.get_value(class_no, i as i32, j as i32, false).max(clearance);
                cm.set_value(class_no as usize, i, j, v);
                cm.set_value(i, class_no as usize, j, v);
            }
        }
        board.rules.net_classes.get_mut(class).default_item_clearance_classes.set_all(class_no);
    }
    board.rules.net_classes.get_mut(class).set_trace_clearance_class(class_no);
    let cn = class_no as usize;
    if pairs.is_empty() {
        let cm = &mut board.rules.clearance_matrix;
        if layer < 0 {
            cm.set_value_all_layers(cn, cn, clearance);
        } else {
            cm.set_value(cn, cn, layer as usize, clearance);
        }
        return;
    }
    if contains_wire_clearance_pair(pairs) {
        for n in ["via", "smd", "pin", "area"] {
            get_clearance_class(board, class, n);
        }
    }
    for s in pairs {
        let p = java_split(s, "_");
        if p.len() != 2 {
            continue;
        }
        let f = get_clearance_class(board, class, &p[0]) as usize;
        let sc = get_clearance_class(board, class, &p[1]) as usize;
        set_pair_value(&mut board.rules.clearance_matrix, f, sc, layer, clearance);
    }
}

fn set_pair_value(cm: &mut ClearanceMatrix, f: usize, s: usize, layer: i32, clearance: i32) {
    if layer < 0 {
        cm.set_value_all_layers(f, s, clearance);
        cm.set_value_all_layers(s, f, clearance);
    } else {
        cm.set_value(f, s, layer as usize, clearance);
        cm.set_value(s, f, layer as usize, clearance);
    }
}

/// `Network.getClearanceClass`: the clearance class `<net class>-<item class>`, created if missing.
fn get_clearance_class(board: &mut Board, class: NetClassId, item_class_name: &str) -> i32 {
    let net_class_name = board.rules.net_classes.get(class).get_name().to_string();
    let new_name = if item_class_name != "wire" {
        format!("{net_class_name}-{item_class_name}")
    } else {
        net_class_name.clone()
    };
    let cm = &mut board.rules.clearance_matrix;
    let found = cm.get_no(&new_name);
    if found >= 0 {
        return found;
    }
    cm.append_class(&new_name);
    let result = cm.get_no(&new_name);
    let net_class_no = cm.get_no(&net_class_name);
    if net_class_no < 0 || result < 0 {
        log::warn!("Network.get_clearance_class: clearance class not found at '{net_class_name}'");
        return result;
    }
    for i in 1..cm.get_class_count() {
        for j in 0..cm.get_layer_count() {
            let v = cm.get_value(net_class_no, i as i32, j as i32, false);
            cm.set_value(result as usize, i, j, v);
            cm.set_value(i, result as usize, j, v);
        }
    }
    let dc = &mut board.rules.net_classes.get_mut(class).default_item_clearance_classes;
    match item_class_name {
        "via" => dc.set(ItemClass::Via, result),
        "pin" => dc.set(ItemClass::Pin, result),
        "smd" => dc.set(ItemClass::Smd, result),
        "area" => dc.set(ItemClass::Area, result),
        _ => {}
    }
    result
}

fn insert_class_pairs(class_classes: &[ClassClass], board: &mut Board, ct: &CoordinateTransform) {
    for cc in class_classes {
        let mut classes = Vec::new();
        for name in &cc.class_names {
            let c = if is_kicad_default_net_class_name(name) {
                Some(NetClassId(0))
            } else {
                board.rules.net_classes.find_by_name(name)
            };
            match c {
                Some(c) => classes.push(c),
                None => log::warn!("Network.insert_class_pairs: class '{name}' not found"),
            }
        }
        for i in 0..classes.len() {
            for j in i + 1..classes.len() {
                insert_class_pair_info(cc, classes[i], classes[j], board, ct);
            }
        }
    }
}

fn insert_class_pair_info(cc: &ClassClass, first: NetClassId, second: NetClassId, board: &mut Board, ct: &CoordinateTransform) {
    for r in &cc.rules {
        if let Rule::Clearance { value, pairs } = r {
            add_mixed_clearance_rule(board, first, second, *value, pairs, -1, ct);
        } else {
            log::warn!("Network.insert_class_pair_info: unexpected rule");
        }
    }
    for lr in &cc.layer_rules {
        for layer_name in &lr.layer_names {
            let layer = board.layer_structure.get_no(layer_name);
            if layer < 0 {
                log::warn!("Network.insert_class_pair_info: layer not found at '{layer_name}'");
                continue;
            }
            for r in &lr.rules {
                if let Rule::Clearance { value, pairs } = r {
                    add_mixed_clearance_rule(board, first, second, *value, pairs, layer, ct);
                } else {
                    log::warn!("Network.insert_class_pair_info: unexpected layer rule type");
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn add_mixed_clearance_rule(
    board: &mut Board,
    first: NetClassId,
    second: NetClassId,
    value: f64,
    pairs: &[String],
    layer: i32,
    ct: &CoordinateTransform,
) {
    let clearance = round_i(ct.dsn_to_board(value));
    let first_name = board.rules.net_classes.get(first).get_name().to_string();
    let second_name = board.rules.net_classes.get(second).get_name().to_string();
    let cm = &mut board.rules.clearance_matrix;
    let mut first_no = cm.get_no(&first_name);
    if first_no < 0 {
        cm.append_class(&first_name);
        first_no = cm.get_no(&first_name);
    }
    let mut second_no = cm.get_no(&second_name);
    if second_no < 0 {
        cm.append_class(&second_name);
        second_no = cm.get_no(&second_name);
    }
    if pairs.is_empty() {
        set_pair_value(cm, first_no as usize, second_no as usize, layer, clearance);
        return;
    }
    for s in pairs {
        let p = java_split(s, "_");
        if p.len() != 2 {
            continue;
        }
        for i in 0..2 {
            let (f, sc) = if i == 0 {
                (get_clearance_class(board, first, &p[0]), get_clearance_class(board, second, &p[1]))
            } else {
                (get_clearance_class(board, second, &p[0]), get_clearance_class(board, first, &p[1]))
            };
            set_pair_value(&mut board.rules.clearance_matrix, f as usize, sc as usize, layer, clearance);
        }
    }
}

// ------------------------------------------------------------------ components

fn insert_components(ctx: &mut ReadCtx) {
    let placements = std::mem::take(&mut ctx.placement_list);
    for p in &placements {
        for loc in &p.locations {
            insert_component(ctx, loc, &p.lib_name);
        }
    }
    ctx.placement_list = placements;
}

fn insert_component(ctx: &mut ReadCtx, loc: &ComponentLocation, lib_key: &str) {
    let ct = ctx.ct();
    let board = ctx.board.as_mut().unwrap();
    let front = board.library.packages.get_by_name(lib_key, true);
    let back = board.library.packages.get_by_name(lib_key, false);
    let (Some(front), Some(back)) = (front, back) else {
        log::warn!("Network.insert_component: component package not found at '{}'", ctx.lx.scope_identifier);
        return;
    };
    let exact = loc.coor.map(|c| dsn_to_board_point(&ct, c[0], c[1]));
    let rotation = loc.rotation;
    let comp_id = board.components.add_precise(
        &loc.name,
        exact,
        rotation,
        loc.is_front,
        front,
        back,
        loc.position_fixed,
        loc.part_number.clone(),
    );
    let comp = board.components.get(comp_id);
    if !comp.is_placed() {
        return;
    }
    let l = comp.get_location().unwrap();
    let translation = Vector::new_int(l.x, l.y);
    let fixed = if loc.position_fixed { FixedState::SystemFixed } else { FixedState::Unfixed };
    let package_id = comp.get_package();
    let placed_on_front = comp.placed_on_front();
    let transform = comp.placement_transform(board.components.get_flip_style_rotate_first());
    let pin_count = board.library.packages.get(package_id).pin_count();
    for i in 0..pin_count {
        let pin = board.library.packages.get(package_id).get_pin(i).unwrap().clone();
        let Some(ps) = board.library.padstacks.try_get(pin.padstack_id) else {
            log::warn!("Network.insert_component: pin padstack not found at '{}'", ctx.lx.scope_identifier);
            return;
        };
        let ps = ps.clone();
        let mut nets: Vec<i32> = Vec::new();
        for id in ctx.netlist.get_nets(&loc.name, &pin.name) {
            match board.rules.nets.get_by_name_subnet(&id.name, id.subnet_number) {
                Some(n) => nets.push(n.net_number),
                None => log::warn!("Network.insert_component: board net not found at '{}'", ctx.lx.scope_identifier),
            }
        }
        let net_class = nets
            .first()
            .and_then(|n| board.rules.nets.get(*n))
            .map(|n| n.net_class)
            .unwrap_or(NetClassId(0));
        let mut cl = -1;
        if let Some(info) = loc.pin_infos.get(&pin.name) {
            cl = board.rules.clearance_matrix.get_no(&info.clearance_class);
        }
        if cl < 0 {
            let dc = &board.rules.net_classes.get(net_class).default_item_clearance_classes;
            cl = if ps.from_layer() == ps.to_layer() { dc.get(ItemClass::Smd) } else { dc.get(ItemClass::Pin) };
        }
        board.insert_pin(comp_id, i as i32, &nets, cl, fixed);
        if ps.has_auxiliary_shapes() {
            for pad_layer in 0..ps.board_layer_count() {
                let Some(aux) = ps.get_auxiliary_shapes(pad_layer) else { continue };
                if aux.is_empty() {
                    continue;
                }
                let board_layer = if placed_on_front || ps.placed_absolute {
                    pad_layer
                } else {
                    ps.board_layer_count() - pad_layer - 1
                };
                if board_layer < 0 || board_layer >= board.layer_count() {
                    continue;
                }
                for a in aux {
                    let transformed = transform.shape(a, &pin.get_exact_relative_location(), pin.rotation_in_degree);
                    let area = match transformed {
                        s @ (Shape::Tile(_) | Shape::Polygon(_)) => Area::PolylineArea(PolylineArea::new(s, Vec::new())),
                        s => Area::Shape(s),
                    };
                    if !area.is_empty() {
                        board.insert_obstacle_area(
                            ObstacleKind::Obstacle,
                            area,
                            board_layer,
                            Vector::ZERO,
                            0.0,
                            false,
                            &nets,
                            cl,
                            comp_id,
                            Some(format!("{}_aux", pin.name)),
                            fixed,
                        );
                    }
                }
            }
        }
    }
    // the keepouts of the package
    let package = board.library.packages.get(package_id).clone();
    let layer_count = board.layer_count();
    for k in 0..3 {
        let (keepouts, infos, kind) = match k {
            0 => (&package.keepouts, &loc.keepout_infos, ObstacleKind::Obstacle),
            1 => (&package.via_keepouts, &loc.via_keepout_infos, ObstacleKind::ViaObstacle),
            _ => (&package.place_keepouts, &loc.place_keepout_infos, ObstacleKind::ComponentObstacle),
        };
        for keepout in keepouts {
            let mut layer = keepout.layer;
            if layer >= layer_count {
                log::warn!("Network.insert_component: keepout layer is to big at '{}'", ctx.lx.scope_identifier);
                continue;
            }
            if layer >= 0 && !loc.is_front {
                layer = layer_count - keepout.layer - 1;
            }
            let mut cl = board.rules.get_default_net_class().default_item_clearance_classes.get(ItemClass::Area);
            if let Some(info) = infos.get(&keepout.name) {
                let c = board.rules.clearance_matrix.get_no(&info.clearance_class);
                if c > 0 {
                    cl = c;
                }
            }
            let layers: Vec<i32> = if layer >= 0 {
                vec![layer]
            } else {
                (0..layer_count).filter(|j| board.layer_structure.layers[*j as usize].is_signal).collect()
            };
            for l in layers {
                board.insert_obstacle_area(
                    kind,
                    keepout.area.clone(),
                    l,
                    translation.clone(),
                    rotation,
                    !loc.is_front,
                    &[],
                    cl,
                    comp_id,
                    Some(keepout.name.clone()),
                    fixed,
                );
            }
        }
    }
    // the outline as component outline
    let mut courtyard_idx: i32 = -1;
    if package.outline.len() > 1 {
        let mut max_area = -1.0;
        for (i, o) in package.outline.iter().enumerate() {
            let a = o.bounding_box().area();
            if a > max_area {
                max_area = a;
                courtyard_idx = i as i32;
            }
        }
    }
    for (i, o) in package.outline.iter().enumerate() {
        let mut is_courtyard = i as i32 == courtyard_idx;
        if i < package.outline_widths.len() && package.outline_widths[i] == 0.0 {
            is_courtyard = true;
        }
        let mut is_fabrication = false;
        if !is_courtyard && i < package.outline_widths.len() && package.outline_widths[i] <= 110.0 {
            is_fabrication = true;
        }
        let is_closed = package.outline_is_closed.get(i).copied().unwrap_or(false);
        board.insert_component_outline(
            Area::Shape(o.clone()),
            loc.is_front,
            translation.clone(),
            rotation,
            comp_id,
            is_courtyard,
            is_fabrication,
            is_closed,
            fixed,
        );
    }
}

fn insert_logical_parts(ctx: &mut ReadCtx) -> bool {
    let board = ctx.board.as_mut().unwrap();
    for part in &ctx.logical_parts {
        // search the library package of the part
        let mut package = None;
        let mut mapping_found = false;
        for m in &ctx.logical_part_mappings {
            if m.name == part.name {
                mapping_found = true;
                let Some(first) = m.components.iter().next() else {
                    log::warn!("Network.search_lib_package: component list empty at '{}'", part.name);
                    return false;
                };
                let Some(c) = board.components.get_by_name(first) else {
                    log::warn!("Network.search_lib_package: component not found at '{first}'");
                    return false;
                };
                package = Some(c.get_package());
                break;
            }
        }
        if !mapping_found {
            log::warn!("Network.search_lib_package: library package '{}' not found", part.name);
            return false;
        }
        let pkg = board.library.packages.get(package.unwrap());
        let mut pins = Vec::new();
        for pp in &part.part_pins {
            let idx = pkg.get_pin_index(&pp.pin_name);
            if idx < 0 {
                log::warn!("Network.insert_logical_parts: package pin not found at '{}'", pp.pin_name);
                return false;
            }
            pins.push(PartPin {
                pin_index: idx,
                pin_name: pp.pin_name.clone(),
                gate_name: pp.gate_name.clone(),
                gate_swap_code: pp.gate_swap_code,
                gate_pin_name: pp.gate_pin_name.clone(),
                gate_pin_swap_code: pp.gate_pin_swap_code,
            });
        }
        board.library.logical_parts.add(&part.name, pins);
    }
    for m in &ctx.logical_part_mappings {
        let lp = board.library.logical_parts.get_by_name(&m.name);
        if lp.is_none() {
            log::warn!("Network.insert_logical_parts: logical part not found at '{}'", m.name);
        }
        for cmp in &m.components {
            let id = board.components.get_by_name(cmp).map(|c| c.id);
            match id {
                Some(id) => board.components.get_mut(id).set_logical_part(lp),
                None => log::warn!("Network.insert_logical_parts: board component not found at '{cmp}'"),
            }
        }
    }
    true
}
