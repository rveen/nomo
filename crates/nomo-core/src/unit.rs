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
    /// What `factor` and `offset` are exactly. `None` for a declared unit, whose
    /// factor was computed in `f64` by the worksheet that declared it.
    pub exact: Option<Definition>,
}

/// A built-in unit's factor and offset as exact numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Definition {
    pub factor: Exact,
    pub offset: Option<Exact>,
}

/// An exact constant: `num / den · π^pi`, in lowest terms with `den > 0`.
///
/// The engine computes in `f64`, and this is not used by evaluation at all. It
/// is what each constant of the table *means*, for a host that must compute
/// exactly: a store that keeps decimals and rounds once cannot take `0.0254`
/// back out of the nearest binary64. Every factor in the table is rational but
/// the degree's, which is why π has an exponent of its own rather than a
/// rational approximation. `i128` holds every built-in factor times every
/// prefix with room to spare: the largest, a yotta-bar, is 10²⁹.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exact {
    num: i128,
    den: i128,
    pi: i32,
}

fn gcd128(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

impl Exact {
    pub const ONE: Exact = Exact {
        num: 1,
        den: 1,
        pi: 0,
    };

    pub const fn int(n: i128) -> Exact {
        Exact {
            num: n,
            den: 1,
            pi: 0,
        }
    }

    pub fn ratio(num: i128, den: i128) -> Exact {
        assert!(den != 0, "exact constant with zero denominator");
        let sign = if den < 0 { -1 } else { 1 };
        let g = gcd128(num, den);
        Exact {
            num: sign * num / g,
            den: sign * den / g,
            pi: 0,
        }
    }

    /// A decimal as its digits and a power of ten: `dec(254, -4)` is 0.0254.
    /// Written this way so each definition reads as the number it defines.
    pub fn dec(digits: i128, exp10: i32) -> Exact {
        let p = 10i128.pow(exp10.unsigned_abs());
        if exp10 < 0 {
            Exact::ratio(digits, p)
        } else {
            Exact::int(digits * p)
        }
    }

    /// This constant times π^k.
    pub fn times_pi(self, k: i32) -> Exact {
        Exact {
            pi: self.pi + k,
            ..self
        }
    }

    pub fn mul(self, other: Exact) -> Exact {
        // Cross-reduce first, so intermediate products stay as small as the
        // result: `ksi` is a thousand pounds-force over a square inch.
        let g1 = gcd128(self.num, other.den);
        let g2 = gcd128(other.num, self.den);
        let num = (self.num / g1).checked_mul(other.num / g2);
        let den = (self.den / g2).checked_mul(other.den / g1);
        let (Some(num), Some(den)) = (num, den) else {
            panic!("exact constant out of range");
        };
        Exact {
            pi: self.pi + other.pi,
            ..Exact::ratio(num, den)
        }
    }

    pub fn div(self, other: Exact) -> Exact {
        assert!(other.num != 0, "exact constant divided by zero");
        self.mul(Exact {
            num: other.den,
            den: other.num,
            pi: -other.pi,
        })
    }

    pub fn numerator(self) -> i128 {
        self.num
    }

    pub fn denominator(self) -> i128 {
        self.den
    }

    pub fn pi_exponent(self) -> i32 {
        self.pi
    }
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

/// U+2126 OHM SIGN, which [`UnitTable::resolve`] reads as `Ω` (U+03A9).
const OHM_SIGN: char = '\u{2126}';

/// SI prefixes, longest symbol first so that `da` matches before `d`: the
/// symbol, its factor, and that factor's power of ten, for [`Exact`].
pub(crate) const PREFIXES: &[(&str, f64, i32)] = &[
    ("da", 1e1, 1),
    ("Y", 1e24, 24),
    ("Z", 1e21, 21),
    ("E", 1e18, 18),
    ("P", 1e15, 15),
    ("T", 1e12, 12),
    ("G", 1e9, 9),
    ("M", 1e6, 6),
    ("k", 1e3, 3),
    ("h", 1e2, 2),
    ("d", 1e-1, -1),
    ("c", 1e-2, -2),
    ("m", 1e-3, -3),
    ("µ", 1e-6, -6),
    ("u", 1e-6, -6),
    ("n", 1e-9, -9),
    ("p", 1e-12, -12),
    ("f", 1e-15, -15),
    ("a", 1e-18, -18),
    ("z", 1e-21, -21),
    ("y", 1e-24, -24),
];

/// `(symbol, factor, offset, dimension, prefixable, exact factor and offset)`
type Row = (&'static str, f64, Option<f64>, Dimension, bool, Definition);

/// A linear unit's exact definition.
fn linear(factor: Exact) -> Definition {
    Definition {
        factor,
        offset: None,
    }
}

fn builtin_rows() -> Vec<Row> {
    // Exact conversion constants, by definition:
    //   1 in  = 0.0254 m           1 lb  = 0.45359237 kg
    //   1 lbf = 4.4482216152605 N  1 BTU = 1055.05585262 J (IT)
    //
    // Each row carries its factor twice: as the `f64` the engine computes with,
    // and as the exact number that `f64` stands for. The `f64`s are computed as
    // they always were, so that no result moves; a test holds each to its exact
    // value within two units in the last place.
    let inch = 0.0254_f64;
    let lbf = 4.4482216152605_f64;
    let psi = lbf / (inch * inch);
    let exact_inch = Exact::dec(254, -4);
    let exact_ft = Exact::dec(3048, -4);
    let exact_lbf = Exact::dec(44482216152605, -13);
    let exact_psi = exact_lbf.div(exact_inch.mul(exact_inch));
    let one = linear(Exact::ONE);
    let exact_degree = linear(Exact::ratio(1, 180).times_pi(1));

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
        ("m", 1.0, None, length, true, one),
        ("g", 1e-3, None, mass, true, linear(Exact::dec(1, -3))),
        ("kg", 1.0, None, mass, false, one),
        ("s", 1.0, None, time, true, one),
        ("A", 1.0, None, current, true, one),
        ("K", 1.0, None, temp, true, one),
        ("mol", 1.0, None, amount, true, one),
        ("cd", 1.0, None, luminous, true, one),
        // ---- SI derived --------------------------------------------------
        ("N", 1.0, None, force, true, one),
        ("Pa", 1.0, None, pressure, true, one),
        ("J", 1.0, None, energy, true, one),
        ("W", 1.0, None, power, true, one),
        ("Hz", 1.0, None, frequency, true, one),
        ("C", 1.0, None, charge, true, one),
        ("V", 1.0, None, voltage, true, one),
        ("Ω", 1.0, None, resistance, true, one),
        ("ohm", 1.0, None, resistance, true, one),
        ("S", 1.0, None, d(-2, -1, 3, 2, 0, 0, 0), true, one),
        ("F", 1.0, None, d(-2, -1, 4, 2, 0, 0, 0), true, one),
        ("H", 1.0, None, d(2, 1, -2, -2, 0, 0, 0), true, one),
        ("Wb", 1.0, None, d(2, 1, -2, -1, 0, 0, 0), true, one),
        ("T", 1.0, None, d(0, 1, -2, -1, 0, 0, 0), true, one),
        ("L", 1e-3, None, volume, true, linear(Exact::dec(1, -3))),
        // Apparent and reactive power. Dimensionally watts; kept as separate
        // names because electrical worksheets distinguish them, and the SMath
        // corpus declares exactly these two (`VA : W`, `var : W`).
        ("VA", 1.0, None, power, true, one),
        ("var", 1.0, None, power, true, one),
        // ---- time --------------------------------------------------------
        ("sec", 1.0, None, time, false, one),
        ("min", 60.0, None, time, false, linear(Exact::int(60))),
        ("h", 3600.0, None, time, false, linear(Exact::int(3600))),
        ("hr", 3600.0, None, time, false, linear(Exact::int(3600))),
        ("day", 86400.0, None, time, false, linear(Exact::int(86400))),
        // ---- imperial and US customary ------------------------------------
        ("in", inch, None, length, false, linear(exact_inch)),
        ("ft", 0.3048, None, length, false, linear(exact_ft)),
        (
            "yd",
            0.9144,
            None,
            length,
            false,
            linear(Exact::dec(9144, -4)),
        ),
        (
            "mi",
            1609.344,
            None,
            length,
            false,
            linear(Exact::dec(1609344, -3)),
        ),
        (
            "mil",
            inch / 1000.0,
            None,
            length,
            false,
            linear(exact_inch.div(Exact::int(1000))),
        ),
        (
            "lb",
            0.45359237,
            None,
            mass,
            false,
            linear(Exact::dec(45359237, -8)),
        ),
        (
            "oz",
            0.028349523125,
            None,
            mass,
            false,
            linear(Exact::dec(28349523125, -12)),
        ),
        (
            "slug",
            lbf / 0.3048,
            None,
            mass,
            false,
            linear(exact_lbf.div(exact_ft)),
        ),
        ("lbf", lbf, None, force, false, linear(exact_lbf)),
        (
            "kip",
            1000.0 * lbf,
            None,
            force,
            false,
            linear(Exact::int(1000).mul(exact_lbf)),
        ),
        ("psi", psi, None, pressure, false, linear(exact_psi)),
        (
            "ksi",
            1000.0 * psi,
            None,
            pressure,
            false,
            linear(Exact::int(1000).mul(exact_psi)),
        ),
        (
            "bar",
            1e5,
            None,
            pressure,
            true,
            linear(Exact::int(100_000)),
        ),
        (
            "atm",
            101325.0,
            None,
            pressure,
            false,
            linear(Exact::int(101_325)),
        ),
        (
            "BTU",
            1055.05585262,
            None,
            energy,
            false,
            linear(Exact::dec(105505585262, -8)),
        ),
        (
            "gal",
            0.003785411784,
            None,
            volume,
            false,
            linear(Exact::dec(3785411784, -12)),
        ),
        // ---- dimensionless ------------------------------------------------
        // `rad` is dimensionless: angle is a ratio of lengths. `sin` therefore
        // accepts a bare number, and `sin(2)` and `sin(2 rad)` agree.
        ("rad", 1.0, None, none, true, one),
        (
            "°",
            core::f64::consts::PI / 180.0,
            None,
            none,
            false,
            exact_degree,
        ),
        (
            "deg",
            core::f64::consts::PI / 180.0,
            None,
            none,
            false,
            exact_degree,
        ),
        ("%", 0.01, None, none, false, linear(Exact::dec(1, -2))),
        // ---- affine temperature scales ------------------------------------
        // T_K = factor * x + offset.
        (
            "°C",
            1.0,
            Some(273.15),
            temp,
            false,
            Definition {
                factor: Exact::ONE,
                offset: Some(Exact::dec(27315, -2)),
            },
        ),
        (
            "°F",
            5.0 / 9.0,
            Some(459.67 * 5.0 / 9.0),
            temp,
            false,
            Definition {
                factor: Exact::ratio(5, 9),
                offset: Some(Exact::dec(45967, -2).mul(Exact::ratio(5, 9))),
            },
        ),
        // Rankine is an absolute scale, so it is linear despite being imperial.
        (
            "°R",
            5.0 / 9.0,
            None,
            temp,
            false,
            linear(Exact::ratio(5, 9)),
        ),
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
        for (symbol, factor, offset, dim, prefixable, exact) in builtin_rows() {
            units.insert(
                symbol.to_string(),
                Unit {
                    symbol: symbol.to_string(),
                    factor,
                    offset,
                    dim,
                    prefixable,
                    exact: Some(exact),
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
                exact: None,
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
    ///
    /// The ohm sign U+2126 is read as the Greek capital omega U+03A9 it is
    /// canonically equivalent to. They look identical, some keyboards and
    /// character maps produce the ohm sign, and a worksheet that rejected `kΩ`
    /// over a code point nobody can see would be wrong in a way no one could
    /// diagnose. Normalising here rather than adding a row keeps one spelling in
    /// results and lets prefixes work on both.
    pub fn resolve(&self, name: &str) -> Result<Unit, UnitError> {
        let normalised;
        let name = if name.contains(OHM_SIGN) {
            normalised = name.replace(OHM_SIGN, "Ω");
            normalised.as_str()
        } else {
            name
        };
        if let Some(u) = self.units.get(name) {
            return Ok(u.clone());
        }
        for (prefix, scale, exp10) in PREFIXES {
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
                exact: base_unit
                    .exact
                    .map(|e| linear(e.factor.mul(Exact::dec(1, *exp10)))),
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
    use std::collections::BTreeSet;

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
    fn the_ohm_sign_is_the_ohm() {
        let ohm = t().resolve("\u{2126}").unwrap();
        assert_eq!(ohm.symbol, "Ω");
        assert_eq!(ohm.dim, t().resolve("ohm").unwrap().dim);
        let kohm = t().resolve("k\u{2126}").unwrap();
        assert_eq!(kohm.symbol, "kΩ");
        assert_eq!(kohm.factor, 1e3);
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

    /// The "Every built-in unit" section of `docs/language.md`.
    fn section() -> &'static str {
        let doc = include_str!("../../../docs/language.md");
        let start = doc.find("### Every built-in unit").expect("section");
        let section = &doc[start..];
        &section[..section[4..].find("\n### ").unwrap() + 4]
    }

    /// The section's two tables, each as the backticked symbols in its symbol
    /// columns.
    fn documented() -> (BTreeSet<String>, BTreeSet<String>) {
        let tables: Vec<&str> = section()
            .split("\n\n")
            .filter(|b| b.starts_with('|'))
            .collect();
        assert_eq!(tables.len(), 2, "a unit table and a prefix table");
        // Every cell of a symbol column, split on commas: `µ`, `u` is two.
        let symbols = |table: &str, columns: &[usize]| {
            let mut out = BTreeSet::new();
            for row in table.lines().skip(2) {
                let cells: Vec<&str> = row.split('|').map(str::trim).collect();
                for &c in columns {
                    for sym in cells[c + 1].split(", ").filter(|x| !x.is_empty()) {
                        out.insert(sym.trim_matches('`').to_string());
                    }
                }
            }
            out
        };
        (symbols(tables[0], &[0]), symbols(tables[1], &[0, 4]))
    }

    #[test]
    fn the_language_reference_lists_every_unit() {
        // Both directions: a unit missing from the page is the gap this table
        // exists to close, and a unit on the page the engine does not know is
        // a promise the engine does not keep.
        let (units, prefixes) = documented();
        let built_in: BTreeSet<String> = builtin_rows().iter().map(|r| r.0.to_string()).collect();
        assert_eq!(units, built_in);
        let built_in: BTreeSet<String> = PREFIXES.iter().map(|p| p.0.to_string()).collect();
        assert_eq!(prefixes, built_in);
    }

    /// The `f64` nearest an exact constant, or within an ulp of it: every
    /// built-in unit's numerator and denominator is below 2⁵³, so each converts
    /// exactly and the division rounds once; π adds a second rounding.
    fn approx(e: Exact) -> f64 {
        let r = e.numerator() as f64 / e.denominator() as f64;
        match e.pi_exponent() {
            0 => r,
            1 => r * core::f64::consts::PI,
            k => panic!("π^{k} in a built-in constant"),
        }
    }

    /// How many representable doubles lie between two positive ones.
    fn ulps(a: f64, b: f64) -> u64 {
        assert!(a > 0.0 && b > 0.0, "{a} and {b} must be positive");
        a.to_bits().abs_diff(b.to_bits())
    }

    #[test]
    fn every_factor_is_its_exact_definition() {
        // The `f64`s are computed as they always were, some by several roundings
        // (`psi` is `lbf / (in * in)`), so each may sit an ulp or two from the
        // nearest double to what it means. More than that is a typo in one of
        // the two columns.
        for (symbol, factor, offset, _, _, exact) in builtin_rows() {
            let off = ulps(factor, approx(exact.factor));
            assert!(
                off <= 2,
                "{symbol}: factor is {off} ulps from its definition"
            );
            match (offset, exact.offset) {
                (None, None) => {}
                (Some(o), Some(e)) => {
                    let off = ulps(o, approx(e));
                    assert!(
                        off <= 2,
                        "{symbol}: offset is {off} ulps from its definition"
                    );
                }
                _ => panic!("{symbol}: offset and its definition disagree on existing"),
            }
        }
        // A prefix's literal is the nearest double to its power of ten, but
        // 10²⁴ is not itself a double, so the reference rounds twice for the
        // largest and smallest.
        for (symbol, scale, exp10) in PREFIXES {
            let off = ulps(*scale, approx(Exact::dec(1, *exp10)));
            assert!(
                off <= 1,
                "prefix {symbol} is {off} ulps from its power of ten"
            );
        }
    }

    #[test]
    fn exact_definitions_keep_the_relationships_floats_approximate() {
        let exact = |s: &str| t().resolve(s).unwrap().exact.unwrap();
        let (psi, lbf, inch) = (exact("psi").factor, exact("lbf").factor, exact("in").factor);
        assert_eq!(psi.mul(inch).mul(inch), lbf);
        assert_eq!(exact("ksi").factor, Exact::int(1000).mul(psi));
        assert_eq!(exact("slug").factor.mul(exact("ft").factor), lbf);
        assert_eq!(exact("°F").offset, Some(Exact::ratio(45967, 180)));
        assert_eq!(exact("°").factor.pi_exponent(), 1);
        // Prefixes are exact powers of ten, the ohm sign included.
        assert_eq!(exact("kN").factor, Exact::int(1000));
        assert_eq!(exact("µs").factor, Exact::ratio(1, 1_000_000));
        assert_eq!(exact("us").factor, exact("µs").factor);
        assert_eq!(exact("k\u{2126}").factor, Exact::int(1000));
        assert_eq!(exact("Ybar").factor, Exact::dec(1, 29));
        assert_eq!(exact("mg").factor, Exact::dec(1, -6));
    }

    #[test]
    fn declared_units_have_no_exact_definition() {
        let mut table = t();
        table.declare("tonf", 9964.016418183, d(1, 1, -2, 0, 0, 0, 0));
        assert_eq!(table.resolve("tonf").unwrap().exact, None);
    }

    #[test]
    fn the_language_reference_says_which_units_take_prefixes() {
        for (symbol, _, _, _, prefixable, _) in builtin_rows() {
            let row = format!("| `{symbol}` |");
            let line = section().lines().find(|l| l.starts_with(&row)).unwrap();
            let says = line.trim_end_matches(" |").rsplit("| ").next().unwrap();
            assert_eq!(says == "yes", prefixable, "{symbol}: the page says {says}");
        }
    }
}
