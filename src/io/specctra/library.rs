//! The library scope (port of `parser/Library` and `parser/Package`).

use super::lexer::{java_parse_double, Kw, LexState, Lexer, Tok};
use super::network::strip_dot_digits;
use super::reader::ReadCtx;
use super::scopes::read_on_off_scope;
use super::shapes::{
    dsn_to_board_rel, read_area_scope, read_shape_scope, transform_area_to_board_rel, DsnLayerStructure, DsnShape,
    LayerRef, ReadAreaScopeResult,
};
use crate::board::CoordinateTransform;
use crate::geometry::{Point, Shape, TileShape};
use crate::library::{Keepout, PackagePin, Packages, Padstacks};

/// `Package.PinInfo`.
struct PinInfo {
    padstack_name: String,
    pin_name: String,
    rel_coor: [f64; 2],
    rotation: f64,
}

/// A package read from the library scope (`parser/Package`).
struct DsnPackage {
    name: String,
    pins: Vec<PinInfo>,
    outline: Vec<DsnShape>,
    keepouts: Vec<ReadAreaScopeResult>,
    via_keepouts: Vec<ReadAreaScopeResult>,
    place_keepouts: Vec<ReadAreaScopeResult>,
    is_front: bool,
}

/// `Library.readPadstackScope`.
pub(super) fn read_padstack_scope(
    lx: &mut Lexer,
    ls: &DsnLayerStructure,
    ct: &CoordinateTransform,
    padstacks: &mut Padstacks,
) -> bool {
    let mut is_drillable = true;
    let mut placed_absolute = false;
    let mut shape_list: Vec<DsnShape> = Vec::new();
    let mut next = lx.next_token();
    let Tok::Str(name) = next.clone() else {
        log::warn!("Library.read_padstack_scope: unexpected padstack identifier at '{}'", lx.scope_identifier);
        return false;
    };
    lx.scope_identifier = name.clone();
    while next != Tok::Close {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof {
            log::warn!("Library.read_padstack_scope: unexpected end of file at '{}'", lx.scope_identifier);
            return false;
        }
        if prev == Tok::Open {
            if next.is_kw(Kw::Shape) {
                if let Some(s) = read_shape_scope(lx, Some(ls)) {
                    shape_list.push(s);
                }
                let mut t = lx.next_token();
                while t == Tok::Open {
                    lx.skip_scope();
                    t = lx.next_token();
                }
                if t != Tok::Close {
                    log::warn!("Library.read_padstack_scope: closing bracket expected at '{}'", lx.scope_identifier);
                    return false;
                }
            } else if next.is_kw(Kw::Attach) {
                is_drillable = read_on_off_scope(lx);
            } else if next.is_kw(Kw::Absolute) {
                placed_absolute = read_on_off_scope(lx);
            } else {
                lx.skip_scope();
            }
        }
    }
    if let Some(existing) = padstacks.get_by_name(&name) {
        if padstacks.get(existing).name == name {
            return true;
        }
    }
    if shape_list.is_empty() {
        log::warn!("Library.read_padstack_scope: shape not found for padstack with name '{name}'");
        return true;
    }
    let layer_count = ls.layers.len();
    let mut is_generic_default = vec![false; layer_count];
    let mut layer_pieces: Vec<Vec<Shape>> = vec![Vec::new(); layer_count];
    for pad_shape in &shape_list {
        let current = pad_shape.transform_to_board_rel(ct);
        let mut pieces: Vec<Shape> = Vec::new();
        match &current {
            Some(s @ (Shape::Tile(_) | Shape::Circle(_))) => pieces.push(s.clone()),
            _ => {
                let convex: Option<Vec<TileShape>> = match &current {
                    Some(Shape::Polygon(p)) => p.try_split_to_convex().map(|x| x.to_vec()),
                    _ => None,
                };
                match convex {
                    Some(c) if !c.is_empty() => {
                        for piece in c {
                            pieces.push(Shape::Tile(simplify_simplex(piece)));
                        }
                    }
                    _ => {
                        let fallback: Option<Vec<TileShape>> = match &current {
                            Some(Shape::Polygon(p)) => {
                                let hull = p.convex_hull();
                                hull.try_split_to_convex().map(|x| x.to_vec())
                            }
                            _ => None,
                        };
                        if let Some(f) = &fallback {
                            if f.len() != 1 {
                                log::warn!(
                                    "Library.read_padstack_scope: convex shape expected at '{}'",
                                    lx.scope_identifier
                                );
                            }
                        }
                        if let Some(first) = fallback.and_then(|f| f.into_iter().next()) {
                            pieces.push(Shape::Tile(simplify_simplex(first)));
                        }
                    }
                }
            }
        }
        let mut validated = Vec::with_capacity(pieces.len());
        for mut p in pieces {
            if p.dimension() < 2 {
                log::warn!(
                    "Library.read_padstack_scope: the shape of padstack '{name}' is not an area. We will enlarge it as a workaround, but it may result in unintended consequences."
                );
                p = match p {
                    Shape::Tile(t) => Shape::Tile(t.offset(1.0)),
                    Shape::Circle(c) => Shape::Circle(c.offset(1.0)),
                    other => other,
                };
            }
            if p.dimension() >= 2 {
                validated.push(p);
            }
        }
        if matches!(pad_shape.layer, LayerRef::Pcb | LayerRef::Signal) {
            for l in 0..layer_count {
                if !is_generic_default[l] && layer_pieces[l].is_empty() {
                    is_generic_default[l] = true;
                }
                layer_pieces[l].extend(validated.iter().cloned());
            }
        } else {
            let shape_layer = ls.get_no(pad_shape.layer.name());
            if shape_layer < 0 || shape_layer as usize >= layer_count {
                log::warn!("Library.read_padstack_scope: layer number found at '{}'", lx.scope_identifier);
                return false;
            }
            let sl = shape_layer as usize;
            if is_generic_default[sl] {
                layer_pieces[sl].clear();
                is_generic_default[sl] = false;
            }
            layer_pieces[sl].extend(validated);
        }
    }
    let mut shapes: Vec<Option<Shape>> = vec![None; layer_count];
    let mut aux: Vec<Option<Vec<Shape>>> = vec![None; layer_count];
    let mut has_non_convex = false;
    for l in 0..layer_count {
        let pieces = &layer_pieces[l];
        if pieces.is_empty() {
            continue;
        }
        if pieces.len() == 1 {
            shapes[l] = Some(pieces[0].clone());
            continue;
        }
        has_non_convex = true;
        log::warn!(
            "Library.read_padstack_scope: Padstack '{}' contains non-convex custom geometry ({} convex tiles on layer {}). Decomposing into primary core pin and auxiliary copper obstacles.",
            name,
            pieces.len(),
            l
        );
        let mut core: i32 = pieces.iter().position(|p| p.contains(&Point::ZERO)).map(|i| i as i32).unwrap_or(-1);
        if core < 0 {
            let mut max_area = -1.0;
            for (i, p) in pieces.iter().enumerate() {
                let a = p.area();
                if a > max_area {
                    max_area = a;
                    core = i as i32;
                }
            }
        }
        if core < 0 {
            core = 0;
        }
        let core = core as usize;
        shapes[l] = Some(pieces[core].clone());
        aux[l] = Some(pieces.iter().enumerate().filter(|(i, _)| *i != core).map(|(_, p)| p.clone()).collect());
    }
    let id = padstacks.add(&name, shapes, Some(aux), is_drillable, placed_absolute);
    if has_non_convex {
        padstacks.get_mut(id).has_non_convex_geometry = true;
    }
    true
}

fn simplify_simplex(t: TileShape) -> TileShape {
    match t {
        TileShape::Simplex(_) => t.simplify(),
        other => other,
    }
}

/// `Package.readScope`.
fn read_package_scope(lx: &mut Lexer, ls: &DsnLayerStructure) -> Option<DsnPackage> {
    let mut next = lx.next_token();
    let Tok::Str(name) = next.clone() else {
        log::warn!("Package.read_scope: String expected at '{}'", lx.scope_identifier);
        return None;
    };
    lx.scope_identifier = name.clone();
    let mut p = DsnPackage {
        name,
        pins: Vec::new(),
        outline: Vec::new(),
        keepouts: Vec::new(),
        via_keepouts: Vec::new(),
        place_keepouts: Vec::new(),
        is_front: true,
    };
    loop {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof {
            log::warn!("Package.read_scope: unexpected end of file at '{}'", lx.scope_identifier);
            return None;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            match next {
                Tok::Kw(Kw::Pin) => p.pins.push(read_pin_info(lx)?),
                Tok::Kw(Kw::Side) => p.is_front = read_placement_side(lx),
                Tok::Kw(Kw::Outline) => {
                    if let Some(s) = read_shape_scope(lx, Some(ls)) {
                        p.outline.push(s);
                    }
                    next = lx.next_token();
                    if next != Tok::Close {
                        log::warn!("Package.read_scope: closed bracket expected at '{}'", lx.scope_identifier);
                        return None;
                    }
                }
                Tok::Kw(Kw::Keepout) => match read_area_scope(lx, Some(ls), false) {
                    Some(a) => p.keepouts.push(a),
                    None => log::error!("Package.read_scope: could not read keepout area of package '{}'", p.name),
                },
                Tok::Kw(Kw::ViaKeepout) => {
                    if let Some(a) = read_area_scope(lx, Some(ls), false) {
                        p.via_keepouts.push(a);
                    }
                }
                Tok::Kw(Kw::PlaceKeepout) => {
                    if let Some(a) = read_area_scope(lx, Some(ls), false) {
                        p.place_keepouts.push(a);
                    }
                }
                _ => {
                    lx.skip_scope();
                }
            }
        }
    }
    Some(p)
}

fn token_name(t: &Tok) -> Option<String> {
    match t {
        Tok::Str(s) => Some(s.clone()),
        Tok::Int(i) => Some(i.to_string()),
        _ => None,
    }
}

fn read_pin_info(lx: &mut Lexer) -> Option<PinInfo> {
    lx.yybegin(LexState::Name);
    let Some(padstack_name) = token_name(&lx.next_token()) else {
        log::warn!("Package.read_pin_info: String or Integer expected at '{}'", lx.scope_identifier);
        return None;
    };
    let mut rotation = 0.0;
    lx.yybegin(LexState::Name);
    let mut next = lx.next_token();
    if next == Tok::Open {
        next = lx.next_token();
        if next.is_kw(Kw::Rotate) {
            rotation = read_rotation(lx)?;
        } else {
            lx.skip_scope();
        }
        lx.yybegin(LexState::Name);
        next = lx.next_token();
    }
    let Some(pin_name) = token_name(&next) else {
        log::warn!("Package.read_pin_info: String or Integer expected at '{}'", lx.scope_identifier);
        return None;
    };
    let mut coor = [0.0; 2];
    for c in coor.iter_mut() {
        next = lx.next_token();
        match next.as_number() {
            Some(v) => *c = v,
            None => {
                log::warn!("Package.read_pin_info: number expected at '{}'", lx.scope_identifier);
                return None;
            }
        }
    }
    loop {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof {
            log::warn!("Package.read_pin_info: unexpected end of file at '{}'", lx.scope_identifier);
            return None;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            if next.is_kw(Kw::Rotate) {
                rotation = read_rotation(lx)?;
            } else {
                lx.skip_scope();
            }
        }
    }
    Some(PinInfo {
        padstack_name,
        pin_name,
        rel_coor: coor,
        rotation,
    })
}

/// `Package.readRotation`. None, if the original throws a NumberFormatException.
fn read_rotation(lx: &mut Lexer) -> Option<f64> {
    let s = lx.next_string();
    let Some(result) = java_parse_double(&s) else {
        lx.set_fatal(format!("NumberFormatException: For input string: \"{s}\""));
        return None;
    };
    if lx.next_token() != Tok::Close {
        log::warn!("Package.read_rotation: closing bracket expected at '{}'", lx.scope_identifier);
    }
    Some(result)
}

fn read_placement_side(lx: &mut Lexer) -> bool {
    let result = !lx.next_token().is_kw(Kw::Back);
    if lx.next_token() != Tok::Close {
        log::warn!("Package.read_placement_side: closing bracket expected at '{}'", lx.scope_identifier);
    }
    result
}

fn generate_missing_keepout_names(prefix: &str, list: &mut [ReadAreaScopeResult]) {
    if list.iter().all(|k| k.area_name.is_some()) {
        return;
    }
    for (i, k) in list.iter_mut().enumerate() {
        k.area_name = Some(format!("{}{}", prefix, i + 1));
    }
}

fn convert_keepouts(list: &[ReadAreaScopeResult], ct: &CoordinateTransform) -> Vec<Keepout> {
    let mut result = Vec::new();
    for k in list {
        let layer = k.layer().no();
        // a missing area leads to no item on the board in the original
        if let Some(area) = transform_area_to_board_rel(&k.shape_list, ct) {
            result.push(Keepout {
                name: k.area_name.clone().unwrap_or_default(),
                area,
                layer,
            });
        }
    }
    result
}

/// `Library.readScope`.
pub fn read_library_scope(ctx: &mut ReadCtx) -> bool {
    let Some(board) = ctx.board.as_mut() else {
        return ctx.fatal("NullPointerException in Library.readScope: board missing");
    };
    let ls = ctx.layer_structure.clone().unwrap_or_default();
    let ct = ctx.coordinate_transform.unwrap_or(CoordinateTransform::new(1.0, 0.0, 0.0));
    board.library.padstacks = Padstacks::new(board.layer_structure.count());
    let mut package_list = Vec::new();
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, ctx.lx.next_token());
        if next == Tok::Eof {
            log::warn!("Library.read_scope: unexpected end of file at '{}'", ctx.lx.scope_identifier);
            return false;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            if next.is_kw(Kw::Padstack) {
                if !read_padstack_scope(&mut ctx.lx, &ls, &ct, &mut board.library.padstacks) {
                    return false;
                }
            } else if next.is_kw(Kw::Image) {
                match read_package_scope(&mut ctx.lx, &ls) {
                    Some(p) => package_list.push(p),
                    None => return false,
                }
            } else {
                ctx.lx.skip_scope();
            }
        }
    }
    let mut packages = Packages::default();
    for mut p in package_list {
        let mut pins = Vec::with_capacity(p.pins.len());
        for pi in &p.pins {
            let exact_rel = dsn_to_board_rel(&ct, pi.rel_coor[0], pi.rel_coor[1]);
            let ps = board
                .library
                .padstacks
                .get_by_name(&pi.padstack_name)
                .or_else(|| board.library.padstacks.get_by_name(&strip_dot_digits(&pi.padstack_name)));
            let Some(ps) = ps else {
                log::warn!(
                    "Library.read_scope: board padstack '{}' not found at '{}'",
                    pi.padstack_name,
                    ctx.lx.scope_identifier
                );
                return false;
            };
            pins.push(PackagePin::new(&pi.pin_name, ps, exact_rel, pi.rotation));
        }
        let mut outlines = Vec::new();
        let mut widths = Vec::new();
        let mut closed = Vec::new();
        for s in &p.outline {
            match s.transform_to_board_rel(&ct) {
                Some(shape) => {
                    outlines.push(shape);
                    match s.path_width_coords() {
                        Some((w, c)) => {
                            widths.push(w);
                            closed.push(c.len() >= 4 && c[0] == c[c.len() - 2] && c[1] == c[c.len() - 1]);
                        }
                        None => {
                            widths.push(0.0);
                            closed.push(true);
                        }
                    }
                }
                None => log::warn!(
                    "Library.read_scope: outline shape is null at '{}'",
                    ctx.lx.scope_identifier
                ),
            }
        }
        generate_missing_keepout_names("keepout_", &mut p.keepouts);
        generate_missing_keepout_names("via_keepout_", &mut p.via_keepouts);
        generate_missing_keepout_names("place_keepout_", &mut p.place_keepouts);
        let keepouts = convert_keepouts(&p.keepouts, &ct);
        let via_keepouts = convert_keepouts(&p.via_keepouts, &ct);
        let place_keepouts = convert_keepouts(&p.place_keepouts, &ct);
        packages.add(&p.name, pins, outlines, widths, closed, keepouts, via_keepouts, place_keepouts, p.is_front);
    }
    board.library.packages = packages;
    true
}
