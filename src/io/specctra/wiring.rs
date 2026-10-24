//! The wiring scope (port of `parser/Wiring`).

use super::lexer::{Kw, Lexer, Tok};
use super::network::{strip_dot_digits, NetId};
use super::reader::ReadCtx;
use super::scopes::read_string_scope;
use super::shapes::{
    dsn_to_board_point, read_circle_scope, read_polygon_path_scope, read_polygon_scope, read_polyline_path_scope,
    read_rectangle_scope, read_shape_scope, round_i, transform_area_to_board, DsnShape, DsnShapeKind, LayerRef,
};
use crate::board::{Board, FixedState, ItemId};
use crate::geometry::{IntPoint, Line, Point, Polygon, Polyline, TileShape};
use crate::rules::{ItemClass, NetClassId, Nets};

fn read_net_id(lx: &mut Lexer) -> NetId {
    let mut subnet = 0;
    let name = lx.next_string();
    lx.scope_identifier = name.clone();
    let mut t = lx.next_token();
    if let Tok::Int(i) = t {
        subnet = i;
        t = lx.next_token();
    }
    if t != Tok::Close {
        log::warn!("Wiring.read_net_id: closing bracket expected at '{}'", lx.scope_identifier);
    }
    NetId::new(&name, subnet)
}

/// `Wiring.calcFixed`.
fn calc_fixed(lx: &mut Lexer) -> FixedState {
    let t = lx.next_token();
    let result = if t.is_kw(Kw::ShoveFixed) {
        FixedState::ShoveFixed
    } else if t.is_kw(Kw::Fix) {
        FixedState::SystemFixed
    } else if !t.is_kw(Kw::Normal) {
        FixedState::UserFixed
    } else {
        FixedState::Unfixed
    };
    if lx.next_token() != Tok::Close {
        log::warn!("Wiring.is_fixed: ) expected at '{}'", lx.scope_identifier);
        return FixedState::Unfixed;
    }
    result
}

/// The net numbers of the subnets of `net_id` and the net class of the last one.
fn get_subnets(net_id: Option<&NetId>, nets: &Nets) -> (Vec<i32>, Option<NetClassId>) {
    let mut found = Vec::new();
    let mut class = None;
    if let Some(id) = net_id {
        if id.subnet_number > 0 {
            if let Some(n) = nets.get_by_name_subnet(&id.name, id.subnet_number) {
                found.push(n.net_number);
                class = Some(n.net_class);
            }
        } else {
            for n in nets.get_by_name(&id.name) {
                found.push(n.net_number);
                class = Some(n.net_class);
            }
        }
    }
    (found, class)
}

/// `Wiring.readScope`.
pub fn read_wiring_scope(ctx: &mut ReadCtx) -> bool {
    if ctx.board.is_none() {
        return ctx.fatal("NullPointerException in Wiring.readScope: board missing");
    }
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, ctx.lx.next_token());
        if next == Tok::Eof {
            log::warn!("Wiring.read_scope: unexpected end of file at '{}'", ctx.lx.scope_identifier);
            return false;
        }
        if next == Tok::Close {
            break;
        }
        let mut read_ok = true;
        if prev == Tok::Open {
            if next.is_kw(Kw::Wire) {
                read_wire_scope(ctx);
            } else if next.is_kw(Kw::Via) {
                read_ok = read_via_scope(ctx);
            } else {
                ctx.lx.skip_scope();
            }
        }
        if !read_ok {
            return false;
        }
    }
    ctx.board.as_mut().unwrap().normalize_all_traces();
    true
}

fn read_wire_scope(ctx: &mut ReadCtx) -> Option<ItemId> {
    let mut net_id: Option<NetId> = None;
    let mut cl_name: Option<String> = None;
    let mut fixed = FixedState::Unfixed;
    let mut path: Option<DsnShape> = None;
    let mut border: Option<DsnShape> = None;
    let mut holes: Vec<Option<DsnShape>> = Vec::new();
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, ctx.lx.next_token());
        if next == Tok::Eof {
            log::warn!("Wiring.read_wire_scope: unexpected end of file at '{}'", ctx.lx.scope_identifier);
            return None;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            let ls = ctx.layer_structure.as_ref();
            match next {
                Tok::Kw(Kw::PolygonPath) => path = read_polygon_path_scope(&mut ctx.lx, ls),
                Tok::Kw(Kw::PolylinePath) => path = read_polyline_path_scope(&mut ctx.lx, ls),
                Tok::Kw(Kw::Rectangle) => border = read_rectangle_scope(&mut ctx.lx, ls),
                Tok::Kw(Kw::Polygon) => border = read_polygon_scope(&mut ctx.lx, ls),
                Tok::Kw(Kw::Circle) => border = read_circle_scope(&mut ctx.lx, ls),
                Tok::Kw(Kw::Window) => {
                    let hole = read_shape_scope(&mut ctx.lx, ls);
                    holes.push(hole);
                    next = ctx.lx.next_token();
                    if next != Tok::Close {
                        log::warn!("Wiring.read_wire_scope: closing bracket expected at '{}'", ctx.lx.scope_identifier);
                        return None;
                    }
                }
                Tok::Kw(Kw::Net) => net_id = Some(read_net_id(&mut ctx.lx)),
                Tok::Kw(Kw::ClearanceClass) => cl_name = Some(read_string_scope(&mut ctx.lx)),
                Tok::Kw(Kw::Type) => fixed = calc_fixed(&mut ctx.lx),
                _ => {
                    ctx.lx.skip_scope();
                }
            }
        }
    }
    if path.is_none() && border.is_none() {
        let msg = format!("Wiring: wire has no shape at '{}'", ctx.lx.scope_identifier);
        ctx.warn(msg);
        return None;
    }
    let ct = ctx.ct();
    let board = ctx.board.as_mut().unwrap();
    let (nets, class) = get_subnets(net_id.as_ref(), &board.rules.nets);
    let net_class = class.unwrap_or(NetClassId(0));
    let mut cl = -1;
    if let Some(n) = &cl_name {
        cl = board.rules.clearance_matrix.get_no(n);
    }
    let shape = path.as_ref().or(border.as_ref()).unwrap();
    if shape.layer == LayerRef::Missing {
        ctx.fatal("NullPointerException in Wiring.readWireScope");
        return None;
    }
    let layer = shape.layer.no();
    let half_width = match path.as_ref().and_then(|p| p.path_width_coords()) {
        Some((w, _)) => round_i(ct.dsn_to_board(w / 2.0)),
        None => 0,
    };
    if layer < 0 || layer >= board.layer_count() {
        let msg = format!(
            "Wiring: wire ignored — unknown layer '{}' at '{}'",
            shape.layer.name(),
            ctx.lx.scope_identifier
        );
        ctx.warn(msg);
        return None;
    }
    let bounding_box = TileShape::Box(board.bounding_box);
    let dc = board.rules.net_classes.get(net_class).default_item_clearance_classes.clone();
    let mut result: Option<ItemId> = None;
    if let Some(b) = border {
        if cl < 0 {
            cl = dc.get(ItemClass::Area);
        }
        let mut area = vec![Some(b)];
        area.extend(holes);
        match transform_area_to_board(&area, &ct) {
            Some(a) => result = Some(board.insert_conduction_area(a, layer, &nets, cl, false, fixed)),
            None => log::warn!("BasicBoard.insert_conduction_area: area is null"),
        }
    } else {
        let path = path.unwrap();
        if cl < 0 {
            cl = dc.get(ItemClass::Trace);
        }
        match &path.kind {
            DsnShapeKind::PolygonPath { coords, .. } => {
                let mut corners = Vec::with_capacity(coords.len() / 2);
                for i in 0..coords.len() / 2 {
                    let (x, y) = (coords[2 * i], coords[2 * i + 1]);
                    let c = dsn_to_board_point(&ct, x, y);
                    if !bounding_box.contains_float(&c) {
                        let msg = format!(
                            "Wiring: wire corner ({},{}) is outside board bounds at '{}'",
                            x as i32, y as i32, ctx.lx.scope_identifier
                        );
                        ctx.warn(msg);
                        return None;
                    }
                    corners.push(Point::Int(c.round()));
                }
                let polygon = Polygon::new(&corners);
                let pc = polygon.corner_array();
                let distinct = pc.iter().skip(1).any(|p| *p != pc[0]);
                if pc.len() >= 2 && distinct {
                    let polyline = Polyline::from_polygon(&polygon);
                    result = board.insert_trace_without_cleaning(polyline, layer, half_width, &nets, cl, fixed);
                } else {
                    ctx.warnings.push(format!(
                        "Wiring: degenerate wire trace skipped (all {} corners are identical — zero-length trace) on layer '{}'. This is likely a DSN export issue in your EDA tool.",
                        pc.len(),
                        path.layer.name()
                    ));
                }
            }
            DsnShapeKind::PolylinePath { coords, .. } => {
                let mut lines = Vec::with_capacity(coords.len() / 4);
                for i in 0..coords.len() / 4 {
                    let a = dsn_to_board_point(&ct, coords[4 * i], coords[4 * i + 1]).round();
                    let b = dsn_to_board_point(&ct, coords[4 * i + 2], coords[4 * i + 3]).round();
                    lines.push(Line::new(a, b));
                }
                let polyline = Polyline::from_lines(&lines);
                result = board.insert_trace_without_cleaning(polyline, layer, half_width, &nets, cl, fixed);
            }
            _ => {
                log::warn!("Wiring.read_wire_scope: unexpected Path subclass at '{}'", ctx.lx.scope_identifier);
                return None;
            }
        }
    }
    if let Some(id) = result {
        if board.item(id).net_count() == 0 {
            try_correct_net(board, id);
        }
    }
    result
}

/// Maybe a trace of type turret without net in a Mentor design: assigns the net of a contact.
fn try_correct_net(board: &mut Board, id: ItemId) {
    let Some(t) = board.item(id).as_trace() else { return };
    let (first, last) = (t.first_corner(), t.last_corner());
    let mut contacts = board.trace_contacts_at(id, &first, true);
    contacts.extend(board.trace_contacts_at(id, &last, true));
    let mut corrected = 0;
    for c in &contacts {
        let item = board.item(*c);
        if item.net_count() == 1 {
            corrected = item.get_net_number(0);
            break;
        }
    }
    if corrected != 0 {
        board.assign_net_no(id, corrected);
    }
}

fn via_exists(board: &Board, location: &IntPoint, padstack: crate::library::PadstackId, nets: &[i32]) -> bool {
    let ps = board.library.padstacks.get(padstack);
    let (from, to) = (ps.from_layer(), ps.to_layer());
    let loc = Point::Int(*location);
    for id in board.pick_items(&loc, from) {
        let item = board.item(id);
        let Some(via) = item.as_via() else { continue };
        if item.nets_equal(nets) && via.drill.center == loc && item.first_layer() == from && item.last_layer_with(board.layer_count()) == to {
            return true;
        }
    }
    false
}

fn read_via_scope(ctx: &mut ReadCtx) -> bool {
    let mut fixed = FixedState::Unfixed;
    let Tok::Str(padstack_name) = ctx.lx.next_token() else {
        log::warn!("Wiring.read_via_scope: padstack name expected at '{}'", ctx.lx.scope_identifier);
        return false;
    };
    ctx.lx.scope_identifier = padstack_name.clone();
    let mut location = [0.0; 2];
    let mut next = Tok::Eof;
    for l in location.iter_mut() {
        next = ctx.lx.next_token();
        match next.as_number() {
            Some(v) => *l = v,
            None => {
                log::warn!("Wiring.read_via_scope: number expected at '{}'", ctx.lx.scope_identifier);
                return false;
            }
        }
    }
    let mut net_id: Option<NetId> = None;
    let mut cl_name: Option<String> = None;
    loop {
        let prev = std::mem::replace(&mut next, ctx.lx.next_token());
        if next == Tok::Eof {
            log::warn!("Wiring.read_via_scope: unexpected end of file at '{}'", ctx.lx.scope_identifier);
            return false;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            if next.is_kw(Kw::Net) {
                net_id = Some(read_net_id(&mut ctx.lx));
            } else if next.is_kw(Kw::ClearanceClass) {
                cl_name = Some(read_string_scope(&mut ctx.lx));
            } else if next.is_kw(Kw::Type) {
                fixed = calc_fixed(&mut ctx.lx);
            } else {
                ctx.lx.skip_scope();
            }
        }
    }
    let ct = ctx.ct();
    let via_at_smd = ctx.via_at_smd_allowed;
    let board = ctx.board.as_mut().unwrap();
    let ps = board
        .library
        .padstacks
        .get_by_name(&padstack_name)
        .or_else(|| board.library.padstacks.get_by_name(&strip_dot_digits(&padstack_name)));
    let Some(ps) = ps else {
        let msg = format!("Wiring: via padstack '{}' not found at '{}'", padstack_name, ctx.lx.scope_identifier);
        ctx.warn(msg);
        return false;
    };
    let (nets, class) = get_subnets(net_id.as_ref(), &board.rules.nets);
    if let Some(id) = &net_id {
        if nets.is_empty() {
            let msg = format!("Wiring: via net '{}' not found at '{}'", id.name, ctx.lx.scope_identifier);
            log::warn!("{msg}");
            ctx.warnings.push(msg);
        }
    }
    let net_class = class.unwrap_or(NetClassId(0));
    let mut cl = -1;
    if let Some(n) = &cl_name {
        cl = board.rules.clearance_matrix.get_no(n);
    }
    if cl < 0 {
        cl = board.rules.net_classes.get(net_class).default_item_clearance_classes.get(ItemClass::Via);
    }
    let board_location = dsn_to_board_point(&ct, location[0], location[1]).round();
    if via_exists(board, &board_location, ps, &nets) {
        let msg = format!("Wiring: duplicate via skipped at ({}, {})", board_location.x, board_location.y);
        ctx.warn(msg);
    } else {
        let padstack = board.library.padstacks.get(ps);
        let attach = via_at_smd && padstack.attach_allowed;
        let has_aux = padstack.has_auxiliary_shapes();
        let ps_name = padstack.name.clone();
        board.insert_via(ps, &Point::Int(board_location), &nets, cl, fixed, attach);
        if has_aux {
            let msg = format!(
                "Wiring: Via at ({}, {}) uses non-convex padstack '{}'. Non-convex geometry on vias is not routable and auxiliary companion tiles are omitted.",
                board_location.x, board_location.y, ps_name
            );
            ctx.warn(msg);
        }
    }
    true
}
