//! Named units and the table that resolves them.
//!
//! A unit is a factor onto the SI base, a dimension, and — for temperature
//! scales and gauge pressures — an additive offset. A value expressed in unit
//! `u` has the base-SI magnitude `factor * x + offset`.
//!
//! Imperial and US customary units are first class rather than an afterthought.
//! In the 54-worksheet SMath corpus surveyed in the design note, `in` is the
//! single most-used unit, ahead of `mm` and `MPa`.

use crate::dim::{Dimension, Ratio};
// Ordered, not hashed: iteration order is observable in diagnostics and
// completion lists, and everything this engine emits must be deterministic.
use std::collections::BTreeMap;

/// Anything that can go wrong resolving or combining units.
#[derive(Debug, Clone, PartialEq)]
pub enum UnitError {
    UnknownUnit(String),
    /// Operands had different dimensions where the operation required the same.
    DimensionMismatch {
        lhs: Dimension,
        rhs: Dimension,
    },
    /// An operation needed a dimensionless operand and did not get one.
    ExpectedDimensionless {
        found: Dimension,
    },
    /// `20°C + 5°C` — adding two points on an offset scale is meaningless.
    AffineAddition,
    /// `20°C - 5 K` is fine; `5 K - 20°C` is not.
    AffineSubtraction,
    /// `2 * 20°C` — an offset scale cannot be scaled.
    AffineScaling,
    /// An exponent that is not a ratio this system can represent, e.g. `x^π`.
    IrrationalExponent(f64),
    /// A prefix was applied to a unit that does not admit one, such as `k°C`.
    NotPrefixable(String),
}

impl core::fmt::Display for UnitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            UnitError::UnknownUnit(n) => write!(f, "unknown unit `{n}`"),
            UnitError::DimensionMismatch { lhs, rhs } => {
                write!(f, "cannot combine {lhs} with {rhs}")
            }
            UnitError::ExpectedDimensionless { found } => {
                write!(f, "expected a dimensionless value, found {found}")
            }
            UnitError::AffineAddition => write!(
                f,
                "cannot add two temperatures on an offset scale; \
                 add a difference instead, as in `20°C + 5 K`"
            ),
            UnitError::AffineSubtraction => write!(
                f,
                "cannot subtract a temperature on an offset scale from a difference"
            ),
            UnitError::AffineScaling => write!(
                f,
                "cannot scale a temperature on an offset scale; \
                 convert to an absolute scale such as K first"
            ),
            UnitError::IrrationalExponent(x) => {
                write!(f, "cannot raise a dimensioned value to the power {x}")
            }
            UnitError::NotPrefixable(n) => write!(f, "`{n}` does not take an SI prefix"),
        }
    }
}

impl std::error::Error for UnitError {}

/// A named unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub symbol: String,
    /// Multiply a magnitude in this unit by `factor` to reach base SI.
    pub factor: f64,
    /// Added after scaling, for offset scales. `Some` makes the unit affine.
    pub offset: Option<f64>,
    pub dim: Dimension,
    /// Whether an SI prefix may be attached.
    pub prefixable: bool,
}

impl Unit {
    pub fn is_affine(&self) -> bool {
        self.offset.is_some()
    }
}

/// Compact dimension constructor: length, mass, time, current, temperature,
/// amount, luminous.
const fn dims(l: i32, m: i32, t: i32, i: i32, k: i32, n: i32, j: i32) -> [Ratio; 7] {
    [
        Ratio::int(l),
        Ratio::int(m),
        Ratio::int(t),
        Ratio::int(i),
        Ratio::int(k),
        Ratio::int(n),
        Ratio::int(j),
    ]
}

fn d(l: i32, m: i32, t: i32, i: i32, k: i32, n: i32, j: i32) -> Dimension {
    Dimension::from_exponents(dims(l, m, t, i, k, n, j))
}

/// SI prefixes, longest symbol first so that `da` matches before `d`.
const PREFIXES: &[(&str, f64)] = &[
    ("da", 1e1),
    ("Y", 1e24),
    ("Z", 1e21),
    ("E", 1e18),
    ("P", 1e15),
    ("T", 1e12),
    ("G", 1e9),
    ("M", 1e6),
    ("k", 1e3),
    ("h", 1e2),
    ("d", 1e-1),
    ("c", 1e-2),
    ("m", 1e-3),
    ("µ", 1e-6),
    ("u", 1e-6),
    ("n", 1e-9),
    ("p", 1e-12),
    ("f", 1e-15),
    ("a", 1e-18),
    ("z", 1e-21),
    ("y", 1e-24),
];

/// `(symbol, factor, offset, dimension, prefixable)`
type Row = (&'static str, f64, Option<f64>, Dimension, bool);

fn builtin_rows() -> Vec<Row> {
    // Exact conversion constants, by definition:
    //   1 in  = 0.0254 m           1 lb  = 0.45359237 kg
    //   1 lbf = 4.4482216152605 N  1 BTU = 1055.05585262 J (IT)
    let inch = 0.0254_f64;
    let lbf = 4.4482216152605_f64;
    let psi = lbf / (inch * inch);

    let length = d(1, 0, 0, 0, 0, 0, 0);
    let mass = d(0, 1, 0, 0, 0, 0, 0);
    let time = d(0, 0, 1, 0, 0, 0, 0);
    let current = d(0, 0, 0, 1, 0, 0, 0);
    let temp = d(0, 0, 0, 0, 1, 0, 0);
    let amount = d(0, 0, 0, 0, 0, 1, 0);
    let luminous = d(0, 0, 0, 0, 0, 0, 1);
    let none = Dimension::DIMENSIONLESS;

    let force = d(1, 1, -2, 0, 0, 0, 0);
    let pressure = d(-1, 1, -2, 0, 0, 0, 0);
    let energy = d(2, 1, -2, 0, 0, 0, 0);
    let power = d(2, 1, -3, 0, 0, 0, 0);
    let charge = d(0, 0, 1, 1, 0, 0, 0);
    let voltage = d(2, 1, -3, -1, 0, 0, 0);
    let resistance = d(2, 1, -3, -2, 0, 0, 0);
    let volume = d(3, 0, 0, 0, 0, 0, 0);
    let frequency = d(0, 0, -1, 0, 0, 0, 0);

    vec![
        // ---- SI base -----------------------------------------------------
        ("m", 1.0, None, length, true),
        ("g", 1e-3, None, mass, true),
        ("kg", 1.0, None, mass, false),
        ("s", 1.0, None, time, true),
        ("A", 1.0, None, current, true),
        ("K", 1.0, None, temp, true),
        ("mol", 1.0, None, amount, true),
        ("cd", 1.0, None, luminous, true),
        // ---- SI derived --------------------------------------------------
        ("N", 1.0, None, force, true),
        ("Pa", 1.0, None, pressure, true),
        ("J", 1.0, None, energy, true),
        ("W", 1.0, None, power, true),
        ("Hz", 1.0, None, frequency, true),
        ("C", 1.0, None, charge, true),
        ("V", 1.0, None, voltage, true),
        ("Ω", 1.0, None, resistance, true),
        ("ohm", 1.0, None, resistance, true),
        ("S", 1.0, None, d(-2, -1, 3, 2, 0, 0, 0), true),
        ("F", 1.0, None, d(-2, -1, 4, 2, 0, 0, 0), true),
        ("H", 1.0, None, d(2, 1, -2, -2, 0, 0, 0), true),
        ("Wb", 1.0, None, d(2, 1, -2, -1, 0, 0, 0), true),
        ("T", 1.0, None, d(0, 1, -2, -1, 0, 0, 0), true),
        ("L", 1e-3, None, volume, true),
        // Apparent and reactive power. Dimensionally watts; kept as separate
        // names because electrical worksheets distinguish them, and the SMath
        // corpus declares exactly these two (`VA : W`, `var : W`).
        ("VA", 1.0, None, power, true),
        ("var", 1.0, None, power, true),
        // ---- time --------------------------------------------------------
        ("sec", 1.0, None, time, false),
        ("min", 60.0, None, time, false),
        ("h", 3600.0, None, time, false),
        ("hr", 3600.0, None, time, false),
        ("day", 86400.0, None, time, false),
        // ---- imperial and US customary ------------------------------------
        ("in", inch, None, length, false),
        ("ft", 0.3048, None, length, false),
        ("yd", 0.9144, None, length, false),
        ("mi", 1609.344, None, length, false),
        ("mil", inch / 1000.0, None, length, false),
        ("lb", 0.45359237, None, mass, false),
        ("oz", 0.028349523125, None, mass, false),
        ("slug", lbf / 0.3048, None, mass, false),
        ("lbf", lbf, None, force, false),
        ("kip", 1000.0 * lbf, None, force, false),
        ("psi", psi, None, pressure, false),
        ("ksi", 1000.0 * psi, None, pressure, false),
        ("bar", 1e5, None, pressure, true),
        ("atm", 101325.0, None, pressure, false),
        ("BTU", 1055.05585262, None, energy, false),
        ("gal", 0.003785411784, None, volume, false),
        // ---- dimensionless ------------------------------------------------
        // `rad` is dimensionless: angle is a ratio of lengths. `sin` therefore
        // accepts a bare number, and `sin(2)` and `sin(2 rad)` agree.
        ("rad", 1.0, None, none, true),
        ("°", core::f64::consts::PI / 180.0, None, none, false),
        ("deg", core::f64::consts::PI / 180.0, None, none, false),
        ("%", 0.01, None, none, false),
        // ---- affine temperature scales ------------------------------------
        // T_K = factor * x + offset.
        ("°C", 1.0, Some(273.15), temp, false),
        ("°F", 5.0 / 9.0, Some(459.67 * 5.0 / 9.0), temp, false),
        // Rankine is an absolute scale, so it is linear despite being imperial.
        ("°R", 5.0 / 9.0, None, temp, false),
    ]
}

/// Resolves unit names, including SI prefixes and user declarations.
#[derive(Debug, Clone)]
pub struct UnitTable {
    units: BTreeMap<String, Unit>,
}

impl Default for UnitTable {
    fn default() -> Self {
        Self::new()
    }
}

impl UnitTable {
    /// A table of the built-in units.
    pub fn new() -> UnitTable {
        let mut units = BTreeMap::new();
        for (symbol, factor, offset, dim, prefixable) in builtin_rows() {
            units.insert(
                symbol.to_string(),
                Unit {
                    symbol: symbol.to_string(),
                    factor,
                    offset,
                    dim,
                    prefixable,
                },
            );
        }
        UnitTable { units }
    }

    /// Add a user-declared unit, as produced by `unit kip = 1000 lbf`.
    ///
    /// Declared units never take prefixes: `k` followed by a name the user just
    /// invented is far more likely to be a typo than a deliberate kilo-.
    pub fn declare(&mut self, symbol: &str, factor: f64, dim: Dimension) {
        self.units.insert(
            symbol.to_string(),
            Unit {
                symbol: symbol.to_string(),
                factor,
                offset: None,
                dim,
                prefixable: false,
            },
        );
    }

    pub fn contains(&self, name: &str) -> bool {
        self.resolve(name).is_ok()
    }

    /// Resolve a unit name, applying an SI prefix if one is present.
    ///
    /// Exact matches always win, which is what keeps `min` a minute rather than
    /// a milli-inch, and `cd` a candela rather than a centi-day.
    pub fn resolve(&self, name: &str) -> Result<Unit, UnitError> {
        if let Some(u) = self.units.get(name) {
            return Ok(u.clone());
        }
        for (prefix, scale) in PREFIXES {
            let Some(rest) = name.strip_prefix(prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            let Some(base_unit) = self.units.get(rest) else {
                continue;
            };
            // Affine first: `k°C` is a specific mistake worth naming, and
            // reporting it as an unknown unit would hide what went wrong.
            if base_unit.is_affine() {
                return Err(UnitError::NotPrefixable(name.to_string()));
            }
            if !base_unit.prefixable {
                // Keep looking. Another prefix spelling may yet match, and if
                // none does the error should name the whole symbol.
                continue;
            }
            return Ok(Unit {
                symbol: name.to_string(),
                factor: base_unit.factor * scale,
                offset: None,
                dim: base_unit.dim,
                prefixable: false,
            });
        }
        Err(UnitError::UnknownUnit(name.to_string()))
    }

    /// Every unit symbol known to the table, in sorted order.
    pub fn symbols(&self) -> impl Iterator<Item = &str> {
        self.units.keys().map(String::as_str)
    }

    /// The coherent SI unit to display a value of this dimension in, if there is
    /// a named one.
    ///
    /// Reverse lookup exists so a result reads `24525 N` rather than
    /// `24525 m·kg·s^-2`. The search runs over a fixed, ordered list rather than
    /// over the table itself, because several units share one dimension — `W`,
    /// `VA` and `var` are all power — and which is chosen must not depend on map
    /// iteration order.
    pub fn preferred_for(&self, dim: &Dimension) -> Option<&Unit> {
        const PREFERRED: &[&str] = &[
            // Base units first, then derived, most commonly written first.
            "m", "kg", "s", "A", "K", "mol", "cd", //
            "N", "Pa", "J", "W", "Hz", "C", "V", "Ω", "S", "F", "H", "Wb", "T",
        ];
        PREFERRED
            .iter()
            .filter_map(|s| self.units.get(*s))
            // Exactly one, not approximately one: a coherent SI unit has a
            // factor of 1 by definition, and anything else is a different unit.
            .find(|u| u.dim == *dim && u.offset.is_none() && u.factor.to_bits() == 1.0f64.to_bits())
    }
}

#[cfg(test)]
// Exact float comparison is the point here, not an oversight: this engine
// promises bit-reproducible results, so its tests assert bit equality.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn t() -> UnitTable {
        UnitTable::new()
    }

    #[test]
    fn base_units_resolve() {
        for s in ["m", "kg", "s", "A", "K", "mol", "cd"] {
            assert!(t().resolve(s).is_ok(), "{s} should resolve");
        }
    }

    #[test]
    fn prefixes_scale_the_factor() {
        assert_eq!(t().resolve("km").unwrap().factor, 1e3);
        assert_eq!(t().resolve("mm").unwrap().factor, 1e-3);
        assert_eq!(t().resolve("cm").unwrap().factor, 1e-2);
        assert_eq!(t().resolve("kN").unwrap().factor, 1e3);
        assert_eq!(t().resolve("MPa").unwrap().factor, 1e6);
        assert_eq!(t().resolve("GPa").unwrap().factor, 1e9);
    }

    #[test]
    fn exact_matches_beat_prefixes() {
        // `min` is a minute, not a milli-inch.
        assert_eq!(t().resolve("min").unwrap().factor, 60.0);
        // `cd` is a candela, not a centi-day.
        assert_eq!(t().resolve("cd").unwrap().dim, d(0, 0, 0, 0, 0, 0, 1));
        // `T` is a tesla, not a bare tera- prefix.
        assert_eq!(t().resolve("T").unwrap().dim, d(0, 1, -2, -1, 0, 0, 0));
        // `h` is an hour.
        assert_eq!(t().resolve("h").unwrap().factor, 3600.0);
    }

    #[test]
    fn unknown_units_are_named_in_the_error() {
        match t().resolve("furlong") {
            Err(UnitError::UnknownUnit(n)) => assert_eq!(n, "furlong"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn imperial_units_are_not_prefixable() {
        // `kin` is not a kilo-inch.
        assert!(t().resolve("kin").is_err());
    }

    #[test]
    fn affine_units_reject_prefixes() {
        assert!(matches!(
            t().resolve("k°C"),
            Err(UnitError::NotPrefixable(_))
        ));
    }

    // Derived constants are computed from the defining ones rather than typed
    // in, and these tests pin the relationships so a typo cannot slip through.

    #[test]
    fn psi_is_one_pound_force_per_square_inch() {
        let psi = t().resolve("psi").unwrap();
        let lbf = t().resolve("lbf").unwrap();
        let inch = t().resolve("in").unwrap();
        let expected = lbf.factor / (inch.factor * inch.factor);
        assert_eq!(psi.factor, expected);
        assert!((psi.factor - 6894.757293168361).abs() < 1e-9);
    }

    #[test]
    fn ksi_is_a_thousand_psi() {
        assert_eq!(
            t().resolve("ksi").unwrap().factor,
            1000.0 * t().resolve("psi").unwrap().factor
        );
    }

    #[test]
    fn kip_is_a_thousand_pounds_force() {
        assert_eq!(
            t().resolve("kip").unwrap().factor,
            1000.0 * t().resolve("lbf").unwrap().factor
        );
    }

    #[test]
    fn a_slug_is_a_pound_force_second_squared_per_foot() {
        let slug = t().resolve("slug").unwrap();
        assert!((slug.factor - 14.593902937206363).abs() < 1e-9);
    }

    #[test]
    fn temperature_scales_carry_offsets() {
        let c = t().resolve("°C").unwrap();
        assert!(c.is_affine());
        assert_eq!(c.offset, Some(273.15));

        let f = t().resolve("°F").unwrap();
        assert!(f.is_affine());

        // Rankine is absolute, so linear despite being an imperial scale.
        let r = t().resolve("°R").unwrap();
        assert!(!r.is_affine());
    }

    #[test]
    fn kelvin_is_linear() {
        assert!(!t().resolve("K").unwrap().is_affine());
    }

    #[test]
    fn radians_are_dimensionless() {
        assert!(t().resolve("rad").unwrap().dim.is_dimensionless());
        assert!(t().resolve("°").unwrap().dim.is_dimensionless());
        assert!(t().resolve("%").unwrap().dim.is_dimensionless());
    }

    #[test]
    fn apparent_and_reactive_power_match_watts_dimensionally() {
        let w = t().resolve("W").unwrap().dim;
        assert_eq!(t().resolve("VA").unwrap().dim, w);
        assert_eq!(t().resolve("var").unwrap().dim, w);
    }

    #[test]
    fn declared_units_resolve() {
        let mut table = t();
        let force = d(1, 1, -2, 0, 0, 0, 0);
        table.declare("tonf", 9964.016418183, force);
        let u = table.resolve("tonf").unwrap();
        assert_eq!(u.dim, force);
        assert!(!u.prefixable);
    }

    #[test]
    fn every_unit_in_the_smath_corpus_resolves() {
        // The 33 distinct symbols found across the 54 surveyed worksheets,
        // minus the two user-invented ones (`ΔF`, `g.e`) which a worksheet must
        // declare for itself.
        let corpus = [
            "in", "mm", "MPa", "m", "ft", "kN", "kip", "ksi", "cm", "sec", "lbf", "psi", "%", "°",
            "VA", "W", "V", "GPa", "kg", "var", "A", "Ω", "BTU", "lb", "gal", "min", "°F", "K",
            "N", "slug", "s",
        ];
        let table = t();
        let missing: Vec<&str> = corpus
            .iter()
            .copied()
            .filter(|s| !table.contains(s))
            .collect();
        assert!(missing.is_empty(), "unresolved: {missing:?}");
    }
}
