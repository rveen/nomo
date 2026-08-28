//! A quantity carrying its own derivative, for `derivative(f, x)`.
//!
//! # Why this is not a computer algebra system
//!
//! SMath differentiates *symbolically*: `diff(Mg(f), f)` hands the expression
//! to its CAS, which returns another expression. Nomo has no CAS and is not
//! getting one — but a worksheet that writes a derivative almost never wants
//! the expression. It wants the **number**, at a point, so that something else
//! can plot it or find its zero. The resonant-converter worksheets in the
//! corpus are the case: the derivative exists only to be sampled by a root
//! search looking for where the gain curve peaks.
//!
//! Forward-mode automatic differentiation gives exactly that number, and it is
//! arithmetic rather than algebra. Every value carries a second component — how
//! fast it is changing — and every operation carries the chain rule alongside
//! the value it was already computing. Nothing is simplified, nothing is
//! rearranged, no expression is ever built. `(u·v)' = u'v + uv'` is a
//! multiplication and two additions.
//!
//! # It is exact, and that is the point
//!
//! A finite difference would need a step, and a step is a tuning knob: the
//! answer would depend on a number nobody could derive from the worksheet, and
//! its error would sit between the truncation of a step too large and the
//! cancellation of a step too small. This has no step. The derivative comes out
//! correct to the same rounding as the value, which is what lets a stored
//! answer be reproduced rather than approached.
//!
//! # The derivative's dimension is implicit, and never stored
//!
//! Everything here is in base SI, so `d` is a bare magnitude: the derivative of
//! a value with dimension `V` with respect to a variable with dimension `X` has
//! dimension `V/X`, and `V` is on the value beside it while `X` is fixed for
//! the whole evaluation. Carrying it would mean storing the same `X` on every
//! intermediate and checking it against itself; instead
//! [`crate::eval`] attaches it once, when the answer comes out.
//!
//! # A missing rule is a refusal
//!
//! Only the operations below know what to do with a dual. Anything else — a
//! comparison, `floor`, a matrix — meets one and reports that it cannot
//! differentiate it, because the alternative is the failure mode this project
//! refuses everywhere else: an answer that is quietly wrong. A derivative
//! silently reported as zero because a rule was never written would be exactly
//! that.

use crate::math;
use crate::quantity::Quantity;
use crate::unit::UnitError;

/// What can go wrong differentiating, beyond the unit rules the value obeys.
#[derive(Debug, Clone, PartialEq)]
pub enum DualError {
    /// The value's own rules refused first.
    Unit(UnitError),
    /// The value exists and the slope does not: `u^v` where `u` is not positive
    /// and `v` varies, which would need `ln u`.
    NoSlope,
}

impl From<UnitError> for DualError {
    fn from(e: UnitError) -> DualError {
        DualError::Unit(e)
    }
}

/// A quantity and its first two derivatives with respect to one variable.
///
/// Both derivatives are carried always, even when only the first is asked for.
/// Two reasons: the second costs a multiply and an add per operation, which is
/// nothing beside evaluating the expression at all; and a rule written only for
/// the order somebody asked about is a rule that is wrong the first time
/// somebody asks about the other one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DualQuantity {
    pub value: Quantity,
    /// The first derivative's magnitude in base SI, then the second's. See the
    /// module docs for why their dimensions are not stored here.
    pub d: f64,
    pub dd: f64,
}

impl DualQuantity {
    /// The variable being differentiated with respect to: it changes at one
    /// unit per unit of itself.
    pub fn seed(value: Quantity) -> DualQuantity {
        DualQuantity {
            value,
            d: 1.0,
            dd: 0.0,
        }
    }

    /// Anything else the body reads — a constant as far as this derivative is
    /// concerned, whatever it is elsewhere in the worksheet.
    pub fn constant(value: Quantity) -> DualQuantity {
        DualQuantity {
            value,
            d: 0.0,
            dd: 0.0,
        }
    }

    fn with(&self, value: Quantity, d: f64, dd: f64) -> DualQuantity {
        let _ = self;
        DualQuantity { value, d, dd }
    }

    pub fn add(&self, other: &DualQuantity) -> Result<DualQuantity, DualError> {
        Ok(self.with(
            self.value.add(&other.value)?,
            self.d + other.d,
            self.dd + other.dd,
        ))
    }

    pub fn sub(&self, other: &DualQuantity) -> Result<DualQuantity, DualError> {
        Ok(self.with(
            self.value.sub(&other.value)?,
            self.d - other.d,
            self.dd - other.dd,
        ))
    }

    pub fn neg(&self) -> Result<DualQuantity, DualError> {
        Ok(self.with(self.value.neg()?, -self.d, -self.dd))
    }

    /// `(u·v)' = u'v + uv'`, and `(u·v)'' = u''v + 2u'v' + uv''`.
    pub fn mul(&self, other: &DualQuantity) -> Result<DualQuantity, DualError> {
        let value = self.value.mul(&other.value)?;
        let (u, v) = (self.value.value, other.value.value);
        Ok(self.with(
            value,
            self.d * v + u * other.d,
            self.dd * v + 2.0 * self.d * other.d + u * other.dd,
        ))
    }

    /// `(u/v)' = (u'v − uv')/v²`, and
    /// `(u/v)'' = u''/v − 2u'v'/v² − uv''/v² + 2uv'²/v³`.
    pub fn div(&self, other: &DualQuantity) -> Result<DualQuantity, DualError> {
        let value = self.value.div(&other.value)?;
        let (u, v) = (self.value.value, other.value.value);
        let (u1, v1, u2, v2) = (self.d, other.d, self.dd, other.dd);
        Ok(self.with(
            value,
            (u1 * v - u * v1) / (v * v),
            u2 / v - 2.0 * u1 * v1 / (v * v) - u * v2 / (v * v) + 2.0 * u * v1 * v1 / (v * v * v),
        ))
    }

    /// `u^v`, with the rule chosen by which side is actually varying.
    ///
    /// The general form `(u^v)' = u^v·(v'·ln u + v·u'/u)` needs `ln u`, so a
    /// base that is not positive would have no derivative to report even where
    /// the *value* is perfectly well defined — `(-8)^(1/3)`. The two cases the
    /// worksheets write are each free of that: a constant exponent needs only
    /// `v·u^(v−1)·u'`, and a constant base only `u^v·ln(u)·v'`. Both are taken
    /// first, so a squared length differentiates without ever asking for the
    /// logarithm of anything.
    pub fn pow(&self, other: &DualQuantity) -> Result<DualQuantity, DualError> {
        let value = self.value.pow(&other.value)?;
        // `u^v` with `v` constant. Written with `powf` on the magnitude rather
        // than `Quantity::pow`, because `u^(v−1)` may be a dimension the unit
        // system would refuse to name while the product `v·u^(v−1)·u'` is
        // perfectly ordinary.
        let u = self.value.value;
        if other.d == 0.0 && other.dd == 0.0 {
            let n = other.value.value;
            // `(uⁿ)'' = n(n−1)uⁿ⁻²u'² + nuⁿ⁻¹u''`.
            let slope = n * math::powf(u, n - 1.0) * self.d;
            let curve = n * (n - 1.0) * math::powf(u, n - 2.0) * self.d * self.d
                + n * math::powf(u, n - 1.0) * self.dd;
            return Ok(self.with(value, slope, curve));
        }
        if self.d == 0.0 && self.dd == 0.0 {
            if u <= 0.0 {
                return Err(DualError::NoSlope);
            }
            // `(aᵛ)'' = aᵛ(ln a)²v'² + aᵛ ln a · v''`.
            let ln_a = math::ln(u);
            let slope = value.value * ln_a * other.d;
            let curve = value.value * ln_a * (ln_a * other.d * other.d + other.dd);
            return Ok(self.with(value, slope, curve));
        }
        if u <= 0.0 {
            return Err(DualError::NoSlope);
        }
        // Both sides varying. The first derivative is `uᵛ·g` where
        // `g = v'·ln u + v·u'/u`, so the second is `uᵛ·(g² + g')` — and `g'`
        // has four terms of its own. Written out rather than refused because it
        // is the same rule one level down, and refusing here would mean `x^x`
        // differentiating once and not twice for no reason a reader could see.
        let (v, u1, v1, u2, v2) = (other.value.value, self.d, other.d, self.dd, other.dd);
        let ln_u = math::ln(u);
        let g = v1 * ln_u + v * u1 / u;
        let g1 = v2 * ln_u + 2.0 * v1 * u1 / u + v * (u2 / u - u1 * u1 / (u * u));
        Ok(self.with(value, value.value * g, value.value * (g * g + g1)))
    }

    /// A function of one dimensionless argument, and its slope there.
    ///
    /// Every transcendental takes the same shape — check the argument is
    /// dimensionless, apply the function, apply the chain rule — so they are
    /// written once here and named once in [`crate::eval`].
    pub fn chain(
        &self,
        f: fn(f64) -> f64,
        df: fn(f64) -> f64,
        ddf: fn(f64) -> f64,
    ) -> Result<DualQuantity, DualError> {
        if !self.value.is_dimensionless() {
            return Err(DualError::Unit(UnitError::ExpectedDimensionless {
                found: self.value.dim,
            }));
        }
        let x = self.value.value;
        // `f(u)'' = f''(u)·u'² + f'(u)·u''` — the chain rule twice, which is
        // where the square comes from and why a second derivative is not just
        // the first one applied again.
        Ok(self.with(
            Quantity::scalar(f(x)),
            df(x) * self.d,
            ddf(x) * self.d * self.d + df(x) * self.dd,
        ))
    }

    /// `sqrt`, which is its own case because it is the one function here that
    /// halves the dimension instead of demanding none.
    pub fn sqrt(&self) -> Result<DualQuantity, DualError> {
        let value = self.value.sqrt()?;
        let r = value.value;
        // `√u'' = u''/(2√u) − u'²/(4u^{3/2})`, and `u^{3/2}` is `r³`.
        Ok(self.with(
            value,
            self.d / (2.0 * r),
            self.dd / (2.0 * r) - self.d * self.d / (4.0 * r * r * r),
        ))
    }

    /// `|u|`, whose slope is the sign of `u`.
    ///
    /// At exactly zero there is no derivative, and zero is reported: it is the
    /// one value in the subgradient that is symmetric, and refusing the whole
    /// evaluation because a curve touched the axis at one of two hundred
    /// samples would be worse. Everywhere else this is exact.
    pub fn abs(&self) -> DualQuantity {
        let x = self.value.value;
        let slope = if x > 0.0 {
            self.d
        } else if x < 0.0 {
            -self.d
        } else {
            0.0
        };
        // Away from zero `|u|` is `±u`, so its second derivative is `±u''` by
        // the same sign. At zero neither exists and both are reported as zero,
        // for the reason above.
        let curve = if x > 0.0 {
            self.dd
        } else if x < 0.0 {
            -self.dd
        } else {
            0.0
        };
        self.with(
            Quantity {
                value: math::abs(x),
                ..self.value
            },
            slope,
            curve,
        )
    }
}

#[cfg(test)]
mod tests {
    // Exact equality throughout, and deliberately: every rule below has an
    // answer that is a small integer or a power of two, and the point of
    // carrying the slope rather than differencing is that it arrives exactly.
    #![allow(clippy::float_cmp)]

    use super::*;

    fn seed(v: f64) -> DualQuantity {
        DualQuantity::seed(Quantity::scalar(v))
    }

    fn constant(v: f64) -> DualQuantity {
        DualQuantity::constant(Quantity::scalar(v))
    }

    #[test]
    fn the_product_rule() {
        // (x·x)' = 2x at x = 3.
        let x = seed(3.0);
        assert_eq!(x.mul(&x).unwrap().d, 6.0);
    }

    #[test]
    fn the_quotient_rule() {
        // (1/x)' = -1/x² at x = 2.
        let one = constant(1.0);
        assert_eq!(one.div(&seed(2.0)).unwrap().d, -0.25);
    }

    #[test]
    fn a_constant_exponent_never_asks_for_a_logarithm() {
        // (x³)' = 3x² at x = -2, which the general rule could not answer
        // because ln(-2) does not exist. The value is defined and so is the
        // slope.
        let cube = seed(-2.0).pow(&constant(3.0)).unwrap();
        assert_eq!(cube.value.value, -8.0);
        assert_eq!(cube.d, 12.0);
    }

    #[test]
    fn a_constant_base_differentiates_the_exponent() {
        // (2^x)' = 2^x·ln 2 at x = 3.
        let e = constant(2.0).pow(&seed(3.0)).unwrap();
        assert!((e.d - 8.0 * math::ln(2.0)).abs() < 1e-12);
    }

    #[test]
    fn the_square_root_halves_the_dimension_and_the_slope() {
        // (√x)' = 1/(2√x) at x = 4.
        let r = seed(4.0).sqrt().unwrap();
        assert_eq!(r.value.value, 2.0);
        assert_eq!(r.d, 0.25);
    }

    #[test]
    fn a_constant_carries_no_slope() {
        assert_eq!(constant(7.0).mul(&constant(3.0)).unwrap().d, 0.0);
    }
}
