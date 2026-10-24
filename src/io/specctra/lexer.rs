//! Hand written replacement of the JFlex generated scanner `SpecctraDsnStreamReader`.
//!
//! The token semantics (longest match, rule priority, lexical states, the character
//! classes of the generated tables and the raw buffer hacks of `nextString`) are
//! reproduced exactly. The keyword list was extracted from the DFA of the generated
//! scanner (it differs slightly from the `.flex` file in the repository, e.g.
//! `generated_by_freeroute` and `prefered_direction`).

/// Keywords returned by the scanner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kw {
    Absolute,
    Active,
    AgainstPreferredDirectionTraceCosts,
    Attach,
    Autoroute,
    AutorouteSettings,
    Back,
    Boundary,
    Circle,
    Circuit,
    Class,
    ClassClass,
    Classes,
    Clearance,
    ClearanceClass,
    ComponentScope,
    Constant,
    Control,
    Fanout,
    Fix,
    FlipStyle,
    FortyfiveDegree,
    Fromto,
    Front,
    GeneratedByFreerouting,
    Horizontal,
    HostCad,
    HostVersion,
    Image,
    Keepout,
    Layer,
    LayerRule,
    Length,
    LibraryScope,
    LockType,
    LogicalPart,
    LogicalPartMapping,
    Net,
    NetworkOut,
    NetworkScope,
    NinetyDegree,
    None,
    Normal,
    Off,
    On,
    Order,
    Outline,
    Padstack,
    ParserScope,
    PartLibraryScope,
    PolygonPath,
    PcbScope,
    Pin,
    Pins,
    Place,
    PlaceControl,
    PlaceKeepout,
    PlacementScope,
    PlaneScope,
    PlaneViaCosts,
    Polygon,
    PolylinePath,
    Position,
    Postroute,
    Power,
    PreferredDirection,
    PreferredDirectionTraceCosts,
    PullTight,
    Rectangle,
    ResolutionScope,
    Rotate,
    RotateFirst,
    Routes,
    Rule,
    Rules,
    Session,
    Shape,
    ShoveFixed,
    Side,
    Signal,
    SnapAngle,
    Spare,
    StartPassNo,
    StartRipupCosts,
    StringQuote,
    StructureScope,
    Type,
    UseLayer,
    UseNet,
    UseVia,
    Vertical,
    Via,
    Vias,
    ViaAtSmd,
    ViaCosts,
    ViaKeepout,
    ViaRule,
    Width,
    Window,
    Wire,
    WiringScope,
    WriteResolution,
}

impl Kw {
    /// The name of the keyword object in the original (`Keyword.getName()`).
    pub fn name(self) -> &'static str {
        use Kw::*;
        match self {
            Absolute => "absolute",
            Active => "active",
            AgainstPreferredDirectionTraceCosts => "against_preferred_direction_trace_costs",
            Attach => "attach",
            Autoroute => "autoroute",
            AutorouteSettings => "autoroute_settings",
            Back => "back",
            Boundary => "boundary",
            Circle => "circle",
            Circuit => "circuit",
            Class => "class",
            ClassClass => "class_class",
            Classes => "classes",
            Clearance => "clearance",
            ClearanceClass => "clearance_class",
            ComponentScope => "component",
            Constant => "constant",
            Control => "control",
            Fanout => "fanout",
            Fix => "fix",
            FlipStyle => "flip_style",
            FortyfiveDegree => "fortyfive_degree",
            Fromto => "fromto",
            Front => "front",
            GeneratedByFreerouting => "generated_by_freerouting",
            Horizontal => "horizontal",
            HostCad => "host_cad",
            HostVersion => "host_version",
            Image => "image",
            Keepout => "keepout",
            Layer => "layer",
            LayerRule => "layer_rule",
            Length => "length",
            LibraryScope => "library",
            LockType => "lock_type",
            LogicalPart => "logical_part",
            LogicalPartMapping => "logical_part_mapping",
            Net => "net",
            NetworkOut => "network_out",
            NetworkScope => "network",
            NinetyDegree => "ninety_degree",
            None => "none",
            Normal => "normal",
            Off => "off",
            On => "on",
            Order => "order",
            Outline => "outline",
            Padstack => "padstack",
            ParserScope => "parser",
            PartLibraryScope => "part_library",
            PolygonPath => "polygon_path",
            PcbScope => "pcb",
            Pin => "pin",
            Pins => "pins",
            Place => "place",
            PlaceControl => "place_control",
            PlaceKeepout => "place_keepout",
            PlacementScope => "placement",
            PlaneScope => "plane",
            PlaneViaCosts => "plane_via_costs",
            Polygon => "polygon",
            PolylinePath => "polyline_path",
            Position => "position",
            Postroute => "postroute",
            Power => "power",
            PreferredDirection => "preferred_direction",
            PreferredDirectionTraceCosts => "preferred_direction_trace_costs",
            PullTight => "pull_tight",
            Rectangle => "rectangle",
            ResolutionScope => "resolution",
            Rotate => "rotate",
            RotateFirst => "rotate_first",
            Routes => "routes",
            Rule => "rule",
            Rules => "rules",
            Session => "session",
            Shape => "shape",
            ShoveFixed => "shove_fixed",
            Side => "side",
            Signal => "signal",
            SnapAngle => "snap_angle",
            Spare => "spare",
            StartPassNo => "start_pass_no",
            StartRipupCosts => "start_ripup_costs",
            StringQuote => "string_quote",
            StructureScope => "structure",
            Type => "type",
            UseLayer => "use_layer",
            UseNet => "use_net",
            UseVia => "use_via",
            Vertical => "vertical",
            Via => "via",
            Vias => "vias",
            ViaAtSmd => "via_at_smd",
            ViaCosts => "via_costs",
            ViaKeepout => "via_keepout",
            ViaRule => "via_rule",
            Width => "width",
            Window => "window",
            Wire => "wire",
            WiringScope => "wiring",
            WriteResolution => "write_resolution",
        }
    }

    /// True for the keywords, which are `ScopeKeyword` instances in the original.
    pub fn is_scope_keyword(self) -> bool {
        matches!(
            self,
            Kw::ComponentScope
                | Kw::LibraryScope
                | Kw::NetworkScope
                | Kw::PartLibraryScope
                | Kw::ParserScope
                | Kw::PcbScope
                | Kw::PlaceControl
                | Kw::PlacementScope
                | Kw::PlaneScope
                | Kw::ResolutionScope
                | Kw::StructureScope
                | Kw::WiringScope
        )
    }
}

/// The lexical state that follows a keyword.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexState {
    Initial,
    String1,
    String2,
    Name,
    LayerName,
    /// Unused by the parser (as in the original), kept for completeness.
    #[allow(dead_code)]
    ComponentName,
    SpecChar,
    IgnoreQuote,
}

/// Keyword table of the YYINITIAL state: (text, keyword, following state).
const KEYWORDS: &[(&str, Kw, Option<LexState>)] = &[
    ("absolute", Kw::Absolute, None),
    ("active", Kw::Active, None),
    ("against_prefered_direction_trace_costs", Kw::AgainstPreferredDirectionTraceCosts, None),
    ("attach", Kw::Attach, None),
    ("autoroute", Kw::Autoroute, None),
    ("autoroute_settings", Kw::AutorouteSettings, None),
    ("back", Kw::Back, None),
    ("boundary", Kw::Boundary, None),
    ("circ", Kw::Circle, Some(LexState::LayerName)),
    ("circle", Kw::Circle, Some(LexState::LayerName)),
    ("circuit", Kw::Circuit, None),
    ("class", Kw::Class, Some(LexState::Name)),
    ("class_class", Kw::ClassClass, None),
    ("classes", Kw::Classes, None),
    ("clear", Kw::Clearance, None),
    ("clearance", Kw::Clearance, None),
    ("clearance_class", Kw::ClearanceClass, Some(LexState::Name)),
    ("comp", Kw::ComponentScope, Some(LexState::Name)),
    ("component", Kw::ComponentScope, Some(LexState::Name)),
    ("constant", Kw::Constant, None),
    ("control", Kw::Control, None),
    ("fanout", Kw::Fanout, None),
    ("fix", Kw::Fix, None),
    ("flip_style", Kw::FlipStyle, None),
    ("fortyfive_degree", Kw::FortyfiveDegree, None),
    ("fromto", Kw::Fromto, None),
    ("front", Kw::Front, None),
    ("generated_by_freeroute", Kw::GeneratedByFreerouting, None),
    ("horizontal", Kw::Horizontal, None),
    ("host_cad", Kw::HostCad, Some(LexState::Name)),
    ("host_version", Kw::HostVersion, Some(LexState::Name)),
    ("image", Kw::Image, Some(LexState::Name)),
    ("keepout", Kw::Keepout, Some(LexState::Name)),
    ("layer", Kw::Layer, Some(LexState::Name)),
    ("layer_rule", Kw::LayerRule, Some(LexState::Name)),
    ("length", Kw::Length, None),
    ("library", Kw::LibraryScope, None),
    ("lock_type", Kw::LockType, None),
    ("logical_part", Kw::LogicalPart, Some(LexState::Name)),
    ("logical_part_mapping", Kw::LogicalPartMapping, Some(LexState::Name)),
    ("net", Kw::Net, Some(LexState::Name)),
    ("network", Kw::NetworkScope, None),
    ("network_out", Kw::NetworkOut, None),
    ("ninety_degree", Kw::NinetyDegree, None),
    ("none", Kw::None, None),
    ("normal", Kw::Normal, None),
    ("off", Kw::Off, None),
    ("on", Kw::On, None),
    ("order", Kw::Order, None),
    ("outline", Kw::Outline, None),
    ("padstack", Kw::Padstack, Some(LexState::Name)),
    ("parser", Kw::ParserScope, None),
    ("part_library", Kw::PartLibraryScope, None),
    ("path", Kw::PolygonPath, Some(LexState::LayerName)),
    ("pcb", Kw::PcbScope, None),
    ("pin", Kw::Pin, None),
    ("pins", Kw::Pins, None),
    ("place", Kw::Place, Some(LexState::Name)),
    ("place_control", Kw::PlaceControl, None),
    ("place_keepout", Kw::PlaceKeepout, Some(LexState::Name)),
    ("placement", Kw::PlacementScope, None),
    ("plane", Kw::PlaneScope, Some(LexState::Name)),
    ("plane_via_costs", Kw::PlaneViaCosts, None),
    ("poly", Kw::Polygon, Some(LexState::LayerName)),
    ("polygon", Kw::Polygon, Some(LexState::LayerName)),
    ("polyline_path", Kw::PolylinePath, Some(LexState::LayerName)),
    ("position", Kw::Position, None),
    ("postroute", Kw::Postroute, None),
    ("power", Kw::Power, None),
    ("prefered_direction", Kw::PreferredDirection, None),
    ("prefered_direction_trace_costs", Kw::PreferredDirectionTraceCosts, None),
    ("pull_tight", Kw::PullTight, None),
    ("rect", Kw::Rectangle, Some(LexState::LayerName)),
    ("rectangle", Kw::Rectangle, Some(LexState::LayerName)),
    ("resolution", Kw::ResolutionScope, None),
    ("rotate", Kw::Rotate, None),
    ("rotate_first", Kw::RotateFirst, None),
    ("routes", Kw::Routes, None),
    ("rule", Kw::Rule, None),
    ("rules", Kw::Rules, None),
    ("session", Kw::Session, None),
    ("shape", Kw::Shape, None),
    ("shove_fixed", Kw::ShoveFixed, None),
    ("side", Kw::Side, None),
    ("signal", Kw::Signal, None),
    ("snap_angle", Kw::SnapAngle, None),
    ("spare", Kw::Spare, None),
    ("start_pass_no", Kw::StartPassNo, None),
    ("start_ripup_costs", Kw::StartRipupCosts, None),
    ("string_quote", Kw::StringQuote, Some(LexState::IgnoreQuote)),
    ("structure", Kw::StructureScope, None),
    ("type", Kw::Type, None),
    ("use_layer", Kw::UseLayer, Some(LexState::Name)),
    ("use_net", Kw::UseNet, Some(LexState::Name)),
    ("use_via", Kw::UseVia, Some(LexState::Name)),
    ("vertical", Kw::Vertical, None),
    ("via", Kw::Via, Some(LexState::Name)),
    ("vias", Kw::Vias, None),
    ("via_at_smd", Kw::ViaAtSmd, None),
    ("via_costs", Kw::ViaCosts, None),
    ("via_keepout", Kw::ViaKeepout, Some(LexState::Name)),
    ("via_rule", Kw::ViaRule, None),
    ("width", Kw::Width, None),
    ("window", Kw::Window, None),
    ("wire", Kw::Wire, Some(LexState::Name)),
    ("wire_keepout", Kw::Keepout, None),
    ("wiring", Kw::WiringScope, None),
    ("write_resolution", Kw::WriteResolution, None),
];

static KEYWORD_MAP: std::sync::LazyLock<std::collections::HashMap<&'static str, (Kw, Option<LexState>)>> =
    std::sync::LazyLock::new(|| KEYWORDS.iter().map(|(t, k, s)| (*t, (*k, *s))).collect());

fn lookup_keyword(text: &str) -> Option<(Kw, Option<LexState>)> {
    if text.len() > 40 || !text.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_') {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    KEYWORD_MAP.get(lower.as_str()).map(|(k, s)| (*k, *s))
}

/// A token of the scanner.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Open,
    Close,
    Kw(Kw),
    Str(String),
    Int(i32),
    Dbl(f64),
    /// End of file (`null` in the original).
    Eof,
}

impl Tok {
    pub fn is_kw(&self, k: Kw) -> bool {
        matches!(self, Tok::Kw(x) if *x == k)
    }

    /// `Keyword.matches(token, keyword)`: identity or case insensitive name comparison.
    pub fn matches(&self, name: &str) -> bool {
        match self {
            Tok::Kw(k) => k.name() == name,
            Tok::Str(s) => s.eq_ignore_ascii_case(name),
            _ => false,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tok::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A number token as double (Double or Integer in the original).
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Tok::Int(i) => Some(*i as f64),
            Tok::Dbl(d) => Some(*d),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ character classes

#[inline]
fn is_letter(c: char) -> bool {
    c.is_ascii_alphabetic()
}

#[inline]
fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// Character class 14 of the generated tables (special ASCII and ANSI characters).
fn is_spec_misc(c: char) -> bool {
    let u = c as u32;
    matches!(
        u,
        0x1f | 0x21 | 0x24..=0x26 | 0x2c | 0x3a..=0x40 | 0x5b | 0x5d..=0x5e | 0x7e | 0xa1..=0xff
            | 0x152..=0x153
            | 0x160..=0x161
            | 0x178
            | 0x17d..=0x17e
            | 0x191..=0x192
            | 0x2c6
            | 0x2dc
            | 0x2013..=0x2014
            | 0x2018..=0x201a
            | 0x201c..=0x201e
            | 0x2020..=0x2022
            | 0x2026
            | 0x2030
            | 0x2039..=0x203a
            | 0x20ac
            | 0x2122
    )
}

/// SpecChar1 of the specification: `_ . / \ # *` and class 14.
#[inline]
fn is_spec1(c: char) -> bool {
    matches!(c, '_' | '.' | '/' | '\\' | '#' | '*') || is_spec_misc(c)
}

/// SpecChar2 = SpecChar1 | - | +
#[inline]
fn is_spec2(c: char) -> bool {
    is_spec1(c) || c == '-' || c == '+'
}

/// SpecChar3 = SpecChar2 | " | '
#[inline]
fn is_spec3(c: char) -> bool {
    is_spec2(c) || c == '"' || c == '\''
}

/// SpecChar4 = SpecChar1 | +
#[inline]
fn is_spec4(c: char) -> bool {
    is_spec1(c) || c == '+'
}

/// SpecChar5 = SpecChar4 | " | '
#[inline]
fn is_spec5(c: char) -> bool {
    is_spec4(c) || c == '"' || c == '\''
}

#[inline]
fn is_white(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\x0c' | '\r' | '\n')
}

/// The scanner.
pub struct Lexer {
    chars: Vec<char>,
    /// `zzMarkedPos`: the position after the last read token.
    pos: usize,
    state: LexState,
    string_buffer: String,
    /// Set if the original would have thrown an unchecked exception.
    pub fatal: Option<String>,
    pub scope_identifier: String,
}

impl Lexer {
    pub fn new(text: &str) -> Lexer {
        Lexer {
            chars: text.chars().collect(),
            pos: 0,
            state: LexState::Initial,
            string_buffer: String::new(),
            fatal: None,
            scope_identifier: String::new(),
        }
    }

    pub fn yybegin(&mut self, state: LexState) {
        self.state = state;
    }

    pub fn set_fatal(&mut self, msg: impl Into<String>) {
        if self.fatal.is_none() {
            self.fatal = Some(msg.into());
        }
        // Stop the scanning: the original aborts with an exception.
        self.pos = self.chars.len();
    }

    #[inline]
    fn at(&self, i: usize) -> Option<char> {
        self.chars.get(i).copied()
    }

    fn text(&self, from: usize, to: usize) -> String {
        self.chars[from..to].iter().collect()
    }

    fn non_ansi(&mut self, c: char) {
        log::warn!(
            "Non-ansi character '{}' found at position {} just after '{}'.",
            c,
            self.pos,
            self.string_buffer
        );
        self.pos += 1;
    }

    /// Length of an Identifier (`(Letter|SpecChar1)(Letter|Digit|SpecChar3)*`) at `p`.
    fn ident_len(&self, p: usize, first: fn(char) -> bool, rest: fn(char) -> bool) -> usize {
        match self.at(p) {
            Some(c) if first(c) => {}
            _ => return 0,
        }
        let mut q = p + 1;
        while let Some(c) = self.at(q) {
            if rest(c) {
                q += 1;
            } else {
                break;
            }
        }
        q - p
    }

    fn digits_from(&self, mut q: usize) -> usize {
        while let Some(c) = self.at(q) {
            if is_digit(c) {
                q += 1;
            } else {
                break;
            }
        }
        q
    }

    /// Lengths of DecIntegerLiteral and DecFloatLiteral at `p` (0 if no match).
    /// The float length already includes the extension of `readFloatingToken`.
    fn number_lens(&self, p: usize) -> (usize, usize) {
        let mut q = p;
        if matches!(self.at(q), Some('+') | Some('-')) {
            q += 1;
        }
        if !matches!(self.at(q), Some(c) if is_digit(c)) {
            return (0, 0);
        }
        // integer: 0 | [1-9][0-9]*
        let int_end = if self.at(q) == Some('0') { q + 1 } else { self.digits_from(q) };
        // float: [0-9]+ ("." [0-9]+)? ([Ee] [+-]? digits)?
        let mut f = self.digits_from(q);
        if self.at(f) == Some('.') && matches!(self.at(f + 1), Some(c) if is_digit(c)) {
            f = self.digits_from(f + 1);
        }
        if matches!(self.at(f), Some('e') | Some('E')) {
            let mut g = f + 1;
            if matches!(self.at(g), Some('+') | Some('-')) {
                g += 1;
            }
            if matches!(self.at(g), Some(c) if is_digit(c)) {
                f = self.digits_from(g);
            }
        }
        (int_end - p, f - p)
    }

    /// Length of a comment at `p` (0 if no match).
    fn comment_len(&self, p: usize) -> usize {
        match self.at(p) {
            Some('#') => {
                let mut q = p + 1;
                while let Some(c) = self.at(q) {
                    if c == '\r' || c == '\n' {
                        break;
                    }
                    q += 1;
                }
                match self.at(q) {
                    Some('\r') => {
                        if self.at(q + 1) == Some('\n') {
                            q + 2 - p
                        } else {
                            q + 1 - p
                        }
                    }
                    Some('\n') => q + 1 - p,
                    _ => 0,
                }
            }
            Some('/') if self.at(p + 1) == Some('*') => {
                let mut best = 0;
                // "/*" [^*] ~"*/"
                if let Some(c) = self.at(p + 2) {
                    if c != '*' {
                        let mut q = p + 3;
                        loop {
                            match self.at(q) {
                                None => break,
                                Some('*') if self.at(q + 1) == Some('/') => {
                                    best = q + 2 - p;
                                    break;
                                }
                                _ => q += 1,
                            }
                        }
                    }
                }
                // "/*" "*"+ "/"
                let mut q = p + 2;
                while self.at(q) == Some('*') {
                    q += 1;
                }
                if q > p + 2 && self.at(q) == Some('/') {
                    best = best.max(q + 1 - p);
                }
                best
            }
            _ => 0,
        }
    }

    /// Reads the next token (`nextToken`).
    pub fn next_token(&mut self) -> Tok {
        loop {
            let Some(c) = self.at(self.pos) else {
                return Tok::Eof;
            };
            match self.state {
                LexState::Initial => {
                    if c == '(' {
                        self.pos += 1;
                        return Tok::Open;
                    }
                    if c == ')' {
                        self.pos += 1;
                        return Tok::Close;
                    }
                    if c == '"' {
                        self.string_buffer.clear();
                        self.state = LexState::String1;
                        self.pos += 1;
                        continue;
                    }
                    if c == '\'' {
                        self.string_buffer.clear();
                        self.state = LexState::String2;
                        self.pos += 1;
                        continue;
                    }
                    if is_white(c) {
                        // "\r\n" is one token, the result is the same
                        self.pos += 1;
                        continue;
                    }
                    let p = self.pos;
                    let ident = self.ident_len(p, |c| is_letter(c) || is_spec1(c), |c| {
                        is_letter(c) || is_digit(c) || is_spec3(c)
                    });
                    let (int_len, float_len) = self.number_lens(p);
                    let comment = self.comment_len(p);
                    // priority on equal length: keyword, identifier, integer, float, comment
                    let best = ident.max(int_len).max(float_len).max(comment);
                    if best == 0 {
                        self.non_ansi(c);
                        continue;
                    }
                    if ident == best {
                        let text = self.text(p, p + ident);
                        self.pos = p + ident;
                        if let Some((kw, st)) = lookup_keyword(&text) {
                            if let Some(s) = st {
                                self.state = s;
                            }
                            return Tok::Kw(kw);
                        }
                        return Tok::Str(text);
                    }
                    if int_len == best {
                        let text = self.text(p, p + int_len);
                        self.pos = p + int_len;
                        match text.parse::<i32>() {
                            Ok(v) => return Tok::Int(v),
                            Err(_) => {
                                self.set_fatal(format!("For input string: \"{text}\""));
                                return Tok::Eof;
                            }
                        }
                    }
                    if float_len == best {
                        let text = self.text(p, p + float_len);
                        self.pos = p + float_len;
                        return Tok::Dbl(text.parse::<f64>().unwrap_or(0.0));
                    }
                    // comment
                    self.pos = p + comment;
                    continue;
                }
                LexState::String1 | LexState::String2 => {
                    let quote = if self.state == LexState::String1 { '"' } else { '\'' };
                    let mut q = self.pos;
                    while let Some(ch) = self.at(q) {
                        if ch == quote {
                            break;
                        }
                        self.string_buffer.push(ch);
                        q += 1;
                    }
                    if self.at(q).is_none() {
                        self.pos = q;
                        return Tok::Eof;
                    }
                    self.pos = q + 1;
                    self.state = LexState::Initial;
                    return Tok::Str(std::mem::take(&mut self.string_buffer));
                }
                LexState::Name | LexState::LayerName | LexState::ComponentName | LexState::IgnoreQuote => {
                    if c == '(' {
                        self.pos += 1;
                        self.state = LexState::Initial;
                        return Tok::Open;
                    }
                    if c == ')' {
                        self.pos += 1;
                        self.state = LexState::Initial;
                        return Tok::Close;
                    }
                    if self.state != LexState::IgnoreQuote && (c == '"' || c == '\'') {
                        self.string_buffer.clear();
                        self.state = if c == '"' { LexState::String1 } else { LexState::String2 };
                        self.pos += 1;
                        continue;
                    }
                    if is_white(c) {
                        self.pos += 1;
                        continue;
                    }
                    let p = self.pos;
                    let len = match self.state {
                        LexState::Name | LexState::LayerName => self.ident_len(
                            p,
                            |c| is_letter(c) || is_digit(c) || is_spec2(c),
                            |c| is_letter(c) || is_digit(c) || is_spec3(c),
                        ),
                        LexState::ComponentName => self.ident_len(
                            p,
                            |c| is_letter(c) || is_digit(c) || is_spec4(c),
                            |c| is_letter(c) || is_digit(c) || is_spec5(c),
                        ),
                        _ => self.ident_len(
                            p,
                            |c| is_letter(c) || is_digit(c) || is_spec3(c),
                            |c| is_letter(c) || is_digit(c) || is_spec3(c),
                        ),
                    };
                    if len == 0 {
                        self.non_ansi(c);
                        continue;
                    }
                    let text = self.text(p, p + len);
                    self.pos = p + len;
                    let layer_state = self.state == LexState::LayerName;
                    self.state = LexState::Initial;
                    if layer_state {
                        if text.eq_ignore_ascii_case("pcb") {
                            return Tok::Kw(Kw::PcbScope);
                        }
                        if text.eq_ignore_ascii_case("signal") {
                            return Tok::Kw(Kw::Signal);
                        }
                    }
                    return Tok::Str(text);
                }
                LexState::SpecChar => {
                    if is_spec2(c) {
                        self.pos += 1;
                        return Tok::Str(c.to_string());
                    }
                    self.non_ansi(c);
                    continue;
                }
            }
        }
    }

    // ------------------------------------------------------------------ raw string reading

    pub fn next_string(&mut self) -> String {
        self.next_string_ext(false, ' ')
    }

    pub fn next_string_nl(&mut self, ignore_newline: bool) -> String {
        self.next_string_ext(ignore_newline, ' ')
    }

    /// `nextString(ignoreNewline, leading)`: reads the raw characters after the last token.
    pub fn next_string_ext(&mut self, ignore_newline: bool, leading: char) -> String {
        let mut result = String::new();
        let base = self.pos;
        let mut i = 0;
        while let Some(c) = self.at(base + i) {
            if !(c == ' '
                || c == '\t'
                || c == '\u{8}'
                || c == leading
                || (ignore_newline && (c == '\n' || c == '\r')))
            {
                break;
            }
            i += 1;
        }
        let quoted = self.at(base + i) == Some('"');
        if quoted {
            i += 1;
        }
        while let Some(c) = self.at(base + i) {
            let stop = if quoted {
                c == '\n' || c == '\r' || c == '"'
            } else {
                c == ' ' || c == '\t' || c == '\u{8}' || c == '\n' || c == '\r' || c == '(' || c == ')' || c == leading
            };
            if stop {
                break;
            }
            result.push(c);
            i += 1;
        }
        if quoted && self.at(base + i).is_some() {
            i += 1;
        }
        if i > 0 {
            self.pos += i;
            self.state = LexState::Initial;
            let last = self.chars[self.pos - 1];
            if last == '(' || last == ')' {
                self.pos -= 1;
            }
        }
        result
    }

    pub fn next_string_list(&mut self) -> Vec<String> {
        self.next_string_list_sep(' ')
    }

    pub fn next_string_list_sep(&mut self, separator: char) -> Vec<String> {
        let mut result = Vec::new();
        let first = self.next_string_ext(true, separator);
        if !first.is_empty() {
            result.push(first);
        }
        loop {
            let s = self.next_string_ext(true, separator);
            if s.is_empty() {
                break;
            }
            result.push(s);
        }
        result
    }

    /// `nextDouble()`: None if the string is no number.
    pub fn next_double(&mut self) -> Option<f64> {
        let s = self.next_string();
        if s.contains('e') || s.contains('E') {
            return java_parse_double(&s).filter(|v| v.is_finite());
        }
        number_format_parse(&s)
    }

    /// `nextClosingBracket()`.
    pub fn next_closing_bracket(&mut self) -> bool {
        let t = self.next_token();
        if t == Tok::Eof {
            log::warn!("Network.read_net_pins: unexpected end of file");
            return false;
        }
        if t != Tok::Close {
            log::warn!("Network.read_net_pins: expected closed bracket is missing");
            return false;
        }
        true
    }

    /// `ScopeKeyword.skipScope`: skips the current scope. Returns false at end of file.
    pub fn skip_scope(&mut self) -> bool {
        let mut open = 1;
        while open > 0 {
            self.state = LexState::Name;
            match self.next_token() {
                Tok::Eof => return false,
                Tok::Open => open += 1,
                Tok::Close => open -= 1,
                _ => {}
            }
        }
        self.state = LexState::Initial;
        true
    }
}

/// `Double.parseDouble` (whitespace trimmed, optional type suffix). None on failure.
pub fn java_parse_double(s: &str) -> Option<f64> {
    let t = s.trim_matches(|c: char| c <= ' ');
    let t = t.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(t);
    if t.is_empty() {
        return None;
    }
    match t {
        "NaN" | "+NaN" | "-NaN" => return Some(f64::NAN),
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    // Rust accepts "inf"/"nan", which Java does not.
    if t.chars().any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E') {
        return None;
    }
    t.parse::<f64>().ok()
}

/// `NumberFormat.getInstance(Locale.US).parse(s)`: parses the longest numeric prefix
/// (with grouping separators). None on failure.
fn number_format_parse(s: &str) -> Option<f64> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let negative = chars.first() == Some(&'-');
    if negative {
        i += 1;
    }
    let mut digits = String::new();
    let mut seen_digit = false;
    let mut seen_point = false;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            digits.push(c);
            seen_digit = true;
        } else if c == ',' && !seen_point {
            // grouping separator
        } else if c == '.' && !seen_point {
            seen_point = true;
            digits.push('.');
        } else {
            break;
        }
        i += 1;
    }
    if !seen_digit {
        return None;
    }
    let v: f64 = if digits.starts_with('.') {
        format!("0{digits}").parse().ok()?
    } else {
        digits.trim_end_matches('.').parse().ok()?
    };
    Some(if negative { -v } else { v })
}
