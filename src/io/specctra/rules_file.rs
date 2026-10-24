//! Reading and writing of Specctra `.rules` files (port of `RulesReader` and `RulesWriter`).

use super::autoroute_settings::{read_autoroute_settings, DsnAutorouteSettings};
use super::java_fmt::java_double_to_string;
use super::lexer::{Kw, LexState, Lexer, Tok};
use super::library::read_padstack_scope;
use super::network::{add_via_rule, insert_net_class, read_net_class_scope, read_via_info, read_via_rule};
use super::reader::DsnError;
use super::scopes::{read_rule_scope, Rule};
use super::ses_writer::{board_to_dsn_shape, write_shape, IdentifierType, IndentWriter};
use super::shapes::{round_i, DsnLayerStructure};
use super::structure::{read_snap_angle, set_clearance_rule};
use crate::board::{Board, CoordinateTransform};
use crate::library::Padstack;
use crate::rules::{AngleRestriction, BoardRules, NetClassId, ViaInfo, ViaInfoId, ViaInfos, ViaRule};

/// Result of reading a rules file.
pub struct RulesReadResult {
    /// The `(autoroute_settings ...)` scope of the rules file, if present.
    pub autoroute_settings: Option<DsnAutorouteSettings>,
}

fn board_ct(board: &Board) -> CoordinateTransform {
    board.communication.coordinate_transform.unwrap_or(CoordinateTransform::new(1.0, 0.0, 0.0))
}

/// Reads the rules of a `.rules` file and applies them to `board` (`RulesReader.read`).
pub fn read_rules_str(text: &str, design_name: &str, board: &mut Board) -> Result<RulesReadResult, DsnError> {
    let mut lx = Lexer::new(text);
    if lx.next_token() != Tok::Open {
        log::warn!("RulesReader.read: open bracket expected at '{}'", lx.scope_identifier);
        return Err(DsnError::Parse("open bracket expected".into()));
    }
    if !lx.next_token().is_kw(Kw::Rules) {
        log::warn!("RulesReader.read: keyword 'rules' expected at '{}'", lx.scope_identifier);
        return Err(DsnError::Parse("keyword 'rules' expected".into()));
    }
    if !lx.next_token().is_kw(Kw::PcbScope) {
        log::warn!("RulesReader.read: keyword 'pcb' expected at '{}'", lx.scope_identifier);
        return Err(DsnError::Parse("keyword 'pcb' expected".into()));
    }
    lx.yybegin(LexState::Name);
    let t = lx.next_token();
    if t.as_str() != Some(design_name) {
        log::warn!(
            "RulesReader.read: designName not matching at '{}' (expected '{}', got '{:?}')",
            lx.scope_identifier,
            design_name,
            t
        );
    }
    let ls = DsnLayerStructure::from_board(&board.layer_structure);
    let ct = board_ct(board);
    let mut result = RulesReadResult { autoroute_settings: None };
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof {
            log::warn!("RulesReader.read: unexpected end of file at '{}'", lx.scope_identifier);
            return Err(DsnError::Parse(lx.fatal.take().unwrap_or("unexpected end of file".into())));
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            match next {
                Tok::Kw(Kw::Rule) => {
                    let rules = read_rule_scope(&mut lx);
                    apply_rules(&rules, board, None);
                }
                Tok::Kw(Kw::Layer) => apply_layer_rules(&mut lx, board),
                Tok::Kw(Kw::Padstack) => {
                    read_padstack_scope(&mut lx, &ls, &ct, &mut board.library.padstacks);
                }
                Tok::Kw(Kw::Via) => {
                    if let Some(v) = read_via_info(&mut lx, board) {
                        replace_via_info(&mut board.rules, v);
                    }
                }
                Tok::Kw(Kw::ViaRule) => {
                    if let Some(r) = read_via_rule(&mut lx) {
                        // the original reads the rule list without length check
                        if !r.is_empty() {
                            add_via_rule(&r, board);
                        }
                    }
                }
                Tok::Kw(Kw::Class) => {
                    if let Some(c) = read_net_class_scope(&mut lx) {
                        insert_net_class(&c, &ls, board, &ct, false);
                    }
                }
                Tok::Kw(Kw::SnapAngle) => {
                    if let Some(a) = read_snap_angle(&mut lx) {
                        board.rules.set_trace_angle_restriction(a);
                    }
                }
                Tok::Kw(Kw::AutorouteSettings) => {
                    result.autoroute_settings = read_autoroute_settings(&mut lx, &ls);
                }
                _ => {
                    lx.skip_scope();
                }
            }
        }
    }
    if let Some(f) = lx.fatal.take() {
        return Err(DsnError::Parse(f));
    }
    Ok(result)
}

/// Reads a rules file and applies it to `board`.
pub fn read_rules(path: impl AsRef<std::path::Path>, design_name: &str, board: &mut Board) -> Result<RulesReadResult, DsnError> {
    let bytes = std::fs::read(path)?;
    read_rules_str(&String::from_utf8_lossy(&bytes), design_name, board)
}

/// Removes an existing via info with the same name and appends `info`
/// (`viaInfos.remove(existing); viaInfos.add(viaInfo)`).
///
/// The via infos are referenced by index in the Rust port: via rules, which referenced the
/// removed via info, are redirected to the new one (in the original they keep the removed object).
fn replace_via_info(rules: &mut BoardRules, info: ViaInfo) {
    let Some(existing) = rules.via_infos.get_by_name(&info.name) else {
        rules.via_infos.add(info);
        return;
    };
    let mut new_infos = ViaInfos::default();
    let mut map: Vec<Option<ViaInfoId>> = Vec::with_capacity(rules.via_infos.count());
    for i in 0..rules.via_infos.count() {
        if i == existing.0 {
            map.push(None);
        } else {
            map.push(Some(new_infos.add_unchecked(rules.via_infos.get(ViaInfoId(i)).clone())));
        }
    }
    let new_id = new_infos.add_unchecked(info);
    for r in rules.via_rules.iter_mut() {
        let mut nr = ViaRule::new(&r.name);
        for v in r.vias() {
            nr.append_via(map[v.0].unwrap_or(new_id));
        }
        *r = nr;
    }
    rules.via_infos = new_infos;
}

fn apply_rules(rules: &[Rule], board: &mut Board, layer_name: Option<&str>) {
    let mut layer = -1;
    if let Some(n) = layer_name {
        layer = board.layer_structure.get_no(n);
        if layer < 0 {
            log::warn!("RulesReader.applyRules: layer not found: '{n}'");
        }
    }
    let ct = board_ct(board);
    let quote = board.communication.specctra_parser_info.string_quote.clone();
    for r in rules {
        match r {
            Rule::Width(w) => {
                let hw = round_i(ct.dsn_to_board(*w) / 2.0);
                if layer < 0 {
                    board.rules.set_default_trace_half_widths(hw);
                } else {
                    board.rules.set_default_trace_half_width(layer as usize, hw);
                }
            }
            Rule::Clearance { value, pairs } => {
                set_clearance_rule(*value, pairs, layer, &ct, &mut board.rules, &quote);
            }
            _ => {}
        }
    }
}

fn apply_layer_rules(lx: &mut Lexer, board: &mut Board) {
    let Tok::Str(layer) = lx.next_token() else {
        log::warn!("RulesReader.applyLayerRules: String expected at '{}'", lx.scope_identifier);
        return;
    };
    let mut t = lx.next_token();
    while t != Tok::Close {
        if t != Tok::Open {
            log::warn!("RulesReader.applyLayerRules: '(' expected at '{}'", lx.scope_identifier);
            return;
        }
        t = lx.next_token();
        if t.is_kw(Kw::Rule) {
            let rules = read_rule_scope(lx);
            apply_rules(&rules, board, Some(&layer));
        } else {
            lx.skip_scope();
        }
        t = lx.next_token();
    }
}

// ------------------------------------------------------------------ writing

const RESERVED: [&str; 6] = ["(", ")", " ", ";", "-", "_"];

/// Writes the design rules of `board` in Specctra rules format (`RulesWriter.write`
/// without router settings).
pub fn rules_string(board: &Board, design_name: &str) -> String {
    let mut file = IndentWriter::new();
    let id = IdentifierType::new(&RESERVED, &board.communication.specctra_parser_info.string_quote);
    let ct = board_ct(board);
    file.start_scope();
    file.write("rules PCB ");
    file.write(design_name);
    // snap angle
    file.start_scope();
    file.write("snapAngle ");
    file.new_line();
    file.write(match board.rules.get_trace_angle_restriction() {
        AngleRestriction::NinetyDegree => "ninety_degree",
        AngleRestriction::FortyfiveDegree => "fortyfive_degree",
        AngleRestriction::None => "none",
    });
    file.end_scope();
    write_default_rule(board, &id, &ct, &mut file, 0);
    for ps in board.library.padstacks.iter() {
        if board.library.get_via_padstack_by_name(&ps.name).is_some() {
            write_padstack_scope(board, ps, &id, &ct, &mut file);
        }
    }
    // via infos
    for i in 0..board.rules.via_infos.count() {
        let v = board.rules.via_infos.get(ViaInfoId(i));
        file.start_scope();
        file.write("via ");
        file.new_line();
        id.write(&v.name, &mut file);
        file.write(" ");
        id.write(&board.library.padstacks.get(v.padstack).name, &mut file);
        file.write(" ");
        id.write(board.rules.clearance_matrix.get_name(v.clearance_class).unwrap_or(""), &mut file);
        if v.attach_smd_allowed {
            file.write(" attach");
        }
        file.end_scope();
    }
    // via rules
    for r in &board.rules.via_rules {
        file.start_scope();
        file.write("viaRule");
        file.new_line();
        id.write(&r.name, &mut file);
        for v in r.vias() {
            file.write(" ");
            id.write(&board.rules.via_infos.get(*v).name, &mut file);
        }
        file.end_scope();
    }
    for (cid, _) in board.rules.net_classes.iter() {
        write_net_class(board, cid, &id, &ct, &mut file);
    }
    file.end_scope();
    file.into_string()
}

/// Writes the rules file of `board`.
pub fn write_rules<W: std::io::Write>(board: &Board, design_name: &str, out: &mut W) -> std::io::Result<()> {
    out.write_all(rules_string(board, design_name).as_bytes())?;
    out.flush()
}

fn write_default_rule(board: &Board, id: &IdentifierType, ct: &CoordinateTransform, file: &mut IndentWriter, layer: i32) {
    let rules = &board.rules;
    file.start_scope();
    file.write("rule");
    let w = 2.0 * ct.board_to_dsn(rules.get_default_net_class().get_trace_half_width(0) as f64);
    file.new_line();
    file.write("(width ");
    file.write(&java_double_to_string(w));
    file.write(")");
    let dc = BoardRules::default_clearance_class();
    let c = ct.board_to_dsn(rules.clearance_matrix.get_value(dc, dc, layer, false) as f64);
    file.new_line();
    file.write("(clearance ");
    file.write(&java_double_to_string(c));
    file.write(")");
    let smd = ct.board_to_dsn(rules.get_pin_edge_to_turn_dist());
    file.new_line();
    file.write("(clearance ");
    file.write(&java_double_to_string(smd));
    file.write(" (type smd_to_turn_gap))");
    let cm = &rules.clearance_matrix;
    for i in 1..cm.get_class_count() as i32 {
        let name = cm.get_name(i).unwrap_or("");
        if name == "default" {
            continue;
        }
        let v = ct.board_to_dsn(cm.get_value(i, i, layer, false) as f64);
        file.new_line();
        file.write("(clearance ");
        file.write(&java_double_to_string(v));
        file.write(" (type ");
        id.write(name, file);
        file.write("))");
    }
    file.end_scope();
}

fn write_padstack_scope(board: &Board, ps: &Padstack, id: &IdentifierType, ct: &CoordinateTransform, file: &mut IndentWriter) {
    let lc = board.layer_count();
    let shape = |l: i32| ps.shapes().get(l as usize).and_then(|s| s.as_ref());
    let mut first = 0;
    while first < lc && shape(first).is_none() {
        first += 1;
    }
    let mut last = lc - 1;
    while last >= 0 && shape(last).is_none() {
        last -= 1;
    }
    if first >= lc || last < 0 {
        log::warn!("Library.write_padstack_scope: padstack shape not found at '{}'", ps.name);
        return;
    }
    file.start_scope();
    file.write("padstack ");
    id.write(&ps.name, file);
    for i in first..=last {
        let Some(s) = shape(i) else { continue };
        let layer = &board.layer_structure.layers[i as usize].name;
        file.start_scope();
        file.write("shape");
        write_shape(&board_to_dsn_shape(ct, s, true), layer, id, file, false);
        file.end_scope();
        if let Some(aux) = ps.get_auxiliary_shapes(i) {
            for a in aux {
                file.start_scope();
                file.write("shape");
                write_shape(&board_to_dsn_shape(ct, a, true), layer, id, file, false);
                file.end_scope();
            }
        }
    }
    if !ps.attach_allowed {
        file.new_line();
        file.write("(attach off)");
    }
    if ps.placed_absolute {
        file.new_line();
        file.write("(absolute on)");
    }
    file.end_scope();
}

fn write_net_class(board: &Board, cid: NetClassId, id: &IdentifierType, ct: &CoordinateTransform, file: &mut IndentWriter) {
    let rules = &board.rules;
    let nc = rules.net_classes.get(cid);
    file.start_scope();
    file.write("class ");
    id.write(nc.get_name(), file);
    let mut counter = 0;
    for n in rules.nets.iter() {
        if n.net_class == cid {
            if counter % 8 == 0 {
                file.new_line();
            } else {
                file.write(" ");
            }
            id.write(&n.name, file);
            counter += 1;
        }
    }
    file.new_line();
    file.write("(clearance_class ");
    id.write(rules.clearance_matrix.get_name(nc.get_trace_clearance_class()).unwrap_or(""), file);
    file.write(")");
    if let Some(vr) = nc.get_via_rule() {
        file.new_line();
        file.write("(via_rule ");
        id.write(&rules.via_rule(vr).name, file);
        file.write(")");
    }
    // the rule scope
    let hw0 = nc.get_trace_half_width(0);
    file.start_scope();
    file.write("rule");
    file.new_line();
    file.write("(width ");
    file.write(&java_double_to_string(2.0 * ct.board_to_dsn(hw0 as f64)));
    file.write(")");
    file.end_scope();
    for i in 1..board.layer_count() as usize {
        let hw = nc.get_trace_half_width(i);
        if hw != hw0 {
            file.start_scope();
            file.write("layer_rule ");
            file.write(&board.layer_structure.layers[i].name);
            file.start_scope();
            file.write("rule ");
            file.new_line();
            file.write("(width ");
            file.write(&java_double_to_string(2.0 * ct.board_to_dsn(hw as f64)));
            file.write(") ");
            file.end_scope();
            file.end_scope();
        }
    }
    // the circuit scope
    let min = nc.get_minimum_trace_length();
    let max = nc.get_maximum_trace_length();
    file.start_scope();
    file.write("circuit ");
    file.new_line();
    file.write("(use_layer");
    for i in 0..nc.layer_count() {
        if nc.is_active_routing_layer(i as i32) {
            file.write(" ");
            file.write(&board.layer_structure.layers[i].name);
        }
    }
    file.write(")");
    if min > 0.0 || max > 0.0 {
        file.new_line();
        file.write("(length ");
        let tmax = if max <= 0.0 { -1.0 } else { ct.board_to_dsn(max) };
        file.write(&java_double_to_string(tmax));
        file.write(" ");
        let tmin = if min <= 0.0 { 0.0 } else { ct.board_to_dsn(min) };
        file.write(&java_double_to_string(tmin));
        file.write(")");
    }
    file.end_scope();
    if !nc.get_pull_tight() {
        file.new_line();
        file.write("(pull_tight off)");
    }
    if nc.is_shove_fixed() {
        file.new_line();
        file.write("(shove_fixed on)");
    }
    file.end_scope();
}
