//! Infinite directed lines through two integer points.

use super::basics::{bigint_aux, java_round, Side, Signum, CRIT_INT};
use super::point::sign;
use super::{
    Direction, FloatPoint, IntDirection, IntPoint, IntVector, Point, RationalPoint, TileShape,
    Vector,
};
use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::cmp::Ordering;

/// A directed line defined by two distinct integer points `a` and `b`.
/// The interior of a convex tile is on the right of all its border lines.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub a: IntPoint,
    pub b: IntPoint,
    /// Identity token emulating Java object identity: every constructed line gets a new
    /// one, copies keep it. Ignored by `==` and `Hash`; compared by `Line::same_object`.
    uid: u64,
}

impl std::hash::Hash for Line {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.a.hash(state);
        self.b.hash(state);
    }
}

/// Process wide unique line ids without contention: each thread reserves blocks of ids.
fn next_line_uid() -> u64 {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU64, Ordering};
    const BLOCK: u64 = 1 << 20;
    static NEXT_BLOCK: AtomicU64 = AtomicU64::new(1);
    thread_local! {
        static RANGE: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
    }
    RANGE.with(|r| {
        let (mut next, mut end) = r.get();
        if next == end {
            next = NEXT_BLOCK.fetch_add(1, Ordering::Relaxed) * BLOCK;
            end = next + BLOCK;
        }
        r.set((next + 1, end));
        next
    })
}

impl Line {
    #[inline]
    pub fn new(a: IntPoint, b: IntPoint) -> Line {
        Line { a, b, uid: next_line_uid() }
    }

    /// Java reference identity (`==`) of two lines.
    #[inline]
    pub fn same_object(&self, other: &Line) -> bool {
        self.uid == other.uid
    }

    /// Creates a line from points, which are expected to be integer points.
    pub fn from_points(a: &Point, b: &Point) -> Line {
        Line::new(a.to_int(), b.to_int())
    }

    #[inline]
    pub fn from_coords(ax: i32, ay: i32, bx: i32, by: i32) -> Line {
        Line::new(IntPoint::new(ax, ay), IntPoint::new(bx, by))
    }

    #[inline]
    pub fn from_point_dir(a: IntPoint, dir: &IntDirection) -> Line {
        Line::new(a, a.translate_by_int(dir.get_vector()))
    }

    pub fn from_point_direction(a: &Point, dir: &Direction) -> Line {
        let b = a.translate_by(&dir.get_vector());
        Line::from_points(a, &b)
    }

    pub fn get_id(&self) -> i32 {
        31i32.wrapping_mul(self.a.get_id()).wrapping_add(self.b.get_id())
    }

    /// Like equals, but only for lines with the same direction already known to be normalized.
    pub fn fast_equals(&self, other: &Line) -> bool {
        let dx1 = other.a.x as f64 - self.a.x as f64;
        let dy1 = other.a.y as f64 - self.a.y as f64;
        let dx2 = self.b.x as f64 - self.a.x as f64;
        let dy2 = self.b.y as f64 - self.a.y as f64;
        let det = dx1 * dy2 - dx2 * dy1;
        if det != 0.0 {
            return false;
        }
        self.direction() == other.direction()
    }

    #[inline]
    pub fn vector(&self) -> IntVector {
        self.b.difference_by(&self.a)
    }

    #[inline]
    pub fn direction(&self) -> IntDirection {
        self.vector().to_normalized_direction()
    }

    #[inline]
    pub fn direction_dyn(&self) -> Direction {
        Direction::Int(self.direction())
    }

    #[inline]
    pub fn side_of_int(&self, p: &IntPoint) -> Side {
        p.side_of_line(self).negate()
    }

    #[inline]
    pub fn side_of(&self, p: &Point) -> Side {
        match p {
            Point::Int(ip) => self.side_of_int(ip),
            _ => p.side_of_line(self).negate(),
        }
    }

    pub fn side_of_float_tol(&self, p: &FloatPoint, tolerance: f64) -> Side {
        let det = (self.b.y as f64 - self.a.y as f64) * (p.x - self.a.x as f64)
            - (self.b.x as f64 - self.a.x as f64) * (p.y - self.a.y as f64);
        if det - tolerance > 0.0 {
            Side::OnTheLeft
        } else if det + tolerance < 0.0 {
            Side::OnTheRight
        } else {
            Side::Collinear
        }
    }

    #[inline]
    pub fn side_of_float(&self, p: &FloatPoint) -> Side {
        self.side_of_float_tol(p, 0.0)
    }

    pub fn side_of_intersection(&self, p1: &Line, p2: &Line) -> Side {
        let approx = p1.intersection_approx(p2);
        let r = self.side_of_float_tol(&approx, 1.0);
        if r == Side::Collinear {
            let is = p1.intersection(p2);
            return self.side_of(&is);
        }
        r
    }

    pub fn is_on_the_left(&self, tile: &TileShape) -> bool {
        for i in 0..tile.border_line_count() {
            if self.side_of(&tile.corner(i)) == Side::OnTheRight {
                return false;
            }
        }
        true
    }

    pub fn is_on_the_right(&self, tile: &TileShape) -> bool {
        for i in 0..tile.border_line_count() {
            if self.side_of(&tile.corner(i)) == Side::OnTheLeft {
                return false;
            }
        }
        true
    }

    pub fn signed_distance(&self, p: &FloatPoint) -> f64 {
        let dx = self.b.x as f64 - self.a.x as f64;
        let dy = self.b.y as f64 - self.a.y as f64;
        let det = dy * (p.x - self.a.x as f64) - dx * (p.y - self.a.y as f64);
        det / (dx * dx + dy * dy).sqrt()
    }

    pub fn overlaps(&self, other: &Line) -> bool {
        self.side_of_int(&other.a) == Side::Collinear && self.side_of_int(&other.b) == Side::Collinear
    }

    #[inline]
    pub fn opposite(&self) -> Line {
        Line::new(self.b, self.a)
    }

    /// The exact intersection of the two lines. Infinite (z == 0) for parallel lines.
    pub fn intersection(&self, other: &Line) -> Point {
        let delta1 = self.vector();
        let delta2 = other.vector();
        let (ta, oa) = (self.a, other.a);
        if delta1.x == 0 {
            if delta2.y == 0 {
                return Point::Int(IntPoint::new(ta.x, oa.y));
            }
            if delta2.x == delta2.y {
                return Point::Int(IntPoint::new(ta.x, oa.y + ta.x - oa.x));
            }
            if delta2.x == -delta2.y {
                return Point::Int(IntPoint::new(ta.x, oa.y + oa.x - ta.x));
            }
        } else if delta1.y == 0 {
            if delta2.x == 0 {
                return Point::Int(IntPoint::new(oa.x, ta.y));
            }
            if delta2.x == delta2.y {
                return Point::Int(IntPoint::new(oa.x + ta.y - oa.y, ta.y));
            }
            if delta2.x == -delta2.y {
                return Point::Int(IntPoint::new(oa.x + oa.y - ta.y, ta.y));
            }
        } else if delta1.x == delta1.y {
            if delta2.x == 0 {
                return Point::Int(IntPoint::new(oa.x, ta.y + oa.x - ta.x));
            }
            if delta2.y == 0 {
                return Point::Int(IntPoint::new(ta.x + oa.y - ta.y, oa.y));
            }
        } else if delta1.x == -delta1.y {
            if delta2.x == 0 {
                return Point::Int(IntPoint::new(oa.x, ta.y + ta.x - oa.x));
            }
            if delta2.y == 0 {
                return Point::Int(IntPoint::new(ta.x + ta.y - oa.y, oa.y));
            }
        }
        let det1 = self.a.determinant(&self.b) as i128;
        let det2 = other.a.determinant(&other.b) as i128;
        let det = delta2.determinant(&delta1) as i128;
        // i128 is sufficient: |det| < 2^64, |detN| < 2^63, |delta| < 2^33
        let is_x = det1 * delta2.x as i128 - det2 * delta1.x as i128;
        let is_y = det1 * delta2.y as i128 - det2 * delta1.y as i128;
        let (mut det, mut is_x, mut is_y) = (det, is_x, is_y);
        if det != 0 {
            if det < 0 {
                det = -det;
                is_x = -is_x;
                is_y = -is_y;
            }
            if is_x.rem_euclid(det) == 0 && is_y.rem_euclid(det) == 0 {
                let x = is_x / det;
                let y = is_y / det;
                if (x as f64).abs() <= CRIT_INT as f64 && (y as f64).abs() <= CRIT_INT as f64 {
                    return Point::Int(IntPoint::new(x as i32, y as i32));
                }
                return Point::Rational(RationalPoint::new(
                    BigInt::from(x),
                    BigInt::from(y),
                    BigInt::one(),
                ));
            }
        }
        Point::Rational(RationalPoint::new(
            BigInt::from(is_x),
            BigInt::from(is_y),
            BigInt::from(det),
        ))
    }

    pub fn intersection_approx(&self, other: &Line) -> FloatPoint {
        let (ta, tb, oa, ob) = (self.a, self.b, other.a, other.b);
        let d1x = tb.x as f64 - ta.x as f64;
        let d1y = tb.y as f64 - ta.y as f64;
        let d2x = ob.x as f64 - oa.x as f64;
        let d2y = ob.y as f64 - oa.y as f64;
        let det1 = ta.x as f64 * tb.y as f64 - ta.y as f64 * tb.x as f64;
        let det2 = oa.x as f64 * ob.y as f64 - oa.y as f64 * ob.x as f64;
        let det = d2x * d1y - d2y * d1x;
        if det == 0.0 {
            FloatPoint::new(i32::MAX as f64, i32::MAX as f64)
        } else {
            FloatPoint::new((d2x * det1 - d1x * det2) / det, (d2y * det1 - d1y * det2) / det)
        }
    }

    pub fn perpendicular_projection(&self, p: &Point) -> Point {
        p.perpendicular_projection(self)
    }

    /// Translates the line perpendicular at about `dist` to the left.
    pub fn translate(&self, dist: f64) -> Line {
        let ai = self.a;
        let dir = self.direction();
        let v = dir.get_vector();
        let vxvx = v.x as f64 * v.x as f64;
        let vyvy = v.y as f64 * v.y as f64;
        let length = (vxvx + vyvy).sqrt();
        let new_a = if vxvx <= vyvy {
            let rel_x = java_round((dist * length) / v.y as f64);
            IntPoint::new(ai.x - rel_x, ai.y)
        } else {
            let rel_y = java_round((dist * length) / v.x as f64);
            IntPoint::new(ai.x, ai.y + rel_y)
        };
        Line::from_point_dir(new_a, &dir)
    }

    pub fn translate_by(&self, v: &Vector) -> Line {
        if v.is_zero() {
            return *self;
        }
        match v {
            Vector::Int(iv) => Line::new(self.a.translate_by_int(*iv), self.b.translate_by_int(*iv)),
            _ => {
                let na = Point::Int(self.a).translate_by(v);
                let nb = Point::Int(self.b).translate_by(v);
                Line::from_points(&na, &nb)
            }
        }
    }

    pub fn translate_by_int(&self, v: IntVector) -> Line {
        Line::new(self.a.translate_by_int(v), self.b.translate_by_int(v))
    }

    pub fn is_orthogonal(&self) -> bool {
        self.direction().is_orthogonal()
    }

    pub fn is_diagonal(&self) -> bool {
        self.direction().is_diagonal()
    }

    pub fn is_multiple_of_45_degree(&self) -> bool {
        let d = self.direction();
        d.is_orthogonal() || d.is_diagonal()
    }

    pub fn is_parallel(&self, other: &Line) -> bool {
        self.direction().get_vector().side_of(&other.direction().get_vector()) == Side::Collinear
    }

    pub fn is_perpendicular(&self, other: &Line) -> bool {
        self.direction().get_vector().projection(&other.direction().get_vector()) == Signum::Zero
    }

    pub fn is_equal_or_opposite(&self, other: &Line) -> bool {
        self.overlaps(other)
    }

    pub fn cos_angle(&self, other: &Line) -> f64 {
        Vector::Int(self.vector()).cos_angle(&Vector::Int(other.vector()))
    }

    /// Compares the directions of the lines by their angle with the x axis.
    pub fn compare_to(&self, other: &Line) -> Ordering {
        let dx1 = self.b.x - self.a.x;
        let dy1 = self.b.y - self.a.y;
        let dx2 = other.b.x - other.a.x;
        let dy2 = other.b.y - other.a.y;
        if dy1 > 0 {
            if dy2 < 0 {
                return Ordering::Less;
            }
            if dy2 == 0 {
                if dx2 > 0 {
                    return Ordering::Greater;
                }
                return Ordering::Less;
            }
        } else if dy1 < 0 {
            if dy2 >= 0 {
                return Ordering::Greater;
            }
        } else {
            if dx1 > 0 {
                if dy2 != 0 || dx2 < 0 {
                    return Ordering::Less;
                }
                return Ordering::Equal;
            }
            if dy2 > 0 || dy2 == 0 && dx2 > 0 {
                return Ordering::Greater;
            }
            if dy2 < 0 {
                return Ordering::Less;
            }
            return Ordering::Equal;
        }
        let determinant = dx2 as f64 * dy1 as f64 - dy2 as f64 * dx1 as f64;
        Signum::as_int(determinant).cmp(&0)
    }

    pub fn function_value_approx(&self, x: f64) -> f64 {
        let p1 = self.a.to_float();
        let p2 = self.b.to_float();
        let dx = p2.x - p1.x;
        if dx == 0.0 {
            log::warn!("function_value_approx: line is vertical");
            return 0.0;
        }
        let dy = p2.y - p1.y;
        let det = p1.x * p2.y - p2.x * p1.y;
        (dy * x - det) / dx
    }

    pub fn function_in_y_value_approx(&self, y: f64) -> f64 {
        let p1 = self.a.to_float();
        let p2 = self.b.to_float();
        let dy = p2.y - p1.y;
        if dy == 0.0 {
            log::warn!("function_in_y_value_approx: line is horizontal");
            return 0.0;
        }
        let dx = p2.x - p1.x;
        let det = p1.x * p2.y - p2.x * p1.y;
        (dx * y + det) / dy
    }

    /// Direction perpendicular to this line pointing from `from_point` towards the line.
    pub fn perpendicular_direction(&self, from_point: &Point) -> Option<IntDirection> {
        let line_side = self.side_of(from_point);
        if line_side == Side::Collinear {
            return None;
        }
        let dir1 = self.direction().turn_45_degree(2);
        let dir2 = self.direction().turn_45_degree(6);
        let check1 = from_point.translate_by(&Vector::Int(dir1.get_vector()));
        if self.side_of(&check1) != line_side {
            return Some(dir1);
        }
        let check2 = from_point.translate_by(&Vector::Int(dir2.get_vector()));
        if self.side_of(&check2) != line_side {
            return Some(dir2);
        }
        let nearest = from_point.to_float().projection_approx(self);
        if nearest.distance_square(&check1.to_float()) <= nearest.distance_square(&check2.to_float())
        {
            Some(dir1)
        } else {
            Some(dir2)
        }
    }

    pub fn turn_90_degree(&self, factor: i32, pole: &IntPoint) -> Line {
        Line::new(self.a.turn_90_degree(factor, pole), self.b.turn_90_degree(factor, pole))
    }

    pub fn mirror_vertical(&self, pole: &IntPoint) -> Line {
        Line::new(self.b.mirror_vertical(pole), self.a.mirror_vertical(pole))
    }

    pub fn mirror_horizontal(&self, pole: &IntPoint) -> Line {
        Line::new(self.b.mirror_horizontal(pole), self.a.mirror_horizontal(pole))
    }

    pub fn length(&self) -> f32 {
        let dx = self.b.x as f64 - self.a.x as f64;
        let dy = self.b.y as f64 - self.a.y as f64;
        (dx * dx + dy * dy).sqrt() as f32
    }
}

impl PartialEq for Line {
    fn eq(&self, other: &Line) -> bool {
        if self.side_of_int(&other.a) != Side::Collinear {
            return false;
        }
        if self.side_of_int(&other.b) != Side::Collinear {
            return false;
        }
        self.vector().projection(&other.vector()) == Signum::Positive
    }
}

#[allow(dead_code)]
fn unused_bigint_helpers(a: &BigInt) -> bool {
    // keep imports used in all cfgs
    a.is_zero() || a.is_negative() || a.to_i32().is_some() || sign(a) == 0 || {
        let _ = bigint_aux::determinant;
        false
    }
}
