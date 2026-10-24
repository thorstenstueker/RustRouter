//! General shapes (convex tiles, circles, polygons) and areas with holes.

use super::tile_shape::{BoundingDirections, RegularTileShape};
use super::{
    Circle, FloatPoint, IntBox, IntOctagon, IntPoint, LineSegment, Point, PolygonShape, Polyline,
    TileShape, Vector,
};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug)]
pub enum Shape {
    Tile(TileShape),
    Circle(Circle),
    Polygon(PolygonShape),
}

impl From<TileShape> for Shape {
    fn from(t: TileShape) -> Shape {
        Shape::Tile(t)
    }
}

impl From<IntBox> for Shape {
    fn from(b: IntBox) -> Shape {
        Shape::Tile(TileShape::Box(b))
    }
}

impl From<Circle> for Shape {
    fn from(c: Circle) -> Shape {
        Shape::Circle(c)
    }
}

impl Shape {
    pub fn as_tile(&self) -> Option<&TileShape> {
        match self {
            Shape::Tile(t) => Some(t),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Shape::Tile(t) => t.is_empty(),
            Shape::Circle(_) => false,
            Shape::Polygon(p) => p.is_empty(),
        }
    }

    pub fn is_bounded(&self) -> bool {
        match self {
            Shape::Tile(t) => t.is_bounded(),
            _ => true,
        }
    }

    pub fn dimension(&self) -> i32 {
        match self {
            Shape::Tile(t) => t.dimension(),
            Shape::Circle(c) => c.dimension(),
            Shape::Polygon(p) => p.dimension(),
        }
    }

    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        match self {
            Shape::Tile(t) => t.is_contained_in(b),
            Shape::Circle(c) => c.is_contained_in(b),
            Shape::Polygon(p) => b.contains_box(&p.bounding_box()),
        }
    }

    pub fn bounding_box(&self) -> IntBox {
        match self {
            Shape::Tile(t) => t.bounding_box(),
            Shape::Circle(c) => c.bounding_box(),
            Shape::Polygon(p) => p.bounding_box(),
        }
    }

    pub fn bounding_octagon(&self) -> IntOctagon {
        match self {
            Shape::Tile(t) => t.bounding_octagon(),
            Shape::Circle(c) => c.bounding_octagon(),
            Shape::Polygon(p) => p.bounding_octagon(),
        }
    }

    pub fn bounding_tile(&self) -> TileShape {
        match self {
            Shape::Tile(t) => t.clone(),
            Shape::Circle(c) => c.bounding_tile(),
            Shape::Polygon(p) => p.bounding_tile(),
        }
    }

    pub fn bounding_shape(&self, dirs: BoundingDirections) -> RegularTileShape {
        match dirs {
            BoundingDirections::Orthogonal => RegularTileShape::Box(self.bounding_box()),
            BoundingDirections::FortyfiveDegree => RegularTileShape::Octagon(self.bounding_octagon()),
        }
    }

    pub fn contains_float(&self, p: &FloatPoint) -> bool {
        match self {
            Shape::Tile(t) => t.contains_float(p),
            Shape::Circle(c) => c.contains_float(p),
            Shape::Polygon(s) => s.contains_float(p),
        }
    }

    pub fn contains(&self, p: &Point) -> bool {
        match self {
            Shape::Tile(t) => t.contains(p),
            Shape::Circle(c) => c.contains(p),
            Shape::Polygon(s) => s.contains(p),
        }
    }

    pub fn contains_inside(&self, p: &Point) -> bool {
        match self {
            Shape::Tile(t) => t.contains_inside(p),
            Shape::Circle(c) => c.contains_inside(p),
            Shape::Polygon(s) => s.contains_inside(p),
        }
    }

    pub fn contains_on_border(&self, p: &Point) -> bool {
        match self {
            Shape::Tile(t) => t.contains_on_border(p),
            Shape::Circle(c) => c.contains_on_border(p),
            Shape::Polygon(_) => false,
        }
    }

    pub fn is_outside(&self, p: &Point) -> bool {
        match self {
            Shape::Tile(t) => t.is_outside(p),
            Shape::Circle(c) => c.is_outside(p),
            Shape::Polygon(s) => s.is_outside(p),
        }
    }

    pub fn nearest_point_approx(&self, p: &FloatPoint) -> Option<FloatPoint> {
        match self {
            Shape::Tile(t) => t.nearest_point_approx(p),
            Shape::Circle(_) => None,
            Shape::Polygon(s) => s.nearest_point_approx(p),
        }
    }

    pub fn distance(&self, p: &FloatPoint) -> f64 {
        match self {
            Shape::Tile(t) => t.distance(p),
            Shape::Circle(c) => c.distance(p),
            Shape::Polygon(_) => 0.0,
        }
    }

    pub fn border_distance(&self, p: &FloatPoint) -> f64 {
        match self {
            Shape::Tile(t) => t.border_distance(p),
            Shape::Circle(c) => c.border_distance(p),
            Shape::Polygon(s) => s.border_distance(p),
        }
    }

    pub fn smallest_radius(&self) -> f64 {
        match self {
            Shape::Tile(t) => t.smallest_radius(),
            Shape::Circle(c) => c.smallest_radius(),
            Shape::Polygon(s) => s.smallest_radius(),
        }
    }

    pub fn circumference(&self) -> f64 {
        match self {
            Shape::Tile(t) => t.circumference(),
            Shape::Circle(c) => c.circumference(),
            Shape::Polygon(s) => s.circumference(),
        }
    }

    pub fn area(&self) -> f64 {
        match self {
            Shape::Tile(t) => t.area(),
            Shape::Circle(c) => c.area(),
            Shape::Polygon(s) => s.area(),
        }
    }

    pub fn centre_of_gravity(&self) -> FloatPoint {
        match self {
            Shape::Tile(t) => t.centre_of_gravity(),
            Shape::Circle(c) => c.centre_of_gravity(),
            Shape::Polygon(s) => s.centre_of_gravity(),
        }
    }

    pub fn max_width(&self) -> f64 {
        match self {
            Shape::Tile(t) => t.max_width(),
            Shape::Circle(c) => c.max_width(),
            Shape::Polygon(p) => p.bounding_box().max_width(),
        }
    }

    pub fn min_width(&self) -> f64 {
        match self {
            Shape::Tile(t) => t.min_width(),
            Shape::Circle(c) => c.min_width(),
            Shape::Polygon(p) => p.bounding_box().min_width(),
        }
    }

    /// Enlarges the shape. Polygon shapes can only be enlarged by 0.
    pub fn enlarge(&self, offset: f64) -> Shape {
        match self {
            Shape::Tile(t) => Shape::Tile(t.enlarge(offset)),
            Shape::Circle(c) => Shape::Circle(c.enlarge(offset)),
            Shape::Polygon(p) => {
                if offset != 0.0 {
                    log::warn!("PolygonShape.enlarge not yet implemented");
                }
                Shape::Polygon(p.clone())
            }
        }
    }

    /// Offset of a convex shape (tile or circle).
    pub fn offset(&self, distance: f64) -> Shape {
        match self {
            Shape::Tile(t) => Shape::Tile(t.offset(distance)),
            Shape::Circle(c) => Shape::Circle(c.offset(distance)),
            Shape::Polygon(_) => self.clone(),
        }
    }

    pub fn shrink(&self, offset: f64) -> Shape {
        match self {
            Shape::Tile(t) => Shape::Tile(t.shrink(offset)),
            Shape::Circle(c) => Shape::Circle(c.shrink(offset)),
            Shape::Polygon(_) => self.clone(),
        }
    }

    pub fn intersects(&self, other: &Shape) -> bool {
        match (self, other) {
            (Shape::Tile(a), Shape::Tile(b)) => a.intersects(b),
            (Shape::Tile(t), Shape::Circle(c)) | (Shape::Circle(c), Shape::Tile(t)) => c.intersects_tile(t),
            (Shape::Circle(a), Shape::Circle(b)) => a.intersects_circle(b),
            (Shape::Polygon(p), Shape::Tile(t)) | (Shape::Tile(t), Shape::Polygon(p)) => p.intersects_tile(t),
            (Shape::Polygon(p), Shape::Circle(c)) | (Shape::Circle(c), Shape::Polygon(p)) => p.intersects_circle(c),
            (Shape::Polygon(a), Shape::Polygon(b)) => a.intersects_polygon(b),
        }
    }

    pub fn intersects_tile(&self, t: &TileShape) -> bool {
        match self {
            Shape::Tile(a) => a.intersects(t),
            Shape::Circle(c) => c.intersects_tile(t),
            Shape::Polygon(p) => p.intersects_tile(t),
        }
    }

    pub fn cutout(&self, polyline: &Polyline) -> Vec<Polyline> {
        match self {
            Shape::Tile(t) => t.cutout_polyline(polyline),
            _ => {
                log::warn!("cutout not yet implemented for circles and polygons");
                Vec::new()
            }
        }
    }

    pub fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        match self {
            Shape::Tile(t) => t.corner_approx_arr(),
            Shape::Circle(_) => Vec::new(),
            Shape::Polygon(p) => p.corner_approx_arr(),
        }
    }

    pub fn split_to_convex(&self) -> Vec<TileShape> {
        match self {
            Shape::Tile(t) => vec![t.clone()],
            Shape::Circle(c) => c.split_to_convex(),
            Shape::Polygon(p) => p.split_to_convex().to_vec(),
        }
    }

    pub fn translate_by(&self, v: &Vector) -> Shape {
        match self {
            Shape::Tile(t) => Shape::Tile(t.translate_by(v)),
            Shape::Circle(c) => Shape::Circle(c.translate_by(v)),
            Shape::Polygon(p) => Shape::Polygon(p.translate_by(v)),
        }
    }

    pub fn turn_90_degree(&self, factor: i32, pole: &IntPoint) -> Shape {
        match self {
            Shape::Tile(t) => Shape::Tile(t.turn_90_degree(factor, pole)),
            Shape::Circle(c) => Shape::Circle(c.turn_90_degree(factor, pole)),
            Shape::Polygon(p) => Shape::Polygon(p.turn_90_degree(factor, pole)),
        }
    }

    pub fn rotate_approx(&self, angle: f64, pole: &FloatPoint) -> Shape {
        match self {
            Shape::Tile(t) => Shape::Tile(t.rotate_approx(angle, pole)),
            Shape::Circle(c) => Shape::Circle(c.rotate_approx(angle, pole)),
            Shape::Polygon(p) => Shape::Polygon(p.rotate_approx(angle, pole)),
        }
    }

    pub fn mirror_vertical(&self, pole: &IntPoint) -> Shape {
        match self {
            Shape::Tile(t) => Shape::Tile(t.mirror_vertical(pole)),
            Shape::Circle(c) => Shape::Circle(c.mirror_vertical(pole)),
            Shape::Polygon(p) => Shape::Polygon(p.mirror_vertical(pole)),
        }
    }

    pub fn mirror_horizontal(&self, pole: &IntPoint) -> Shape {
        match self {
            Shape::Tile(t) => Shape::Tile(t.mirror_horizontal(pole)),
            Shape::Circle(c) => Shape::Circle(c.mirror_horizontal(pole)),
            Shape::Polygon(p) => Shape::Polygon(p.mirror_horizontal(pole)),
        }
    }

    /// Number of corners for polyline shapes, 0 for circles.
    pub fn border_line_count(&self) -> usize {
        match self {
            Shape::Tile(t) => t.border_line_count(),
            Shape::Circle(_) => 0,
            Shape::Polygon(p) => p.border_line_count(),
        }
    }

    pub fn corner(&self, i: usize) -> Point {
        match self {
            Shape::Tile(t) => t.corner(i),
            Shape::Circle(c) => Point::Int(c.center),
            Shape::Polygon(p) => p.corner(i),
        }
    }

    pub fn corner_approx(&self, i: usize) -> FloatPoint {
        match self {
            Shape::Tile(t) => t.corner_approx(i),
            Shape::Circle(c) => c.center.to_float(),
            Shape::Polygon(p) => p.corner_approx(i),
        }
    }
}

#[derive(Debug)]
struct PolylineAreaInner {
    border: Shape,
    holes: Vec<Shape>,
    convex_pieces: OnceLock<Option<Vec<TileShape>>>,
}

/// An area with a polyline border and polyline holes.
#[derive(Clone, Debug)]
pub struct PolylineArea(Arc<PolylineAreaInner>);

impl PolylineArea {
    pub fn new(border: Shape, holes: Vec<Shape>) -> PolylineArea {
        PolylineArea(Arc::new(PolylineAreaInner {
            border,
            holes,
            convex_pieces: OnceLock::new(),
        }))
    }

    pub fn border(&self) -> &Shape {
        &self.0.border
    }

    pub fn holes(&self) -> &[Shape] {
        &self.0.holes
    }

    pub fn contains_float(&self, p: &FloatPoint) -> bool {
        if !self.0.border.contains_float(p) {
            return false;
        }
        !self.0.holes.iter().any(|h| h.contains_float(p))
    }

    pub fn contains(&self, p: &Point) -> bool {
        if !self.0.border.contains(p) {
            return false;
        }
        !self.0.holes.iter().any(|h| h.contains_inside(p))
    }

    pub fn split_to_convex(&self) -> Option<&[TileShape]> {
        self.0
            .convex_pieces
            .get_or_init(|| {
                let border_pieces = match &self.0.border {
                    Shape::Polygon(p) => p.try_split_to_convex()?.to_vec(),
                    other => other.split_to_convex(),
                };
                let mut pieces = border_pieces;
                for hole in &self.0.holes {
                    if hole.dimension() < 2 {
                        log::warn!("PolylineArea. split_to_convex: dimension 2 for hole expected");
                        continue;
                    }
                    let hole_pieces = match hole {
                        Shape::Polygon(p) => p.try_split_to_convex()?.to_vec(),
                        other => other.split_to_convex(),
                    };
                    for hp in &hole_pieces {
                        let mut new_pieces = Vec::new();
                        for dp in &pieces {
                            for r in dp.cutout(hp) {
                                if r.dimension() == 2 {
                                    new_pieces.push(r);
                                }
                            }
                        }
                        pieces = new_pieces;
                    }
                }
                Some(pieces)
            })
            .as_deref()
    }
}

#[derive(Clone, Debug)]
pub enum Area {
    Shape(Shape),
    PolylineArea(PolylineArea),
}

impl From<Shape> for Area {
    fn from(s: Shape) -> Area {
        Area::Shape(s)
    }
}

impl From<TileShape> for Area {
    fn from(t: TileShape) -> Area {
        Area::Shape(Shape::Tile(t))
    }
}

impl Area {
    pub fn get_border(&self) -> &Shape {
        match self {
            Area::Shape(s) => s,
            Area::PolylineArea(a) => a.border(),
        }
    }

    pub fn get_holes(&self) -> &[Shape] {
        match self {
            Area::Shape(_) => &[],
            Area::PolylineArea(a) => a.holes(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.get_border().is_empty()
    }

    pub fn is_bounded(&self) -> bool {
        self.get_border().is_bounded()
    }

    pub fn dimension(&self) -> i32 {
        self.get_border().dimension()
    }

    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        self.get_border().is_contained_in(b)
    }

    pub fn bounding_box(&self) -> IntBox {
        self.get_border().bounding_box()
    }

    pub fn bounding_octagon(&self) -> IntOctagon {
        self.get_border().bounding_octagon()
    }

    pub fn contains_float(&self, p: &FloatPoint) -> bool {
        match self {
            Area::Shape(s) => s.contains_float(p),
            Area::PolylineArea(a) => a.contains_float(p),
        }
    }

    pub fn contains(&self, p: &Point) -> bool {
        match self {
            Area::Shape(s) => s.contains(p),
            Area::PolylineArea(a) => a.contains(p),
        }
    }

    pub fn nearest_point_approx(&self, from: &FloatPoint) -> Option<FloatPoint> {
        match self {
            Area::Shape(s) => s.nearest_point_approx(from),
            Area::PolylineArea(_) => {
                let mut min_dist = f64::MAX;
                let mut result = None;
                for t in self.split_to_convex() {
                    if let Some(n) = t.nearest_point_approx(from) {
                        let d = n.distance_square(from);
                        if d < min_dist {
                            min_dist = d;
                            result = Some(n);
                        }
                    }
                }
                result
            }
        }
    }

    pub fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        let mut r = self.get_border().corner_approx_arr();
        for h in self.get_holes() {
            r.extend(h.corner_approx_arr());
        }
        r
    }

    /// The convex pieces of the area. Empty, if the division failed.
    pub fn split_to_convex(&self) -> Vec<TileShape> {
        match self {
            Area::Shape(s) => s.split_to_convex(),
            Area::PolylineArea(a) => a.split_to_convex().map(|p| p.to_vec()).unwrap_or_default(),
        }
    }

    fn map(&self, f: impl Fn(&Shape) -> Shape) -> Area {
        match self {
            Area::Shape(s) => Area::Shape(f(s)),
            Area::PolylineArea(a) => {
                Area::PolylineArea(PolylineArea::new(f(a.border()), a.holes().iter().map(f).collect()))
            }
        }
    }

    pub fn translate_by(&self, v: &Vector) -> Area {
        if v.is_zero() {
            return self.clone();
        }
        self.map(|s| s.translate_by(v))
    }

    pub fn turn_90_degree(&self, factor: i32, pole: &IntPoint) -> Area {
        self.map(|s| s.turn_90_degree(factor, pole))
    }

    pub fn rotate_approx(&self, angle: f64, pole: &FloatPoint) -> Area {
        self.map(|s| s.rotate_approx(angle, pole))
    }

    pub fn mirror_vertical(&self, pole: &IntPoint) -> Area {
        self.map(|s| s.mirror_vertical(pole))
    }

    pub fn mirror_horizontal(&self, pole: &IntPoint) -> Area {
        self.map(|s| s.mirror_horizontal(pole))
    }
}

/// Transformation of library shapes into board coordinates.
#[derive(Clone, Copy, Debug)]
pub struct PlacementTransform {
    location: FloatPoint,
    rotation: f64,
    mirrored: bool,
    rotate_first: bool,
}

impl PlacementTransform {
    pub fn new(location: FloatPoint, rotation: f64, mirrored: bool, rotate_first: bool) -> Self {
        PlacementTransform {
            location,
            rotation,
            mirrored,
            rotate_first,
        }
    }

    pub fn direction(&self, p: &FloatPoint) -> FloatPoint {
        let mut r = *p;
        if self.mirrored && !self.rotate_first {
            r = FloatPoint::new(-r.x, r.y);
        }
        r = Self::rotate(&r, self.rotation);
        if self.mirrored && self.rotate_first {
            r = FloatPoint::new(-r.x, r.y);
        }
        r
    }

    pub fn point(&self, p: &FloatPoint) -> FloatPoint {
        let o = self.direction(p);
        FloatPoint::new(self.location.x + o.x, self.location.y + o.y)
    }

    pub fn rotate(p: &FloatPoint, degrees: f64) -> FloatPoint {
        if degrees % 90.0 == 0.0 {
            p.turn_90_degree_around(((degrees / 90.0) as i32).rem_euclid(4), &FloatPoint::ZERO)
        } else {
            p.rotate(degrees.to_radians(), &FloatPoint::ZERO)
        }
    }

    fn pad_point(&self, p: &FloatPoint, offset: &FloatPoint, pin_rotation: f64) -> FloatPoint {
        let r = Self::rotate(p, pin_rotation);
        self.point(&FloatPoint::new(offset.x + r.x, offset.y + r.y))
    }

    pub fn shape(&self, shape: &Shape, offset: &FloatPoint, pin_rotation: f64) -> Shape {
        if shape.is_empty() {
            return shape.clone();
        }
        if let Shape::Circle(c) = shape {
            return Shape::Circle(Circle::new(
                self.pad_point(&c.center.to_float(), offset, pin_rotation).round(),
                c.radius,
            ));
        }
        let corners = shape.corner_approx_arr();
        let n = corners.len();
        let mut placed: Vec<Point> = vec![Point::ZERO; n];
        for (i, c) in corners.iter().enumerate() {
            let idx = if self.mirrored { n - 1 - i } else { i };
            placed[idx] = Point::Int(self.pad_point(c, offset, pin_rotation).round());
        }
        if let Shape::Tile(_) = shape {
            let mut unique: Vec<Point> = Vec::new();
            for p in placed {
                if !unique.contains(&p) {
                    unique.push(p);
                }
            }
            let normalized: Vec<Point> = if unique.len() < 3 {
                unique
            } else {
                PolygonShape::new(&unique).corners().to_vec()
            };
            return Shape::Tile(match normalized.len() {
                0 => TileShape::Box(IntBox::EMPTY),
                1 => TileShape::from_point(&normalized[0]),
                2 => TileShape::Simplex(
                    LineSegment::from_polyline(&Polyline::from_points(&normalized), 1).to_simplex(),
                ),
                _ => TileShape::from_convex_polygon(&normalized),
            });
        }
        Shape::Polygon(PolygonShape::new(&placed))
    }

    pub fn area(&self, area: &Area) -> Area {
        let border = self.shape(area.get_border(), &FloatPoint::ZERO, 0.0);
        let holes = area.get_holes();
        if holes.is_empty() {
            return Area::Shape(border);
        }
        let placed: Vec<Shape> = holes.iter().map(|h| self.shape(h, &FloatPoint::ZERO, 0.0)).collect();
        Area::PolylineArea(PolylineArea::new(border, placed))
    }
}
