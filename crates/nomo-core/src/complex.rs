//! Complex quantities: a real and an imaginary part sharing one dimension.
//!
//! # Why one dimension and not two
//!
//! An impedance is `(1 + 2i)·Ω`, not one ohm plus two of something else. The
//! two parts of a complex quantity are components of a single measurement, so
//! they carry a single dimension and adding `1 m` to `2i s` is an error rather
//! than a value with two dimensions in it. That is also what makes every
//! operation below a dimension rule already written for [`Quantity`], applied
//! once instead of twice.
//!
//! # A complex value never becomes real again on its own
//!
//! `(1 + 2i) - 2i` stays complex and displays as `1 + 0i`. Demoting it to a
//! real when the imaginary part happens to be zero would make the *type* of a
//! result depend on its value, and depend on it through a floating-point
//! comparison: the imaginary part of a computation is hardly ever exactly zero,
//! so the rule would fire on some worksheets and not on others that differ in
//! the last bit. A reader who wants the real part asks for it — `Re(z)` — and
//! gets a real. Nothing here decides that on their behalf.
//!
//! # No points
//!
//! [`Kind::Point`] — a reading on an offset scale like `20°C` — has no
//! imaginary part to have. There is no complex temperature, and a point cannot
//! be scaled at all (see [`crate::quantity`]), so the promotion below refuses
//! one rather than inventing a rule for it.

use crate::dim::{Dimension, Ratio};
use crate::math;
use crate::quantity::Quantity;
use crate::unit::UnitError;

/// A complex quantity: `re + im·i`, both in base SI, sharing `dim`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComplexQuantity {
    pub re: f64,
    pub im: f64,
    pub dim: Dimension,
}

impl ComplexQuantity {
    pub fn new(re: f64, im: f64, dim: Dimension) -> ComplexQuantity {
        ComplexQuantity { re, im, dim }
    }

    /// The imaginary unit: dimensionless, and the only way into this type from
    /// source text.
    pub fn i() -> ComplexQuantity {
        ComplexQuantity::new(0.0, 1.0, Dimension::DIMENSIONLESS)
    }

    /// A real quantity seen as a complex one, for an operation with a complex
    /// other side.
    pub fn promote(q: &Quantity) -> Result<ComplexQuantity, UnitError> {
        if q.is_point() {
            // `20°C + 3i` has no meaning to give: the reading is a position on
            // a scale, and the imaginary part would be a displacement from it
            // in a direction the scale does not have.
            return Err(UnitError::AffineScaling);
        }
        Ok(ComplexQuantity::new(q.value, 0.0, q.dim))
    }

    /// The real part, as a real quantity in the same dimension.
    pub fn real_part(&self) -> Quantity {
        Quantity::new(self.re, self.dim)
    }

    /// The imaginary part, as a real quantity in the same dimension.
    ///
    /// The *coefficient* of `i`, not the component with `i` still attached:
    /// `Im((1 + 2i)·Ω)` is `2 Ω`, which is the number an engineer reads off a
    /// reactance and the one that makes `Re(z) + Im(z)·i` reconstruct `z`.
    pub fn imaginary_part(&self) -> Quantity {
        Quantity::new(self.im, self.dim)
    }

    pub fn conj(&self) -> ComplexQuantity {
        ComplexQuantity::new(self.re, -self.im, self.dim)
    }

    /// The modulus, in the same dimension.
    ///
    /// `hypot` rather than `sqrt(re² + im²)` because the naive form overflows
    /// to infinity for a value whose modulus is perfectly representable — an
    /// impedance in the `1e200` range squares out of `f64` — and underflows to
    /// zero at the other end.
    pub fn abs(&self) -> Quantity {
        Quantity::new(math::hypot(self.re, self.im), self.dim)
    }

    /// The argument, in radians, and dimensionless whatever the quantity's
    /// dimension is: an angle between two components of the same measurement
    /// has nothing left of that measurement in it.
    pub fn arg(&self) -> Quantity {
        Quantity::scalar(math::atan2(self.im, self.re))
    }

    pub fn add(&self, other: &ComplexQuantity) -> Result<ComplexQuantity, UnitError> {
        self.same_dim(other)?;
        Ok(ComplexQuantity::new(
            self.re + other.re,
            self.im + other.im,
            self.dim,
        ))
    }

    pub fn sub(&self, other: &ComplexQuantity) -> Result<ComplexQuantity, UnitError> {
        self.same_dim(other)?;
        Ok(ComplexQuantity::new(
            self.re - other.re,
            self.im - other.im,
            self.dim,
        ))
    }

    pub fn neg(&self) -> ComplexQuantity {
        ComplexQuantity::new(-self.re, -self.im, self.dim)
    }

    pub fn mul(&self, other: &ComplexQuantity) -> ComplexQuantity {
        ComplexQuantity::new(
            self.re * other.re - self.im * other.im,
            self.re * other.im + self.im * other.re,
            self.dim.mul(&other.dim),
        )
    }

    /// Division, by Smith's method.
    ///
    /// The textbook formula multiplies by the conjugate and divides by
    /// `c² + d²`, which overflows to infinity — and then returns `NaN` — for
    /// operands whose quotient is an ordinary number: `1e200` squared is not an
    /// `f64`. Smith's method divides through by the larger of the two parts
    /// first, so nothing is squared and the intermediate stays in range.
    ///
    /// It uses only `+`, `-`, `*` and `/`, all of which IEEE 754 specifies
    /// exactly, so this is bit-reproducible wherever it runs. It is written out
    /// here rather than taken from a library for that reason: which formula is
    /// used decides the last bits, so it is part of the language rather than an
    /// implementation detail.
    pub fn div(&self, other: &ComplexQuantity) -> ComplexQuantity {
        let (a, b, c, d) = (self.re, self.im, other.re, other.im);
        let dim = self.dim.div(&other.dim);
        if math::abs(d) <= math::abs(c) {
            let r = d / c;
            let den = c + d * r;
            ComplexQuantity::new((a + b * r) / den, (b - a * r) / den, dim)
        } else {
            let r = c / d;
            let den = c * r + d;
            ComplexQuantity::new((a * r + b) / den, (b * r - a) / den, dim)
        }
    }

    /// Raise to a real power.
    ///
    /// **Whole exponents only, by repeated multiplication.** Anything else
    /// needs a complex logarithm, and a complex logarithm needs a branch cut —
    /// a choice about where `arg` jumps from `π` to `-π` — which decides the
    /// answer to `(-1 + 0i)^0.5` and has no reading this worksheet language can
    /// take on the author's behalf. So a fractional exponent says it is not
    /// implemented rather than picking a branch quietly.
    ///
    /// One multiplication at a time, in the order written, for the reason
    /// `iterate` applies one step at a time: `z*z*z*z` and `(z*z)*(z*z)` differ
    /// in the last bits, so which one is meant is part of the language.
    pub fn pow(&self, exponent: &Quantity) -> Result<ComplexQuantity, PowError> {
        if !exponent.is_dimensionless() {
            return Err(PowError::Dimension(UnitError::ExpectedDimensionless {
                found: exponent.dim,
            }));
        }
        let n = exponent.value;
        // Exact, and deliberately so: "is this exponent a whole number" is a
        // question about the bits, and `2.0000000000000004` is not one.
        #[allow(clippy::float_cmp)]
        let whole = n == math::round(n);
        if !n.is_finite() || !whole {
            return Err(PowError::Fractional);
        }
        // A dimensioned base needs the exponent as a rational for the resulting
        // dimension to be exact, exactly as `Quantity::pow` does.
        let dim = if self.dim.is_dimensionless() {
            Dimension::DIMENSIONLESS
        } else {
            let Some(r) = Ratio::from_f64(n) else {
                return Err(PowError::Dimension(UnitError::IrrationalExponent(n)));
            };
            self.dim.pow(r)
        };
        // Whole, finite and inside the repetition cap, so this fits.
        let times = math::abs(n);
        if times > MAX_POWER as f64 {
            return Err(PowError::TooManySteps);
        }
        let mut acc = ComplexQuantity::new(1.0, 0.0, Dimension::DIMENSIONLESS);
        for _ in 0..(times as u32) {
            acc = acc.mul(&ComplexQuantity::new(
                self.re,
                self.im,
                Dimension::DIMENSIONLESS,
            ));
        }
        if n < 0.0 {
            acc = ComplexQuantity::new(1.0, 0.0, Dimension::DIMENSIONLESS).div(&acc);
        }
        Ok(ComplexQuantity::new(acc.re, acc.im, dim))
    }

    fn same_dim(&self, other: &ComplexQuantity) -> Result<(), UnitError> {
        if self.dim == other.dim {
            Ok(())
        } else {
            Err(UnitError::DimensionMismatch {
                lhs: self.dim,
                rhs: other.dim,
            })
        }
    }
}

/// Why a complex power could not be taken.
///
/// Separate from [`UnitError`] because two of the three reasons are not about
/// units at all, and reporting a branch cut as "cannot raise a dimensioned
/// value to the power 0.5" sends the reader looking for a dimension that was
/// never the problem.
#[derive(Debug, Clone, PartialEq)]
pub enum PowError {
    /// The exponent, or the resulting dimension, is not something units allow.
    Dimension(UnitError),
    /// A whole exponent is all this can do; see [`ComplexQuantity::pow`].
    Fractional,
    /// More repeated multiplications than the cap allows.
    TooManySteps,
}

/// How many repeated multiplications a power may cost.
///
/// The same reasoning as `MAX_RANGE` in the evaluator: a browser tab has no way
/// out of a loop, and a worksheet asking for `z` to the millionth has already
/// left the range where the answer is a number rather than an infinity.
const MAX_POWER: u32 = 1_000_000;

/// A real quantity is a complex one with no imaginary part; a point is not.
impl TryFrom<Quantity> for ComplexQuantity {
    type Error = UnitError;

    fn try_from(q: Quantity) -> Result<ComplexQuantity, UnitError> {
        ComplexQuantity::promote(&q)
    }
}

// Exact float comparison is the point here, not an oversight: this engine
// promises bit-reproducible results, so its tests assert bit equality.
#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::quantity::Kind;

    fn c(re: f64, im: f64) -> ComplexQuantity {
        ComplexQuantity::new(re, im, Dimension::DIMENSIONLESS)
    }

    #[test]
    fn the_imaginary_unit_squares_to_minus_one() {
        let i = ComplexQuantity::i();
        let squared = i.mul(&i);
        assert_eq!(squared.re, -1.0);
        assert_eq!(squared.im, 0.0);
    }

    #[test]
    fn multiplication_and_division_are_inverse() {
        let z = c(3.0, 4.0);
        let w = c(1.0, -2.0);
        let back = z.mul(&w).div(&w);
        assert!((back.re - 3.0).abs() < 1e-12, "{back:?}");
        assert!((back.im - 4.0).abs() < 1e-12, "{back:?}");
    }

    #[test]
    fn division_survives_operands_whose_squares_do_not() {
        // The conjugate formula computes `c² + d²` and overflows to infinity
        // here, returning NaN for a quotient that is exactly 1.
        let big = c(1e200, 1e200);
        let q = big.div(&big);
        assert_eq!(q.re, 1.0, "{q:?}");
        assert_eq!(q.im, 0.0, "{q:?}");
    }

    #[test]
    fn division_by_a_real_is_the_obvious_thing() {
        let q = c(3.0, 4.0).div(&c(2.0, 0.0));
        assert_eq!((q.re, q.im), (1.5, 2.0));
    }

    #[test]
    fn the_modulus_does_not_overflow_either() {
        // `sqrt(re² + im²)` is `inf` here. The exact last bits are libm's to
        // decide — they are the same on every target, which is the property
        // that matters — so this asks only that the answer is a number.
        let m = c(3e200, 4e200).abs().value;
        assert!(m.is_finite() && (m / 5e200 - 1.0).abs() < 1e-15, "{m}");
    }

    /// A stand-in for a dimensioned quantity; which base it is does not matter.
    fn length() -> Dimension {
        Dimension::base(crate::dim::base::LENGTH)
    }

    #[test]
    fn a_dimension_multiplies_once_and_not_twice() {
        // Both parts are components of one measurement, so a product squares
        // the dimension once — `(1 + 2i)·m` times itself is an area, not a
        // fourth power of length.
        let z = ComplexQuantity::new(1.0, 2.0, length());
        assert_eq!(z.mul(&z).dim, length().pow(Ratio::int(2)));
        assert_eq!(z.div(&z).dim, Dimension::DIMENSIONLESS);
        assert_eq!(
            z.pow(&Quantity::scalar(3.0)).unwrap().dim,
            length().pow(Ratio::int(3))
        );
    }

    #[test]
    fn adding_across_dimensions_is_refused() {
        let metres = ComplexQuantity::new(1.0, 0.0, length());
        let area = ComplexQuantity::new(1.0, 0.0, length().pow(Ratio::int(2)));
        assert!(metres.add(&area).is_err());
        assert!(metres.sub(&area).is_err());
        // A product of unlike dimensions is fine, as it is for real quantities.
        assert_eq!(metres.mul(&area).dim, length().pow(Ratio::int(3)));
    }

    #[test]
    fn a_whole_power_is_repeated_multiplication() {
        let z = c(0.0, 1.0);
        let fourth = z.pow(&Quantity::scalar(4.0)).unwrap();
        assert_eq!((fourth.re, fourth.im), (1.0, 0.0));
    }

    #[test]
    fn a_negative_power_is_the_reciprocal() {
        let z = c(0.0, 2.0);
        let inverse = z.pow(&Quantity::scalar(-1.0)).unwrap();
        assert_eq!(inverse.re, 0.0);
        assert!((inverse.im + 0.5).abs() < 1e-15, "{inverse:?}");
    }

    #[test]
    fn a_zeroth_power_is_one() {
        let z = c(3.0, 4.0).pow(&Quantity::scalar(0.0)).unwrap();
        assert_eq!((z.re, z.im), (1.0, 0.0));
    }

    #[test]
    fn a_fractional_power_says_it_cannot_choose_a_branch() {
        // It needs a complex logarithm, and a complex logarithm needs a branch
        // cut this language has no way to state.
        assert!(c(-1.0, 0.0).pow(&Quantity::scalar(0.5)).is_err());
    }

    #[test]
    fn the_argument_is_dimensionless_whatever_the_quantity_is() {
        let z = ComplexQuantity::new(0.0, 1.0, length());
        assert!(z.arg().is_dimensionless());
        assert_eq!(z.abs().dim, length(), "the modulus keeps the dimension");
        assert_eq!(z.real_part().dim, length());
    }

    #[test]
    fn the_imaginary_part_is_the_coefficient_and_reconstructs_the_value() {
        let z = c(1.0, 2.0);
        assert_eq!(z.imaginary_part().value, 2.0);
        let rebuilt = ComplexQuantity::promote(&z.real_part())
            .unwrap()
            .add(
                &ComplexQuantity::promote(&z.imaginary_part())
                    .unwrap()
                    .mul(&ComplexQuantity::i()),
            )
            .unwrap();
        assert_eq!((rebuilt.re, rebuilt.im), (1.0, 2.0));
    }

    #[test]
    fn a_temperature_reading_has_no_imaginary_part() {
        let point = Quantity {
            value: 293.15,
            dim: Dimension::base(crate::dim::base::TEMPERATURE),
            kind: Kind::Point,
        };
        assert!(ComplexQuantity::promote(&point).is_err());
    }
}
