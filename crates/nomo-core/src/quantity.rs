//! Scalar quantities: a magnitude with a dimension.
//!
//! # Everything is stored in base SI
//!
//! A quantity's magnitude is always in base SI units, whatever the user wrote.
//! Arithmetic is therefore plain arithmetic on `f64`, and dimensional
//! consistency is enforced by the operators rather than by a separate checking
//! pass that could disagree with them. Which unit to *display* is a separate
//! question, answered by the renderer in a later phase.
//!
//! # Points and intervals
//!
//! Offset temperature scales are the reason this type has a [`Kind`]. `20°C` is
//! a *point* on a scale; `5 K` is an *interval*, a displacement. The distinction
//! is not pedantry — it is what makes these rules fall out instead of having to
//! be special-cased:
//!
//! | Expression | Result | Why |
//! |---|---|---|
//! | `20°C + 5 K` | `25°C` | point + interval is a point |
//! | `20°C - 15°C` | `5 K` | the difference of two points is an interval |
//! | `20°C + 5°C` | error | two points do not add |
//! | `2 * 20°C` | error | a point on an offset scale cannot be scaled |
//! | `20°C -> K` | `293.15 K` | conversion is always allowed |
//!
//! A quantity becomes a point only by being formed from an affine unit. Anything
//! written in a linear unit — including kelvin — is an interval.

use crate::dim::{Dimension, Ratio};
use crate::unit::{Unit, UnitError};

/// Whether a quantity is a position on a scale or a displacement along it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An ordinary quantity. Closed under all arithmetic.
    Interval,
    /// A reading on an offset scale, such as `20°C`.
    Point,
}

/// A magnitude in base SI units, with its dimension.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    pub value: f64,
    pub dim: Dimension,
    pub kind: Kind,
}

impl Quantity {
    /// A dimensionless number.
    pub fn scalar(value: f64) -> Quantity {
        Quantity {
            value,
            dim: Dimension::DIMENSIONLESS,
            kind: Kind::Interval,
        }
    }

    pub fn new(value: f64, dim: Dimension) -> Quantity {
        Quantity {
            value,
            dim,
            kind: Kind::Interval,
        }
    }

    /// Build a quantity from a magnitude written in `unit`.
    ///
    /// This is the *only* place a point comes into existence. Note what that
    /// implies for the evaluator: `20°C` cannot be evaluated as `20` multiplied
    /// by the value of `°C`, because an offset scale has no meaningful value of
    /// "one". The evaluator must recognise a number applied to an affine unit and
    /// call this directly; a bare affine unit used as a factor is an error.
    pub fn from_unit(magnitude: f64, unit: &Unit) -> Quantity {
        match unit.offset {
            Some(offset) => Quantity {
                value: unit.factor * magnitude + offset,
                dim: unit.dim,
                kind: Kind::Point,
            },
            None => Quantity {
                value: unit.factor * magnitude,
                dim: unit.dim,
                kind: Kind::Interval,
            },
        }
    }

    /// The magnitude of this quantity expressed in `unit`.
    pub fn to_unit(&self, unit: &Unit) -> Result<f64, UnitError> {
        if self.dim != unit.dim {
            return Err(UnitError::DimensionMismatch {
                lhs: self.dim,
                rhs: unit.dim,
            });
        }
        Ok(match unit.offset {
            Some(offset) => (self.value - offset) / unit.factor,
            None => self.value / unit.factor,
        })
    }

    pub fn is_point(&self) -> bool {
        self.kind == Kind::Point
    }

    pub fn is_dimensionless(&self) -> bool {
        self.dim.is_dimensionless()
    }

    fn same_dim(&self, other: &Quantity) -> Result<(), UnitError> {
        if self.dim == other.dim {
            Ok(())
        } else {
            Err(UnitError::DimensionMismatch {
                lhs: self.dim,
                rhs: other.dim,
            })
        }
    }

    pub fn add(&self, other: &Quantity) -> Result<Quantity, UnitError> {
        self.same_dim(other)?;
        let kind = match (self.kind, other.kind) {
            (Kind::Interval, Kind::Interval) => Kind::Interval,
            // A point displaced by an interval is still a point.
            (Kind::Point, Kind::Interval) | (Kind::Interval, Kind::Point) => Kind::Point,
            (Kind::Point, Kind::Point) => return Err(UnitError::AffineAddition),
        };
        Ok(Quantity {
            value: self.value + other.value,
            dim: self.dim,
            kind,
        })
    }

    pub fn sub(&self, other: &Quantity) -> Result<Quantity, UnitError> {
        self.same_dim(other)?;
        let kind = match (self.kind, other.kind) {
            (Kind::Interval, Kind::Interval) => Kind::Interval,
            // The difference between two readings is a difference, not a reading.
            (Kind::Point, Kind::Point) => Kind::Interval,
            (Kind::Point, Kind::Interval) => Kind::Point,
            (Kind::Interval, Kind::Point) => return Err(UnitError::AffineSubtraction),
        };
        Ok(Quantity {
            value: self.value - other.value,
            dim: self.dim,
            kind,
        })
    }

    fn reject_points(&self, other: Option<&Quantity>) -> Result<(), UnitError> {
        if self.is_point() || other.is_some_and(Quantity::is_point) {
            Err(UnitError::AffineScaling)
        } else {
            Ok(())
        }
    }

    pub fn mul(&self, other: &Quantity) -> Result<Quantity, UnitError> {
        self.reject_points(Some(other))?;
        Ok(Quantity::new(
            self.value * other.value,
            self.dim.mul(&other.dim),
        ))
    }

    pub fn div(&self, other: &Quantity) -> Result<Quantity, UnitError> {
        self.reject_points(Some(other))?;
        Ok(Quantity::new(
            self.value / other.value,
            self.dim.div(&other.dim),
        ))
    }

    pub fn neg(&self) -> Result<Quantity, UnitError> {
        self.reject_points(None)?;
        Ok(Quantity {
            value: -self.value,
            dim: self.dim,
            kind: Kind::Interval,
        })
    }

    /// Raise to a power.
    ///
    /// The exponent must be dimensionless — `x^(2 m)` is meaningless. A
    /// dimensioned base additionally requires an exponent this system can turn
    /// into a rational, so that the resulting dimension is exact; `2^0.3` is fine
    /// but `(5 m)^0.3` is not.
    pub fn pow(&self, exponent: &Quantity) -> Result<Quantity, UnitError> {
        self.reject_points(Some(exponent))?;
        if !exponent.is_dimensionless() {
            return Err(UnitError::ExpectedDimensionless {
                found: exponent.dim,
            });
        }
        let value = crate::math::powf(self.value, exponent.value);
        if self.dim.is_dimensionless() {
            return Ok(Quantity::scalar(value));
        }
        let Some(r) = Ratio::from_f64(exponent.value) else {
            return Err(UnitError::IrrationalExponent(exponent.value));
        };
        Ok(Quantity::new(value, self.dim.pow(r)))
    }

    /// Square root, as a power of one half.
    pub fn sqrt(&self) -> Result<Quantity, UnitError> {
        self.pow(&Quantity::scalar(0.5))
    }
}

#[cfg(test)]
// Exact float comparison is the point here, not an oversight: this engine
// promises bit-reproducible results, so its tests assert bit equality.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::unit::UnitTable;

    fn q(magnitude: f64, unit: &str) -> Quantity {
        let table = UnitTable::new();
        Quantity::from_unit(magnitude, &table.resolve(unit).unwrap())
    }

    fn in_unit(x: &Quantity, unit: &str) -> f64 {
        let table = UnitTable::new();
        x.to_unit(&table.resolve(unit).unwrap()).unwrap()
    }

    // ---- conversion -------------------------------------------------------

    #[test]
    fn conversion_round_trips() {
        for (magnitude, unit) in [
            (5.0, "cm"),
            (12.0, "in"),
            (3.5, "ft"),
            (250.0, "MPa"),
            (60.0, "kip"),
            (1.0, "BTU"),
        ] {
            let value = q(magnitude, unit);
            let back = in_unit(&value, unit);
            assert!(
                (back - magnitude).abs() < 1e-12,
                "{magnitude} {unit} round-tripped to {back}"
            );
        }
    }

    #[test]
    fn known_conversions_are_right() {
        assert!((in_unit(&q(1.0, "in"), "mm") - 25.4).abs() < 1e-9);
        assert!((in_unit(&q(1.0, "ft"), "in") - 12.0).abs() < 1e-9);
        assert!((in_unit(&q(1.0, "kip"), "lbf") - 1000.0).abs() < 1e-9);
        assert!((in_unit(&q(1.0, "MPa"), "psi") - 145.03773773020922).abs() < 1e-9);
    }

    #[test]
    fn converting_across_dimensions_is_rejected() {
        let table = UnitTable::new();
        let length = q(1.0, "m");
        assert!(matches!(
            length.to_unit(&table.resolve("s").unwrap()),
            Err(UnitError::DimensionMismatch { .. })
        ));
    }

    // ---- the affine rules, exactly as specified ---------------------------

    #[test]
    fn affine_plus_linear_is_affine() {
        // 20°C + 5 K = 25°C
        let r = q(20.0, "°C").add(&q(5.0, "K")).unwrap();
        assert_eq!(r.kind, Kind::Point);
        assert!((in_unit(&r, "°C") - 25.0).abs() < 1e-9);
    }

    #[test]
    fn difference_of_two_affine_is_linear() {
        // 20°C - 15°C = 5 K
        let r = q(20.0, "°C").sub(&q(15.0, "°C")).unwrap();
        assert_eq!(r.kind, Kind::Interval);
        assert!((in_unit(&r, "K") - 5.0).abs() < 1e-9);
    }

    #[test]
    fn adding_two_affine_is_an_error() {
        assert_eq!(
            q(20.0, "°C").add(&q(5.0, "°C")),
            Err(UnitError::AffineAddition)
        );
    }

    #[test]
    fn scaling_an_affine_is_an_error() {
        assert_eq!(
            Quantity::scalar(2.0).mul(&q(20.0, "°C")),
            Err(UnitError::AffineScaling)
        );
        assert_eq!(
            q(20.0, "°C").mul(&Quantity::scalar(2.0)),
            Err(UnitError::AffineScaling)
        );
        assert_eq!(q(20.0, "°C").neg(), Err(UnitError::AffineScaling));
    }

    #[test]
    fn subtracting_an_affine_from_an_interval_is_an_error() {
        assert_eq!(
            q(5.0, "K").sub(&q(20.0, "°C")),
            Err(UnitError::AffineSubtraction)
        );
    }

    #[test]
    fn affine_converts_to_absolute() {
        // 20°C -> K = 293.15 K
        assert!((in_unit(&q(20.0, "°C"), "K") - 293.15).abs() < 1e-9);
        // 0°C -> K = 273.15 K
        assert!((in_unit(&q(0.0, "°C"), "K") - 273.15).abs() < 1e-9);
    }

    #[test]
    fn fahrenheit_anchors_are_right() {
        assert!((in_unit(&q(32.0, "°F"), "°C") - 0.0).abs() < 1e-9);
        assert!((in_unit(&q(212.0, "°F"), "°C") - 100.0).abs() < 1e-9);
        assert!((in_unit(&q(-40.0, "°F"), "°C") + 40.0).abs() < 1e-9);
        assert!((in_unit(&q(32.0, "°F"), "K") - 273.15).abs() < 1e-9);
    }

    #[test]
    fn kelvin_arithmetic_is_unrestricted() {
        // K is linear, so none of the affine restrictions apply.
        let a = q(300.0, "K");
        assert!(a.add(&q(5.0, "K")).is_ok());
        assert!(a.mul(&Quantity::scalar(2.0)).is_ok());
        assert!(a.neg().is_ok());
    }

    // ---- dimensional arithmetic -------------------------------------------

    #[test]
    fn adding_different_dimensions_is_rejected() {
        assert!(matches!(
            q(1.0, "m").add(&q(1.0, "s")),
            Err(UnitError::DimensionMismatch { .. })
        ));
    }

    #[test]
    fn multiplication_combines_dimensions() {
        // 2 m * 3 m = 6 m², and that is an area not a length.
        let area = q(2.0, "m").mul(&q(3.0, "m")).unwrap();
        assert_eq!(area.value, 6.0);
        assert_eq!(area.dim, q(1.0, "m").dim.pow(Ratio::int(2)));
        assert!(area.add(&q(1.0, "m")).is_err());
    }

    #[test]
    fn force_over_area_is_a_pressure() {
        let f = q(10.0, "kN");
        let a = q(2.0, "m").mul(&q(1.0, "m")).unwrap();
        let p = f.div(&a).unwrap();
        assert_eq!(p.dim, q(1.0, "Pa").dim);
        assert!((in_unit(&p, "kPa") - 5.0).abs() < 1e-9);
    }

    #[test]
    fn sqrt_halves_the_dimension() {
        let area = q(4.0, "m").mul(&q(4.0, "m")).unwrap();
        let side = area.sqrt().unwrap();
        assert!((side.value - 4.0).abs() < 1e-12);
        assert_eq!(side.dim, q(1.0, "m").dim);
    }

    #[test]
    fn sqrt_of_an_odd_dimension_is_representable() {
        // MPa·√m, the fracture-toughness case that motivates rational exponents.
        let root_len = q(1.0, "m").sqrt().unwrap();
        let toughness = q(50.0, "MPa").mul(&root_len).unwrap();
        assert_eq!(toughness.dim, q(1.0, "MPa").dim.mul(&root_len.dim));
        // Squaring gets back to pressure² · length.
        assert!(toughness.pow(&Quantity::scalar(2.0)).is_ok());
    }

    #[test]
    fn a_dimensioned_base_rejects_an_irrational_exponent() {
        assert!(matches!(
            q(5.0, "m").pow(&Quantity::scalar(core::f64::consts::PI)),
            Err(UnitError::IrrationalExponent(_))
        ));
        // A dimensionless base is fine, because no dimension has to be exact.
        assert!(Quantity::scalar(2.0)
            .pow(&Quantity::scalar(core::f64::consts::PI))
            .is_ok());
    }

    #[test]
    fn a_dimensioned_exponent_is_rejected() {
        assert!(matches!(
            Quantity::scalar(2.0).pow(&q(3.0, "m")),
            Err(UnitError::ExpectedDimensionless { .. })
        ));
    }

    #[test]
    fn percent_is_a_dimensionless_hundredth() {
        assert_eq!(q(50.0, "%").value, 0.5);
        assert!(q(50.0, "%").is_dimensionless());
    }

    #[test]
    fn degrees_convert_to_radians() {
        assert!((q(180.0, "°").value - core::f64::consts::PI).abs() < 1e-12);
        assert!(q(180.0, "°").is_dimensionless());
    }
}
