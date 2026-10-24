//! Small scope readers of `DsnFile`, the rule scopes (`Rule`) and circuit scopes (`Circuit`).

use super::lexer::{java_parse_double, Kw, LexState, Lexer, Tok};

/// `DsnFile.readOnOffScope`.
pub fn read_on_off_scope(lx: &mut Lexer) -> bool {
    let t = lx.next_token();
    let mut result = false;
    if t.is_kw(Kw::On) {
        result = true;
    } else if !t.is_kw(Kw::Off) {
        log::warn!("DsnFile.read_boolean: Keyword.OFF expected at '{}'", lx.scope_identifier);
    }
    lx.skip_scope();
    result
}

/// `DsnFile.readIntegerScope`.
pub fn read_integer_scope(lx: &mut Lexer) -> i32 {
    let value = match lx.next_token() {
        Tok::Int(i) => i,
        _ => {
            log::warn!("DsnFile.read_integer_scope: number expected at '{}'", lx.scope_identifier);
            return 0;
        }
    };
    if lx.next_token() != Tok::Close {
        log::warn!("DsnFile.read_integer_scope: closing bracket expected at '{}'", lx.scope_identifier);
        return 0;
    }
    value
}

/// `DsnFile.readFloatScope`.
pub fn read_float_scope(lx: &mut Lexer) -> f64 {
    let Some(value) = lx.next_token().as_number() else {
        log::warn!("DsnFile.read_float_scope: number expected at '{}'", lx.scope_identifier);
        return 0.0;
    };
    if lx.next_token() != Tok::Close {
        log::warn!("DsnFile.read_float_scope: closing bracket expected at '{}'", lx.scope_identifier);
        return 0.0;
    }
    value
}

/// `DsnFile.readStringScope`.
pub fn read_string_scope(lx: &mut Lexer) -> String {
    let result = lx.next_string_nl(true);
    let mut t = lx.next_token();
    if t != Tok::Close {
        log::warn!(
            "DsnFile.read_string_scope: closing bracket expected at '{}', got: {:?}",
            lx.scope_identifier,
            t
        );
        while t != Tok::Eof && t != Tok::Close {
            t = lx.next_token();
        }
    }
    result
}

/// `DsnFile.readStringListScope`.
pub fn read_string_list_scope(lx: &mut Lexer) -> Option<Vec<String>> {
    let result = lx.next_string_list();
    if !lx.next_closing_bracket() {
        return None;
    }
    Some(result)
}

/// `Structure.readViaPadstacks`.
pub fn read_via_padstacks(lx: &mut Lexer) -> Option<Vec<String>> {
    let mut normal = Vec::new();
    let mut spare = Vec::new();
    loop {
        let t = lx.next_token();
        match t {
            Tok::Close => break,
            Tok::Open => {
                let t2 = lx.next_token();
                if t2.is_kw(Kw::Spare) {
                    match read_via_padstacks(lx) {
                        Some(s) => spare = s,
                        None => {
                            lx.set_fatal("NullPointerException in Structure.readViaPadstacks");
                            return None;
                        }
                    }
                } else {
                    lx.skip_scope();
                }
            }
            Tok::Str(s) => normal.push(s),
            _ => {
                log::warn!("Structure.read_via_padstack: String expected at '{}'", lx.scope_identifier);
                return None;
            }
        }
    }
    normal.extend(spare);
    Some(normal)
}

// ------------------------------------------------------------------ rules

/// Meander pattern descriptors (parsed for token compatibility; not used by the router port).
#[derive(Clone, Debug, Default)]
pub struct MeanderRule {
    pub min_amplitude: Option<f64>,
    pub max_amplitude: Option<f64>,
    pub spacing: Option<f64>,
    pub corner_style: Option<String>,
    pub radius_percentage: Option<f64>,
    pub single_sided: Option<bool>,
    pub pattern_type: Option<String>,
}

impl MeanderRule {
    pub fn has_attributes(&self) -> bool {
        self.max_amplitude.is_some()
            || self.min_amplitude.is_some()
            || self.spacing.is_some()
            || self.corner_style.is_some()
            || self.radius_percentage.is_some()
            || self.single_sided.is_some()
            || self.pattern_type.is_some()
    }

    /// `MeanderDescriptorBuilder.readDescriptor`.
    pub fn read_descriptor(&mut self, tok: &Tok, lx: &mut Lexer) -> bool {
        if tok.matches("length_amplitude") {
            if let Some((max, min)) = read_length_amplitude_scope(lx) {
                self.max_amplitude = Some(max);
                self.min_amplitude = Some(min);
            }
            return true;
        }
        if tok.matches("length_gap") {
            if let Some(g) = read_number_scope(lx) {
                self.spacing = Some(g);
            }
            return true;
        }
        if tok.matches("corner_style") {
            if let Some(s) = circuit_read_string_scope(lx) {
                self.corner_style = Some(s);
            }
            return true;
        }
        if tok.matches("radius") {
            if let Some(r) = read_number_scope(lx) {
                self.radius_percentage = Some(r);
            }
            return true;
        }
        if tok.matches("single_sided") {
            if let Some(s) = circuit_read_string_scope(lx) {
                let s = s.to_ascii_lowercase();
                self.single_sided = Some(s == "on" || s == "true" || s == "yes" || s == "1");
            }
            return true;
        }
        if tok.matches("meander_pattern") {
            read_meander_pattern_scope_into(lx, self);
            return true;
        }
        false
    }
}

#[derive(Clone, Debug)]
pub enum Rule {
    Width(f64),
    Clearance { value: f64, pairs: Vec<String> },
    Length { max: f64, min: f64 },
    /// Meander descriptors: parsed, but not used (no meander constraints in the Rust rules).
    #[allow(dead_code)]
    Meander(MeanderRule),
}

#[derive(Clone, Debug)]
pub struct LayerRule {
    pub layer_names: Vec<String>,
    pub rules: Vec<Rule>,
}

/// `Rule.readScope`. On an unexpected end of file the original returns null, which
/// leads to a NullPointerException in all callers; this is reported as fatal error.
pub fn read_rule_scope(lx: &mut Lexer) -> Vec<Rule> {
    let mut result = Vec::new();
    let mut meander = MeanderRule::default();
    let mut cur = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut cur, lx.next_token());
        if cur == Tok::Eof {
            log::warn!("Rule.read_scope: unexpected end of file at '{}'", lx.scope_identifier);
            lx.set_fatal("NullPointerException after Rule.read_scope");
            return result;
        }
        if cur == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            let mut rule = None;
            if meander.read_descriptor(&cur, lx) {
            } else if cur.is_kw(Kw::Width) {
                rule = read_width_rule(lx);
            } else if cur.is_kw(Kw::Clearance) {
                rule = read_clearance_rule(lx);
            } else if cur.is_kw(Kw::Length) {
                if let Some((max, min)) = read_length_scope(lx) {
                    rule = Some(Rule::Length { max, min });
                }
            } else {
                lx.skip_scope();
            }
            if let Some(r) = rule {
                result.push(r);
            }
        }
    }
    if meander.has_attributes() {
        result.push(Rule::Meander(meander));
    }
    result
}

/// `Rule.readLayerRuleScope`.
pub fn read_layer_rule_scope(lx: &mut Lexer) -> Option<LayerRule> {
    let mut layer_names = Vec::new();
    let mut rules = Vec::new();
    loop {
        lx.yybegin(LexState::LayerName);
        match lx.next_token() {
            Tok::Open => break,
            Tok::Str(s) => layer_names.push(s),
            _ => {
                log::warn!("Rule.read_layer_rule_scope: string expected at '{}'", lx.scope_identifier);
                return None;
            }
        }
    }
    loop {
        let t = lx.next_token();
        if t == Tok::Close {
            break;
        }
        if !t.is_kw(Kw::Rule) {
            log::warn!("Rule.read_layer_rule_scope: rule expected at '{}'", lx.scope_identifier);
            return None;
        }
        rules.extend(read_rule_scope(lx));
        if lx.fatal.is_some() {
            return None;
        }
    }
    Some(LayerRule { layer_names, rules })
}

pub fn read_width_rule(lx: &mut Lexer) -> Option<Rule> {
    let Some(value) = lx.next_double() else {
        lx.set_fatal("NullPointerException in Rule.readWidthRule");
        return None;
    };
    if !lx.next_closing_bracket() {
        return None;
    }
    Some(Rule::Width(value))
}

pub fn read_clearance_rule(lx: &mut Lexer) -> Option<Rule> {
    let Some(value) = lx.next_double() else {
        lx.set_fatal("NullPointerException in Rule.readClearanceRule");
        return None;
    };
    let mut pairs = Vec::new();
    let t = lx.next_token();
    if t != Tok::Close {
        if t != Tok::Open {
            log::warn!("Rule.read_clearance_rule: ( expected at '{}'", lx.scope_identifier);
            return None;
        }
        if !lx.next_token().is_kw(Kw::Type) {
            log::warn!("Rule.read_clearance_rule: type expected at '{}'", lx.scope_identifier);
            return None;
        }
        pairs = lx.next_string_list_sep('-');
        if !lx.next_closing_bracket() {
            log::warn!("Rule.read_clearance_rule: closing bracket expected at '{}'", lx.scope_identifier);
            return None;
        }
        if !lx.next_closing_bracket() {
            log::warn!("Rule.read_clearance_rule: closing bracket expected at '{}'", lx.scope_identifier);
            return None;
        }
    }
    Some(Rule::Clearance { value, pairs })
}

// ------------------------------------------------------------------ circuit

/// Result of `Circuit.readScope`.
#[derive(Clone, Debug, Default)]
pub struct CircuitResult {
    pub max_length: f64,
    pub min_length: f64,
    pub use_via: Vec<String>,
    pub use_layer: Vec<String>,
    pub meander: MeanderRule,
}

/// `Circuit.readScope`.
pub fn read_circuit_scope(lx: &mut Lexer) -> Option<CircuitResult> {
    let mut r = CircuitResult::default();
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof {
            log::warn!("Circuit.read_scope: unexpected end of file at '{}'", lx.scope_identifier);
            return None;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            if r.meander.read_descriptor(&next, lx) {
            } else if next.matches("length") {
                if let Some((max, min)) = read_length_scope(lx) {
                    r.min_length = min;
                    r.max_length = max;
                }
            } else if next.is_kw(Kw::UseVia) {
                match read_via_padstacks(lx) {
                    Some(v) => r.use_via.extend(v),
                    None => {
                        lx.set_fatal("NullPointerException in Circuit.readScope");
                        return None;
                    }
                }
            } else if next.is_kw(Kw::UseLayer) {
                match read_string_list_scope(lx) {
                    Some(v) => r.use_layer.extend(v),
                    None => {
                        lx.set_fatal("NullPointerException in Circuit.readScope");
                        return None;
                    }
                }
            } else {
                lx.skip_scope();
            }
        }
    }
    Some(r)
}

/// `Circuit.readLengthScope`: (max_length, min_length).
pub fn read_length_scope(lx: &mut Lexer) -> Option<(f64, f64)> {
    let mut next = lx.next_token();
    let Some(max) = next.as_number() else {
        log::warn!("Circuit.read_length_scope: number expected at '{}'", lx.scope_identifier);
        return None;
    };
    let mut min = 0.0;
    let mut min_read = false;
    loop {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof {
            log::warn!("Circuit.read_length_scope: unexpected end of file at '{}'", lx.scope_identifier);
            return None;
        }
        if next == Tok::Close {
            break;
        }
        if !min_read && prev != Tok::Open {
            if let Some(v) = next.as_number() {
                min = v;
                min_read = true;
                continue;
            }
        }
        if prev == Tok::Open {
            lx.skip_scope();
        }
    }
    Some((max, min))
}

fn parse_double_tok(t: &Tok) -> Option<f64> {
    match t {
        Tok::Int(i) => Some(*i as f64),
        Tok::Dbl(d) => Some(*d),
        Tok::Str(s) => java_parse_double(s),
        _ => None,
    }
}

fn skip_to_closed_bracket(lx: &mut Lexer) {
    loop {
        let t = lx.next_token();
        if t == Tok::Eof || t == Tok::Close {
            break;
        }
        if t == Tok::Open {
            lx.skip_scope();
        }
    }
}

fn read_length_amplitude_scope(lx: &mut Lexer) -> Option<(f64, f64)> {
    let mut next = lx.next_token();
    let Some(mut max) = parse_double_tok(&next) else {
        log::warn!("Circuit.read_length_amplitude: number expected at '{}'", lx.scope_identifier);
        skip_to_closed_bracket(lx);
        return None;
    };
    let mut min = 0.0;
    let mut min_read = false;
    loop {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof || next == Tok::Close {
            break;
        }
        if !min_read && prev != Tok::Open {
            if let Some(v) = parse_double_tok(&next) {
                min = v;
                min_read = true;
                continue;
            }
        }
        if prev == Tok::Open {
            lx.skip_scope();
        }
    }
    if max > 0.0 && min > 0.0 && min > max {
        std::mem::swap(&mut min, &mut max);
    }
    Some((max, min))
}

fn read_number_scope(lx: &mut Lexer) -> Option<f64> {
    let t = lx.next_token();
    let r = parse_double_tok(&t);
    if r.is_none() {
        log::warn!("Circuit.read_number_scope: number expected at '{}'", lx.scope_identifier);
    }
    skip_to_closed_bracket(lx);
    r
}

fn circuit_read_string_scope(lx: &mut Lexer) -> Option<String> {
    let t = lx.next_token();
    let r = match &t {
        Tok::Str(s) => Some(s.clone()),
        Tok::Kw(k) => Some(k.name().to_string()),
        _ => None,
    };
    skip_to_closed_bracket(lx);
    r
}

fn read_meander_pattern_scope_into(lx: &mut Lexer, b: &mut MeanderRule) {
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof || next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            if next.matches("type") || next.matches("pattern_type") {
                if let Some(s) = circuit_read_string_scope(lx) {
                    b.pattern_type = Some(s);
                }
            } else if !b.read_descriptor(&next, lx) {
                lx.skip_scope();
            }
        }
    }
}
