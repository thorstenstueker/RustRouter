//! Convex tile shapes: boxes, octagons and general simplexes.

use super::basics::Side;
use super::{
    Circle, Direction, FloatLine, FloatPoint, IntBox, IntOctagon, IntPoint, Line, LineSegment,
    Point, Polygon, Polyline, Simplex, Vector,
};

#[derive(Clone, Debug)]
pub enum TileShape {
    Box(IntBox),
    Octagon(IntOctagon),
    Simplex(Simplex),
}

/// The bounding directions used for search trees and room calculations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoundingDirections {
    /// 4 directions: bounding boxes.
    Orthogonal,
    /// 8 directions: bounding octagons.
    FortyfiveDegree,
}

/// A box or an octagon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RegularTileShape {
    Box(IntBox),
    Octagon(IntOctagon),
}

impl RegularTileShape {
    pub fn to_tile(&self) -> TileShape {
        match self {
            RegularTileShape::Box(b) => TileShape::Box(*b),
            RegularTileShape::Octagon(o) => TileShape::Octagon(*o),
        }
    }

    pub fn to_octagon(&self) -> IntOctagon {
        match self {
            RegularTileShape::Box(b) => b.to_int_octagon(),
            RegularTileShape::Octagon(o) => *o,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            RegularTileShape::Box(b) => b.is_empty(),
            RegularTileShape::Octagon(o) => o.is_empty(),
        }
    }

    pub fn union(&self, other: &RegularTileShape) -> RegularTileShape {
        match (self, other) {
            (RegularTileShape::Box(a), RegularTileShape::Box(b)) => RegularTileShape::Box(a.union(b)),
            _ => RegularTileShape::Octagon(self.to_octagon().union(&other.to_octagon())),
        }
    }

    /// Returns true, if `other` is contained in self.
    pub fn contains(&self, other: &RegularTileShape) -> bool {
        match (self, other) {
            (RegularTileShape::Box(a), RegularTileShape::Box(b)) => b.is_contained_in(a),
            (RegularTileShape::Box(a), RegularTileShape::Octagon(o)) => o.is_contained_in_box(a),
            (RegularTileShape::Octagon(a), RegularTileShape::Box(b)) => {
                a.contains_octagon(&b.to_int_octagon())
            }
            (RegularTileShape::Octagon(a), RegularTileShape::Octagon(b)) => b.is_contained_in(a),
        }
    }

    /// Compares the edge `edge_index` of self with that of `other`
    /// (Java: `this.compare(other, edgeIndex)`).
    pub fn compare(&self, other: &RegularTileShape, edge_index: usize) -> Side {
        match (self, other) {
            (RegularTileShape::Box(a), RegularTileShape::Box(b)) => a.compare(b, edge_index),
            _ => self.to_octagon().compare(&other.to_octagon(), edge_index),
        }
    }

    #[inline]
    pub fn intersects(&self, other: &RegularTileShape) -> bool {
        match (self, other) {
            (RegularTileShape::Box(a), RegularTileShape::Box(b)) => a.intersects(b),
            (RegularTileShape::Octagon(a), RegularTileShape::Octagon(b)) => a.intersects(b),
            _ => self.to_octagon().intersects(&other.to_octagon()),
        }
    }

    pub fn border_line_count(&self) -> usize {
        match self {
            RegularTileShape::Box(_) => 4,
            RegularTileShape::Octagon(_) => 8,
        }
    }

    #[inline]
    pub fn area(&self) -> f64 {
        match self {
            RegularTileShape::Box(b) => b.area(),
            RegularTileShape::Octagon(o) => o.area(),
        }
    }

    pub fn offset(&self, dist: f64) -> RegularTileShape {
        match self {
            RegularTileShape::Box(b) => RegularTileShape::Box(b.offset(dist)),
            RegularTileShape::Octagon(o) => RegularTileShape::Octagon(o.offset(dist)),
        }
    }

    pub fn bounding_box(&self) -> IntBox {
        match self {
            RegularTileShape::Box(b) => *b,
            RegularTileShape::Octagon(o) => o.bounding_box(),
        }
    }
}

impl From<IntBox> for TileShape {
    fn from(b: IntBox) -> TileShape {
        TileShape::Box(b)
    }
}

impl From<IntOctagon> for TileShape {
    fn from(o: IntOctagon) -> TileShape {
        TileShape::Octagon(o)
    }
}

impl From<Simplex> for TileShape {
    fn from(s: Simplex) -> TileShape {
        TileShape::Simplex(s)
    }
}

impl TileShape {
    pub fn empty() -> TileShape {
        TileShape::Simplex(Simplex::empty())
    }

    /// Creates a normalized and simplified tile from the half planes right of the lines.
    pub fn from_lines(lines: &[Line]) -> TileShape {
        TileShape::Simplex(Simplex::get_instance(lines)).simplify()
    }

    /// Creates a tile from the corners of a convex polygon.
    pub fn from_convex_polygon(points: &[Point]) -> TileShape {
        let n = points.len();
        if n == 0 {
            return TileShape::empty();
        }
        let mut lines = Vec::with_capacity(n);
        for j in 0..n - 1 {
            lines.push(Line::from_points(&points[j], &points[j + 1]));
        }
        lines.push(Line::from_points(&points[n - 1], &points[0]));
        TileShape::from_lines(&lines)
    }

    pub fn from_line(line: &Line) -> TileShape {
        TileShape::Simplex(Simplex::get_instance(&[*line]))
    }

    pub fn from_point(p: &Point) -> TileShape {
        TileShape::Box(p.surrounding_box())
    }

    pub fn is_empty(&self) -> bool {
        match self {
            TileShape::Box(b) => b.is_empty(),
            TileShape::Octagon(o) => o.is_empty(),
            TileShape::Simplex(s) => s.is_empty(),
        }
    }

    pub fn simplify(self) -> TileShape {
        match self {
            TileShape::Box(_) => self,
            TileShape::Octagon(o) => {
                if o.is_int_box() {
                    TileShape::Box(o.bounding_box())
                } else {
                    self
                }
            }
            TileShape::Simplex(ref s) => {
                if s.is_empty() {
                    TileShape::empty()
                } else if s.is_int_box() {
                    TileShape::Box(s.bounding_box())
                } else if s.is_int_octagon() {
                    TileShape::Octagon(s.to_int_octagon().unwrap())
                } else {
                    self
                }
            }
        }
    }

    pub fn get_id(&self) -> i32 {
        match self {
            TileShape::Box(b) => b.get_id(),
            TileShape::Octagon(o) => o.get_id(),
            TileShape::Simplex(s) => s.get_id(),
        }
    }

    pub fn is_int_box(&self) -> bool {
        match self {
            TileShape::Box(_) => true,
            TileShape::Octagon(o) => o.is_int_box(),
            TileShape::Simplex(s) => s.is_int_box(),
        }
    }

    pub fn is_int_octagon(&self) -> bool {
        match self {
            TileShape::Box(_) | TileShape::Octagon(_) => true,
            TileShape::Simplex(s) => s.is_int_octagon(),
        }
    }

    pub fn as_box(&self) -> Option<&IntBox> {
        match self {
            TileShape::Box(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_octagon(&self) -> Option<&IntOctagon> {
        match self {
            TileShape::Octagon(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_regular(&self) -> Option<RegularTileShape> {
        match self {
            TileShape::Box(b) => Some(RegularTileShape::Box(*b)),
            TileShape::Octagon(o) => Some(RegularTileShape::Octagon(*o)),
            _ => None,
        }
    }

    pub fn to_simplex(&self) -> Simplex {
        match self {
            TileShape::Box(b) => b.to_simplex(),
            TileShape::Octagon(o) => o.to_simplex(),
            TileShape::Simplex(s) => s.clone(),
        }
    }

    #[inline]
    pub fn border_line_count(&self) -> usize {
        match self {
            TileShape::Box(_) => 4,
            TileShape::Octagon(_) => 8,
            TileShape::Simplex(s) => s.border_line_count(),
        }
    }

    #[inline]
    pub fn border_line(&self, no: usize) -> Line {
        match self {
            TileShape::Box(b) => b.border_line(no),
            TileShape::Octagon(o) => o.border_line(no),
            TileShape::Simplex(s) => s.border_line(no),
        }
    }

    pub fn border_line_index(&self, line: &Line) -> i32 {
        match self {
            TileShape::Simplex(s) => s.border_line_index(line),
            _ => {
                log::warn!("borderLineIndex not yet implemented for boxes and octagons");
                -1
            }
        }
    }

    #[inline]
    pub fn corner(&self, no: usize) -> Point {
        match self {
            TileShape::Box(b) => Point::Int(b.corner(no)),
            TileShape::Octagon(o) => Point::Int(o.corner(no)),
            TileShape::Simplex(s) => s.corner(no),
        }
    }

    #[inline]
    pub fn corner_approx(&self, no: usize) -> FloatPoint {
        match self {
            TileShape::Box(b) => b.corner(no).to_float(),
            TileShape::Octagon(o) => o.corner(no).to_float(),
            TileShape::Simplex(s) => s.corner_approx(no),
        }
    }

    pub fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        match self {
            TileShape::Simplex(s) => s.corner_approx_arr(),
            _ => (0..self.border_line_count()).map(|i| self.corner_approx(i)).collect(),
        }
    }

    pub fn corner_is_bounded(&self, no: usize) -> bool {
        match self {
            TileShape::Simplex(s) => s.corner_is_bounded(no),
            _ => true,
        }
    }

    pub fn bounded_corners(&self) -> Vec<Point> {
        (0..self.border_line_count())
            .filter(|i| self.corner_is_bounded(*i))
            .map(|i| self.corner(i))
            .collect()
    }

    pub fn is_bounded(&self) -> bool {
        match self {
            TileShape::Simplex(s) => s.is_bounded(),
            _ => true,
        }
    }

    pub fn dimension(&self) -> i32 {
        match self {
            TileShape::Box(b) => b.dimension(),
            TileShape::Octagon(o) => o.dimension(),
            TileShape::Simplex(s) => s.dimension(),
        }
    }

    pub fn bounding_box(&self) -> IntBox {
        match self {
            TileShape::Box(b) => *b,
            TileShape::Octagon(o) => o.bounding_box(),
            TileShape::Simplex(s) => s.bounding_box(),
        }
    }

    pub fn bounding_octagon(&self) -> IntOctagon {
        match self {
            TileShape::Box(b) => b.to_int_octagon(),
            TileShape::Octagon(o) => *o,
            TileShape::Simplex(s) => s.bounding_octagon().unwrap_or(IntOctagon::EMPTY),
        }
    }

    pub fn bounding_shape(&self, dirs: BoundingDirections) -> RegularTileShape {
        match dirs {
            BoundingDirections::Orthogonal => RegularTileShape::Box(self.bounding_box()),
            BoundingDirections::FortyfiveDegree => RegularTileShape::Octagon(self.bounding_octagon()),
        }
    }

    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        match self {
            TileShape::Box(s) => s.is_contained_in(b),
            TileShape::Octagon(o) => o.is_contained_in_box(b),
            TileShape::Simplex(s) => b.contains_box(&s.bounding_box()),
        }
    }

    pub fn max_width(&self) -> f64 {
        match self {
            TileShape::Box(b) => b.max_width(),
            TileShape::Octagon(o) => o.max_width(),
            TileShape::Simplex(s) => s.max_width(),
        }
    }

    pub fn min_width(&self) -> f64 {
        match self {
            TileShape::Box(b) => b.min_width(),
            TileShape::Octagon(o) => o.min_width(),
            TileShape::Simplex(s) => s.min_width(),
        }
    }

    pub fn area(&self) -> f64 {
        match self {
            TileShape::Box(b) => b.area(),
            TileShape::Octagon(o) => o.area(),
            TileShape::Simplex(_) => {
                if !self.is_bounded() {
                    return f64::MAX;
                }
                if self.dimension() < 2 {
                    return 0.0;
                }
                let n = self.border_line_count();
                let mut result = 0.0;
                let mut prev = self.corner_approx(n - 2);
                let mut cur = self.corner_approx(n - 1);
                for i in 0..n {
                    let next = self.corner_approx(i);
                    result += cur.x * (next.y - prev.y);
                    prev = cur;
                    cur = next;
                }
                0.5 * f64::abs(result)
            }
        }
    }

    pub fn circumference(&self) -> f64 {
        match self {
            TileShape::Box(b) => b.circumference(),
            _ => {
                if !self.is_bounded() {
                    return i32::MAX as f64;
                }
                let n = self.border_line_count();
                if n == 0 {
                    return 0.0;
                }
                let mut result = 0.0;
                let mut prev = self.corner_approx(n - 1);
                for i in 0..n {
                    let c = self.corner_approx(i);
                    result += c.distance(&prev);
                    prev = c;
                }
                result
            }
        }
    }

    pub fn centre_of_gravity(&self) -> FloatPoint {
        let n = self.border_line_count();
        let mut x = 0.0;
        let mut y = 0.0;
        for i in 0..n {
            let c = self.corner_approx(i);
            x += c.x;
            y += c.y;
        }
        FloatPoint::new(x / n as f64, y / n as f64)
    }

    pub fn equals_corner(&self, p: &Point) -> i32 {
        for i in 0..self.border_line_count() {
            if *p == self.corner(i) {
                return i as i32;
            }
        }
        -1
    }

    pub fn prev_no(&self, no: usize) -> usize {
        if no == 0 {
            self.border_line_count() - 1
        } else {
            no - 1
        }
    }

    pub fn next_no(&self, no: usize) -> usize {
        (no + 1) % self.border_line_count()
    }

    pub fn index_of_left_most_corner(&self, from: &FloatPoint) -> usize {
        let mut lmc = self.corner_approx(0);
        let mut result = 0;
        for i in 1..self.border_line_count() {
            let c = self.corner_approx(i);
            if c.side_of(from, &lmc) == Side::OnTheLeft {
                lmc = c;
                result = i;
            }
        }
        result
    }

    pub fn index_of_right_most_corner(&self, from: &FloatPoint) -> usize {
        let mut rmc = self.corner_approx(0);
        let mut result = 0;
        for i in 1..self.border_line_count() {
            let c = self.corner_approx(i);
            if c.side_of(from, &rmc) == Side::OnTheRight {
                rmc = c;
                result = i;
            }
        }
        result
    }

    pub fn polar_line_segment(&self, from: &FloatPoint) -> Option<FloatLine> {
        if self.is_empty() {
            return None;
        }
        let mut lmc = self.corner_approx(0);
        let mut rmc = lmc;
        for i in 1..self.border_line_count() {
            let c = self.corner_approx(i);
            if c.side_of(from, &rmc) == Side::OnTheRight {
                rmc = c;
            }
            if c.side_of(from, &lmc) == Side::OnTheLeft {
                lmc = c;
            }
        }
        Some(FloatLine::new(lmc, rmc))
    }

    pub fn intersects_line(&self, line: &Line) -> bool {
        let s0 = line.side_of(&self.corner(0));
        if s0 == Side::Collinear {
            return true;
        }
        for i in 1..self.border_line_count() {
            if line.side_of(&self.corner(i)) != s0 {
                return true;
            }
        }
        false
    }

    pub fn left_most_corner(&self, from: &Point) -> Point {
        if self.is_empty() {
            return from.clone();
        }
        let mut result = self.corner(0);
        for i in 1..self.border_line_count() {
            let c = self.corner(i);
            if c.side_of(from, &result) == Side::OnTheLeft {
                result = c;
            }
        }
        result
    }

    pub fn right_most_corner(&self, from: &Point) -> Point {
        if self.is_empty() {
            return from.clone();
        }
        let mut result = self.corner(0);
        for i in 1..self.border_line_count() {
            let c = self.corner(i);
            if c.side_of(from, &result) == Side::OnTheRight {
                result = c;
            }
        }
        result
    }

    // ---------------------------------------------------------------- intersection

    pub fn intersection(&self, other: &TileShape) -> TileShape {
        use TileShape as T;
        match (self, other) {
            (T::Box(a), T::Box(b)) => T::Box(a.intersection(b)),
            (T::Box(a), T::Octagon(b)) => T::Octagon(b.intersection(&a.to_int_octagon())),
            (T::Octagon(a), T::Box(b)) => T::Octagon(a.intersection(&b.to_int_octagon())),
            (T::Octagon(a), T::Octagon(b)) => T::Octagon(a.intersection(b)),
            (T::Simplex(s), _) => T::Simplex(s.intersection(&other.to_simplex())),
            (_, T::Simplex(s)) => T::Simplex(s.intersection(&self.to_simplex())),
        }
    }

    pub fn intersection_with_simplify(&self, other: &TileShape) -> TileShape {
        self.intersection(other).simplify()
    }

    pub fn intersects(&self, other: &TileShape) -> bool {
        use TileShape as T;
        match (self, other) {
            (T::Box(a), T::Box(b)) => a.intersects(b),
            (T::Box(a), T::Octagon(b)) | (T::Octagon(b), T::Box(a)) => b.intersects(&a.to_int_octagon()),
            (T::Octagon(a), T::Octagon(b)) => a.intersects(b),
            (T::Simplex(s), _) => s.intersects(&other.to_simplex()),
            (_, T::Simplex(s)) => s.intersects(&self.to_simplex()),
        }
    }

    pub fn intersects_circle(&self, c: &Circle) -> bool {
        c.intersects_tile(self)
    }

    /// Like intersects, but the intersection must be 2-dimensional (only for boxes and octagons
    /// computed exactly).
    pub fn overlaps(&self, other: &TileShape) -> bool {
        use TileShape as T;
        match (self, other) {
            (T::Box(a), T::Box(b)) => a.overlaps(b),
            (T::Box(a), T::Octagon(b)) | (T::Octagon(b), T::Box(a)) => b.overlaps(&a.to_int_octagon()),
            (T::Octagon(a), T::Octagon(b)) => a.overlaps(b),
            _ => self.intersection(other).dimension() == 2,
        }
    }

    // ---------------------------------------------------------------- point location

    pub fn is_outside(&self, p: &Point) -> bool {
        let n = self.border_line_count();
        if n == 0 {
            return true;
        }
        for i in 0..n {
            if self.border_line(i).side_of(p) == Side::OnTheLeft {
                return true;
            }
        }
        false
    }

    pub fn contains(&self, p: &Point) -> bool {
        !self.is_outside(p)
    }

    pub fn contains_int(&self, p: &IntPoint) -> bool {
        match self {
            TileShape::Box(b) => b.contains_point(p),
            TileShape::Octagon(o) => o.contains_point(p),
            TileShape::Simplex(_) => self.contains(&Point::Int(*p)),
        }
    }

    pub fn contains_float(&self, p: &FloatPoint) -> bool {
        match self {
            TileShape::Octagon(o) => o.contains_float(p),
            _ => self.contains_float_tol(p, 0.0),
        }
    }

    pub fn contains_float_tol(&self, p: &FloatPoint, tolerance: f64) -> bool {
        let n = self.border_line_count();
        if n == 0 {
            return false;
        }
        for i in 0..n {
            if self.border_line(i).side_of_float_tol(p, tolerance) != Side::OnTheRight {
                return false;
            }
        }
        true
    }

    /// Returns true, if `other` is contained in this shape.
    pub fn contains_tile(&self, other: &TileShape) -> bool {
        for i in 0..other.border_line_count() {
            if !self.contains(&other.corner(i)) {
                return false;
            }
        }
        true
    }

    pub fn contains_inside(&self, p: &Point) -> bool {
        let n = self.border_line_count();
        if n == 0 {
            return false;
        }
        for i in 0..n {
            if self.border_line(i).side_of(p) != Side::OnTheRight {
                return false;
            }
        }
        true
    }

    pub fn side_of_border(&self, p: &FloatPoint, tolerance: f64) -> Side {
        let n = self.border_line_count();
        if n == 0 {
            return Side::Collinear;
        }
        let mut result = Side::OnTheRight;
        for i in 0..n {
            let s = self.border_line(i).side_of_float_tol(p, tolerance);
            if s == Side::OnTheLeft {
                return Side::OnTheLeft;
            } else if s == Side::Collinear {
                result = s;
            }
        }
        result
    }

    pub fn contains_on_border_line_no(&self, p: &Point) -> i32 {
        let n = self.border_line_count();
        if n == 0 {
            return -1;
        }
        let mut containing = -1;
        for i in 0..n {
            let s = self.border_line(i).side_of(p);
            if s == Side::OnTheLeft {
                return -1;
            }
            if s == Side::Collinear {
                containing = i as i32;
            }
        }
        containing
    }

    pub fn contains_on_border(&self, p: &Point) -> bool {
        self.contains_on_border_line_no(p) >= 0
    }

    pub fn contains_approx(&self, other: &TileShape) -> bool {
        other.corner_approx_arr().iter().all(|c| self.contains_float(c))
    }

    pub fn distance(&self, p: &FloatPoint) -> f64 {
        match self {
            TileShape::Box(b) => b.distance(p),
            _ => match self.nearest_point_approx(p) {
                Some(n) => n.distance(p),
                None => f64::MAX,
            },
        }
    }

    pub fn border_distance(&self, p: &FloatPoint) -> f64 {
        match self.nearest_border_point_approx(p) {
            Some(n) => n.distance(p),
            None => f64::MAX,
        }
    }

    pub fn smallest_radius(&self) -> f64 {
        self.border_distance(&self.centre_of_gravity())
    }

    pub fn nearest_point(&self, from: &Point) -> Option<Point> {
        if !self.is_outside(from) {
            return Some(from.clone());
        }
        self.nearest_border_point(from)
    }

    pub fn nearest_point_approx(&self, from: &FloatPoint) -> Option<FloatPoint> {
        if self.contains_float(from) {
            return Some(*from);
        }
        self.nearest_border_point_approx(from)
    }

    pub fn nearest_border_point(&self, from: &Point) -> Option<Point> {
        let n = self.border_line_count();
        if n == 0 {
            return None;
        }
        let from_f = from.to_float();
        if n == 1 {
            return Some(self.border_line(0).perpendicular_projection(from));
        }
        let mut min_dist = f64::MAX;
        let mut min_ind = 0;
        for i in 0..n {
            let d = self.corner_approx(i).distance_square(&from_f);
            if d < min_dist {
                min_dist = d;
                min_ind = i;
            }
        }
        let mut nearest = self.corner(min_ind);
        let mut prev_ind = n - 2;
        let mut cur_ind = n - 1;
        for next_ind in 0..n {
            let projection = self.border_line(cur_ind).perpendicular_projection(from);
            if (!self.corner_is_bounded(cur_ind)
                || self.border_line(prev_ind).side_of(&projection) == Side::OnTheRight)
                && (!self.corner_is_bounded(next_ind)
                    || self.border_line(next_ind).side_of(&projection) == Side::OnTheRight)
            {
                let d = projection.to_float().distance_square(&from_f);
                if d < min_dist {
                    min_dist = d;
                    nearest = projection;
                }
            }
            prev_ind = cur_ind;
            cur_ind = next_ind;
        }
        Some(nearest)
    }

    pub fn nearest_border_point_approx(&self, from: &FloatPoint) -> Option<FloatPoint> {
        self.nearest_border_points_approx(from, 1).into_iter().next()
    }

    pub fn nearest_border_points_approx(&self, from: &FloatPoint, count: usize) -> Vec<FloatPoint> {
        if count == 0 {
            return Vec::new();
        }
        let n = self.border_line_count();
        if n == 0 {
            return Vec::new();
        }
        if n == 1 {
            return vec![from.projection_approx(&self.border_line(0))];
        }
        if self.dimension() == 0 {
            return vec![self.corner_approx(0)];
        }
        let rc = count.min(n);
        let mut nearest: Vec<Option<FloatPoint>> = vec![None; rc];
        let mut min_dists = vec![f64::MAX; rc];
        let insert = |d: f64, p: FloatPoint, nearest: &mut Vec<Option<FloatPoint>>, min_dists: &mut Vec<f64>| {
            for j in 0..rc {
                if d < min_dists[j] {
                    for k in j + 1..rc {
                        min_dists[k] = min_dists[k - 1];
                        nearest[k] = nearest[k - 1];
                    }
                    min_dists[j] = d;
                    nearest[j] = Some(p);
                    break;
                }
            }
        };
        for i in 0..n {
            if self.corner_is_bounded(i) {
                let c = self.corner_approx(i);
                let d = c.distance_square(from);
                insert(d, c, &mut nearest, &mut min_dists);
            }
        }
        let mut prev_ind = n - 2;
        let mut cur_ind = n - 1;
        for next_ind in 0..n {
            let projection = from.projection_approx(&self.border_line(cur_ind));
            if (!self.corner_is_bounded(cur_ind)
                || self.border_line(prev_ind).side_of_float(&projection) == Side::OnTheRight)
                && (!self.corner_is_bounded(next_ind)
                    || self.border_line(next_ind).side_of_float(&projection) == Side::OnTheRight)
            {
                let d = projection.distance_square(from);
                insert(d, projection, &mut nearest, &mut min_dists);
            }
            prev_ind = cur_ind;
            cur_ind = next_ind;
        }
        nearest.into_iter().flatten().collect()
    }

    pub fn index_of_nearest_corner(&self, from: &Point) -> usize {
        let from_f = from.to_float();
        let mut result = 0;
        let mut min_dist = f64::MAX;
        for i in 0..self.border_line_count() {
            let d = self.corner_approx(i).distance(&from_f);
            if d < min_dist {
                min_dist = d;
                result = i;
            }
        }
        result
    }

    pub fn diagonal_corner_segment(&self) -> Option<FloatLine> {
        if self.is_empty() {
            return None;
        }
        Some(FloatLine::new(
            self.corner_approx(0),
            self.corner_approx(self.border_line_count() / 2),
        ))
    }

    pub fn nearest_relative_outside_locations(&self, shape: &TileShape, count: usize) -> Vec<FloatPoint> {
        let n = self.border_line_count();
        if count == 0 || n < 3 || !self.intersects(shape) {
            return Vec::new();
        }
        let rc = count.min(n);
        let mut coors: Vec<FloatPoint> = vec![FloatPoint::ZERO; rc];
        let mut min_dists = vec![f64::MAX; rc];
        let mut cur_ind = n - 1;
        let other_count = shape.border_line_count();
        for next_ind in 0..n {
            let mut cur_max = 0.0;
            let mut cur_coor = FloatPoint::ZERO;
            let bl = self.border_line(cur_ind);
            for ci in 0..other_count {
                let c = shape.corner_approx(ci);
                if bl.side_of_float(&c) == Side::OnTheRight {
                    let proj = c.projection_approx(&bl);
                    let d = proj.distance_square(&c);
                    if d > cur_max {
                        cur_max = d;
                        cur_coor = proj.subtract(&c);
                    }
                }
            }
            for j in 0..rc {
                if cur_max < min_dists[j] {
                    for k in j + 1..rc {
                        min_dists[k] = min_dists[k - 1];
                        coors[k] = coors[k - 1];
                    }
                    min_dists[j] = cur_max;
                    coors[j] = cur_coor;
                    break;
                }
            }
            cur_ind = next_ind;
        }
        coors
    }

    // ---------------------------------------------------------------- offsets

    /// Enlarges the shape by `distance` (shrinks for negative values).
    pub fn offset(&self, distance: f64) -> TileShape {
        match self {
            TileShape::Box(b) => TileShape::Box(b.offset(distance)),
            TileShape::Octagon(o) => TileShape::Octagon(o.offset(distance)),
            TileShape::Simplex(s) => TileShape::Simplex(s.offset(distance)),
        }
    }

    pub fn enlarge(&self, offset: f64) -> TileShape {
        match self {
            TileShape::Box(b) => TileShape::Octagon(b.enlarge(offset)),
            TileShape::Octagon(o) => TileShape::Octagon(o.enlarge(offset)),
            TileShape::Simplex(s) => TileShape::Simplex(s.enlarge(offset)),
        }
    }

    pub fn shrink(&self, offset: f64) -> TileShape {
        let r = self.offset(-offset);
        if r.is_empty() {
            let centre_box = self.centre_of_gravity().bounding_box();
            return self.intersection(&TileShape::Box(centre_box));
        }
        r
    }

    pub fn length(&self) -> f64 {
        if !self.is_bounded() {
            return i32::MAX as f64;
        }
        let dim = self.dimension();
        if dim <= 0 {
            return 0.0;
        }
        if dim == 1 {
            return self.circumference() / 2.0;
        }
        let mut md = -1.0;
        let mut md2 = -1.0;
        let g = self.centre_of_gravity();
        for i in 0..self.border_line_count() {
            let d = self.border_line(i).signed_distance(&g).abs();
            if d > md {
                md2 = md;
                md = d;
            } else if d > md2 {
                md2 = d;
            }
        }
        md + md2
    }

    pub fn touching_sides(&self, other: &TileShape) -> Option<(usize, usize)> {
        let mut side_no2 = 0;
        let mut dir2 = None;
        for i in 0..other.border_line_count() {
            let d = Direction::Int(other.border_line(i).direction());
            if d.compare_to(&Direction::LEFT) != std::cmp::Ordering::Less {
                side_no2 = i;
                dir2 = Some(d.opposite());
                break;
            }
        }
        let Some(mut dir2) = dir2 else {
            log::warn!("touching_side : dir2 not found");
            return None;
        };
        let mut side_no1 = 0;
        let mut dir1 = Direction::Int(self.border_line(0).direction());
        let max_ind = self.border_line_count() + other.border_line_count();
        for _ in 0..max_ind {
            let cmp = dir2.compare_to(&dir1);
            if cmp == std::cmp::Ordering::Equal
                && self.border_line(side_no1).is_equal_or_opposite(&other.border_line(side_no2))
            {
                return Some((side_no1, side_no2));
            }
            if cmp != std::cmp::Ordering::Less {
                side_no1 = (side_no1 + 1) % self.border_line_count();
                dir1 = Direction::Int(self.border_line(side_no1).direction());
            } else {
                side_no2 = (side_no2 + 1) % other.border_line_count();
                dir2 = Direction::Int(other.border_line(side_no2).direction()).opposite();
            }
        }
        None
    }

    pub fn distance_to_the_left(&self, line: &Line) -> f64 {
        let mut result = i32::MAX as f64;
        for i in 0..self.border_line_count() {
            let c = self.corner_approx(i);
            let mut s = line.side_of_float_tol(&c, 1.0);
            if s == Side::Collinear {
                s = line.side_of(&self.corner(i));
            }
            if s == Side::OnTheRight {
                result = -1.0;
                break;
            }
            result = result.min(line.signed_distance(&c));
        }
        result
    }

    pub fn side_of(&self, line: &Line) -> Side {
        let mut on_the_left = false;
        let mut on_the_right = false;
        for i in 0..self.border_line_count() {
            let s = line.side_of(&self.corner(i));
            if s == Side::OnTheLeft {
                on_the_right = true;
            } else if s == Side::OnTheRight {
                on_the_left = true;
            }
            if on_the_left && on_the_right {
                return Side::Collinear;
            }
        }
        if on_the_left {
            Side::OnTheLeft
        } else {
            Side::OnTheRight
        }
    }

    // ---------------------------------------------------------------- transformations

    pub fn translate_by(&self, v: &Vector) -> TileShape {
        match self {
            TileShape::Box(b) => TileShape::Box(b.translate_by(v)),
            TileShape::Octagon(o) => TileShape::Octagon(o.translate_by(v)),
            TileShape::Simplex(s) => TileShape::Simplex(s.translate_by(v)),
        }
    }

    pub fn turn_90_degree(&self, factor: i32, pole: &IntPoint) -> TileShape {
        if let TileShape::Box(b) = self {
            return TileShape::Box(b.turn_90_degree(factor, pole));
        }
        let lines: Vec<Line> = (0..self.border_line_count())
            .map(|i| self.border_line(i).turn_90_degree(factor, pole))
            .collect();
        TileShape::from_lines(&lines)
    }

    pub fn rotate_approx(&self, angle: f64, pole: &FloatPoint) -> TileShape {
        if angle == 0.0 {
            return self.clone();
        }
        let new_corners: Vec<Point> = (0..self.border_line_count())
            .map(|i| Point::Int(self.corner_approx(i).rotate(angle, pole).round()))
            .collect();
        let poly = Polygon::new(&new_corners);
        let pc = poly.corner_array();
        if pc.len() >= 3 {
            TileShape::from_convex_polygon(&pc)
        } else if pc.len() == 2 {
            let pl = Polyline::from_points(&pc);
            TileShape::Simplex(LineSegment::from_polyline(&pl, 1).to_simplex())
        } else if pc.len() == 1 {
            TileShape::from_point(&pc[0])
        } else {
            TileShape::empty()
        }
    }

    pub fn mirror_vertical(&self, pole: &IntPoint) -> TileShape {
        let lines: Vec<Line> = (0..self.border_line_count())
            .map(|i| self.border_line(i).mirror_vertical(pole))
            .collect();
        TileShape::from_lines(&lines)
    }

    pub fn mirror_horizontal(&self, pole: &IntPoint) -> TileShape {
        let lines: Vec<Line> = (0..self.border_line_count())
            .map(|i| self.border_line(i).mirror_horizontal(pole))
            .collect();
        TileShape::from_lines(&lines)
    }

    pub fn intersecting_border_line_no(&self, p: &Point, dir: &Direction) -> i32 {
        if !self.contains(p) {
            return -1;
        }
        let from = p.to_float();
        let il = Line::from_point_direction(p, dir);
        let second = il.b.to_float();
        let mut result = -1;
        let mut min_distance = f32::MAX as f64;
        for i in 0..self.border_line_count() {
            let bl = self.border_line(i);
            let is = bl.intersection_approx(&il);
            if is.x >= i32::MAX as f64 {
                continue;
            }
            let d = is.distance_square(&from);
            if d < min_distance {
                let dir_ok = bl.side_of_float(&second) == Side::OnTheLeft || second.distance_square(&is) < d;
                if dir_ok {
                    result = i as i32;
                    min_distance = d;
                }
            }
        }
        result
    }

    // ---------------------------------------------------------------- cutouts

    /// Returns the pieces of `self` outside of `shape`.
    pub fn cutout(&self, shape: &TileShape) -> Vec<TileShape> {
        use TileShape as T;
        match self {
            T::Box(a) => {
                let tmp: Vec<TileShape> = match shape {
                    T::Box(b) => b.cutout_from_box(a).into_iter().map(T::Box).collect(),
                    T::Octagon(o) => o.cutout_from_box(a).into_iter().map(T::Octagon).collect(),
                    T::Simplex(s) => simplex_cutout(s, &a.to_simplex()),
                };
                tmp.into_iter().map(|t| t.simplify()).collect()
            }
            T::Octagon(a) => match shape {
                T::Box(b) => b.to_int_octagon().cutout_from_octagon(a).into_iter().map(T::Octagon).collect(),
                T::Octagon(o) => o.cutout_from_octagon(a).into_iter().map(T::Octagon).collect(),
                T::Simplex(s) => simplex_cutout(s, &a.to_simplex()),
            },
            T::Simplex(a) => simplex_cutout(&shape.to_simplex(), a),
        }
    }

    /// Returns the pieces of the polyline outside of this shape.
    pub fn cutout_polyline(&self, polyline: &Polyline) -> Vec<Polyline> {
        let intersection_no = self.entrance_points(polyline);
        let first_corner = polyline.first_corner();
        let first_corner_is_inside = self.contains_inside(&first_corner);
        if intersection_no.is_empty() {
            if first_corner_is_inside {
                return Vec::new();
            }
            return vec![polyline.clone()];
        }
        let plines = polyline.lines();
        let mut pieces = Vec::new();
        let mut cur_no = 0;
        let tuple = intersection_no[cur_no];
        let first_intersection = plines[tuple.0].intersection(&self.border_line(tuple.1));
        if !first_corner_is_inside {
            if first_corner != first_intersection {
                let pno = tuple.0;
                let mut cl: Vec<Line> = plines[..pno + 1].to_vec();
                cl.push(self.border_line(tuple.1));
                let piece = Polyline::from_lines(&cl);
                if !piece.is_empty() {
                    pieces.push(piece);
                }
            }
            cur_no += 1;
        }
        while cur_no + 1 < intersection_no.len() {
            let cur = intersection_no[cur_no];
            let next = intersection_no[cur_no + 1];
            let cpn = cur.0;
            let npn = next.0;
            let mut insert_piece = false;
            for i in cpn + 1..npn {
                if self.is_outside(&polyline.corner(i)) {
                    insert_piece = true;
                    break;
                }
            }
            if insert_piece {
                let len = npn - cpn + 3;
                let mut cl = Vec::with_capacity(len);
                cl.push(self.border_line(cur.1));
                cl.extend_from_slice(&plines[cpn..cpn + len - 2]);
                cl.push(self.border_line(next.1));
                let piece = Polyline::from_lines(&cl);
                if !piece.is_empty() {
                    pieces.push(piece);
                }
            }
            cur_no += 2;
        }
        if cur_no < intersection_no.len() {
            let cur = intersection_no[cur_no];
            let pno = cur.0;
            let mut cl = Vec::with_capacity(plines.len() - pno + 1);
            cl.push(self.border_line(cur.1));
            cl.extend_from_slice(&plines[pno..]);
            let piece = Polyline::from_lines(&cl);
            if !piece.is_empty() {
                pieces.push(piece);
            }
        }
        pieces
    }

    /// Pairs of (polyline line index, border line index) where the polyline crosses the border.
    pub fn entrance_points(&self, polyline: &Polyline) -> Vec<(usize, usize)> {
        let mut result = Vec::new();
        let mut prev_line_no: isize = -1;
        let mut prev_edge_no: isize = -1;
        let n = polyline.lines().len();
        for line_index in 1..n.saturating_sub(1) {
            let seg = LineSegment::from_polyline(polyline, line_index);
            for edge_index in seg.border_intersections(self) {
                if line_index as isize != prev_line_no || edge_index as isize != prev_edge_no {
                    result.push((line_index, edge_index));
                    prev_line_no = line_index as isize;
                    prev_edge_no = edge_index as isize;
                }
            }
        }
        result
    }

    pub fn split_to_convex(&self) -> Vec<TileShape> {
        vec![self.clone()]
    }

    pub fn divide_into_sections(&self, max_section_width: f64) -> Vec<TileShape> {
        if self.is_empty() {
            return vec![self.clone()];
        }
        let boxes = self.bounding_box().divide_into_sections(max_section_width);
        let mut result = Vec::new();
        for b in boxes {
            let s = self.intersection_with_simplify(&TileShape::Box(b));
            if s.dimension() == 2 {
                result.push(s);
            }
        }
        result
    }

    pub fn is_intersected_interior_by(&self, seg: &LineSegment) -> bool {
        self.is_intersected_interior_by_points(&seg.start_point(), &seg.end_point(), &seg.get_line())
    }

    pub fn is_intersected_interior_by_points(&self, start: &Point, end: &Point, line: &Line) -> bool {
        let fs = start.to_float();
        let fe = end.to_float();
        let n = self.border_line_count();
        let mut ss = Vec::with_capacity(n);
        let mut es = Vec::with_capacity(n);
        for i in 0..n {
            let bl = self.border_line(i);
            let mut s = bl.side_of_float_tol(&fs, 1.0);
            if s == Side::Collinear {
                s = bl.side_of(start);
            }
            let mut e = bl.side_of_float_tol(&fe, 1.0);
            if e == Side::Collinear {
                e = bl.side_of(end);
            }
            if s != Side::OnTheRight && e != Side::OnTheRight {
                return false;
            }
            ss.push(s);
            es.push(e);
        }
        if ss.iter().all(|s| *s == Side::OnTheRight) {
            return true;
        }
        if es.iter().all(|s| *s == Side::OnTheRight) {
            return true;
        }
        for i in 0..n {
            let s = ss[i];
            let e = es[i];
            if s != e {
                if s == Side::Collinear && e == Side::OnTheLeft || e == Side::Collinear && s == Side::OnTheLeft {
                    continue;
                }
                let mut pcs = line.side_of_float_tol(&self.corner_approx(i), 1.0);
                if pcs == Side::Collinear {
                    pcs = line.side_of(&self.corner(i));
                }
                let nci = if i == n - 1 { 0 } else { i + 1 };
                let mut ncs = line.side_of_float_tol(&self.corner_approx(nci), 1.0);
                if ncs == Side::Collinear {
                    ncs = line.side_of(&self.corner(nci));
                }
                if pcs == Side::OnTheLeft && ncs == Side::OnTheRight || pcs == Side::OnTheRight && ncs == Side::OnTheLeft {
                    return true;
                }
            }
        }
        false
    }
}

fn simplex_cutout(inner: &Simplex, outer: &Simplex) -> Vec<TileShape> {
    match inner.cutout_from(outer) {
        Some(v) => v.into_iter().map(TileShape::Simplex).collect(),
        None => vec![TileShape::Simplex(outer.clone())],
    }
}
