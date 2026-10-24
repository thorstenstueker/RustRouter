//! Component placement scopes (port of `parser/Component`, `ComponentPlacement`, `PlaceControl`).

use super::lexer::{Kw, LexState, Lexer, Tok};
use super::reader::ReadCtx;
use super::scopes::read_string_scope;
use super::structure::read_flip_style_rotate_first;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ItemClearanceInfo {
    pub name: String,
    pub clearance_class: String,
}

/// The placement of a component (`ComponentPlacement.ComponentLocation`).
#[derive(Clone, Debug)]
pub struct ComponentLocation {
    pub name: String,
    /// None, if the component is not placed.
    pub coor: Option<[f64; 2]>,
    pub is_front: bool,
    pub rotation: f64,
    pub position_fixed: bool,
    pub pin_infos: BTreeMap<String, ItemClearanceInfo>,
    pub keepout_infos: BTreeMap<String, ItemClearanceInfo>,
    pub via_keepout_infos: BTreeMap<String, ItemClearanceInfo>,
    pub place_keepout_infos: BTreeMap<String, ItemClearanceInfo>,
    pub part_number: Option<String>,
}

/// All placements of a library component (`ComponentPlacement`).
#[derive(Clone, Debug)]
pub struct ComponentPlacement {
    pub lib_name: String,
    pub locations: Vec<ComponentLocation>,
}

fn read_component(lx: &mut Lexer) -> Option<ComponentPlacement> {
    let mut next = lx.next_token();
    let Tok::Str(name) = &next else {
        log::warn!("Component.read_scope: component name expected at '{}'", lx.scope_identifier);
        return None;
    };
    let mut cp = ComponentPlacement {
        lib_name: name.clone(),
        locations: Vec::new(),
    };
    let mut prev = next.clone();
    next = lx.next_token();
    while next != Tok::Close && next != Tok::Eof {
        if prev == Tok::Open && next.is_kw(Kw::Place) {
            cp.locations.push(read_place_scope(lx)?);
        }
        prev = next;
        next = lx.next_token();
    }
    Some(cp)
}

fn read_place_scope(lx: &mut Lexer) -> Option<ComponentLocation> {
    let mut loc = ComponentLocation {
        name: lx.next_string_nl(true),
        coor: None,
        is_front: true,
        rotation: 0.0,
        position_fixed: false,
        pin_infos: BTreeMap::new(),
        keepout_infos: BTreeMap::new(),
        via_keepout_infos: BTreeMap::new(),
        place_keepout_infos: BTreeMap::new(),
        part_number: None,
    };
    let mut location = [0.0; 2];
    for l in location.iter_mut() {
        let t = lx.next_token();
        if let Some(v) = t.as_number() {
            *l = v;
        } else if t == Tok::Close {
            // the component is not yet placed
            return Some(loc);
        } else {
            log::warn!(
                "Component.read_place_scope: Double was expected as the second and third parameter of the component/place command at '{}'",
                lx.scope_identifier
            );
            return None;
        }
    }
    let t = lx.next_token();
    let mut is_front = true;
    if t.is_kw(Kw::Back) {
        is_front = false;
    } else if !t.is_kw(Kw::Front) {
        log::warn!("Component.read_place_scope: Keyword.FRONT expected at '{}'", lx.scope_identifier);
    }
    let Some(rotation) = lx.next_token().as_number() else {
        log::warn!("Component.read_place_scope: number expected at '{}'", lx.scope_identifier);
        return None;
    };
    let mut t = lx.next_token();
    while t == Tok::Open {
        t = lx.next_token();
        match &t {
            Tok::Kw(Kw::LockType) => loc.position_fixed = read_lock_type(lx),
            Tok::Kw(Kw::Pin) => {
                let i = read_item_clearance_info(lx)?;
                loc.pin_infos.insert(i.name.clone(), i);
            }
            Tok::Kw(Kw::Keepout) => {
                let i = read_item_clearance_info(lx)?;
                loc.keepout_infos.insert(i.name.clone(), i);
            }
            Tok::Kw(Kw::ViaKeepout) => {
                let i = read_item_clearance_info(lx)?;
                loc.via_keepout_infos.insert(i.name.clone(), i);
            }
            Tok::Kw(Kw::PlaceKeepout) => {
                let i = read_item_clearance_info(lx)?;
                loc.place_keepout_infos.insert(i.name.clone(), i);
            }
            Tok::Str(s) if s.eq_ignore_ascii_case("PN") => loc.part_number = Some(read_string_scope(lx)),
            _ => {
                lx.skip_scope();
            }
        }
        t = lx.next_token();
    }
    if t != Tok::Close {
        log::warn!("Component.read_place_scope: ) expected at '{}'", lx.scope_identifier);
        return None;
    }
    loc.coor = Some(location);
    loc.is_front = is_front;
    loc.rotation = rotation;
    Some(loc)
}

fn read_item_clearance_info(lx: &mut Lexer) -> Option<ItemClearanceInfo> {
    lx.yybegin(LexState::Name);
    let Tok::Str(name) = lx.next_token() else {
        log::warn!("Component.read_item_clearance_info: String expected at '{}'", lx.scope_identifier);
        return None;
    };
    let mut cl = None;
    let mut t = lx.next_token();
    while t == Tok::Open {
        t = lx.next_token();
        let is_cl = t.is_kw(Kw::ClearanceClass)
            || matches!(&t, Tok::Str(s) if s.eq_ignore_ascii_case("clearance_class") || s.eq_ignore_ascii_case("clearanceClass"));
        if is_cl {
            cl = Some(read_string_scope(lx));
        } else {
            lx.skip_scope();
        }
        t = lx.next_token();
    }
    if t != Tok::Close {
        log::warn!("Component.read_item_clearance_info: ) expected at '{}'", lx.scope_identifier);
        return None;
    }
    let Some(cl) = cl else {
        log::warn!(
            "Component.read_item_clearance_info: clearance class name not found at '{}'",
            lx.scope_identifier
        );
        return None;
    };
    Some(ItemClearanceInfo {
        name,
        clearance_class: cl,
    })
}

fn read_lock_type(lx: &mut Lexer) -> bool {
    let mut result = false;
    loop {
        let t = lx.next_token();
        if t == Tok::Eof || t == Tok::Close {
            break;
        }
        if t.is_kw(Kw::Position) {
            result = true;
        }
    }
    result
}

/// `Component.readScope` (the scope keyword).
pub fn read_component_scope(ctx: &mut ReadCtx) -> bool {
    match read_component(&mut ctx.lx) {
        Some(c) => {
            ctx.placement_list.push(c);
            true
        }
        None => false,
    }
}

/// `PlaceControl.readScope`.
pub fn read_place_control_scope(ctx: &mut ReadCtx) -> bool {
    let mut flip = false;
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, ctx.lx.next_token());
        if next == Tok::Eof {
            log::warn!("PlaceControl.read_scope: unexpected end of file at '{}'", ctx.lx.scope_identifier);
            return false;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open && next.is_kw(Kw::FlipStyle) {
            flip = read_flip_style_rotate_first(&mut ctx.lx);
        }
    }
    if flip {
        match ctx.board.as_mut() {
            Some(b) => b.components.set_flip_style_rotate_first(true),
            None => return ctx.fatal("NullPointerException in PlaceControl.readScope"),
        }
    }
    true
}
