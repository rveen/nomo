//! Physical dimensions as exponent vectors over the SI base dimensions.
//!
//! Dimensional analysis here is structural, not textual: `N` and `kg·m/s²` are
//! the same dimension because their exponent vectors are equal, not because
//! anything compares strings. That is what makes user-declared units fall out for
//! free — a declaration is just a name bound to a factor and a vector.
//!
//! # Why exponents are rational
//!
//! Integer exponents would be simpler, but they cannot express fracture
//! toughness, whose unit is `MPa·√m` — a genuine structural-engineering quantity
//! with a half-integer length exponent. Rationals also make `sqrt` total on any
//! dimension rather than only on even ones.

use core::fmt;

/// Number of SI base dimensions.
pub const N_BASE: usize = 7;

/// Index of each base dimension within a [`Dimension`]'s exponent array.
pub mod base {
    pub const LENGTH: usize = 0;
    pub const MASS: usize = 1;
    pub const TIME: usize = 2;
    pub const CURRENT: usize = 3;
    pub const TEMPERATURE: usize = 4;
    pub const AMOUNT: usize = 5;
    pub const LUMINOUS: usize = 6;
}

/// SI symbol for each base dimension, in array order.
pub const BASE_SYMBOLS: [&str; N_BASE] = ["m", "kg", "s", "A", "K", "mol", "cd"];

fn gcd(a: u32, b: u32) -> u32 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

/// A rational number, always normalised: `den > 0` and `gcd(|num|, den) == 1`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ratio {
    num: i32,
    den: u32,
}

impl Ratio {
    pub const ZERO: Ratio = Ratio { num: 0, den: 1 };
    pub const ONE: Ratio = Ratio { num: 1, den: 1 };

    pub fn new(num: i32, den: i32) -> Ratio {
        assert!(den != 0, "dimension exponent with zero denominator");
        let sign = if (num < 0) != (den < 0) { -1 } else { 1 };
        let n = num.unsigned_abs();
        let d = den.unsigned_abs();
        if n == 0 {
            return Ratio::ZERO;
        }
        let g = gcd(n, d);
        Ratio {
            num: sign * (n / g) as i32,
            den: d / g,
        }
    }

    pub const fn int(n: i32) -> Ratio {
        Ratio { num: n, den: 1 }
    }

    pub fn numerator(self) -> i32 {
        self.num
    }

    pub fn denominator(self) -> u32 {
        self.den
    }

    pub fn is_zero(self) -> bool {
        self.num == 0
    }

    pub fn is_integer(self) -> bool {
        self.den == 1
    }

    /// The integer value, if this ratio is one.
    pub fn as_int(self) -> Option<i32> {
        (self.den == 1).then_some(self.num)
    }

    pub fn add(self, other: Ratio) -> Ratio {
        Ratio::new(
            self.num * other.den as i32 + other.num * self.den as i32,
            (self.den * other.den) as i32,
        )
    }

    pub fn sub(self, other: Ratio) -> Ratio {
        self.add(other.neg())
    }

    pub fn neg(self) -> Ratio {
        Ratio {
            num: -self.num,
            den: self.den,
        }
    }

    pub fn mul(self, other: Ratio) -> Ratio {
        Ratio::new(self.num * other.num, (self.den * other.den) as i32)
    }

    pub fn to_f64(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }

    /// Recover the small rational a floating-point exponent was meant to be.
    ///
    /// Powers in a worksheet arrive as `f64` because that is what the parser
    /// produces, and only a rational exponent yields an exact dimension. The rule
    /// is: accept `x` if it lies within floating-point noise of `n/d` for some
    /// denominator up to 12, otherwise reject.
    ///
    /// Absorbing the noise is deliberate rather than sloppy. `0.1 + 0.2` is
    /// `0.30000000000000004`, and someone writing `x^(0.1+0.2)` means three
    /// tenths; refusing that would be pedantry with no upside. The tolerance is
    /// far tighter than the gap between any genuine irrational and a small
    /// fraction — π differs from 22/7 by more than a thousandth — so `x^π` is
    /// still correctly refused.
    pub fn from_f64(x: f64) -> Option<Ratio> {
        if !x.is_finite() {
            return None;
        }
        for den in 1..=12u32 {
            let scaled = x * f64::from(den);
            let rounded = scaled.round();
            if (scaled - rounded).abs() < 1e-9 && rounded.abs() < f64::from(i32::MAX) {
                return Some(Ratio::new(rounded as i32, den as i32));
            }
        }
        None
    }
}

impl fmt::Debug for Ratio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

impl fmt::Display for Ratio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// A physical dimension: one rational exponent per SI base dimension.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dimension {
    exps: [Ratio; N_BASE],
}

impl Dimension {
    pub const DIMENSIONLESS: Dimension = Dimension {
        exps: [Ratio::ZERO; N_BASE],
    };

    /// The dimension of a single base quantity, e.g. `Dimension::base(base::MASS)`.
    pub fn base(index: usize) -> Dimension {
        let mut exps = [Ratio::ZERO; N_BASE];
        exps[index] = Ratio::ONE;
        Dimension { exps }
    }

    pub fn from_exponents(exps: [Ratio; N_BASE]) -> Dimension {
        Dimension { exps }
    }

    pub fn exponents(&self) -> &[Ratio; N_BASE] {
        &self.exps
    }

    pub fn is_dimensionless(&self) -> bool {
        self.exps.iter().all(|e| e.is_zero())
    }

    /// Dimension of a product: exponents add.
    pub fn mul(&self, other: &Dimension) -> Dimension {
        Dimension {
            exps: core::array::from_fn(|i| self.exps[i].add(other.exps[i])),
        }
    }

    /// Dimension of a quotient: exponents subtract.
    pub fn div(&self, other: &Dimension) -> Dimension {
        Dimension {
            exps: core::array::from_fn(|i| self.exps[i].sub(other.exps[i])),
        }
    }

    /// Dimension of a power: exponents scale.
    pub fn pow(&self, e: Ratio) -> Dimension {
        Dimension {
            exps: core::array::from_fn(|i| self.exps[i].mul(e)),
        }
    }

    pub fn recip(&self) -> Dimension {
        Dimension::DIMENSIONLESS.div(self)
    }
}

impl fmt::Display for Dimension {
    /// SI base symbols with explicit exponents, positive first: `kg·m·s^-2`.
    ///
    /// Deliberately not an attempt to guess that this is a newton — naming
    /// derived units is the renderer's job, and an error message is clearer when
    /// it shows the dimension it actually computed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_dimensionless() {
            return write!(f, "dimensionless");
        }
        let mut first = true;
        for (i, e) in self.exps.iter().enumerate() {
            if e.is_zero() {
                continue;
            }
            if !first {
                write!(f, "·")?;
            }
            first = false;
            write!(f, "{}", BASE_SYMBOLS[i])?;
            if *e != Ratio::ONE {
                write!(f, "^{e}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length() -> Dimension {
        Dimension::base(base::LENGTH)
    }
    fn mass() -> Dimension {
        Dimension::base(base::MASS)
    }
    fn time() -> Dimension {
        Dimension::base(base::TIME)
    }

    #[test]
    fn ratio_normalises() {
        assert_eq!(Ratio::new(2, 4), Ratio::new(1, 2));
        assert_eq!(Ratio::new(-2, -4), Ratio::new(1, 2));
        assert_eq!(Ratio::new(2, -4), Ratio::new(-1, 2));
        assert_eq!(Ratio::new(0, 5), Ratio::ZERO);
    }

    #[test]
    fn ratio_arithmetic() {
        assert_eq!(Ratio::new(1, 2).add(Ratio::new(1, 3)), Ratio::new(5, 6));
        assert_eq!(Ratio::new(1, 2).sub(Ratio::new(1, 2)), Ratio::ZERO);
        assert_eq!(Ratio::new(1, 2).mul(Ratio::int(4)), Ratio::int(2));
    }

    #[test]
    fn ratio_from_f64_accepts_the_exponents_people_write() {
        assert_eq!(Ratio::from_f64(2.0), Some(Ratio::int(2)));
        assert_eq!(Ratio::from_f64(-3.0), Some(Ratio::int(-3)));
        assert_eq!(Ratio::from_f64(0.5), Some(Ratio::new(1, 2)));
        assert_eq!(Ratio::from_f64(1.0 / 3.0), Some(Ratio::new(1, 3)));
    }

    #[test]
    fn ratio_from_f64_sees_through_floating_point_noise() {
        // `0.1 + 0.2` is not 0.3, but someone writing it means three tenths.
        assert_eq!(Ratio::from_f64(0.1 + 0.2), Some(Ratio::new(3, 10)));
        assert_eq!(Ratio::from_f64(1.0 / 3.0 * 3.0), Some(Ratio::ONE));
    }

    #[test]
    fn ratio_from_f64_rejects_genuine_irrationals() {
        // π is more than a thousandth from 22/7, far outside the tolerance.
        assert_eq!(Ratio::from_f64(core::f64::consts::PI), None);
        assert_eq!(Ratio::from_f64(core::f64::consts::E), None);
        assert_eq!(Ratio::from_f64(2.0_f64.sqrt()), None);
        assert_eq!(Ratio::from_f64(f64::NAN), None);
        assert_eq!(Ratio::from_f64(f64::INFINITY), None);
    }

    #[test]
    fn ratio_from_f64_rejects_denominators_it_cannot_represent() {
        // 1/13 is beyond the accepted denominators, and must not be rounded to
        // a neighbour such as 1/12.
        assert_eq!(Ratio::from_f64(1.0 / 13.0), None);
    }

    #[test]
    fn products_and_quotients() {
        // force = mass · length / time²
        let force = mass().mul(&length()).div(&time().pow(Ratio::int(2)));
        assert_eq!(force.to_string(), "m·kg·s^-2");
        // pressure = force / area
        let pressure = force.div(&length().pow(Ratio::int(2)));
        assert_eq!(pressure.to_string(), "m^-1·kg·s^-2");
    }

    #[test]
    fn dividing_by_itself_is_dimensionless() {
        assert!(length().div(&length()).is_dimensionless());
        assert_eq!(length().div(&length()), Dimension::DIMENSIONLESS);
    }

    #[test]
    fn half_powers_survive_round_trip() {
        // MPa·√m — fracture toughness, the reason exponents are rational.
        let root_m = length().pow(Ratio::new(1, 2));
        assert_eq!(root_m.to_string(), "m^1/2");
        assert_eq!(root_m.pow(Ratio::int(2)), length());
    }

    #[test]
    fn dimensionless_displays_as_a_word() {
        assert_eq!(Dimension::DIMENSIONLESS.to_string(), "dimensionless");
    }

    #[test]
    fn equality_is_structural_not_textual() {
        // A newton built two different ways is one dimension.
        let a = mass().mul(&length()).div(&time().pow(Ratio::int(2)));
        let b = mass().mul(&length()).mul(&time().pow(Ratio::int(-2)));
        assert_eq!(a, b);
    }
}
