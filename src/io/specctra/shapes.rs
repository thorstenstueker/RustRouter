//! DSN layers and shapes (port of `parser/Layer`, `LayerStructure`, `Shape`, `Rectangle`,
//! `Polygon`, `Circle`, `PolygonPath`, `PolylinePath`) and the coordinate transform helpers
//! of `io/CoordinateTransform`.

use super::lexer::{Kw, Lexer, Tok};
use super::scopes::read_string_scope;
use crate::board::CoordinateTransform;
use crate::geometry::{Area, Circle, FloatPoint, IntBox, IntPoint, Point, PolygonShape, PolylineArea, Shape};

// ------------------------------------------------------------------ coordinate transform

/// `CoordinateTransform.dsnToBoard(double[])` (absolute coordinates, base applied).
pub fn dsn_to_board_point(t: &CoordinateTransform, x: f64, y: f64) -> FloatPoint {
    FloatPoint::new(t.dsn_to_board(x - t.base_x), t.dsn_to_board(y - t.base_y))
}

/// `CoordinateTransform.dsnToBoardRel(double[])`.
pub fn dsn_to_board_rel(t: &CoordinateTransform, x: f64, y: f64) -> FloatPoint {
    FloatPoint::new(t.dsn_to_board(x), t.dsn_to_board(y))
}

/// `CoordinateTransform.boardToDsn(FloatPoint)`.
pub fn board_to_dsn_point(t: &CoordinateTransform, p: &FloatPoint) -> [f64; 2] {
    [t.board_to_dsn(p.x) + t.base_x, t.board_to_dsn(p.y) + t.base_y]
}

/// `CoordinateTransform.boardToDsnRel(FloatPoint)`.
pub fn board_to_dsn_rel_point(t: &CoordinateTransform, p: &FloatPoint) -> [f64; 2] {
    [t.board_to_dsn(p.x), t.board_to_dsn(p.y)]
}

/// `(int) Math.round(x)`.
#[inline]
pub fn round_i(x: f64) -> i32 {
    crate::geometry::basics::java_round(x)
}

// ------------------------------------------------------------------ layers

/// A layer of the DSN layer structure.
#[derive(Clone, Debug, PartialEq)]
pub struct DsnLayer {
    pub name: String,
    pub no: i32,
    pub is_signal: bool,
    pub net_names: Vec<String>,
}

/// The layer of a DSN shape: the special layers `pcb` and `signal`, or a real layer.
#[derive(Clone, Debug, PartialEq)]
pub enum LayerRef {
    Pcb,
    Signal,
    Layer(DsnLayer),
    /// A missing layer (null in the original, only possible for polyline paths).
    Missing,
}

impl LayerRef {
    pub fn no(&self) -> i32 {
        match self {
            LayerRef::Layer(l) => l.no,
            _ => -1,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            LayerRef::Pcb => "pcb",
            LayerRef::Signal => "signal",
            LayerRef::Layer(l) => &l.name,
            LayerRef::Missing => "",
        }
    }
}

/// The layer structure read from a DSN file.
#[derive(Clone, Debug, Default)]
pub struct DsnLayerStructure {
    pub layers: Vec<DsnLayer>,
}

impl DsnLayerStructure {
    pub fn new(layers: Vec<DsnLayer>) -> Self {
        DsnLayerStructure { layers }
    }

    pub fn from_board(ls: &crate::board::LayerStructure) -> Self {
        DsnLayerStructure {
            layers: ls
                .layers
                .iter()
                .enumerate()
                .map(|(i, l)| DsnLayer {
                    name: l.name.clone(),
                    no: i as i32,
                    is_signal: l.is_signal,
                    net_names: Vec::new(),
                })
                .collect(),
        }
    }

    pub fn get_no(&self, name: &str) -> i32 {
        for (i, l) in self.layers.iter().enumerate() {
            if l.name == name {
                return i as i32;
            }
        }
        if name.contains("Top") {
            return 0;
        }
        if name.contains("Bottom") {
            return self.layers.len() as i32 - 1;
        }
        -1
    }

    pub fn contains_plane(&self, net_name: &str) -> bool {
        self.layers
            .iter()
            .any(|l| !l.is_signal && l.net_names.iter().any(|n| n == net_name))
    }
}

// ------------------------------------------------------------------ shapes

#[derive(Clone, Debug)]
pub enum DsnShapeKind {
    /// ll x, ll y, ur x, ur y
    Rect([f64; 4]),
    Polygon(Vec<f64>),
    /// diameter, center x, center y
    Circle([f64; 3]),
    PolygonPath { width: f64, coords: Vec<f64> },
    PolylinePath { width: f64, coords: Vec<f64> },
}

#[derive(Clone, Debug)]
pub struct DsnShape {
    pub layer: LayerRef,
    pub kind: DsnShapeKind,
}

impl DsnShape {
    pub fn rect(layer: LayerRef, coor: [f64; 4]) -> DsnShape {
        DsnShape {
            layer,
            kind: DsnShapeKind::Rect(coor),
        }
    }

    pub fn is_path(&self) -> bool {
        matches!(self.kind, DsnShapeKind::PolygonPath { .. } | DsnShapeKind::PolylinePath { .. })
    }

    pub fn path_width_coords(&self) -> Option<(f64, &[f64])> {
        match &self.kind {
            DsnShapeKind::PolygonPath { width, coords } | DsnShapeKind::PolylinePath { width, coords } => {
                Some((*width, coords))
            }
            _ => None,
        }
    }

    /// `boundingBox()`: None for polyline paths (not implemented in the original).
    pub fn bounding_box(&self) -> Option<[f64; 4]> {
        match &self.kind {
            DsnShapeKind::Rect(c) => Some(*c),
            DsnShapeKind::Polygon(coor) => {
                let mut b = [i32::MAX as f64, i32::MAX as f64, i32::MIN as f64, i32::MIN as f64];
                for (i, v) in coor.iter().enumerate() {
                    if i % 2 == 0 {
                        b[0] = b[0].min(*v);
                        b[2] = b[2].max(*v);
                    } else {
                        b[1] = b[1].min(*v);
                        b[3] = b[3].max(*v);
                    }
                }
                Some(b)
            }
            DsnShapeKind::Circle(c) => Some([c[1] - c[0], c[2] - c[0], c[1] + c[0], c[2] + c[0]]),
            DsnShapeKind::PolygonPath { width, coords } => {
                let offset = width / 2.0;
                let mut b = [i32::MAX as f64, i32::MAX as f64, i32::MIN as f64, i32::MIN as f64];
                for (i, v) in coords.iter().enumerate() {
                    if i % 2 == 0 {
                        b[0] = b[0].min(v - offset);
                        // the misplaced parenthesis of the original is kept
                        b[2] = b[2].max(*v) + offset;
                    } else {
                        b[1] = b[1].min(v - offset);
                        b[3] = b[3].max(v + offset);
                    }
                }
                Some(b)
            }
            DsnShapeKind::PolylinePath { .. } => {
                log::warn!("PolylinePath.boundingBox not implemented");
                None
            }
        }
    }

    /// `transformToBoard`: None for polyline paths.
    pub fn transform_to_board(&self, t: &CoordinateTransform) -> Option<Shape> {
        match &self.kind {
            DsnShapeKind::Rect(coor) => {
                let ll = dsn_to_board_point(t, coor[0].min(coor[2]), coor[1].min(coor[3]));
                let ur = dsn_to_board_point(t, coor[0].max(coor[2]), coor[1].max(coor[3]));
                Some(Shape::from(IntBox::from_points(ll.round(), ur.round())))
            }
            DsnShapeKind::Polygon(coor) => {
                let corners: Vec<Point> = (0..coor.len() / 2)
                    .map(|i| Point::Int(dsn_to_board_point(t, coor[2 * i], coor[2 * i + 1]).round()))
                    .collect();
                Some(Shape::Polygon(PolygonShape::new(&corners)))
            }
            DsnShapeKind::Circle(coor) => {
                let center = dsn_to_board_point(t, coor[1], coor[2]).round();
                let radius = round_i(t.dsn_to_board(coor[0]) / 2.0);
                Some(Shape::Circle(Circle::new(center, radius)))
            }
            DsnShapeKind::PolygonPath { width, coords } => {
                let corners: Vec<FloatPoint> = (0..coords.len() / 2)
                    .map(|i| dsn_to_board_point(t, coords[2 * i], coords[2 * i + 1]))
                    .collect();
                Some(polygon_path_shape(&corners, t.dsn_to_board(*width) / 2.0))
            }
            DsnShapeKind::PolylinePath { .. } => {
                log::warn!("PolylinePath.transform_to_board not implemented");
                None
            }
        }
    }

    /// `transformToBoardRel`: None for polyline paths.
    pub fn transform_to_board_rel(&self, t: &CoordinateTransform) -> Option<Shape> {
        match &self.kind {
            DsnShapeKind::Rect(coor) => {
                let b: Vec<i32> = coor.iter().map(|c| round_i(t.dsn_to_board(*c))).collect();
                let r = if b[1] <= b[3] {
                    IntBox::new(b[0], b[1], b[2], b[3])
                } else {
                    IntBox::new(b[0], b[3], b[2], b[1])
                };
                Some(Shape::from(r))
            }
            DsnShapeKind::Polygon(coor) => {
                if coor.len() < 2 {
                    return Some(Shape::Tile(crate::geometry::TileShape::Simplex(
                        crate::geometry::Simplex::empty(),
                    )));
                }
                let corners: Vec<Point> = (0..coor.len() / 2)
                    .map(|i| {
                        Point::Int(IntPoint::new(
                            round_i(t.dsn_to_board(coor[2 * i])),
                            round_i(t.dsn_to_board(coor[2 * i + 1])),
                        ))
                    })
                    .collect();
                Some(Shape::Polygon(PolygonShape::new(&corners)))
            }
            DsnShapeKind::Circle(coor) => {
                let r = round_i(t.dsn_to_board(coor[0]) / 2.0);
                let x = round_i(t.dsn_to_board(coor[1]));
                let y = round_i(t.dsn_to_board(coor[2]));
                Some(Shape::Circle(Circle::new(IntPoint::new(x, y), r)))
            }
            DsnShapeKind::PolygonPath { width, coords } => {
                let corners: Vec<FloatPoint> = (0..coords.len() / 2)
                    .map(|i| dsn_to_board_rel(t, coords[2 * i], coords[2 * i + 1]))
                    .collect();
                Some(polygon_path_shape(&corners, t.dsn_to_board(*width) / 2.0))
            }
            DsnShapeKind::PolylinePath { .. } => {
                log::warn!("PolylinePath.transform_to_board_rel not implemented");
                None
            }
        }
    }
}

fn polygon_path_shape(corners: &[FloatPoint], offset: f64) -> Shape {
    if corners.len() <= 2 {
        let oct = FloatPoint::bounding_octagon(corners);
        return Shape::Tile(crate::geometry::TileShape::Octagon(oct.enlarge(offset)));
    }
    let rounded: Vec<Point> = corners.iter().map(|c| Point::Int(c.round())).collect();
    let result = Shape::Polygon(PolygonShape::new(&rounded));
    if offset > 0.0 {
        return Shape::Tile(result.bounding_tile().enlarge(offset));
    }
    result
}

/// Union of two DSN rectangles (`Rectangle.union`).
pub fn rect_union(a: &[f64; 4], b: &[f64; 4]) -> [f64; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

// ------------------------------------------------------------------ reading

/// `Shape.getLayer`.
fn get_layer(ls: Option<&DsnLayerStructure>, name: &str) -> Option<LayerRef> {
    if name == "pcb" {
        return Some(LayerRef::Pcb);
    }
    if name == "signal" {
        return Some(LayerRef::Signal);
    }
    let Some(ls) = ls else {
        log::warn!("Shape.read_circle_scope: layerStructure != null expected");
        return None;
    };
    let idx = ls.get_no(name);
    if idx < 0 || idx as usize >= ls.layers.len() {
        log::warn!("Shape.read_circle_scope: layer with name '{name}' not found in layer structure.");
        return None;
    }
    Some(LayerRef::Layer(ls.layers[idx as usize].clone()))
}

/// `Shape.readScope`.
pub fn read_shape_scope(lx: &mut Lexer, ls: Option<&DsnLayerStructure>) -> Option<DsnShape> {
    let mut t = lx.next_token();
    if t == Tok::Open {
        t = lx.next_token();
    }
    read_shape_from_keyword(lx, &t, ls)
}

/// `Shape.readScopeFromKeyword`.
pub fn read_shape_from_keyword(lx: &mut Lexer, kw: &Tok, ls: Option<&DsnLayerStructure>) -> Option<DsnShape> {
    match kw {
        Tok::Kw(Kw::Rectangle) => read_rectangle_scope(lx, ls),
        Tok::Kw(Kw::Polygon) => read_polygon_scope(lx, ls),
        Tok::Kw(Kw::Circle) => read_circle_scope(lx, ls),
        Tok::Kw(Kw::PolygonPath) => read_polygon_path_scope(lx, ls),
        Tok::Kw(Kw::PolylinePath) => read_polyline_path_scope(lx, ls),
        _ => {
            lx.skip_scope();
            None
        }
    }
}

fn numbers(tokens: &[Tok]) -> Option<Vec<f64>> {
    tokens.iter().map(|t| t.as_number()).collect()
}

pub fn read_polyline_path_scope(lx: &mut Lexer, ls: Option<&DsnLayerStructure>) -> Option<DsnShape> {
    let layer_name = lx.next_string();
    let layer = get_layer(ls, &layer_name).unwrap_or(LayerRef::Missing);
    let mut list = Vec::new();
    loop {
        let t = lx.next_token();
        if t == Tok::Close {
            break;
        }
        if t == Tok::Eof {
            // endless loop in the original
            return None;
        }
        list.push(t);
    }
    if list.len() < 5 {
        log::warn!("PolylinePath.read_scope: too few numbers in scope at '{}'", lx.scope_identifier);
        return None;
    }
    let Some(width) = list[0].as_number() else {
        log::warn!("PolylinePath.read_scope: number expected at '{}'", lx.scope_identifier);
        return None;
    };
    let Some(coords) = numbers(&list[1..]) else {
        log::warn!("Shape.read_polygon_path_scope: number expected at '{}'", lx.scope_identifier);
        return None;
    };
    Some(DsnShape {
        layer,
        kind: DsnShapeKind::PolylinePath { width, coords },
    })
}

pub fn read_rectangle_scope(lx: &mut Lexer, ls: Option<&DsnLayerStructure>) -> Option<DsnShape> {
    let layer_name = lx.next_string();
    let layer = get_layer(ls, &layer_name).or_else(|| get_layer(ls, "signal"));
    let mut coor = [0.0; 4];
    for c in coor.iter_mut() {
        match lx.next_token().as_number() {
            Some(v) => *c = v,
            None => {
                log::warn!("Shape.read_rectangle_scope: number expected at '{}'", lx.scope_identifier);
                return None;
            }
        }
    }
    if lx.next_token() != Tok::Close {
        log::warn!("Shape.read_rectangle_scope ) expected at '{}'", lx.scope_identifier);
        return None;
    }
    Some(DsnShape::rect(layer?, coor))
}

pub fn read_polygon_scope(lx: &mut Lexer, ls: Option<&DsnLayerStructure>) -> Option<DsnShape> {
    let mut layer = None;
    let mut layer_ok = true;
    let t = lx.next_token();
    match &t {
        Tok::Kw(Kw::PcbScope) => layer = Some(LayerRef::Pcb),
        Tok::Kw(Kw::Signal) => layer = Some(LayerRef::Signal),
        _ => {
            let Some(ls) = ls else {
                log::warn!(
                    "Shape.read_polygon_scope: only layer types pcb or signal expected at '{}'",
                    lx.scope_identifier
                );
                return None;
            };
            let Tok::Str(name) = &t else {
                log::warn!("Shape.read_polygon_scope: layer name string expected at '{}'", lx.scope_identifier);
                return None;
            };
            let idx = ls.get_no(name);
            if idx < 0 || idx as usize >= ls.layers.len() {
                log::warn!(
                    "Shape.read_polygon_scope: layer name '{}' not found in layer structure  at '{}'",
                    name,
                    lx.scope_identifier
                );
                layer_ok = false;
            } else {
                layer = Some(LayerRef::Layer(ls.layers[idx as usize].clone()));
            }
        }
    }
    // overread the aperture width
    lx.next_token();
    let mut list = Vec::new();
    loop {
        let mut t = lx.next_token();
        if t == Tok::Eof {
            log::warn!("Shape.read_polygon_scope: unexpected end of file at '{}'", lx.scope_identifier);
            return None;
        }
        if t == Tok::Open {
            lx.skip_scope();
            t = lx.next_token();
        }
        if t == Tok::Close {
            break;
        }
        list.push(t);
    }
    if !layer_ok {
        return None;
    }
    let Some(coor) = numbers(&list) else {
        log::warn!("Shape.read_polygon_scope: number expected at '{}'", lx.scope_identifier);
        return None;
    };
    Some(DsnShape {
        layer: layer?,
        kind: DsnShapeKind::Polygon(coor),
    })
}

pub fn read_circle_scope(lx: &mut Lexer, ls: Option<&DsnLayerStructure>) -> Option<DsnShape> {
    let layer_name = lx.next_string();
    let layer = get_layer(ls, &layer_name);
    if layer.is_none() {
        log::warn!(
            "Circle.read_circle_scope: layer with name '{}' not found in layer structure at '{}'",
            layer_name,
            lx.scope_identifier
        );
    }
    let mut coor = [0.0; 3];
    let mut idx = 0;
    loop {
        let t = lx.next_token();
        if t == Tok::Close {
            break;
        }
        if idx > 2 {
            log::warn!("Shape.read_circle_scope: closed bracket expected at '{}'", lx.scope_identifier);
            return None;
        }
        match t.as_number() {
            Some(v) => coor[idx] = v,
            None => {
                log::warn!("Shape.read_circle_scope: number expected at '{}'", lx.scope_identifier);
                return None;
            }
        }
        idx += 1;
    }
    Some(DsnShape {
        layer: layer?,
        kind: DsnShapeKind::Circle(coor),
    })
}

pub fn read_polygon_path_scope(lx: &mut Lexer, ls: Option<&DsnLayerStructure>) -> Option<DsnShape> {
    let layer_name = lx.next_string();
    let layer = get_layer(ls, &layer_name);
    let mut list = Vec::new();
    loop {
        let mut t = lx.next_token();
        if t == Tok::Open {
            lx.skip_scope();
            t = lx.next_token();
        }
        if t == Tok::Close {
            break;
        }
        if t == Tok::Eof {
            // endless loop in the original
            return None;
        }
        list.push(t);
    }
    if list.len() < 5 {
        return None;
    }
    let layer = layer?;
    let Some(width) = list[0].as_number() else {
        log::warn!("Shape.read_polygon_path_scope: number expected at '{}'", lx.scope_identifier);
        return None;
    };
    let Some(coords) = numbers(&list[1..]) else {
        log::warn!("Shape.read_polygon_path_scope: number expected at '{}'", lx.scope_identifier);
        return None;
    };
    Some(DsnShape {
        layer,
        kind: DsnShapeKind::PolygonPath { width, coords },
    })
}

/// Result of `Shape.readAreaScope`. The first shape is the border, the others are holes.
#[derive(Clone, Debug)]
pub struct ReadAreaScopeResult {
    pub area_name: Option<String>,
    pub shape_list: Vec<Option<DsnShape>>,
    pub clearance_class_name: Option<String>,
}

impl ReadAreaScopeResult {
    /// The layer of the border shape.
    pub fn layer(&self) -> LayerRef {
        self.shape_list
            .first()
            .and_then(|s| s.as_ref())
            .map(|s| s.layer.clone())
            .unwrap_or(LayerRef::Missing)
    }
}

pub fn read_area_scope(
    lx: &mut Lexer,
    ls: Option<&DsnLayerStructure>,
    skip_window_scopes: bool,
) -> Option<ReadAreaScopeResult> {
    let mut shape_list = Vec::new();
    let mut clearance_class_name = None;
    let mut area_name = None;
    let mut result_ok = true;
    let t = lx.next_token();
    if let Tok::Str(name) = &t {
        lx.scope_identifier = name.clone();
        if !name.is_empty() {
            area_name = Some(name.clone());
        }
    }
    let current = read_shape_scope(lx, ls);
    if current.is_none() {
        log::warn!("Shape.read_area_scope: could not read shape at '{}'", lx.scope_identifier);
        result_ok = false;
    }
    shape_list.push(current);
    let mut next = Tok::Eof;
    loop {
        let prev = std::mem::replace(&mut next, lx.next_token());
        if next == Tok::Eof {
            log::warn!("Shape.read_area_scope: unexpected end of file at '{}'", lx.scope_identifier);
            return None;
        }
        if next == Tok::Close {
            break;
        }
        if prev == Tok::Open {
            if next.is_kw(Kw::Window) && !skip_window_scopes {
                let hole = read_shape_scope(lx, ls);
                shape_list.push(hole);
                next = lx.next_token();
                if next != Tok::Close {
                    log::warn!("Shape.read_area_scope: closed bracket expected at '{}'", lx.scope_identifier);
                    return None;
                }
            } else if next.is_kw(Kw::ClearanceClass) {
                clearance_class_name = Some(read_string_scope(lx));
            } else {
                lx.skip_scope();
            }
        }
    }
    if !result_ok {
        return None;
    }
    Some(ReadAreaScopeResult {
        area_name,
        shape_list,
        clearance_class_name,
    })
}

fn transform_area_impl(area: &[Option<DsnShape>], rel: bool, t: &CoordinateTransform) -> Option<Area> {
    if area.is_empty() {
        log::warn!("Shape.transform_area_to_board: area.size() > 0 expected");
        return None;
    }
    let tf = |s: &DsnShape| if rel { s.transform_to_board_rel(t) } else { s.transform_to_board(t) };
    let boundary = tf(area[0].as_ref()?);
    if area.len() == 1 {
        return boundary.map(Area::Shape);
    }
    let border = match boundary {
        Some(s @ (Shape::Tile(_) | Shape::Polygon(_))) => s,
        _ => {
            log::warn!("Shape.transform_area_to_board: PolylineShape expected");
            return None;
        }
    };
    let mut holes = Vec::new();
    for h in &area[1..] {
        match h.as_ref().and_then(tf) {
            Some(s @ (Shape::Tile(_) | Shape::Polygon(_))) => holes.push(s),
            _ => {
                log::warn!("Shape.transform_area_to_board: PolylineShape expected");
                return None;
            }
        }
    }
    Some(Area::PolylineArea(PolylineArea::new(border, holes)))
}

/// `Shape.transformAreaToBoard`.
pub fn transform_area_to_board(area: &[Option<DsnShape>], t: &CoordinateTransform) -> Option<Area> {
    transform_area_impl(area, false, t)
}

/// `Shape.transformAreaToBoardRel`.
pub fn transform_area_to_board_rel(area: &[Option<DsnShape>], t: &CoordinateTransform) -> Option<Area> {
    transform_area_impl(area, true, t)
}
