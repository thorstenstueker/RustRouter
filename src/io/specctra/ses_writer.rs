//! Writer of Specctra session files (port of `SesWriter`, `SesTracePath`,
//! `IndentFileWriter`, `IdentifierType` and the `writeScopeInt` methods of the shapes).

use super::java_fmt::{java_double_to_string, java_plain_string};
use super::shapes::{board_to_dsn_point, board_to_dsn_rel_point, round_i};
use crate::board::{Board, CoordinateTransform, FixedState, ItemId};
use crate::geometry::{FloatPoint, Point, Shape, TileShape};
use crate::library::{PackageId, Padstack};
use std::io::Write;

/// `IndentFileWriter`.
pub struct IndentWriter {
    out: String,
    indent: usize,
}

impl IndentWriter {
    pub fn new() -> Self {
        IndentWriter {
            out: String::new(),
            indent: 0,
        }
    }

    pub fn write(&mut self, s: &str) {
        self.out.push_str(s);
    }

    pub fn start_scope_nl(&mut self, new_line: bool) {
        if new_line {
            self.new_line();
        }
        self.out.push('(');
        self.indent += 1;
    }

    pub fn start_scope(&mut self) {
        self.start_scope_nl(true);
    }

    pub fn end_scope(&mut self) {
        self.indent = self.indent.saturating_sub(1);
        self.new_line();
        self.out.push(')');
    }

    pub fn new_line(&mut self) {
        self.out.push('\n');
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
    }

    pub fn into_string(self) -> String {
        self.out
    }
}

impl Default for IndentWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// `IdentifierType`: quotes identifiers containing reserved characters.
pub struct IdentifierType {
    reserved: Vec<&'static str>,
    quote: String,
}

impl IdentifierType {
    pub fn new(reserved: &[&'static str], quote: &str) -> Self {
        IdentifierType {
            reserved: reserved.to_vec(),
            quote: quote.to_string(),
        }
    }

    pub fn write(&self, name: &str, file: &mut IndentWriter) {
        let mut name: String = name.to_string();
        while name.chars().count() >= 2 && name.starts_with('"') && name.ends_with('"') {
            let mut c = name.chars();
            c.next();
            c.next_back();
            name = c.collect();
        }
        if name.contains(self.quote.as_str()) && !self.quote.is_empty() {
            name = name.replace(self.quote.as_str(), "");
        }
        let mut need_quotes = self.reserved.iter().any(|r| name.contains(r));
        if !need_quotes && !name.is_ascii() {
            need_quotes = true;
        }
        if !need_quotes && name.as_bytes().contains(&0) {
            need_quotes = true;
        }
        if !need_quotes {
            // name.matches("^-?\\d.*"): '.' does not match line terminators
            let b = name.strip_prefix('-').unwrap_or(&name);
            if b.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
                && !b.chars().skip(1).any(|c| matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'))
            {
                need_quotes = true;
            }
        }
        if need_quotes {
            file.write(&self.quote);
            file.write(&name);
            file.write(&self.quote);
        } else {
            file.write(&name);
        }
    }
}

/// Error while writing a session file.
#[derive(Debug)]
pub enum SesError {
    Io(std::io::Error),
    /// The board cannot be represented (an IOException of the original).
    Geometry(String),
}

impl std::fmt::Display for SesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SesError::Io(e) => write!(f, "IO error: {e}"),
            SesError::Geometry(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for SesError {}

impl From<std::io::Error> for SesError {
    fn from(e: std::io::Error) -> Self {
        SesError::Io(e)
    }
}

// ------------------------------------------------------------------ DSN shapes for writing

/// A shape in DSN coordinates (`CoordinateTransform.boardToDsn(Shape, Layer)`).
pub(super) enum OutShape {
    Rect([f64; 4]),
    Polygon(Vec<f64>),
    /// diameter, x, y
    Circle([f64; 3]),
}

pub(super) fn board_to_dsn_shape(t: &CoordinateTransform, s: &Shape, rel: bool) -> OutShape {
    let pt = |p: &FloatPoint| if rel { board_to_dsn_rel_point(t, p) } else { board_to_dsn_point(t, p) };
    match s {
        Shape::Tile(TileShape::Box(b)) => {
            let (bx, by) = if rel { (0.0, 0.0) } else { (t.base_x, t.base_y) };
            let f = t.scale_factor;
            OutShape::Rect([
                b.ll.x as f64 / f + bx,
                b.ll.y as f64 / f + by,
                b.ur.x as f64 / f + bx,
                b.ur.y as f64 / f + by,
            ])
        }
        Shape::Circle(c) => {
            let d = 2.0 * t.board_to_dsn(c.radius as f64);
            let p = pt(&c.center.to_float());
            OutShape::Circle([d, p[0], p[1]])
        }
        _ => {
            let mut v = Vec::new();
            for c in s.corner_approx_arr() {
                let p = pt(&c);
                v.push(p[0]);
                v.push(p[1]);
            }
            OutShape::Polygon(v)
        }
    }
}

pub(super) fn write_shape(s: &OutShape, layer: &str, id: &IdentifierType, file: &mut IndentWriter, int: bool) {
    let num = |v: f64| if int { round_i(v).to_string() } else { java_double_to_string(v) };
    match s {
        OutShape::Rect(c) => {
            file.new_line();
            file.write("(rect ");
            id.write(layer, file);
            for v in c {
                file.write(" ");
                file.write(&num(*v));
            }
            file.write(")");
        }
        OutShape::Circle(c) => {
            file.new_line();
            file.write("(circle ");
            id.write(layer, file);
            for v in c {
                file.write(" ");
                file.write(&num(*v));
            }
            file.write(")");
        }
        OutShape::Polygon(c) => {
            file.start_scope();
            file.write("polygon ");
            id.write(layer, file);
            file.write(" ");
            file.write("0");
            for i in 0..c.len() / 2 {
                file.new_line();
                file.write(&num(c[2 * i]));
                file.write(" ");
                file.write(&num(c[2 * i + 1]));
            }
            file.end_scope();
        }
    }
}

// ------------------------------------------------------------------ session writer

const RESERVED: [&str; 10] = ["(", ")", " ", ";", "-", "_", "/", "~", "{", "}"];

/// Writes the session file of `board` (`SesWriter.write`).
pub fn write_ses<W: Write>(board: &Board, design_name: &str, out: &mut W) -> Result<(), SesError> {
    let s = ses_string(board, design_name)?;
    out.write_all(s.as_bytes())?;
    out.flush()?;
    Ok(())
}

/// The content of the session file of `board` as string.
pub fn ses_string(board: &Board, design_name: &str) -> Result<String, SesError> {
    let mut file = IndentWriter::new();
    let session_name = design_name.replace(".dsn", ".ses");
    let id = IdentifierType::new(&RESERVED, &board.communication.specctra_parser_info.string_quote);
    let ct0 = board
        .communication
        .coordinate_transform
        .unwrap_or(CoordinateTransform::new(1.0, 0.0, 0.0));
    let scale_factor = ct0.dsn_to_board(1.0) / board.communication.resolution as f64;
    let ct = CoordinateTransform::new(scale_factor, 0.0, 0.0);
    file.start_scope_nl(false);
    file.write("session ");
    id.write(&session_name, &mut file);
    file.new_line();
    file.write("(base_design ");
    id.write(design_name, &mut file);
    file.write(")");
    write_placement(board, &id, &ct, &mut file);
    write_was_is(board, &id, &mut file);
    write_routes(board, &id, &ct, &mut file)?;
    file.end_scope();
    Ok(file.into_string())
}

fn write_resolution(board: &Board, file: &mut IndentWriter) {
    file.new_line();
    file.write("(resolution ");
    file.write(board.communication.unit.as_str());
    file.write(" ");
    file.write(&board.communication.resolution.to_string());
    file.write(")");
}

fn write_placement(board: &Board, id: &IdentifierType, ct: &CoordinateTransform, file: &mut IndentWriter) {
    file.start_scope();
    file.write("placement");
    write_resolution(board, file);
    for p in board.library.packages.iter() {
        write_components(board, id, ct, file, p.id);
    }
    file.end_scope();
}

fn write_components(board: &Board, id: &IdentifierType, ct: &CoordinateTransform, file: &mut IndentWriter, pkg: PackageId) {
    let mut found = false;
    for c in board.components.iter() {
        if c.get_package() != pkg {
            continue;
        }
        let undeleted = board.items.iter().any(|i| i.component_id == c.id);
        if !undeleted {
            continue;
        }
        if !found {
            file.start_scope();
            file.write("component ");
            id.write(&board.library.packages.get(pkg).name, file);
            found = true;
        }
        file.new_line();
        file.write("(place ");
        id.write(&c.name, file);
        let loc = board_to_dsn_point(ct, &c.get_exact_location().unwrap_or(FloatPoint::ZERO));
        file.write(" ");
        file.write(&java_double_to_string(loc[0]));
        file.write(" ");
        file.write(&java_double_to_string(loc[1]));
        if c.placed_on_front() {
            file.write(" front ");
        } else {
            file.write(" back ");
        }
        file.write(&java_plain_string(c.get_rotation_in_degree()));
        if c.position_fixed && !board.communication.host_cad_is_kicad() {
            file.new_line();
            file.write(" (lock_type position)");
        }
        file.write(")");
    }
    if found {
        file.end_scope();
    }
}

fn write_pin_name(board: &Board, id: &IdentifierType, file: &mut IndentWriter, pin: ItemId) {
    let item = board.item(pin);
    let p = item.as_pin().unwrap();
    match board.components.try_get(item.component_id) {
        Some(c) => {
            id.write(&c.name, file);
            file.write("-");
            let pp = board.library.packages.get(c.get_package()).get_pin(p.pin_index as usize).unwrap();
            id.write(&pp.name, file);
        }
        None => log::warn!("SesWriter.writeWasIs: component not found"),
    }
}

fn write_was_is(board: &Board, id: &IdentifierType, file: &mut IndentWriter) {
    file.start_scope();
    file.write("was_is");
    for pin_id in board.get_pins() {
        let changed_to = board.item(pin_id).as_pin().unwrap().changed_to;
        if changed_to != pin_id {
            file.new_line();
            file.write("(pins ");
            write_pin_name(board, id, file, pin_id);
            file.write(" ");
            write_pin_name(board, id, file, changed_to);
            file.write(")");
        }
    }
    file.end_scope();
}

fn write_routes(board: &Board, id: &IdentifierType, ct: &CoordinateTransform, file: &mut IndentWriter) -> Result<(), SesError> {
    file.start_scope();
    file.write("routes ");
    write_resolution(board, file);
    // the reduced parser scope
    let info = &board.communication.specctra_parser_info;
    file.start_scope();
    file.write("parser");
    if let Some(h) = &info.host_cad {
        file.new_line();
        file.write("(host_cad ");
        id.write(h, file);
        file.write(")");
    }
    if let Some(h) = &info.host_version {
        file.new_line();
        file.write("(host_version ");
        id.write(h, file);
        file.write(")");
    }
    for (a, b) in &info.constants {
        file.new_line();
        file.write("(constant ");
        id.write(a, file);
        file.write(" ");
        id.write(b, file);
        file.write(" ");
        file.write(")");
    }
    if let Some(w) = &info.write_resolution {
        file.new_line();
        file.write("(write_resolution ");
        file.write(&w.char_name.chars().take(1).collect::<String>());
        file.write(" ");
        file.write(&w.positive_int.to_string());
        file.write(")");
    }
    file.end_scope();
    // library
    file.start_scope();
    file.write("library_out ");
    let mut written: Vec<&str> = Vec::new();
    for ps_id in board.library.get_via_padstacks() {
        let ps = board.library.padstacks.get(*ps_id);
        if written.contains(&ps.name.as_str()) {
            continue;
        }
        written.push(&ps.name);
        write_padstack(ps, board, id, ct, file);
    }
    file.end_scope();
    // network
    file.start_scope();
    file.write("network_out ");
    for net in 1..=board.rules.nets.max_net_number() {
        write_net(net, board, id, ct, file)?;
    }
    file.end_scope();
    file.end_scope();
    Ok(())
}

fn write_padstack(ps: &Padstack, board: &Board, id: &IdentifierType, ct: &CoordinateTransform, file: &mut IndentWriter) {
    let lc = board.layer_count();
    let shape = |l: i32| if l >= 0 && (l as usize) < ps.shapes().len() { ps.shapes()[l as usize].as_ref() } else { None };
    let mut first = 0;
    while first < lc && shape(first).is_none() {
        first += 1;
    }
    let mut last = lc - 1;
    while last >= 0 && shape(last).is_none() {
        last -= 1;
    }
    if first >= lc || last < 0 {
        log::warn!("SesWriter.writePadstack: padstack shape not found");
        return;
    }
    file.start_scope();
    file.write("padstack ");
    id.write(&ps.name, file);
    for i in first..=last {
        let Some(s) = shape(i) else { continue };
        let layer = &board.layer_structure.layers[i as usize].name;
        let out = board_to_dsn_shape(ct, s, true);
        file.start_scope();
        file.write("shape");
        write_shape(&out, layer, id, file, true);
        file.end_scope();
    }
    if !ps.attach_allowed {
        file.new_line();
        file.write("(attach off)");
    }
    file.end_scope();
}

fn write_net(net: i32, board: &Board, id: &IdentifierType, ct: &CoordinateTransform, file: &mut IndentWriter) -> Result<(), SesError> {
    let mut header = false;
    for item_id in board.get_connectable_items(net) {
        let item = board.item(item_id);
        if item.fixed_state == FixedState::SystemFixed {
            continue;
        }
        let is_wire = item.is_trace();
        let is_via = item.is_via();
        let is_conduction = item.is_conduction_area()
            && board.layer_structure.layers[item.first_layer() as usize].is_signal;
        if !header && (is_wire || is_via || is_conduction) {
            file.start_scope();
            file.write("net ");
            match board.rules.nets.get(net) {
                Some(n) => id.write(&n.name, file),
                None => log::warn!("SesWriter.writeNet: net not found"),
            }
            header = true;
        }
        if is_wire {
            write_wire(item_id, board, id, ct, file)?;
        } else if is_via {
            let via = item.as_via().unwrap();
            let ps = board.library.padstacks.get(via.drill.padstack);
            file.start_scope();
            file.write("via ");
            id.write(&ps.name, file);
            file.write(" ");
            let loc = board_to_dsn_point(ct, &via.drill.center.to_float());
            file.write(&java_double_to_string(loc[0]));
            file.write(" ");
            file.write(&java_double_to_string(loc[1]));
            write_fixed_state(file, item.fixed_state);
            file.end_scope();
        } else if is_conduction {
            write_conduction_area(item_id, board, id, ct, file);
        }
    }
    if header {
        file.end_scope();
    }
    Ok(())
}

fn write_fixed_state(file: &mut IndentWriter, fixed: FixedState) {
    if fixed <= FixedState::ShoveFixed {
        return;
    }
    file.new_line();
    file.write("(type ");
    if fixed == FixedState::SystemFixed {
        file.write("fix)");
    } else {
        file.write("protect)");
    }
}

fn write_wire(wire: ItemId, board: &Board, id: &IdentifierType, ct: &CoordinateTransform, file: &mut IndentWriter) -> Result<(), SesError> {
    let item = board.item(wire);
    let t = item.as_trace().unwrap();
    let layer = &board.layer_structure.layers[t.layer as usize].name;
    let width = ct.board_to_dsn((2 * t.half_width) as f64);
    let coordinates = ses_trace_path(wire, board, ct)?;
    file.start_scope();
    file.write("wire");
    if !width.is_finite() || width <= 0.0 {
        return Err(SesError::Geometry("SES trace width must be finite and positive".to_string()));
    }
    file.start_scope();
    file.write("path ");
    id.write(layer, file);
    file.write(&format!(" {}", java_plain_string(width)));
    for i in 0..coordinates.len() / 2 {
        file.new_line();
        file.write(&format!("{} {}", java_plain_string(coordinates[2 * i]), java_plain_string(coordinates[2 * i + 1])));
    }
    file.end_scope();
    write_fixed_state(file, item.fixed_state);
    file.end_scope();
    Ok(())
}

fn write_conduction_area(area_id: ItemId, board: &Board, id: &IdentifierType, ct: &CoordinateTransform, file: &mut IndentWriter) {
    let item = board.item(area_id);
    if item.net_count() != 1 {
        log::warn!("SesWriter.writeConductionArea: unexpected net count");
        return;
    }
    let o = item.as_obstacle().unwrap();
    let layer = &board.layer_structure.layers[o.layer as usize].name;
    file.start_scope();
    file.write("wire ");
    let border = board_to_dsn_shape(ct, o.area.get_border(), false);
    write_shape(&border, layer, id, file, true);
    for h in o.area.get_holes() {
        let hole = board_to_dsn_shape(ct, h, false);
        file.start_scope();
        file.write("window");
        write_shape(&hole, layer, id, file, false);
        file.end_scope();
    }
    file.end_scope();
}

// ------------------------------------------------------------------ SesTracePath

fn ses_trace_path(wire: ItemId, board: &Board, ct: &CoordinateTransform) -> Result<Vec<f64>, SesError> {
    let t = board.item(wire).as_trace().unwrap();
    let corners: Vec<Point> = t.polyline.corners();
    if corners.len() < 2 {
        return Err(SesError::Geometry("SES trace has fewer than two source corners".to_string()));
    }
    let exact_point = corners.iter().all(|c| *c == corners[0]);
    let original: Vec<FloatPoint> = corners.iter().map(|c| c.to_float()).collect();
    let mut snapped = original.clone();
    if !exact_point {
        if let Some(e) = snapped_endpoint(wire, board, true) {
            snapped[0] = e;
        }
        let n = snapped.len();
        if let Some(e) = snapped_endpoint(wire, board, false) {
            snapped[n - 1] = e;
        }
    }
    let mut encoded = encode(&snapped, ct)?;
    if collapses_segment(&encoded, &corners) {
        encoded = encode(&original, ct)?;
        if collapses_segment(&encoded, &corners) {
            return Err(SesError::Geometry(
                "A nonzero SES segment cannot be represented at floating output precision".to_string(),
            ));
        }
    }
    Ok(encoded)
}

fn collapses_segment(c: &[f64], original: &[Point]) -> bool {
    (1..original.len()).any(|i| {
        original[i - 1] != original[i] && c[2 * i - 2] == c[2 * i] && c[2 * i - 1] == c[2 * i + 1]
    })
}

fn encode(corners: &[FloatPoint], ct: &CoordinateTransform) -> Result<Vec<f64>, SesError> {
    let mut r = Vec::with_capacity(2 * corners.len());
    for c in corners {
        let p = board_to_dsn_point(ct, c);
        r.push(p[0]);
        r.push(p[1]);
    }
    for v in &r {
        if !v.is_finite() || *v <= i64::MIN as f64 || *v >= i64::MAX as f64 {
            return Err(SesError::Geometry("Non-finite or out-of-range SES trace coordinate".to_string()));
        }
    }
    Ok(r)
}

/// `SesTracePath.snappedEndpoint`: the center of a contacted pad or via, if the endpoint
/// lies inside the pad inradius.
fn snapped_endpoint(wire: ItemId, board: &Board, start: bool) -> Option<FloatPoint> {
    let t = board.item(wire).as_trace().unwrap();
    let corner = if start { t.first_corner() } else { t.last_corner() };
    let cf = corner.to_float();
    let contacts = if start { board.trace_start_contacts(wire) } else { board.trace_end_contacts(wire) };
    let layer = t.layer;
    for c in contacts {
        let Some(drill) = board.item(c).as_drill() else { continue };
        if layer < drill.first_layer || layer > drill.last_layer {
            continue;
        }
        let Some(pad) = drill.get_shape((layer - drill.first_layer) as usize) else { continue };
        let center = drill.center.to_float();
        let d = cf.distance(&center);
        if d <= 0.5 {
            return None;
        }
        if d <= pad.border_distance(&center) {
            return Some(center);
        }
    }
    None
}
