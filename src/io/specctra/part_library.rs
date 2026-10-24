//! The part library scope (port of `parser/PartLibrary`).

use super::lexer::{Kw, LexState, Lexer, Tok};
use super::reader::ReadCtx;
use std::collections::BTreeSet;

pub struct LogicalPartMapping {
    pub name: String,
    /// The components belonging to the mapping (sorted).
    pub components: BTreeSet<String>,
}

pub struct DsnPartPin {
    pub pin_name: String,
    pub gate_name: String,
    pub gate_swap_code: i32,
    pub gate_pin_name: String,
    pub gate_pin_swap_code: i32,
}

pub struct LogicalPart {
    pub name: String,
    pub part_pins: Vec<DsnPartPin>,
}

pub fn read_part_library_scope(ctx: &mut ReadCtx) -> bool {
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, ctx.lx.next_token());
        if next == Tok::Eof {
            log::warn!("PartLibrary.read_scope: unexpected end of file at '{}'", ctx.lx.scope_identifier);
            return false;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            if next.is_kw(Kw::LogicalPartMapping) {
                match read_logical_part_mapping(&mut ctx.lx) {
                    Some(m) => ctx.logical_part_mappings.push(m),
                    None => return false,
                }
            } else if next.is_kw(Kw::LogicalPart) || next.as_str() == Some("logicalPart") {
                match read_logical_part(&mut ctx.lx) {
                    Some(p) => ctx.logical_parts.push(p),
                    None => return false,
                }
            } else {
                ctx.lx.skip_scope();
            }
        }
    }
    true
}

fn read_logical_part_mapping(lx: &mut Lexer) -> Option<LogicalPartMapping> {
    let Tok::Str(name) = lx.next_token() else {
        log::warn!("PartLibrary.read_logical_part_mapping: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    if lx.next_token() != Tok::Open {
        log::warn!("PartLibrary.read_logical_part_mapping: open bracket expected at '{}'", lx.scope_identifier);
        return None;
    }
    if !lx.next_token().is_kw(Kw::ComponentScope) {
        log::warn!(
            "PartLibrary.read_logical_part_mapping: Keyword.COMPONENT_SCOPE expected at '{}'",
            lx.scope_identifier
        );
        return None;
    }
    let mut components = BTreeSet::new();
    loop {
        lx.yybegin(LexState::Name);
        match lx.next_token() {
            Tok::Close => break,
            Tok::Str(s) => {
                components.insert(s);
            }
            _ => {
                log::warn!("PartLibrary.read_logical_part_mapping: string expected at '{}'", lx.scope_identifier);
                return None;
            }
        }
    }
    if lx.next_token() != Tok::Close {
        log::warn!(
            "PartLibrary.read_logical_part_mapping: closing bracket expected at '{}'",
            lx.scope_identifier
        );
        return None;
    }
    Some(LogicalPartMapping { name, components })
}

fn read_logical_part(lx: &mut Lexer) -> Option<LogicalPart> {
    let mut next = lx.next_token();
    let Tok::Str(name) = next.clone() else {
        log::warn!("PartLibrary.read_logical_part: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    lx.scope_identifier = name.clone();
    let mut part_pins = Vec::new();
    loop {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof {
            log::warn!("PartLibrary.read_logical_part: unexpected end of file at '{}'", lx.scope_identifier);
            return None;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            if next.is_kw(Kw::Pin) {
                part_pins.push(read_part_pin(lx)?);
            } else {
                lx.skip_scope();
            }
        }
    }
    Some(LogicalPart { name, part_pins })
}

fn read_part_pin(lx: &mut Lexer) -> Option<DsnPartPin> {
    lx.yybegin(LexState::Name);
    let Tok::Str(pin_name) = lx.next_token() else {
        log::warn!("PartLibrary.read_part_pin: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    lx.scope_identifier = pin_name.clone();
    let Tok::Int(_) = lx.next_token() else {
        log::warn!("PartLibrary.read_part_pin: integer expected at '{}'", lx.scope_identifier);
        return None;
    };
    lx.yybegin(LexState::Name);
    let Tok::Str(gate_name) = lx.next_token() else {
        log::warn!("PartLibrary.read_part_pin: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    lx.scope_identifier = gate_name.clone();
    let Tok::Int(gate_swap_code) = lx.next_token() else {
        log::warn!("PartLibrary.read_part_pin: integer expected at '{}'", lx.scope_identifier);
        return None;
    };
    lx.yybegin(LexState::Name);
    let Tok::Str(gate_pin_name) = lx.next_token() else {
        log::warn!("PartLibrary.read_part_pin: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    lx.scope_identifier = gate_pin_name.clone();
    let Tok::Int(gate_pin_swap_code) = lx.next_token() else {
        log::warn!("PartLibrary.read_part_pin: integer expected at '{}'", lx.scope_identifier);
        return None;
    };
    loop {
        let t = lx.next_token();
        if t == Tok::Close || t == Tok::Eof {
            break;
        }
    }
    Some(DsnPartPin {
        pin_name,
        gate_name,
        gate_swap_code,
        gate_pin_name,
        gate_pin_swap_code,
    })
}
