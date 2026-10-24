//! The `autoroute_settings` scope of the structure (port of `parser/AutorouteSettings`).
//!
//! The original fills a `RouterSettings` object. Here the values found in the file are
//! returned in a plain struct; `None` means "not given in the file".

use super::lexer::{Kw, LexState, Lexer, Tok};
use super::scopes::{read_float_scope, read_integer_scope, read_on_off_scope};
use super::shapes::DsnLayerStructure;

/// Layer dependent autoroute settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DsnLayerAutorouteSettings {
    pub active: Option<bool>,
    pub preferred_direction_is_horizontal: Option<bool>,
    pub preferred_direction_trace_costs: Option<f64>,
    pub against_preferred_direction_trace_costs: Option<f64>,
}

/// The autoroute settings of a DSN file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DsnAutorouteSettings {
    /// `(autoroute on|off)`, default on.
    pub run_router: bool,
    /// `(postroute on|off)`, default on.
    pub run_optimizer: bool,
    pub vias_allowed: Option<bool>,
    /// Values as set by `RouterSettings` (clamped to at least 1).
    pub via_costs: Option<i32>,
    pub plane_via_costs: Option<i32>,
    pub start_ripup_costs: Option<i32>,
    /// One entry per board layer.
    pub layers: Vec<DsnLayerAutorouteSettings>,
}

/// `AutorouteSettings.readScope`.
pub fn read_autoroute_settings(lx: &mut Lexer, ls: &DsnLayerStructure) -> Option<DsnAutorouteSettings> {
    let mut result = DsnAutorouteSettings {
        layers: vec![DsnLayerAutorouteSettings::default(); ls.layers.len()],
        ..Default::default()
    };
    let mut with_autoroute = true;
    let mut with_postroute = true;
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof {
            log::warn!("AutorouteSettings.read_scope: unexpected end of file at '{}'", lx.scope_identifier);
            return None;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            match next {
                Tok::Kw(Kw::Fanout) => {
                    read_on_off_scope(lx);
                }
                Tok::Kw(Kw::Autoroute) => with_autoroute = read_on_off_scope(lx),
                Tok::Kw(Kw::Postroute) => with_postroute = read_on_off_scope(lx),
                Tok::Kw(Kw::Vias) => result.vias_allowed = Some(read_on_off_scope(lx)),
                Tok::Kw(Kw::ViaCosts) => result.via_costs = Some(read_integer_scope(lx).max(1)),
                Tok::Kw(Kw::PlaneViaCosts) => result.plane_via_costs = Some(read_integer_scope(lx).max(1)),
                Tok::Kw(Kw::StartRipupCosts) => result.start_ripup_costs = Some(read_integer_scope(lx).max(1)),
                Tok::Kw(Kw::LayerRule) => {
                    if !read_layer_rule(lx, ls, &mut result) {
                        return None;
                    }
                }
                _ => {
                    lx.skip_scope();
                }
            }
        }
    }
    result.run_router = with_autoroute;
    result.run_optimizer = with_postroute;
    Some(result)
}

fn read_layer_rule(lx: &mut Lexer, ls: &DsnLayerStructure, settings: &mut DsnAutorouteSettings) -> bool {
    lx.yybegin(LexState::Name);
    let mut next = lx.next_token();
    let Tok::Str(name) = &next else {
        log::warn!("AutorouteSettings.read_layer_rule: String expected at '{}'", lx.scope_identifier);
        return false;
    };
    let layer = ls.get_no(name);
    if layer < 0 {
        log::warn!("AutorouteSettings.read_layer_rule: layer not found at '{}'", lx.scope_identifier);
        return false;
    }
    let l = layer as usize;
    let in_range = l < settings.layers.len();
    loop {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof {
            log::warn!("AutorouteSettings.read_layer_rule: unexpected end of file at '{}'", lx.scope_identifier);
            return false;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            match next {
                Tok::Kw(Kw::Active) => {
                    let v = read_on_off_scope(lx);
                    if in_range {
                        settings.layers[l].active = Some(v);
                    }
                }
                Tok::Kw(Kw::PreferredDirection) => {
                    let t = lx.next_token();
                    let horizontal = if t.is_kw(Kw::Vertical) {
                        false
                    } else if t.is_kw(Kw::Horizontal) {
                        true
                    } else {
                        log::warn!("AutorouteSettings.read_layer_rule: unexpected key word at '{}'", lx.scope_identifier);
                        return false;
                    };
                    if in_range {
                        settings.layers[l].preferred_direction_is_horizontal = Some(horizontal);
                    }
                    if lx.next_token() != Tok::Close {
                        log::warn!(
                            "AutorouteSettings.read_layer_rule: closing bracket expected at '{}'",
                            lx.scope_identifier
                        );
                        return false;
                    }
                }
                Tok::Kw(Kw::PreferredDirectionTraceCosts) => {
                    let v = read_float_scope(lx);
                    if in_range {
                        settings.layers[l].preferred_direction_trace_costs = Some(v);
                    }
                }
                Tok::Kw(Kw::AgainstPreferredDirectionTraceCosts) => {
                    let v = read_float_scope(lx);
                    if in_range {
                        settings.layers[l].against_preferred_direction_trace_costs = Some(v);
                    }
                }
                _ => {
                    lx.skip_scope();
                }
            }
        }
    }
    true
}
