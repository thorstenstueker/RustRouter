//! Exact vectors and directions of the plane.

use super::basics::{bigint_aux, Side, Signum, CRIT_INT, CRIT_INT_BIG};
use super::point::sign;
use super::{FloatPoint, IntPoint, Point, RationalPoint};
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct IntVector {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug)]
pub struct RationalVector {
    pub x: BigInt,
    pub y: BigInt,
    pub z: BigInt,
}

#[derive(Clone, Debug)]
pub enum Vector {
    Int(IntVector),
    Rational(RationalVector),
}

impl IntVector {
    pub const ZERO: IntVector = IntVector { x: 0, y: 0 };

    #[inline]
    pub const fn new(x: i32, y: i32) -> IntVector {
        IntVector { x, y }
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.x == 0 && self.y == 0
    }

    #[inline]
    pub fn negate(&self) -> IntVector {
        IntVector::new(-self.x, -self.y)
    }

    #[inline]
    pub fn is_orthogonal(&self) -> bool {
        self.x == 0 || self.y == 0
    }

    #[inline]
    pub fn is_diagonal(&self) -> bool {
        self.x.unsigned_abs() == self.y.unsigned_abs()
    }

    #[inline]
    pub fn determinant(&self, other: &IntVector) -> i64 {
        self.x as i64 * other.y as i64 - self.y as i64 * other.x as i64
    }

    pub fn turn_90_degree(&self, factor: i32) -> IntVector {
        match factor.rem_euclid(4) {
            0 => *self,
            1 => IntVector::new(-self.y, self.x),
            2 => IntVector::new(-self.x, -self.y),
            _ => IntVector::new(self.y, -self.x),
        }
    }

    /// Side of `self` relative to `other` (Java: `this.sideOf(other)`).
    #[inline]
    pub fn side_of(&self, other: &IntVector) -> Side {
        let determinant = self.x as f64 * other.y as f64 - self.y as f64 * other.x as f64;
        Side::of(determinant).negate()
    }

    #[inline]
    pub fn projection(&self, other: &IntVector) -> Signum {
        Signum::of(self.x as f64 * other.x as f64 + self.y as f64 * other.y as f64)
    }

    #[inline]
    pub fn scalar_product(&self, other: &IntVector) -> f64 {
        self.x as f64 * other.x as f64 + self.y as f64 * other.y as f64
    }

    #[inline]
    pub fn to_float(&self) -> FloatPoint {
        FloatPoint::new(self.x as f64, self.y as f64)
    }

    pub fn to_normalized_direction(&self) -> IntDirection {
        let mut dx = self.x;
        let mut dy = self.y;
        let gcd = bigint_aux::binary_gcd(dx.abs(), dy.abs());
        if gcd > 1 {
            dx /= gcd;
            dy /= gcd;
        }
        IntDirection::new(dx, dy)
    }

    pub fn change_length_approx(&self, length: f64) -> IntVector {
        let p = self.to_float().change_size(length).round();
        IntVector::new(p.x, p.y)
    }
}

impl RationalVector {
    pub fn new(x: BigInt, y: BigInt, z: BigInt) -> RationalVector {
        if !z.is_negative() {
            RationalVector { x, y, z }
        } else {
            RationalVector { x: -x, y: -y, z: -z }
        }
    }

    pub fn from_int(v: &IntVector) -> RationalVector {
        RationalVector {
            x: BigInt::from(v.x),
            y: BigInt::from(v.y),
            z: BigInt::one(),
        }
    }

    pub fn to_float(&self) -> FloatPoint {
        let xd = self.x.to_f64().unwrap_or(f64::MAX);
        let yd = self.y.to_f64().unwrap_or(f64::MAX);
        let zd = self.z.to_f64().unwrap_or(f64::MAX);
        FloatPoint::new(xd / zd, yd / zd)
    }

    pub fn add_to_int(&self, p: &IntPoint) -> Point {
        let nx = &self.z * BigInt::from(p.x) + &self.x;
        let ny = &self.z * BigInt::from(p.y) + &self.y;
        Point::Rational(RationalPoint::new(nx, ny, self.z.clone()))
    }

    pub fn add_to_rational(&self, p: &RationalPoint) -> Point {
        let (x, y, z) =
            bigint_aux::add_rational_coordinates(&self.x, &self.y, &self.z, &p.x, &p.y, &p.z);
        Point::Rational(RationalPoint::new(x, y, z))
    }
}

impl PartialEq for RationalVector {
    fn eq(&self, o: &Self) -> bool {
        bigint_aux::determinant(&self.x, &o.x, &self.z, &o.z).is_zero()
            && bigint_aux::determinant(&self.y, &o.y, &self.z, &o.z).is_zero()
    }
}

impl PartialEq for Vector {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Vector::Int(a), Vector::Int(b)) => a == b,
            (Vector::Rational(a), Vector::Rational(b)) => a == b,
            _ => false,
        }
    }
}

impl From<IntVector> for Vector {
    fn from(v: IntVector) -> Vector {
        Vector::Int(v)
    }
}

fn big_xy(v: &Vector) -> (BigInt, BigInt, BigInt) {
    match v {
        Vector::Int(i) => (BigInt::from(i.x), BigInt::from(i.y), BigInt::one()),
        Vector::Rational(r) => (r.x.clone(), r.y.clone(), r.z.clone()),
    }
}

impl Vector {
    pub const ZERO: Vector = Vector::Int(IntVector::ZERO);

    pub fn new_int(x: i32, y: i32) -> Vector {
        let v = IntVector::new(x, y);
        if x.unsigned_abs() > CRIT_INT as u32 || y.unsigned_abs() > CRIT_INT as u32 {
            return Vector::Rational(RationalVector::from_int(&v));
        }
        Vector::Int(v)
    }

    pub fn new_rational(mut x: BigInt, mut y: BigInt, mut z: BigInt) -> Vector {
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
            return Vector::Int(IntVector::new(x.to_i32().unwrap(), y.to_i32().unwrap()));
        }
        Vector::Rational(RationalVector::new(x, y, z))
    }

    #[inline]
    pub fn as_int(&self) -> Option<IntVector> {
        match self {
            Vector::Int(v) => Some(*v),
            _ => None,
        }
    }

    /// Integer vector; rational vectors are rounded. Use where Java casts to `IntVector`.
    pub fn to_int(&self) -> IntVector {
        match self {
            Vector::Int(v) => *v,
            Vector::Rational(r) => {
                let p = r.to_float().round();
                IntVector::new(p.x, p.y)
            }
        }
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Vector::Int(v) => v.is_zero(),
            Vector::Rational(r) => r.x.is_zero() && r.y.is_zero(),
        }
    }

    pub fn negate(&self) -> Vector {
        match self {
            Vector::Int(v) => Vector::Int(v.negate()),
            Vector::Rational(r) => {
                Vector::Rational(RationalVector::new(-&r.x, -&r.y, r.z.clone()))
            }
        }
    }

    pub fn add(&self, other: &Vector) -> Vector {
        match (self, other) {
            (Vector::Int(a), Vector::Int(b)) => {
                Vector::Int(IntVector::new(a.x.wrapping_add(b.x), a.y.wrapping_add(b.y)))
            }
            _ => {
                let (ax, ay, az) = big_xy(self);
                let (bx, by, bz) = big_xy(other);
                let (x, y, z) = bigint_aux::add_rational_coordinates(&ax, &ay, &az, &bx, &by, &bz);
                Vector::Rational(RationalVector::new(x, y, z))
            }
        }
    }

    /// The side of `self` with respect to `other`.
    pub fn side_of(&self, other: &Vector) -> Side {
        match (self, other) {
            (Vector::Int(a), Vector::Int(b)) => a.side_of(b),
            _ => {
                // RationalVector.sideOf(other): det = y*other.x - x*other.y (homogeneous z > 0)
                let (ax, ay, _) = big_xy(self);
                let (bx, by, _) = big_xy(other);
                Side::of_sign(sign(&(&ay * &bx - &ax * &by)))
            }
        }
    }

    pub fn is_orthogonal(&self) -> bool {
        match self {
            Vector::Int(v) => v.is_orthogonal(),
            Vector::Rational(r) => r.x.is_zero() || r.y.is_zero(),
        }
    }

    pub fn is_diagonal(&self) -> bool {
        match self {
            Vector::Int(v) => v.is_diagonal(),
            Vector::Rational(r) => r.x.abs() == r.y.abs(),
        }
    }

    pub fn is_multiple_of_45_degree(&self) -> bool {
        self.is_orthogonal() || self.is_diagonal()
    }

    pub fn projection(&self, other: &Vector) -> Signum {
        match (self, other) {
            (Vector::Int(a), Vector::Int(b)) => a.projection(b),
            _ => {
                let (ax, ay, _) = big_xy(self);
                let (bx, by, _) = big_xy(other);
                Signum::of(sign(&(&ax * &bx + &ay * &by)) as f64)
            }
        }
    }

    pub fn scalar_product(&self, other: &Vector) -> f64 {
        match (self, other) {
            (Vector::Int(a), Vector::Int(b)) => a.scalar_product(b),
            _ => {
                let v1 = self.to_float();
                let v2 = other.to_float();
                v1.x * v2.x + v1.y * v2.y
            }
        }
    }

    pub fn to_float(&self) -> FloatPoint {
        match self {
            Vector::Int(v) => v.to_float(),
            Vector::Rational(r) => r.to_float(),
        }
    }

    pub fn turn_90_degree(&self, factor: i32) -> Vector {
        match self {
            Vector::Int(v) => Vector::Int(v.turn_90_degree(factor)),
            Vector::Rational(r) => {
                let (x, y, z) = (&r.x, &r.y, r.z.clone());
                Vector::Rational(match factor.rem_euclid(4) {
                    0 => r.clone(),
                    1 => RationalVector::new(-y, x.clone(), z),
                    2 => RationalVector::new(-x, -y, z),
                    _ => RationalVector::new(y.clone(), -x, z),
                })
            }
        }
    }

    pub fn mirror_at_y_axis(&self) -> Vector {
        match self {
            Vector::Int(v) => Vector::Int(IntVector::new(-v.x, v.y)),
            Vector::Rational(r) => {
                Vector::Rational(RationalVector::new(-&r.x, r.y.clone(), r.z.clone()))
            }
        }
    }

    pub fn mirror_at_x_axis(&self) -> Vector {
        match self {
            Vector::Int(v) => Vector::Int(IntVector::new(v.x, -v.y)),
            Vector::Rational(r) => {
                Vector::Rational(RationalVector::new(r.x.clone(), -&r.y, r.z.clone()))
            }
        }
    }

    pub fn length_approx(&self) -> f64 {
        self.to_float().size()
    }

    pub fn cos_angle(&self, other: &Vector) -> f64 {
        let r = self.scalar_product(other);
        r / (self.to_float().size() * other.to_float().size())
    }

    pub fn angle_approx_to(&self, other: &Vector) -> f64 {
        let mut result = self.cos_angle(other).acos();
        if self.side_of(other) == Side::OnTheLeft {
            result = -result;
        }
        result
    }

    pub fn angle_approx(&self) -> f64 {
        Vector::Int(IntVector::new(1, 0)).angle_approx_to(self)
    }

    pub fn change_length_approx(&self, length: f64) -> Vector {
        match self {
            Vector::Int(v) => Vector::Int(v.change_length_approx(length)),
            Vector::Rational(_) => self.clone(),
        }
    }

    pub fn to_normalized_direction(&self) -> Direction {
        match self {
            Vector::Int(v) => Direction::Int(v.to_normalized_direction()),
            Vector::Rational(r) => {
                let gcd = r.x.gcd(&r.y);
                let (dx, dy) = if gcd.is_zero() {
                    (r.x.clone(), r.y.clone())
                } else {
                    (&r.x / &gcd, &r.y / &gcd)
                };
                if dx.abs() <= *CRIT_INT_BIG && dy.abs() <= *CRIT_INT_BIG {
                    return Direction::Int(IntDirection::new(
                        dx.to_i32().unwrap(),
                        dy.to_i32().unwrap(),
                    ));
                }
                Direction::BigInt(BigIntDirection { x: dx, y: dy })
            }
        }
    }
}

/// A direction given by an integer vector with coprime coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct IntDirection {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigIntDirection {
    pub x: BigInt,
    pub y: BigInt,
}

#[derive(Clone, Debug)]
pub enum Direction {
    Int(IntDirection),
    BigInt(BigIntDirection),
}

impl IntDirection {
    pub const NULL: IntDirection = IntDirection { x: 0, y: 0 };
    pub const RIGHT: IntDirection = IntDirection { x: 1, y: 0 };
    pub const RIGHT45: IntDirection = IntDirection { x: 1, y: 1 };
    pub const UP: IntDirection = IntDirection { x: 0, y: 1 };
    pub const UP45: IntDirection = IntDirection { x: -1, y: 1 };
    pub const LEFT: IntDirection = IntDirection { x: -1, y: 0 };
    pub const LEFT45: IntDirection = IntDirection { x: -1, y: -1 };
    pub const DOWN: IntDirection = IntDirection { x: 0, y: -1 };
    pub const DOWN45: IntDirection = IntDirection { x: 1, y: -1 };

    #[inline]
    pub const fn new(x: i32, y: i32) -> IntDirection {
        IntDirection { x, y }
    }

    #[inline]
    pub fn get_vector(&self) -> IntVector {
        IntVector::new(self.x, self.y)
    }

    #[inline]
    pub fn is_orthogonal(&self) -> bool {
        self.x == 0 || self.y == 0
    }

    #[inline]
    pub fn is_diagonal(&self) -> bool {
        self.x.unsigned_abs() == self.y.unsigned_abs()
    }

    pub fn turn_45_degree(&self, factor: i32) -> IntDirection {
        let (x, y) = (self.x, self.y);
        match factor.rem_euclid(8) {
            0 => IntDirection::new(x, y),
            1 => IntDirection::new(x - y, x + y),
            2 => IntDirection::new(-y, x),
            3 => IntDirection::new(-x - y, x - y),
            4 => IntDirection::new(-x, -y),
            5 => IntDirection::new(y - x, -x - y),
            6 => IntDirection::new(y, -x),
            _ => IntDirection::new(x + y, y - x),
        }
    }

    #[inline]
    pub fn opposite(&self) -> IntDirection {
        IntDirection::new(-self.x, -self.y)
    }

    #[inline]
    pub fn determinant(&self, other: &IntDirection) -> f64 {
        self.x as f64 * other.y as f64 - self.y as f64 * other.x as f64
    }

    /// Compares the angles of the directions with the positive x-axis.
    pub fn compare_to(&self, other: &IntDirection) -> Ordering {
        compare_direction_signs(self.x.signum(), self.y.signum(), other.x.signum(), other.y.signum())
            .unwrap_or_else(|| {
                let determinant = other.x as f64 * self.y as f64 - other.y as f64 * self.x as f64;
                Signum::as_int(determinant).cmp(&0)
            })
    }
}

/// Common sign based part of the direction comparison. Returns None, if the
/// determinant has to decide.
#[inline]
fn compare_direction_signs(x1: i32, y1: i32, x2: i32, y2: i32) -> Option<Ordering> {
    if y1 > 0 {
        if y2 < 0 {
            return Some(Ordering::Less);
        }
        if y2 == 0 {
            if x2 > 0 {
                return Some(Ordering::Greater);
            }
            return Some(Ordering::Less);
        }
    } else if y1 < 0 {
        if y2 >= 0 {
            return Some(Ordering::Greater);
        }
    } else {
        if x1 > 0 {
            if y2 != 0 || x2 < 0 {
                return Some(Ordering::Less);
            }
            return Some(Ordering::Equal);
        }
        if y2 > 0 || y2 == 0 && x2 > 0 {
            return Some(Ordering::Greater);
        }
        if y2 < 0 {
            return Some(Ordering::Less);
        }
        return Some(Ordering::Equal);
    }
    None
}

impl BigIntDirection {
    fn from_int(d: &IntDirection) -> BigIntDirection {
        BigIntDirection {
            x: BigInt::from(d.x),
            y: BigInt::from(d.y),
        }
    }

    fn compare_to(&self, other: &BigIntDirection) -> Ordering {
        compare_direction_signs(sign(&self.x), sign(&self.y), sign(&other.x), sign(&other.y))
            .unwrap_or_else(|| sign(&(&self.y * &other.x - &self.x * &other.y)).cmp(&0))
    }
}

impl From<IntDirection> for Direction {
    fn from(d: IntDirection) -> Direction {
        Direction::Int(d)
    }
}

impl Direction {
    pub const NULL: Direction = Direction::Int(IntDirection::NULL);
    pub const RIGHT: Direction = Direction::Int(IntDirection::RIGHT);
    pub const RIGHT45: Direction = Direction::Int(IntDirection::RIGHT45);
    pub const UP: Direction = Direction::Int(IntDirection::UP);
    pub const UP45: Direction = Direction::Int(IntDirection::UP45);
    pub const LEFT: Direction = Direction::Int(IntDirection::LEFT);
    pub const LEFT45: Direction = Direction::Int(IntDirection::LEFT45);
    pub const DOWN: Direction = Direction::Int(IntDirection::DOWN);
    pub const DOWN45: Direction = Direction::Int(IntDirection::DOWN45);

    pub fn from_vector(v: &Vector) -> Direction {
        v.to_normalized_direction()
    }

    pub fn from_int_vector(v: IntVector) -> Direction {
        Direction::Int(v.to_normalized_direction())
    }

    /// Returns None, if `from == to`.
    pub fn from_points(from: &Point, to: &Point) -> Option<Direction> {
        if from == to {
            return None;
        }
        Some(Direction::from_vector(&to.difference_by(from)))
    }

    pub fn get_instance_approx(angle: f64) -> Direction {
        let scale = 10000.0;
        let x = super::basics::java_round(angle.cos() * scale);
        let y = super::basics::java_round(angle.sin() * scale);
        Direction::from_int_vector(IntVector::new(x, y))
    }

    #[inline]
    pub fn as_int(&self) -> Option<IntDirection> {
        match self {
            Direction::Int(d) => Some(*d),
            _ => None,
        }
    }

    pub fn get_vector(&self) -> Vector {
        match self {
            Direction::Int(d) => Vector::Int(d.get_vector()),
            Direction::BigInt(d) => {
                Vector::Rational(RationalVector::new(d.x.clone(), d.y.clone(), BigInt::one()))
            }
        }
    }

    pub fn is_orthogonal(&self) -> bool {
        match self {
            Direction::Int(d) => d.is_orthogonal(),
            Direction::BigInt(d) => d.x.is_zero() || d.y.is_zero(),
        }
    }

    pub fn is_diagonal(&self) -> bool {
        match self {
            Direction::Int(d) => d.is_diagonal(),
            Direction::BigInt(d) => d.x.abs() == d.y.abs(),
        }
    }

    pub fn is_multiple_of_45_degree(&self) -> bool {
        self.is_orthogonal() || self.is_diagonal()
    }

    pub fn turn_45_degree(&self, factor: i32) -> Direction {
        match self {
            Direction::Int(d) => Direction::Int(d.turn_45_degree(factor)),
            Direction::BigInt(_) => {
                log::warn!("BigIntDirection: turn_45_degree not yet implemented");
                self.clone()
            }
        }
    }

    pub fn opposite(&self) -> Direction {
        match self {
            Direction::Int(d) => Direction::Int(d.opposite()),
            Direction::BigInt(d) => Direction::BigInt(BigIntDirection {
                x: -&d.x,
                y: -&d.y,
            }),
        }
    }

    pub fn side_of(&self, other: &Direction) -> Side {
        self.get_vector().side_of(&other.get_vector())
    }

    pub fn projection(&self, other: &Direction) -> Signum {
        self.get_vector().projection(&other.get_vector())
    }

    pub fn middle_approx(&self, other: &Direction) -> Direction {
        let v1 = self.get_vector().to_float();
        let v2 = other.get_vector().to_float();
        let l1 = v1.size();
        let l2 = v2.size();
        let x = v1.x / l1 + v2.x / l2;
        let y = v1.y / l1 + v2.y / l2;
        let scale = 1000.0;
        Direction::from_int_vector(IntVector::new(
            super::basics::java_round(x * scale),
            super::basics::java_round(y * scale),
        ))
    }

    pub fn compare_to(&self, other: &Direction) -> Ordering {
        match (self, other) {
            (Direction::Int(a), Direction::Int(b)) => a.compare_to(b),
            (Direction::Int(a), Direction::BigInt(b)) => BigIntDirection::from_int(a).compare_to(b),
            (Direction::BigInt(a), Direction::Int(b)) => a.compare_to(&BigIntDirection::from_int(b)),
            (Direction::BigInt(a), Direction::BigInt(b)) => a.compare_to(b),
        }
    }

    /// Compares `p1` and `p2` by their angle starting from this direction counterclockwise.
    pub fn compare_from(&self, p1: &Direction, p2: &Direction) -> Ordering {
        if p1.compare_to(self) != Ordering::Less {
            if p2.compare_to(self) != Ordering::Less {
                p1.compare_to(p2)
            } else {
                Ordering::Less
            }
        } else if p2.compare_to(self) != Ordering::Less {
            Ordering::Greater
        } else {
            p1.compare_to(p2)
        }
    }

    pub fn angle_approx(&self) -> f64 {
        self.get_vector().angle_approx()
    }
}

impl PartialEq for Direction {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Direction::Int(a), Direction::Int(b)) => {
                if a == b {
                    return true;
                }
                a.determinant(b) == 0.0 && a.get_vector().projection(&b.get_vector()) == Signum::Positive
            }
            (Direction::BigInt(_), Direction::BigInt(_)) => {
                self.side_of(other) == Side::Collinear
                    && self.get_vector().projection(&other.get_vector()) == Signum::Positive
            }
            _ => false,
        }
    }
}

impl Eq for Direction {}

impl PartialOrd for Direction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Direction {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare_to(other)
    }
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = if self.compare_to(&Direction::RIGHT) == Ordering::Equal {
            "RIGHT"
        } else if self.compare_to(&Direction::RIGHT45) == Ordering::Equal {
            "UP-RIGHT"
        } else if self.compare_to(&Direction::UP) == Ordering::Equal {
            "UP"
        } else if self.compare_to(&Direction::UP45) == Ordering::Equal {
            "UP-LEFT"
        } else if self.compare_to(&Direction::LEFT) == Ordering::Equal {
            "LEFT"
        } else if self.compare_to(&Direction::LEFT45) == Ordering::Equal {
            "DOWN-LEFT"
        } else if self.compare_to(&Direction::DOWN) == Ordering::Equal {
            "DOWN"
        } else if self.compare_to(&Direction::DOWN45) == Ordering::Equal {
            "DOWN-RIGHT"
        } else if self.compare_to(&Direction::NULL) == Ordering::Equal {
            "NULL"
        } else {
            "UNKNOWN"
        };
        f.write_str(s)
    }
}
