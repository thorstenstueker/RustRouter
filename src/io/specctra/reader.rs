//! The read context (`ReadScopeParameter`), the generic scope reader (`ScopeKeyword`),
//! the `parser` and `resolution` scopes and the entry point (`DsnReader.readBoard`).

use super::autoroute_settings::DsnAutorouteSettings;
use super::lexer::{Kw, LexState, Lexer, Tok};
use super::network::NetList;
use super::part_library::{LogicalPart, LogicalPartMapping};
use super::placement::ComponentPlacement;
use super::scopes::read_string_scope;
use super::shapes::{DsnLayerStructure, ReadAreaScopeResult};
use crate::board::structure::WriteResolution;
use crate::board::{Board, CoordinateTransform, Unit};
use crate::rules::AngleRestriction;

/// Information for inserting a plane (`ReadScopeParameter.PlaneInfo`).
pub struct PlaneInfo {
    pub area: Option<ReadAreaScopeResult>,
    pub net_name: String,
}

/// The state of the DSN reader (`ReadScopeParameter`).
pub struct ReadCtx {
    pub lx: Lexer,
    pub board: Option<Board>,
    pub netlist: NetList,
    pub warnings: Vec<String>,
    pub plane_list: Vec<PlaneInfo>,
    pub placement_list: Vec<ComponentPlacement>,
    pub constants: Vec<(String, String)>,
    pub via_padstack_names: Option<Vec<String>>,
    pub via_at_smd_allowed: bool,
    pub snap_angle: AngleRestriction,
    pub logical_part_mappings: Vec<LogicalPartMapping>,
    pub logical_parts: Vec<LogicalPart>,
    pub string_quote: String,
    pub host_cad: Option<String>,
    pub host_version: Option<String>,
    pub dsn_file_generated_by_host: bool,
    pub board_outline_ok: bool,
    pub write_resolution: Option<WriteResolution>,
    pub coordinate_transform: Option<CoordinateTransform>,
    pub layer_structure: Option<DsnLayerStructure>,
    pub autoroute_settings: Option<DsnAutorouteSettings>,
    pub unit: Unit,
    pub resolution: i32,
}

impl ReadCtx {
    pub fn new(lx: Lexer) -> ReadCtx {
        ReadCtx {
            lx,
            board: None,
            netlist: NetList::default(),
            warnings: Vec::new(),
            plane_list: Vec::new(),
            placement_list: Vec::new(),
            constants: Vec::new(),
            via_padstack_names: None,
            via_at_smd_allowed: false,
            snap_angle: AngleRestriction::FortyfiveDegree,
            logical_part_mappings: Vec::new(),
            logical_parts: Vec::new(),
            string_quote: "\"".to_string(),
            host_cad: None,
            host_version: None,
            dsn_file_generated_by_host: true,
            board_outline_ok: true,
            write_resolution: None,
            coordinate_transform: None,
            layer_structure: None,
            autoroute_settings: None,
            unit: Unit::Mil,
            resolution: 100,
        }
    }

    /// The coordinate transform (set when the board is created).
    pub fn ct(&self) -> CoordinateTransform {
        self.coordinate_transform.unwrap_or(CoordinateTransform::new(1.0, 0.0, 0.0))
    }

    /// Reports a situation, in which the original throws an unchecked exception.
    pub fn fatal(&mut self, msg: &str) -> bool {
        self.lx.set_fatal(msg);
        false
    }

    pub fn warn(&mut self, msg: String) {
        log::warn!("{msg}");
        self.warnings.push(msg);
    }
}

/// `ScopeKeyword.readScope` for the generic scope keywords.
pub fn read_generic_scope(ctx: &mut ReadCtx) -> bool {
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, ctx.lx.next_token());
        if next == Tok::Eof {
            return true;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            match next {
                Tok::Kw(k) if k.is_scope_keyword() => {
                    if !read_scope_keyword(ctx, k) {
                        return false;
                    }
                }
                _ => {
                    ctx.lx.skip_scope();
                }
            }
        }
    }
    true
}

/// Dispatches the `readScope` of a scope keyword.
pub fn read_scope_keyword(ctx: &mut ReadCtx, k: Kw) -> bool {
    match k {
        Kw::ParserScope => read_parser_scope(ctx),
        Kw::ResolutionScope => read_resolution_scope(ctx),
        Kw::StructureScope => super::structure::read_structure_scope(ctx),
        Kw::LibraryScope => super::library::read_library_scope(ctx),
        Kw::PartLibraryScope => super::part_library::read_part_library_scope(ctx),
        Kw::NetworkScope => super::network::read_network_scope(ctx),
        Kw::WiringScope => super::wiring::read_wiring_scope(ctx),
        Kw::ComponentScope => super::placement::read_component_scope(ctx),
        Kw::PlaceControl => super::placement::read_place_control_scope(ctx),
        Kw::PlaneScope => super::structure::read_plane_scope(ctx),
        _ => read_generic_scope(ctx),
    }
}

// ------------------------------------------------------------------ parser scope

fn read_write_resolution(lx: &mut Lexer) -> Option<WriteResolution> {
    let Tok::Str(s) = lx.next_token() else {
        log::warn!("Parser.read_write_solution: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    let Tok::Int(v) = lx.next_token() else {
        log::warn!("Parser.read_write_solution: integer expected expected at '{}'", lx.scope_identifier);
        return None;
    };
    if lx.next_token() != Tok::Close {
        log::warn!("Parser.read_write_solution: closing_bracket expected at '{}'", lx.scope_identifier);
        return None;
    }
    Some(WriteResolution {
        char_name: s,
        positive_int: v,
    })
}

fn read_constant(lx: &mut Lexer) -> Option<(String, String)> {
    lx.yybegin(LexState::Name);
    let Tok::Str(a) = lx.next_token() else {
        log::warn!("Parser.read_constant: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    lx.yybegin(LexState::Name);
    let Tok::Str(b) = lx.next_token() else {
        log::warn!("Parser.read_constant: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    if lx.next_token() != Tok::Close {
        log::warn!("Parser.read_constant: closing_bracket expected at '{}'", lx.scope_identifier);
        return None;
    }
    Some((a, b))
}

fn read_quote_char(lx: &mut Lexer) -> Option<String> {
    let Tok::Str(s) = lx.next_token() else {
        log::warn!("Parser.read_quote_char: string expected at '{}'", lx.scope_identifier);
        return None;
    };
    if lx.next_token() != Tok::Close {
        log::warn!("Parser.read_quote_char: closing bracket expected at '{}'", lx.scope_identifier);
        return None;
    }
    Some(s)
}

fn read_parser_scope(ctx: &mut ReadCtx) -> bool {
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, ctx.lx.next_token());
        if next == Tok::Eof {
            log::warn!("Parser.read_scope: unexpected end of file at '{}'", ctx.lx.scope_identifier);
            return false;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            match next {
                Tok::Kw(Kw::StringQuote) => match read_quote_char(&mut ctx.lx) {
                    Some(q) => ctx.string_quote = q,
                    None => return false,
                },
                Tok::Kw(Kw::HostCad) => ctx.host_cad = Some(read_string_scope(&mut ctx.lx)),
                Tok::Kw(Kw::HostVersion) => ctx.host_version = Some(read_string_scope(&mut ctx.lx)),
                Tok::Kw(Kw::Constant) => {
                    if let Some(c) = read_constant(&mut ctx.lx) {
                        ctx.constants.push(c);
                    }
                }
                Tok::Kw(Kw::WriteResolution) => ctx.write_resolution = read_write_resolution(&mut ctx.lx),
                Tok::Kw(Kw::GeneratedByFreerouting) => {
                    ctx.dsn_file_generated_by_host = false;
                    ctx.lx.skip_scope();
                }
                _ => {
                    ctx.lx.skip_scope();
                }
            }
        }
    }
    true
}

fn read_resolution_scope(ctx: &mut ReadCtx) -> bool {
    let Tok::Str(s) = ctx.lx.next_token() else {
        log::warn!("Resolution.read_scope: string expected at '{}'", ctx.lx.scope_identifier);
        return false;
    };
    let Some(unit) = Unit::from_str_opt(&s) else {
        log::warn!(
            "Resolution.read_scope: unit mil, inch or mm expected at '{}'",
            ctx.lx.scope_identifier
        );
        return false;
    };
    ctx.unit = unit;
    let Tok::Int(r) = ctx.lx.next_token() else {
        log::warn!("Resolution.read_scope: integer expected at '{}'", ctx.lx.scope_identifier);
        return false;
    };
    ctx.resolution = r;
    if ctx.lx.next_token() != Tok::Close {
        log::warn!("Resolution.read_scope: closing bracket expected at '{}'", ctx.lx.scope_identifier);
        return false;
    }
    true
}

// ------------------------------------------------------------------ entry point

/// Result of reading a DSN file.
pub struct DsnReadResult {
    pub board: Board,
    /// The `(autoroute_settings ...)` of the structure scope, if present.
    pub autoroute_settings: Option<DsnAutorouteSettings>,
    /// The name of the design from the `(pcb <name>` header.
    pub design_name: Option<String>,
    /// Warnings collected while reading.
    pub warnings: Vec<String>,
}

/// Errors of the DSN reader.
#[derive(Debug)]
pub enum DsnError {
    Io(std::io::Error),
    /// The file does not start with `(pcb`.
    NotDsn,
    /// The board outline is missing (`BoardReadResult.OutlineMissing`).
    OutlineMissing,
    /// Parsing failed (`BoardReadResult.ParseError` or an exception in the original).
    Parse(String),
}

impl std::fmt::Display for DsnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DsnError::Io(e) => write!(f, "IO error: {e}"),
            DsnError::NotDsn => write!(f, "Not a Specctra DSN file: expected '(pcb <name>' header"),
            DsnError::OutlineMissing => write!(f, "The board outline is missing"),
            DsnError::Parse(m) => write!(f, "DSN parse error: {m}"),
        }
    }
}

impl std::error::Error for DsnError {}

impl From<std::io::Error> for DsnError {
    fn from(e: std::io::Error) -> Self {
        DsnError::Io(e)
    }
}

/// Reads a DSN file from its text (`DsnReader.readBoard`).
pub fn read_dsn_str(text: &str) -> Result<DsnReadResult, DsnError> {
    let mut ctx = ReadCtx::new(Lexer::new(text));
    let mut pcb_name = None;
    for i in 0..3 {
        let t = ctx.lx.next_token();
        let ok = match i {
            0 => t == Tok::Open,
            1 => {
                let ok = t.is_kw(Kw::PcbScope);
                ctx.lx.yybegin(LexState::Name);
                ok
            }
            _ => {
                if let Tok::Str(s) = t {
                    pcb_name = Some(s);
                }
                true
            }
        };
        if !ok {
            return Err(DsnError::NotDsn);
        }
    }
    let read_ok = read_generic_scope(&mut ctx);
    if let Some(msg) = ctx.lx.fatal.take() {
        return Err(DsnError::Parse(msg));
    }
    if read_ok {
        let Some(mut board) = ctx.board.take() else {
            return Err(DsnError::Parse("no structure scope found".to_string()));
        };
        if ctx.autoroute_settings.is_none() {
            adjust_plane_autoroute_settings(&mut board);
        }
        if !ctx.warnings.is_empty() {
            log::warn!(
                "DSN file '{}' was loaded with {} warning(s).",
                pcb_name.as_deref().unwrap_or("unknown"),
                ctx.warnings.len()
            );
        }
        Ok(DsnReadResult {
            board,
            autoroute_settings: ctx.autoroute_settings,
            design_name: pcb_name,
            warnings: ctx.warnings,
        })
    } else if !ctx.board_outline_ok {
        Err(DsnError::OutlineMissing)
    } else {
        Err(DsnError::Parse("DSN structure parsing failed".to_string()))
    }
}

/// `DsnFile.adjustPlaneAutorouteSettings`: marks nets of large conduction areas on signal
/// layers as plane nets. Returns true, if something was changed.
pub fn adjust_plane_autoroute_settings(board: &mut Board) -> bool {
    use crate::board::FixedState;
    let layer_count = board.layer_structure.layers.len();
    if layer_count < 1 {
        return false;
    }
    if board.layer_structure.layers.iter().any(|l| !l.is_signal) {
        return false;
    }
    let mut changed_layers = vec![false; layer_count];
    let mut nothing_changed = true;
    let mut board_area = 0.0;
    if let Some(o) = board.get_outline().and_then(|o| o.as_board_outline()) {
        for s in &o.shapes {
            let pieces: Option<Vec<crate::geometry::TileShape>> = match s {
                crate::geometry::Shape::Polygon(p) => p.try_split_to_convex().map(|x| x.to_vec()),
                other => Some(other.split_to_convex()),
            };
            if let Some(pieces) = pieces {
                board_area += pieces.iter().map(|p| p.area()).sum::<f64>();
            }
        }
    }
    if board_area <= 0.0 {
        return false;
    }
    let ids: Vec<_> = board.items.iter().filter(|i| i.is_conduction_area()).map(|i| i.id).collect();
    for id in ids {
        let item = board.item(id);
        let o = item.as_obstacle().unwrap();
        let layer = o.layer;
        if layer < 0 || layer as usize >= layer_count {
            continue;
        }
        if !board.layer_structure.layers[layer as usize].is_signal {
            continue;
        }
        if o.convex_pieces.is_empty() {
            continue;
        }
        let area: f64 = o.convex_pieces.iter().map(|p| p.area()).sum();
        if area < 0.3 * board_area {
            continue;
        }
        let nets: Vec<i32> = item.net_numbers.to_vec();
        let fixed = item.fixed_state;
        for n in nets {
            if let Some(net) = board.rules.nets.get_mut(n) {
                net.set_contains_plane(true);
                nothing_changed = false;
            }
        }
        changed_layers[layer as usize] = true;
        if fixed < FixedState::UserFixed {
            board.set_fixed_state(id, FixedState::UserFixed);
        }
    }
    for (i, c) in changed_layers.iter().enumerate() {
        if *c {
            log::info!(
                "Layer '{}' contains a power plane / large copper pour covering >=30% of the board; associated net(s) configured for plane routing.",
                board.layer_structure.layers[i].name
            );
        }
    }
    !nothing_changed
}
