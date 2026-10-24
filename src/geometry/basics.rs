//! Basic enums and constants of the planar geometry (Side, Signum, Limits).

use num_bigint::BigInt;
use std::sync::LazyLock;

/// Coordinates with an absolute value above this limit are not representable by `IntPoint`
/// without risking overflow in exact calculations.
pub const CRIT_INT: i32 = 33_554_432;
pub const CRIT_DOUBLE: f64 = 9_007_199_254_740_992.0;
pub static CRIT_INT_BIG: LazyLock<BigInt> = LazyLock::new(|| BigInt::from(CRIT_INT));
pub const SQRT2: f64 = std::f64::consts::SQRT_2;

/// Position of a geometric object relative to a line or vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    OnTheLeft,
    OnTheRight,
    Collinear,
}

impl Side {
    pub fn of(value: f64) -> Side {
        if value > 0.0 {
            Side::OnTheLeft
        } else if value < 0.0 {
            Side::OnTheRight
        } else {
            Side::Collinear
        }
    }

    pub fn of_sign(sign: i32) -> Side {
        Side::of(sign as f64)
    }

    pub fn negate(self) -> Side {
        match self {
            Side::OnTheLeft => Side::OnTheRight,
            Side::OnTheRight => Side::OnTheLeft,
            Side::Collinear => Side::Collinear,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Signum {
    Positive,
    Negative,
    Zero,
}

impl Signum {
    pub fn of(value: f64) -> Signum {
        if value > 0.0 {
            Signum::Positive
        } else if value < 0.0 {
            Signum::Negative
        } else {
            Signum::Zero
        }
    }

    pub fn as_int(value: f64) -> i32 {
        if value > 0.0 {
            1
        } else if value < 0.0 {
            -1
        } else {
            0
        }
    }

    pub fn negate(self) -> Signum {
        match self {
            Signum::Positive => Signum::Negative,
            Signum::Negative => Signum::Positive,
            Signum::Zero => Signum::Zero,
        }
    }
}

/// Java's `Math.round` for doubles (round half up, saturating to i64), cast to i32 like `(int)`.
#[inline]
pub fn java_round(x: f64) -> i32 {
    (x + 0.5).floor() as i64 as i32
}

/// Java's `Math.round` returning a long.
#[inline]
pub fn java_round_l(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

/// Java's `Math.rint` (round half to even).
#[inline]
pub fn rint(x: f64) -> f64 {
    x.round_ties_even()
}

/// Exact helpers for big integer arithmetic.
pub mod bigint_aux {
    use num_bigint::BigInt;
    use num_traits::Signed;

    pub fn determinant(x1: &BigInt, y1: &BigInt, x2: &BigInt, y2: &BigInt) -> BigInt {
        x1 * y2 - x2 * y1
    }

    pub fn add_rational_coordinates(
        ax: &BigInt,
        ay: &BigInt,
        az: &BigInt,
        bx: &BigInt,
        by: &BigInt,
        bz: &BigInt,
    ) -> (BigInt, BigInt, BigInt) {
        if az == bz {
            return (ax + bx, ay + by, az.clone());
        }
        (ax * bz + bx * az, ay * bz + by * az, az * bz)
    }

    /// Greatest common divisor of two non negative integers.
    pub fn binary_gcd(a: i32, b: i32) -> i32 {
        let (mut a, mut b) = (a.unsigned_abs(), b.unsigned_abs());
        if b == 0 {
            return a as i32;
        }
        if a == 0 {
            return b as i32;
        }
        while b != 0 {
            let t = a % b;
            a = b;
            b = t;
        }
        a as i32
    }

    /// Java `BigInteger.mod` with a positive modulus: result is always non negative.
    pub fn is_divisible(value: &BigInt, divisor: &BigInt) -> bool {
        use num_integer::Integer;
        value.mod_floor(&divisor.abs()) == BigInt::from(0)
    }

    /// Java `BigInteger.divide` truncates towards zero.
    pub fn div_trunc(a: &BigInt, b: &BigInt) -> BigInt {
        a / b
    }
}

/// Reimplementation of `java.util.Random` to reproduce the deterministic behaviour of the
/// original implementation (used for splitting polygons into convex pieces).
#[derive(Clone, Debug)]
pub struct JavaRandom {
    seed: u64,
}

impl JavaRandom {
    const MULTIPLIER: u64 = 0x5_DEEC_E66D;
    const MASK: u64 = (1 << 48) - 1;

    pub fn new(seed: i64) -> JavaRandom {
        JavaRandom {
            seed: (seed as u64 ^ Self::MULTIPLIER) & Self::MASK,
        }
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed as u64 ^ Self::MULTIPLIER) & Self::MASK;
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(Self::MULTIPLIER).wrapping_add(0xB)) & Self::MASK;
        (self.seed >> (48 - bits)) as i64 as i32
    }

    pub fn next_int(&mut self, bound: i32) -> i32 {
        assert!(bound > 0);
        if bound & -bound == bound {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        let mut bits;
        let mut val;
        loop {
            bits = self.next(31);
            val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                break;
            }
        }
        val
    }

    pub fn next_double(&mut self) -> f64 {
        let a = self.next(26) as i64;
        let b = self.next(27) as i64;
        ((a << 27) + b) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}
