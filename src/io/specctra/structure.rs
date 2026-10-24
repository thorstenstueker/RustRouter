//! The structure scope (port of `parser/Structure` and `parser/Plane`).

use super::autoroute_settings::read_autoroute_settings;
use super::lexer::{Kw, LexState, Lexer, Tok};
use super::network::NetId;
use super::reader::{PlaneInfo, ReadCtx};
use super::scopes::{
    read_on_off_scope, read_rule_scope, read_string_scope, read_via_padstacks, Rule,
};
use super::shapes::{
    read_area_scope, read_shape_from_keyword, read_shape_scope, rect_union, round_i,
    transform_area_to_board, DsnLayer, DsnLayerStructure, DsnShape, DsnShapeKind, LayerRef,
    ReadAreaScopeResult,
};
use crate::board::structure::SpecctraParserInfo;
use crate::board::{Board, Communication, CoordinateTransform, FixedState, IdGenerator, Layer, LayerStructure, ObstacleKind};
use crate::geometry::{Area, IntBox, Shape, TileShape, Vector, CRIT_INT};
use crate::rules::{AngleRestriction, BoardRules, ClearanceMatrix, ItemClass, NetClassId};

#[derive(Default)]
struct BoardConstructionInfo {
    layer_info: Vec<DsnLayer>,
    bounding_shape: Option<DsnShape>,
    outline_shapes: Vec<DsnShape>,
    outline_clearance_class_name: Option<String>,
    found_layer_count: i32,
    default_rules: Vec<Rule>,
    layer_dependent_rules: Vec<(String, Vec<Rule>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeepoutType {
    Keepout,
    ViaKeepout,
    PlaceKeepout,
}

fn read_boundary_scope(lx: &mut Lexer, info: &mut BoardConstructionInfo) -> bool {
    let current = read_shape_scope(lx, None);
    let mut prev = Tok::Eof;
    loop {
        let next = lx.next_token();
        if next == Tok::Close {
            break;
        }
        if next == Tok::Eof {
            // endless loop in the original
            break;
        }
        if prev == Tok::Open {
            if next.is_kw(Kw::ClearanceClass) {
                info.outline_clearance_class_name = Some(read_string_scope(lx));
            } else {
                let additional = read_shape_from_keyword(lx, &next, None);
                add_boundary_shape(info, additional);
            }
        }
        prev = next;
    }
    if current.is_none() {
        log::warn!("Structure.read_boundary_scope: shape is null at '{}'", lx.scope_identifier);
        return true;
    }
    add_boundary_shape(info, current);
    true
}

fn add_boundary_shape(info: &mut BoardConstructionInfo, shape: Option<DsnShape>) {
    let Some(shape) = shape else { return };
    if shape.is_path() {
        info.outline_shapes.push(shape);
        return;
    }
    match shape.layer {
        LayerRef::Pcb => {
            if info.bounding_shape.is_none() {
                info.bounding_shape = Some(shape);
            } else {
                info.outline_shapes.push(shape);
            }
        }
        LayerRef::Signal => info.outline_shapes.push(shape),
        _ => log::warn!("Structure.add_boundary_shape: unexpected layer at boundary"),
    }
}

fn read_layer_scope(lx: &mut Lexer, info: &mut BoardConstructionInfo) -> bool {
    let mut layer_ok = true;
    let mut is_signal = true;
    let layer_string = lx.next_string();
    let mut net_names = Vec::new();
    let mut t = lx.next_token();
    while t != Tok::Close {
        if t != Tok::Open {
            log::warn!("Structure.read_layer_scope: ( expected at '{}'", lx.scope_identifier);
            return false;
        }
        t = lx.next_token();
        if t.is_kw(Kw::Type) {
            t = lx.next_token();
            if t.is_kw(Kw::Power) {
                is_signal = false;
            } else if t == Tok::Eof {
                lx.set_fatal("NullPointerException in Structure.readLayerScope");
                return false;
            } else if !t.is_kw(Kw::Signal) && t.as_str() != Some("jumper") {
                log::warn!(
                    "Structure.read_layer_scope: the layer '{}' has an unknown layer type at '{}'",
                    layer_string,
                    lx.scope_identifier
                );
                layer_ok = false;
            }
            t = lx.next_token();
            if t != Tok::Close {
                log::warn!("Structure.read_layer_scope: ) expected at '{}'", lx.scope_identifier);
                return false;
            }
        } else if t.is_kw(Kw::Rule) {
            let rules = read_rule_scope(lx);
            info.layer_dependent_rules.push((layer_string.clone(), rules));
        } else if t.is_kw(Kw::UseNet) {
            loop {
                lx.yybegin(LexState::Name);
                t = lx.next_token();
                match &t {
                    Tok::Close => break,
                    Tok::Str(s) => net_names.push(s.clone()),
                    Tok::Eof => return false,
                    _ => log::warn!("Structure.read_layer_scope: string expected at '{}'", lx.scope_identifier),
                }
            }
        } else {
            lx.skip_scope();
        }
        t = lx.next_token();
    }
    if layer_ok {
        info.layer_info.push(DsnLayer {
            name: layer_string,
            no: info.found_layer_count,
            is_signal,
            net_names,
        });
        info.found_layer_count += 1;
    }
    true
}

fn read_control_scope(ctx: &mut ReadCtx) -> bool {
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, ctx.lx.next_token());
        if next == Tok::Eof {
            log::warn!("Structure.read_control_scope: unexpected end of file at '{}'", ctx.lx.scope_identifier);
            return false;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            if next.is_kw(Kw::ViaAtSmd) {
                ctx.via_at_smd_allowed = read_on_off_scope(&mut ctx.lx);
            } else {
                ctx.lx.skip_scope();
            }
        }
    }
    true
}

pub fn read_snap_angle(lx: &mut Lexer) -> Option<AngleRestriction> {
    let t = lx.next_token();
    let r = if t.is_kw(Kw::NinetyDegree) {
        AngleRestriction::NinetyDegree
    } else if t.is_kw(Kw::FortyfiveDegree) {
        AngleRestriction::FortyfiveDegree
    } else if t.is_kw(Kw::None) {
        AngleRestriction::None
    } else {
        log::warn!("Structure.read_snap_angle_scope: unexpected token at '{}'", lx.scope_identifier);
        return None;
    };
    if lx.next_token() != Tok::Close {
        log::warn!("Structure.read_selection_layer_scop: closing bracket expected at '{}'", lx.scope_identifier);
        return None;
    }
    Some(r)
}

/// `PlaceControl.readFlipStyleRotateFirst`.
pub fn read_flip_style_rotate_first(lx: &mut Lexer) -> bool {
    let mut result = false;
    if lx.next_token().is_kw(Kw::RotateFirst) {
        result = true;
    }
    if lx.next_token() != Tok::Close {
        log::warn!("Structure.read_flip_style: closing bracket expected at '{}'", lx.scope_identifier);
        return false;
    }
    result
}

/// `Plane.readScope`.
pub fn read_plane_scope(ctx: &mut ReadCtx) -> bool {
    let skip_windows = ctx.host_cad.as_deref().map(|h| h.eq_ignore_ascii_case("allegro")).unwrap_or(false);
    let Tok::Str(net_name) = ctx.lx.next_token() else {
        log::warn!("Plane.read_scope: String expected at '{}'", ctx.lx.scope_identifier);
        return false;
    };
    ctx.lx.scope_identifier = net_name.clone();
    let area = read_area_scope(&mut ctx.lx, ctx.layer_structure.as_ref(), skip_windows);
    ctx.plane_list.push(PlaneInfo { area, net_name });
    true
}

fn ensure_layer_structure(ctx: &mut ReadCtx, info: &BoardConstructionInfo) {
    if ctx.layer_structure.is_none() {
        ctx.layer_structure = Some(DsnLayerStructure::new(info.layer_info.clone()));
    }
}

/// `Structure.readScope`.
pub fn read_structure_scope(ctx: &mut ReadCtx) -> bool {
    let mut info = BoardConstructionInfo::default();
    let mut flip_style_rotate_first = false;
    let mut keepout_list: Vec<Option<ReadAreaScopeResult>> = Vec::new();
    let mut via_keepout_list: Vec<Option<ReadAreaScopeResult>> = Vec::new();
    let mut place_keepout_list: Vec<Option<ReadAreaScopeResult>> = Vec::new();
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, ctx.lx.next_token());
        if next == Tok::Eof {
            log::warn!("Structure.read_scope: unexpected end of file at '{}'", ctx.lx.scope_identifier);
            return false;
        }
        if next == Tok::Close {
            break;
        }
        let mut read_ok = true;
        if prev == Tok::Open {
            match next {
                Tok::Kw(Kw::Boundary) => {
                    read_boundary_scope(&mut ctx.lx, &mut info);
                }
                Tok::Kw(Kw::Layer) => {
                    read_ok = read_layer_scope(&mut ctx.lx, &mut info);
                    if ctx.layer_structure.is_some() {
                        ctx.layer_structure = Some(DsnLayerStructure::new(info.layer_info.clone()));
                    }
                }
                Tok::Kw(Kw::Via) => ctx.via_padstack_names = read_via_padstacks(&mut ctx.lx),
                Tok::Kw(Kw::Rule) => info.default_rules.extend(read_rule_scope(&mut ctx.lx)),
                Tok::Kw(Kw::Keepout) => {
                    ensure_layer_structure(ctx, &info);
                    keepout_list.push(read_area_scope(&mut ctx.lx, ctx.layer_structure.as_ref(), false));
                }
                Tok::Kw(Kw::ViaKeepout) => {
                    ensure_layer_structure(ctx, &info);
                    via_keepout_list.push(read_area_scope(&mut ctx.lx, ctx.layer_structure.as_ref(), false));
                }
                Tok::Kw(Kw::PlaceKeepout) => {
                    ensure_layer_structure(ctx, &info);
                    place_keepout_list.push(read_area_scope(&mut ctx.lx, ctx.layer_structure.as_ref(), false));
                }
                Tok::Kw(Kw::PlaneScope) => {
                    ensure_layer_structure(ctx, &info);
                    read_plane_scope(ctx);
                }
                Tok::Kw(Kw::AutorouteSettings) => {
                    ensure_layer_structure(ctx, &info);
                    ctx.autoroute_settings = read_autoroute_settings(&mut ctx.lx, ctx.layer_structure.as_ref().unwrap());
                }
                Tok::Kw(Kw::Control) => read_ok = read_control_scope(ctx),
                Tok::Kw(Kw::FlipStyle) => flip_style_rotate_first = read_flip_style_rotate_first(&mut ctx.lx),
                Tok::Kw(Kw::SnapAngle) => {
                    if let Some(a) = read_snap_angle(&mut ctx.lx) {
                        ctx.snap_angle = a;
                    }
                }
                _ => {
                    ctx.lx.skip_scope();
                }
            }
        }
        if !read_ok {
            return false;
        }
    }
    let mut result = true;
    if ctx.board.is_none() {
        result = create_board(ctx, &mut info);
    }
    if ctx.board.is_none() {
        return false;
    }
    if flip_style_rotate_first {
        ctx.board.as_mut().unwrap().components.set_flip_style_rotate_first(true);
    }
    for (list, kind) in [
        (keepout_list, KeepoutType::Keepout),
        (via_keepout_list, KeepoutType::ViaKeepout),
        (place_keepout_list, KeepoutType::PlaceKeepout),
    ] {
        for area in list {
            let Some(area) = area else {
                return ctx.fatal("NullPointerException in Structure.insertKeepout");
            };
            if !insert_keepout(ctx, &area, kind, FixedState::SystemFixed) {
                return false;
            }
        }
    }
    // insert the planes
    let planes = std::mem::take(&mut ctx.plane_list);
    for plane in &planes {
        let net_id = NetId::new(&plane.net_name, 1);
        if !ctx.netlist.contains(&net_id) && ctx.netlist.add_net(net_id.clone()) {
            ctx.board.as_mut().unwrap().rules.nets.add(&net_id.name, net_id.subnet_number, true);
        }
        let ct = ctx.ct();
        let board = ctx.board.as_mut().unwrap();
        let Some(net) = board.rules.nets.get_by_name_subnet(&plane.net_name, 1) else {
            log::warn!("Plane.read_scope: net not found at '{}'", ctx.lx.scope_identifier);
            continue;
        };
        let net_no = net.net_number;
        let net_class = net.net_class;
        let Some(area) = plane.area.as_ref() else {
            ctx.plane_list = planes;
            return ctx.fatal("NullPointerException in Structure.readScope (plane)");
        };
        let plane_area = transform_area_to_board(&area.shape_list, &ct);
        let layer = area.layer();
        if layer.no() >= 0 {
            let cl = match &area.clearance_class_name {
                Some(name) => {
                    let c = board.rules.clearance_matrix.get_no(name);
                    if c < 0 {
                        log::warn!("Structure.read_scope: clearance class not found at '{}'", ctx.lx.scope_identifier);
                        BoardRules::clearance_class_none()
                    } else {
                        c
                    }
                }
                None => board.rules.net_classes.get(net_class).default_item_clearance_classes.get(ItemClass::Area),
            };
            match plane_area {
                Some(a) => {
                    board.insert_conduction_area(a, layer.no(), &[net_no], cl, false, FixedState::SystemFixed);
                }
                None => log::warn!("BasicBoard.insert_conduction_area: area is null"),
            }
        } else {
            log::warn!("Plane.read_scope: unexpected layer name at '{}'", ctx.lx.scope_identifier);
            ctx.plane_list = planes;
            return false;
        }
    }
    ctx.plane_list = planes;
    insert_missing_power_planes(ctx, &info.layer_info);
    result
}

fn insert_missing_power_planes(ctx: &mut ReadCtx, layer_info: &[DsnLayer]) {
    let board = ctx.board.as_ref().unwrap();
    let conduction_layers: Vec<i32> = board
        .items
        .iter()
        .filter(|i| i.is_conduction_area())
        .map(|i| i.as_obstacle().unwrap().layer)
        .collect();
    for layer in layer_info {
        if layer.is_signal {
            continue;
        }
        let found = conduction_layers.contains(&layer.no);
        if !found && !layer.net_names.is_empty() {
            let name = layer.net_names[0].clone();
            let net_id = NetId::new(&name, 1);
            if !ctx.netlist.contains(&net_id) && ctx.netlist.add_net(net_id.clone()) {
                ctx.board.as_mut().unwrap().rules.nets.add(&name, 1, true);
            }
            let board = ctx.board.as_mut().unwrap();
            let Some(net) = board.rules.nets.get_by_name_subnet(&name, 1) else {
                log::warn!("Structure.insert_missing_power_planes: net not found at '{name}'");
                continue;
            };
            let net_no = net.net_number;
            let bb = board.bounding_box;
            board.insert_conduction_area(
                Area::Shape(Shape::from(bb)),
                layer.no,
                &[net_no],
                BoardRules::clearance_class_none(),
                false,
                FixedState::SystemFixed,
            );
        }
    }
}

/// Calculates the shapes in `outline_shapes`, which are holes in the outline, removes them
/// and returns them.
fn separate_holes(outline_shapes: &mut Vec<Shape>) -> Vec<Shape> {
    struct OutlineShape {
        bounding_box: IntBox,
        convex: Option<Vec<TileShape>>,
        is_hole: bool,
    }
    let mut shapes: Vec<OutlineShape> = outline_shapes
        .iter()
        .map(|s| OutlineShape {
            bounding_box: s.bounding_box(),
            convex: match s {
                Shape::Polygon(p) => p.try_split_to_convex().map(|x| x.to_vec()),
                other => Some(other.split_to_convex()),
            },
            is_hole: false,
        })
        .collect();
    let n = shapes.len();
    for i in 0..n {
        for j in 0..n {
            if i == j || shapes[j].is_hole {
                continue;
            }
            if !shapes[j].bounding_box.contains_box(&shapes[i].bounding_box) {
                continue;
            }
            // shapes[j] contains all corners of shapes[i]?
            let contains_all = match &shapes[j].convex {
                None => false,
                Some(convex) => {
                    let cur = &outline_shapes[i];
                    (0..cur.border_line_count()).all(|k| {
                        let corner = cur.corner(k);
                        convex.iter().any(|c| c.contains(&corner))
                    })
                }
            };
            shapes[i].is_hole = contains_all;
        }
    }
    let mut holes = Vec::new();
    let mut kept = Vec::new();
    for (s, info) in outline_shapes.drain(..).zip(shapes.iter()) {
        if info.is_hole {
            holes.push(s);
        } else {
            kept.push(s);
        }
    }
    *outline_shapes = kept;
    holes
}

/// `Structure.setClearanceRule`: converts a DSN clearance rule into board clearance values.
/// If `layer` is negative, the rule is set on all layers. Returns true, if `smd_to_turn_gap`
/// was found.
pub fn set_clearance_rule(
    value: f64,
    pairs: &[String],
    layer: i32,
    ct: &CoordinateTransform,
    rules: &mut BoardRules,
    string_quote: &str,
) -> bool {
    let mut result = false;
    let clearance = round_i(ct.dsn_to_board(value));
    if pairs.is_empty() {
        if layer < 0 {
            rules.clearance_matrix.set_default_value(clearance);
        } else {
            rules.clearance_matrix.set_default_value_on_layer(layer as usize, clearance);
        }
        return result;
    }
    if contains_wire_clearance_pair(pairs) {
        create_default_clearance_classes(rules);
    }
    for current_string in pairs {
        if current_string.eq_ignore_ascii_case("smd_to_turn_gap") {
            rules.set_pin_edge_to_turn_dist(clearance as f64);
            result = true;
            continue;
        }
        let pair: [String; 2];
        if pairs.len() == 2 {
            let mut p0 = pairs[0].clone();
            let mut p1 = pairs[1].clone();
            // the loop of the original: replace quotes, strip a leading '_' of the second
            p0 = p0.replace('"', "");
            if let Some(s) = p1.strip_prefix('_') {
                p1 = s.to_string();
            }
            p1 = p1.replace('"', "");
            if let Some(s) = p1.strip_prefix('_') {
                p1 = s.to_string();
            }
            pair = [p0, p1];
        } else if !string_quote.is_empty() && current_string.starts_with(string_quote) {
            let rest = &current_string[string_quote.len()..];
            let parts: Vec<&str> = rest.splitn(2, string_quote).collect();
            if parts.len() != 2 || !parts[1].starts_with('_') {
                log::warn!("Structure.set_clearance_rule: '_' expected at '{rest}'");
                continue;
            }
            pair = [parts[0].to_string(), parts[1][1..].to_string()];
        } else {
            let parts: Vec<&str> = current_string.splitn(2, '_').collect();
            if parts.len() != 2 {
                continue;
            }
            pair = [parts[0].to_string(), parts[1].to_string()];
        }
        let mut first = if pair[0] == "wire" { 1 } else { rules.clearance_matrix.get_no(&pair[0]) };
        if first < 0 {
            first = append_clearance_class(rules, &pair[0]);
        }
        let mut second = if pair[1] == "wire" { 1 } else { rules.clearance_matrix.get_no(&pair[1]) };
        if second < 0 {
            second = append_clearance_class(rules, &pair[1]);
        }
        let (f, s) = (first as usize, second as usize);
        if layer < 0 {
            rules.clearance_matrix.set_value_all_layers(f, s, clearance);
            rules.clearance_matrix.set_value_all_layers(s, f, clearance);
        } else {
            rules.clearance_matrix.set_value(f, s, layer as usize, clearance);
            rules.clearance_matrix.set_value(s, f, layer as usize, clearance);
        }
    }
    result
}

pub fn contains_wire_clearance_pair(pairs: &[String]) -> bool {
    pairs.iter().any(|p| p.starts_with("wire_") || p.ends_with("_wire"))
}

fn create_default_clearance_classes(rules: &mut BoardRules) {
    for n in ["via", "smd", "pin", "area"] {
        append_clearance_class(rules, n);
    }
}

fn append_clearance_class(rules: &mut BoardRules, name: &str) -> i32 {
    rules.clearance_matrix.append_class(name);
    let result = rules.clearance_matrix.get_no(name);
    let dc = &mut rules.get_default_net_class_mut().default_item_clearance_classes;
    match name {
        "via" => dc.set(ItemClass::Via, result),
        "pin" => dc.set(ItemClass::Pin, result),
        "smd" => dc.set(ItemClass::Smd, result),
        "area" => dc.set(ItemClass::Area, result),
        _ => {}
    }
    result
}

fn update_board_rules(ctx: &ReadCtx, info: &BoardConstructionInfo, rules: &mut BoardRules) {
    let ct = ctx.ct();
    let mut smd_to_turn_gap_found = false;
    for r in &info.default_rules {
        if let Rule::Clearance { value, pairs } = r {
            if set_clearance_rule(*value, pairs, -1, &ct, rules, &ctx.string_quote) {
                smd_to_turn_gap_found = true;
            }
        }
    }
    for r in &info.default_rules {
        if let Rule::Width(w) = r {
            let hw = round_i(ct.dsn_to_board(*w) / 2.0);
            rules.set_default_trace_half_widths(hw);
        }
        // meander rules: meander constraints are not part of the Rust BoardRules
    }
    let ls = ctx.layer_structure.as_ref().unwrap();
    for (layer_name, layer_rules) in &info.layer_dependent_rules {
        let layer = ls.get_no(layer_name);
        if layer < 0 {
            continue;
        }
        for r in layer_rules {
            match r {
                Rule::Width(w) => {
                    let hw = round_i(ct.dsn_to_board(*w) / 2.0);
                    rules.set_default_trace_half_width(layer as usize, hw);
                }
                Rule::Clearance { value, pairs } => {
                    set_clearance_rule(*value, pairs, layer, &ct, rules, &ctx.string_quote);
                }
                _ => {}
            }
        }
    }
    if !smd_to_turn_gap_found {
        let v = rules.get_min_trace_half_width() as f64;
        rules.set_pin_edge_to_turn_dist(v);
    }
}

fn insert_keepout(ctx: &mut ReadCtx, area: &ReadAreaScopeResult, kind: KeepoutType, fixed: FixedState) -> bool {
    let ct = ctx.ct();
    let Some(keepout_area) = transform_area_to_board(&area.shape_list, &ct) else {
        return ctx.fatal("NullPointerException in Structure.insertKeepout");
    };
    if keepout_area.dimension() < 2 {
        log::warn!(
            "Keepout zone '{}' was skipped because its geometry is degenerate (e.g. zero-area polygon). This is likely a DSN export issue in your EDA tool. The board will be routed without this keepout constraint.",
            area.area_name.as_deref().unwrap_or("null")
        );
        return true;
    }
    if ctx.board.is_none() {
        log::warn!("Structure.insert_keepout: board not initialized");
        return false;
    }
    let layer = area.layer();
    let signal_layers: Vec<bool> = ctx.layer_structure.as_ref().unwrap().layers.iter().map(|l| l.is_signal).collect();
    let board = ctx.board.as_mut().unwrap();
    if layer == LayerRef::Signal {
        for i in 0..board.layer_count() {
            if signal_layers[i as usize] {
                insert_keepout_on_layer(board, keepout_area.clone(), i, area.clearance_class_name.as_deref(), kind, fixed);
            }
        }
    } else if layer.no() >= 0 {
        insert_keepout_on_layer(board, keepout_area, layer.no(), area.clearance_class_name.as_deref(), kind, fixed);
    } else {
        log::warn!("Structure.insert_keepout: unknown layer name at '{}'", ctx.lx.scope_identifier);
        return false;
    }
    true
}

fn insert_keepout_on_layer(
    board: &mut Board,
    area: Area,
    layer: i32,
    cl_name: Option<&str>,
    kind: KeepoutType,
    fixed: FixedState,
) {
    let cl = match cl_name {
        None => board.rules.get_default_net_class().default_item_clearance_classes.get(ItemClass::Area),
        Some(name) => {
            let c = board.rules.clearance_matrix.get_no(name);
            if c < 0 {
                log::warn!("Keepout.insert_keepout: clearance class not found at '{name}'");
                BoardRules::clearance_class_none()
            } else {
                c
            }
        }
    };
    let ok = match kind {
        KeepoutType::Keepout => ObstacleKind::Obstacle,
        KeepoutType::ViaKeepout => ObstacleKind::ViaObstacle,
        KeepoutType::PlaceKeepout => ObstacleKind::ComponentObstacle,
    };
    board.insert_obstacle_area(ok, area, layer, Vector::ZERO, 0.0, false, &[], cl, 0, None, fixed);
}

fn promote_power_layers_without_plane(ctx: &ReadCtx, info: &mut BoardConstructionInfo) {
    let mut with_plane = Vec::new();
    for p in &ctx.plane_list {
        if let Some(a) = &p.area {
            for s in a.shape_list.iter().flatten() {
                with_plane.push(s.layer.no());
            }
        }
    }
    for l in info.layer_info.iter_mut() {
        if !l.is_signal && l.net_names.is_empty() && !with_plane.contains(&l.no) {
            log::warn!(
                "Layer '{}' is declared as a power layer but no plane is defined for it. It will be treated as a signal layer.",
                l.name
            );
            l.is_signal = true;
        }
    }
}

fn create_board(ctx: &mut ReadCtx, info: &mut BoardConstructionInfo) -> bool {
    let layer_count = info.layer_info.len();
    if layer_count == 0 {
        log::warn!("Structure.create_board: layers missing in structure scope at '{}'", ctx.lx.scope_identifier);
        return false;
    }
    promote_power_layers_without_plane(ctx, info);
    if info.bounding_shape.is_none() {
        if info.outline_shapes.is_empty() {
            log::warn!("Structure.create_board: outline missing at '{}'", ctx.lx.scope_identifier);
            ctx.board_outline_ok = false;
            return false;
        }
        let mut bb: Option<[f64; 4]> = None;
        for s in &info.outline_shapes {
            let Some(b) = s.bounding_box() else {
                return ctx.fatal("NullPointerException in Structure.createBoard");
            };
            bb = Some(match bb {
                None => b,
                Some(a) => rect_union(&a, &b),
            });
        }
        let layer = info.outline_shapes[0].layer.clone();
        info.bounding_shape = Some(DsnShape::rect(layer, bb.unwrap()));
    }
    let Some(bounding_box) = info.bounding_shape.as_ref().unwrap().bounding_box() else {
        return ctx.fatal("NullPointerException in Structure.createBoard");
    };
    let mut board_layers = Vec::with_capacity(layer_count);
    for l in &info.layer_info {
        if l.no < 0 || l.no as usize >= layer_count {
            log::warn!("Structure.create_board: illegal layer number at '{}'", ctx.lx.scope_identifier);
            return false;
        }
        board_layers.push(Layer::new(&l.name, l.is_signal));
    }
    let board_layer_structure = LayerStructure::new(board_layers);
    ctx.layer_structure = Some(DsnLayerStructure::new(info.layer_info.clone()));

    let mut scale_factor = ctx.resolution.max(1) as f64;
    let mut max_coor: f64 = 0.0;
    for c in bounding_box {
        max_coor = max_coor.max((c * scale_factor).abs());
    }
    if max_coor == 0.0 {
        ctx.board_outline_ok = false;
        return false;
    }
    while 5.0 * max_coor >= CRIT_INT as f64 {
        scale_factor /= 10.0;
        max_coor /= 10.0;
    }
    let ct = CoordinateTransform::new(scale_factor, 0.0, 0.0);
    ctx.coordinate_transform = Some(ct);

    let bounds = match DsnShape::rect(LayerRef::Pcb, bounding_box).transform_to_board(&ct) {
        Some(Shape::Tile(TileShape::Box(b))) => b,
        _ => unreachable!(),
    };
    let bounds = bounds.offset(1000.0);

    let mut outline_shapes: Vec<Shape> = Vec::new();
    for s in &info.outline_shapes {
        let s = match &s.kind {
            DsnShapeKind::PolygonPath { width, coords } if *width != 0.0 => DsnShape {
                layer: s.layer.clone(),
                kind: DsnShapeKind::PolygonPath {
                    width: 0.0,
                    coords: coords.clone(),
                },
            },
            _ => s.clone(),
        };
        match s.transform_to_board(&ct) {
            Some(Shape::Circle(_)) => return ctx.fatal("ClassCastException in Structure.createBoard"),
            None => return ctx.fatal("NullPointerException in Structure.createBoard"),
            Some(shape) => {
                if shape.dimension() > 0 {
                    outline_shapes.push(shape);
                }
            }
        }
    }
    if outline_shapes.is_empty() {
        match info.bounding_shape.as_ref().unwrap().transform_to_board(&ct) {
            Some(Shape::Circle(_)) | None => return ctx.fatal("ClassCastException in Structure.createBoard"),
            Some(shape) => outline_shapes.push(shape),
        }
    }
    let hole_shapes = separate_holes(&mut outline_shapes);
    let clearance_matrix = ClearanceMatrix::get_default_instance(layer_count, 0);
    let mut rules = BoardRules::new(&board_layer_structure, clearance_matrix);
    let communication = Communication {
        coordinate_transform: Some(ct),
        unit: ctx.unit,
        resolution: ctx.resolution,
        specctra_parser_info: SpecctraParserInfo {
            string_quote: ctx.string_quote.clone(),
            host_cad: ctx.host_cad.clone(),
            host_version: ctx.host_version.clone(),
            constants: ctx.constants.clone(),
            write_resolution: ctx.write_resolution.clone(),
            dsn_file_generated_by_host: ctx.dsn_file_generated_by_host,
        },
        id_generator: IdGenerator::default(),
    };
    if communication.host_is_old_kicad() {
        log::warn!(
            "Structure.create_board: The DSN file was exported from an old KiCad version that has known compatibility issues. Please update KiCad to version 6 or newer."
        );
    }
    update_board_rules(ctx, info, &mut rules);
    rules.set_trace_angle_restriction(ctx.snap_angle);
    let outline_cl = match &info.outline_clearance_class_name {
        Some(name) => rules.clearance_matrix.get_no(name).max(0),
        None => rules.net_classes.get(NetClassId(0)).default_item_clearance_classes.get(ItemClass::Area),
    };
    let mut board = Board::new(bounds, board_layer_structure, outline_shapes, outline_cl, rules, communication);
    for hole in &hole_shapes {
        for i in 0..board.layer_count() {
            board.insert_obstacle(Area::Shape(hole.clone()), i, 0, FixedState::SystemFixed);
        }
    }
    ctx.board = Some(board);
    true
}
