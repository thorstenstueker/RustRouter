//! Exact points of the plane: integer points and rational (homogeneous) points.

use super::basics::{bigint_aux, Side, CRIT_INT, CRIT_INT_BIG};
use super::{FloatPoint, IntBox, IntOctagon, IntVector, Line, RationalVector, Vector};
use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct IntPoint {
    pub x: i32,
    pub y: i32,
}

/// A point with rational coordinates `(x/z, y/z)`, `z >= 0`. `z == 0` denotes an infinite point.
#[derive(Clone, Debug)]
pub struct RationalPoint {
    pub x: BigInt,
    pub y: BigInt,
    pub z: BigInt,
}

#[derive(Clone, Debug)]
pub enum Point {
    Int(IntPoint),
    Rational(RationalPoint),
}

impl fmt::Display for IntPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({},{})", self.x, self.y)
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Point::Int(p) => p.fmt(f),
            Point::Rational(_) => {
                let fp = self.to_float();
                write!(f, "({},{})", fp.x, fp.y)
            }
        }
    }
}

impl IntPoint {
    pub const ZERO: IntPoint = IntPoint { x: 0, y: 0 };

    #[inline]
    pub const fn new(x: i32, y: i32) -> IntPoint {
        IntPoint { x, y }
    }

    #[inline]
    pub fn to_point(self) -> Point {
        Point::Int(self)
    }

    #[inline]
    pub fn to_float(self) -> FloatPoint {
        FloatPoint::new(self.x as f64, self.y as f64)
    }

    pub fn get_id(&self) -> i32 {
        31i32.wrapping_mul(self.x).wrapping_add(self.y)
    }

    pub fn surrounding_box(&self) -> IntBox {
        IntBox::from_points(*self, *self)
    }

    pub fn surrounding_octagon(&self) -> IntOctagon {
        let tmp1 = self.x - self.y;
        let tmp2 = self.x + self.y;
        IntOctagon::new(self.x, self.y, self.x, self.y, tmp1, tmp1, tmp2, tmp2)
    }

    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        self.x >= b.ll.x && self.y >= b.ll.y && self.x <= b.ur.x && self.y <= b.ur.y
    }

    #[inline]
    pub fn translate_by_int(&self, v: IntVector) -> IntPoint {
        IntPoint::new(self.x.wrapping_add(v.x), self.y.wrapping_add(v.y))
    }

    #[inline]
    pub fn difference_by(&self, other: &IntPoint) -> IntVector {
        IntVector::new(self.x.wrapping_sub(other.x), self.y.wrapping_sub(other.y))
    }

    #[inline]
    pub fn determinant(&self, other: &IntPoint) -> i64 {
        self.x as i64 * other.y as i64 - self.y as i64 * other.x as i64
    }

    pub fn side_of_line(&self, line: &Line) -> Side {
        let v1 = self.difference_by(&line.a);
        let v2 = line.b.difference_by(&line.a);
        v1.side_of(&v2)
    }

    pub fn perpendicular_projection(&self, line: &Line) -> Point {
        let v = line.b.difference_by(&line.a);
        let vxvx = BigInt::from(v.x as i64 * v.x as i64);
        let vyvy = BigInt::from(v.y as i64 * v.y as i64);
        let vxvy = BigInt::from(v.x as i64 * v.y as i64);
        let mut denominator = &vxvx + &vyvy;
        let det = BigInt::from(line.a.determinant(&line.b));
        let px = BigInt::from(self.x);
        let py = BigInt::from(self.y);
        let mut proj_x = &vxvx * &px + &vxvy * &py + &det * BigInt::from(v.y);
        let mut proj_y = &vxvy * &px + &vyvy * &py - &det * BigInt::from(v.x);
        if !denominator.is_zero() {
            if denominator.is_negative() {
                denominator = -denominator;
                proj_x = -proj_x;
                proj_y = -proj_y;
            }
            if bigint_aux::is_divisible(&proj_x, &denominator)
                && bigint_aux::is_divisible(&proj_y, &denominator)
            {
                let x = &proj_x / &denominator;
                let y = &proj_y / &denominator;
                return Point::Int(IntPoint::new(
                    x.to_i64().unwrap_or(0) as i32,
                    y.to_i64().unwrap_or(0) as i32,
                ));
            }
        }
        Point::Rational(RationalPoint::new(proj_x, proj_y, denominator))
    }

    pub fn signed_area(&self, p1: &IntPoint, p2: &IntPoint) -> f64 {
        let d21 = p2.difference_by(p1);
        let d01 = self.difference_by(p1);
        d21.determinant(&d01) as f64
    }

    pub fn distance_square(&self, to: &IntPoint) -> f64 {
        let dx = to.x as f64 - self.x as f64;
        let dy = to.y as f64 - self.y as f64;
        dx * dx + dy * dy
    }

    pub fn distance(&self, to: &IntPoint) -> f64 {
        self.distance_square(to).sqrt()
    }

    pub fn orthogonal_projection(&self, other: &IntPoint) -> IntPoint {
        let horizontal = (self.x - other.x).abs();
        let vertical = (self.y - other.y).abs();
        if horizontal <= vertical {
            IntPoint::new(other.x, self.y)
        } else {
            IntPoint::new(self.x, other.y)
        }
    }

    pub fn fortyfive_degree_projection(&self, other: &IntPoint) -> IntPoint {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let diagonal1 = (dy as f64 - dx as f64) / 2.0;
        let diagonal2 = (dy as f64 + dx as f64) / 2.0;
        let dist = [
            (dx as f64).abs(),
            (dy as f64).abs(),
            diagonal1.abs(),
            diagonal2.abs(),
        ];
        let mut min_dist = dist[0];
        for d in &dist[1..] {
            if *d < min_dist {
                min_dist = *d;
            }
        }
        if min_dist == dist[0] {
            IntPoint::new(other.x, self.y)
        } else if min_dist == dist[1] {
            IntPoint::new(self.x, other.y)
        } else if min_dist == dist[2] {
            let d = diagonal2 as i32;
            IntPoint::new(other.x + d, other.y + d)
        } else {
            let d = diagonal1 as i32;
            IntPoint::new(other.x - d, other.y + d)
        }
    }

    pub fn fortyfive_degree_corner(&self, to: &IntPoint, left_turn: bool) -> Option<IntPoint> {
        let dx = to.x - self.x;
        let dy = to.y - self.y;
        let r = if dy > 0 && dy < dx {
            if left_turn {
                IntPoint::new(to.x - dy, self.y)
            } else {
                IntPoint::new(self.x + dy, to.y)
            }
        } else if dx > 0 && dy > dx {
            if left_turn {
                IntPoint::new(to.x, self.y + dx)
            } else {
                IntPoint::new(self.x, to.y - dx)
            }
        } else if dx < 0 && dy > -dx {
            if left_turn {
                IntPoint::new(self.x, to.y + dx)
            } else {
                IntPoint::new(to.x, self.y - dx)
            }
        } else if dy > 0 && dy < -dx {
            if left_turn {
                IntPoint::new(self.x - dy, to.y)
            } else {
                IntPoint::new(to.x + dy, self.y)
            }
        } else if dy < 0 && dy > dx {
            if left_turn {
                IntPoint::new(to.x - dy, self.y)
            } else {
                IntPoint::new(self.x + dy, to.y)
            }
        } else if dx < 0 && dy < dx {
            if left_turn {
                IntPoint::new(to.x, self.y + dx)
            } else {
                IntPoint::new(self.x, to.y - dx)
            }
        } else if dx > 0 && dy < -dx {
            if left_turn {
                IntPoint::new(self.x, to.y + dx)
            } else {
                IntPoint::new(to.x, self.y - dx)
            }
        } else if dy < 0 && dy > -dx {
            if left_turn {
                IntPoint::new(self.x - dy, to.y)
            } else {
                IntPoint::new(to.x + dy, self.y)
            }
        } else {
            return None;
        };
        Some(r)
    }

    pub fn ninety_degree_corner(&self, to: &IntPoint, left_turn: bool) -> Option<IntPoint> {
        let dx = to.x - self.x;
        let dy = to.y - self.y;
        if dx > 0 && dy > 0 || dx < 0 && dy < 0 {
            Some(if left_turn {
                IntPoint::new(to.x, self.y)
            } else {
                IntPoint::new(self.x, to.y)
            })
        } else if dx < 0 && dy > 0 || dx > 0 && dy < 0 {
            Some(if left_turn {
                IntPoint::new(self.x, to.y)
            } else {
                IntPoint::new(to.x, self.y)
            })
        } else {
            None
        }
    }

    pub fn turn_90_degree(&self, factor: i32, pole: &IntPoint) -> IntPoint {
        let v = self.difference_by(pole).turn_90_degree(factor);
        pole.translate_by_int(v)
    }

    pub fn mirror_vertical(&self, pole: &IntPoint) -> IntPoint {
        let v = self.difference_by(pole);
        pole.translate_by_int(IntVector::new(-v.x, v.y))
    }

    pub fn mirror_horizontal(&self, pole: &IntPoint) -> IntPoint {
        let v = self.difference_by(pole);
        pole.translate_by_int(IntVector::new(v.x, -v.y))
    }
}

impl RationalPoint {
    pub fn new(x: BigInt, y: BigInt, z: BigInt) -> RationalPoint {
        assert!(
            !z.is_negative(),
            "RationalPoint: z is expected to be >= 0"
        );
        RationalPoint { x, y, z }
    }

    pub fn from_int(p: &IntPoint) -> RationalPoint {
        RationalPoint {
            x: BigInt::from(p.x),
            y: BigInt::from(p.y),
            z: BigInt::one(),
        }
    }

    pub fn to_float(&self) -> FloatPoint {
        let xd = self.x.to_f64().unwrap_or(f64::MAX);
        let yd = self.y.to_f64().unwrap_or(f64::MAX);
        let zd = self.z.to_f64().unwrap_or(f64::MAX);
        if zd == 0.0 {
            FloatPoint::new(f32::MAX as f64, f32::MAX as f64)
        } else {
            FloatPoint::new(xd / zd, yd / zd)
        }
    }

    pub fn is_infinite(&self) -> bool {
        self.z.is_zero()
    }

    fn compare_x_rational(&self, other: &RationalPoint) -> Ordering {
        (&self.x * &other.z).cmp(&(&other.x * &self.z))
    }

    fn compare_x_int(&self, other: &IntPoint) -> Ordering {
        self.x.cmp(&(&self.z * BigInt::from(other.x)))
    }

    fn compare_y_rational(&self, other: &RationalPoint) -> Ordering {
        (&self.y * &other.z).cmp(&(&other.y * &self.z))
    }

    fn compare_y_int(&self, other: &IntPoint) -> Ordering {
        self.y.cmp(&(&self.z * BigInt::from(other.y)))
    }

    pub fn perpendicular_projection(&self, line: &Line) -> Point {
        let v = line.b.difference_by(&line.a);
        let vxvx = BigInt::from(v.x as i64 * v.x as i64);
        let vyvy = BigInt::from(v.y as i64 * v.y as i64);
        let vxvy = BigInt::from(v.x as i64 * v.y as i64);
        let mut denominator = &vxvx + &vyvy;
        let det = BigInt::from(line.a.determinant(&line.b));
        let mut proj_x = &vxvx * &self.x + &vxvy * &self.y + &det * BigInt::from(v.y) * &self.z;
        let mut proj_y = &vxvy * &self.x + &vyvy * &self.y + &det * BigInt::from(v.x) * &self.z;
        if !denominator.is_zero() {
            if denominator.is_negative() {
                denominator = -denominator;
                proj_x = -proj_x;
                proj_y = -proj_y;
            }
            if bigint_aux::is_divisible(&proj_x, &denominator)
                && bigint_aux::is_divisible(&proj_y, &denominator)
            {
                proj_x = &proj_x / &denominator;
                proj_y = &proj_y / &denominator;
                if proj_x.abs() <= *CRIT_INT_BIG && proj_y.abs() <= *CRIT_INT_BIG {
                    return Point::Int(IntPoint::new(
                        proj_x.to_i32().unwrap(),
                        proj_y.to_i32().unwrap(),
                    ));
                }
                denominator = BigInt::one();
            }
        }
        Point::Rational(RationalPoint::new(proj_x, proj_y, denominator))
    }
}

impl PartialEq for RationalPoint {
    fn eq(&self, other: &Self) -> bool {
        bigint_aux::determinant(&self.x, &other.x, &self.z, &other.z).is_zero()
            && bigint_aux::determinant(&self.y, &other.y, &self.z, &other.z).is_zero()
    }
}

impl PartialEq for Point {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Point::Int(a), Point::Int(b)) => a == b,
            (Point::Rational(a), Point::Rational(b)) => a == b,
            _ => false,
        }
    }
}

impl From<IntPoint> for Point {
    fn from(p: IntPoint) -> Point {
        Point::Int(p)
    }
}

impl Point {
    pub const ZERO: Point = Point::Int(IntPoint::ZERO);

    pub fn new_int(x: i32, y: i32) -> Point {
        let p = IntPoint::new(x, y);
        if x.unsigned_abs() > CRIT_INT as u32 || y.unsigned_abs() > CRIT_INT as u32 {
            return Point::Rational(RationalPoint::from_int(&p));
        }
        Point::Int(p)
    }

    pub fn new_rational(mut x: BigInt, mut y: BigInt, mut z: BigInt) -> Point {
        if z.is_negative() {
            x = -x;
            y = -y;
            z = -z;
        }
        if !z.is_zero() && bigint_aux::is_divisible(&x, &z) && bigint_aux::is_divisible(&y, &z) {
            x = &x / &z;
            y = &y / &z;
            z = BigInt::one();
        }
        if z.is_one() && x.abs() <= *CRIT_INT_BIG && y.abs() <= *CRIT_INT_BIG {
            return Point::Int(IntPoint::new(x.to_i32().unwrap(), y.to_i32().unwrap()));
        }
        Point::Rational(RationalPoint::new(x, y, z))
    }

    /// Returns the integer point, if this point is integer.
    #[inline]
    pub fn as_int(&self) -> Option<IntPoint> {
        match self {
            Point::Int(p) => Some(*p),
            Point::Rational(_) => None,
        }
    }

    #[inline]
    pub fn is_int(&self) -> bool {
        matches!(self, Point::Int(_))
    }

    /// Returns the integer point; rational points are rounded to the nearest integer point.
    /// Use only where the Java code casts to `IntPoint`.
    #[inline]
    pub fn to_int(&self) -> IntPoint {
        match self {
            Point::Int(p) => *p,
            Point::Rational(_) => self.to_float().round(),
        }
    }

    pub fn translate_by(&self, v: &Vector) -> Point {
        match (self, v) {
            (Point::Int(p), Vector::Int(iv)) => {
                if iv.is_zero() {
                    return self.clone();
                }
                Point::Int(p.translate_by_int(*iv))
            }
            (Point::Int(p), Vector::Rational(rv)) => rv.add_to_int(p),
            (Point::Rational(p), Vector::Int(iv)) => {
                RationalVector::from_int(iv).add_to_rational(p)
            }
            (Point::Rational(p), Vector::Rational(rv)) => rv.add_to_rational(p),
        }
    }

    pub fn difference_by(&self, other: &Point) -> Vector {
        match (self, other) {
            (Point::Int(a), Point::Int(b)) => Vector::Int(a.difference_by(b)),
            (Point::Rational(a), Point::Int(b)) => {
                let (x, y, z) = bigint_aux::add_rational_coordinates(
                    &a.x,
                    &a.y,
                    &a.z,
                    &BigInt::from(-(b.x as i64)),
                    &BigInt::from(-(b.y as i64)),
                    &BigInt::one(),
                );
                Vector::Rational(RationalVector::new(x, y, z))
            }
            (Point::Rational(a), Point::Rational(b)) => {
                let (x, y, z) = bigint_aux::add_rational_coordinates(
                    &a.x,
                    &a.y,
                    &a.z,
                    &(-&b.x),
                    &(-&b.y),
                    &b.z,
                );
                Vector::Rational(RationalVector::new(x, y, z))
            }
            (Point::Int(_), Point::Rational(_)) => other.difference_by(self).negate(),
        }
    }

    pub fn to_float(&self) -> FloatPoint {
        match self {
            Point::Int(p) => p.to_float(),
            Point::Rational(p) => p.to_float(),
        }
    }

    pub fn get_id(&self) -> i32 {
        match self {
            Point::Int(p) => p.get_id(),
            Point::Rational(p) => {
                // only used for hashing purposes
                let fp = p.to_float();
                31i32
                    .wrapping_mul(fp.x as i32)
                    .wrapping_add(fp.y as i32)
            }
        }
    }

    pub fn is_infinite(&self) -> bool {
        match self {
            Point::Int(_) => false,
            Point::Rational(p) => p.is_infinite(),
        }
    }

    pub fn surrounding_box(&self) -> IntBox {
        match self {
            Point::Int(p) => p.surrounding_box(),
            Point::Rational(_) => {
                let fp = self.to_float();
                IntBox::new(
                    fp.x.floor() as i32,
                    fp.y.floor() as i32,
                    fp.x.ceil() as i32,
                    fp.y.ceil() as i32,
                )
            }
        }
    }

    pub fn surrounding_octagon(&self) -> IntOctagon {
        match self {
            Point::Int(p) => p.surrounding_octagon(),
            Point::Rational(_) => {
                let fp = self.to_float();
                let lx = fp.x.floor() as i32;
                let ly = fp.y.floor() as i32;
                let rx = fp.x.ceil() as i32;
                let uy = fp.y.ceil() as i32;
                let tmp = fp.x - fp.y;
                let ulx = tmp.floor() as i32;
                let lrx = tmp.ceil() as i32;
                let tmp = fp.x + fp.y;
                let llx = tmp.floor() as i32;
                let urx = tmp.ceil() as i32;
                IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx)
            }
        }
    }

    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        match self {
            Point::Int(p) => p.is_contained_in(b),
            Point::Rational(p) => {
                if p.x < BigInt::from(b.ll.x) * &p.z {
                    return false;
                }
                if p.y < BigInt::from(b.ll.y) * &p.z {
                    return false;
                }
                if p.x > BigInt::from(b.ur.x) * &p.z {
                    return false;
                }
                p.y <= BigInt::from(b.ur.y) * &p.z
            }
        }
    }

    pub fn side_of_line(&self, line: &Line) -> Side {
        match self {
            Point::Int(p) => p.side_of_line(line),
            Point::Rational(_) => self.side_of(&Point::Int(line.a), &Point::Int(line.b)),
        }
    }

    /// The side of this point with respect to the line from `p1` to `p2`.
    pub fn side_of(&self, p1: &Point, p2: &Point) -> Side {
        match self {
            Point::Int(_) => {
                let v1 = self.difference_by(p1);
                let v2 = p2.difference_by(p1);
                v1.side_of(&v2)
            }
            Point::Rational(r) => {
                let (ax, ay, az) = coordinates(p1);
                let v1 = bigint_aux::add_rational_coordinates(
                    &r.x, &r.y, &r.z, &(-&ax), &(-&ay), &az,
                );
                if let (Point::Int(i1), Point::Int(i2)) = (p1, p2) {
                    let cross = &v1.1 * BigInt::from(i2.x as i64 - i1.x as i64)
                        - &v1.0 * BigInt::from(i2.y as i64 - i1.y as i64);
                    return Side::of_sign(sign(&cross));
                }
                let (bx, by, bz) = coordinates(p2);
                let v2 =
                    bigint_aux::add_rational_coordinates(&bx, &by, &bz, &(-&ax), &(-&ay), &az);
                let cross = &v1.1 * &v2.0 - &v1.0 * &v2.1;
                Side::of_sign(sign(&cross))
            }
        }
    }

    pub fn perpendicular_projection(&self, line: &Line) -> Point {
        match self {
            Point::Int(p) => p.perpendicular_projection(line),
            Point::Rational(p) => p.perpendicular_projection(line),
        }
    }

    pub fn perpendicular_direction(&self, line: &Line) -> super::Direction {
        let side = self.side_of_line(line);
        if side == Side::Collinear {
            return super::Direction::NULL;
        }
        if side == Side::OnTheRight {
            super::Direction::Int(line.direction().turn_45_degree(2))
        } else {
            super::Direction::Int(line.direction().turn_45_degree(6))
        }
    }

    pub fn compare_x(&self, other: &Point) -> Ordering {
        match (self, other) {
            (Point::Int(a), Point::Int(b)) => a.x.cmp(&b.x),
            (Point::Rational(a), Point::Int(b)) => a.compare_x_int(b),
            (Point::Rational(a), Point::Rational(b)) => a.compare_x_rational(b),
            (Point::Int(_), Point::Rational(_)) => other.compare_x(self).reverse(),
        }
    }

    pub fn compare_y(&self, other: &Point) -> Ordering {
        match (self, other) {
            (Point::Int(a), Point::Int(b)) => a.y.cmp(&b.y),
            (Point::Rational(a), Point::Int(b)) => a.compare_y_int(b),
            (Point::Rational(a), Point::Rational(b)) => a.compare_y_rational(b),
            (Point::Int(_), Point::Rational(_)) => other.compare_y(self).reverse(),
        }
    }

    pub fn compare_xy(&self, other: &Point) -> Ordering {
        let r = self.compare_x(other);
        if r == Ordering::Equal {
            self.compare_y(other)
        } else {
            r
        }
    }

    pub fn turn_90_degree(&self, factor: i32, pole: &Point) -> Point {
        let v = self.difference_by(pole).turn_90_degree(factor);
        pole.translate_by(&v)
    }

    pub fn mirror_vertical(&self, pole: &Point) -> Point {
        let v = self.difference_by(pole).mirror_at_y_axis();
        pole.translate_by(&v)
    }

    pub fn mirror_horizontal(&self, pole: &Point) -> Point {
        let v = self.difference_by(pole).mirror_at_x_axis();
        pole.translate_by(&v)
    }
}

fn coordinates(p: &Point) -> (BigInt, BigInt, BigInt) {
    match p {
        Point::Int(p) => (BigInt::from(p.x), BigInt::from(p.y), BigInt::one()),
        Point::Rational(p) => (p.x.clone(), p.y.clone(), p.z.clone()),
    }
}

#[inline]
pub(crate) fn sign(b: &BigInt) -> i32 {
    if b.is_positive() {
        1
    } else if b.is_negative() {
        -1
    } else {
        0
    }
}
