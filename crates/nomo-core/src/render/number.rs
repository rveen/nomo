//! Number formatting.
//!
//! Internals are binary; presentation is decimal. Values are stored as `f64` and
//! rounded to significant figures only here, at the edge, which is what keeps
//! arithmetic reproducible while output stays readable.
//!
//! # No transcendentals
//!
//! The obvious way to find a number's decimal exponent is `log10`, and that would
//! be a mistake twice over: it is a transcendental, so it would have to route
//! through the vendored `libm`, and it is inexact at the boundaries — `log10` of
//! a power of ten can land a hair below the integer and floor to the wrong
//! exponent. Instead the exponent is read back from Rust's own scientific
//! formatting, which is correctly rounded and exact.

use core::fmt::Write;

/// How to present numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    /// Significant figures. Six is what most engineering worksheets use.
    pub significant_figures: usize,
    /// Below this decimal exponent, switch to scientific notation.
    pub lower_exponent: i32,
    /// At or above this decimal exponent, switch to scientific notation.
    pub upper_exponent: i32,
}

impl Default for NumberFormat {
    fn default() -> Self {
        NumberFormat {
            significant_figures: 6,
            lower_exponent: -4,
            upper_exponent: 9,
        }
    }
}

/// Format `x` to the requested significant figures.
pub fn format(x: f64, fmt: &NumberFormat) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "∞".into() } else { "-∞".into() };
    }
    if x == 0.0 {
        // Negative zero prints as zero; the sign is not information a reader of
        // an engineering worksheet wants.
        return "0".into();
    }

    let sig = fmt.significant_figures.max(1);
    // Rust's scientific formatting is correctly rounded, so this both rounds to
    // `sig` figures and hands back the exact decimal exponent.
    let sci = format!("{:.*e}", sig - 1, x);
    let (mantissa, exp) = split_scientific(&sci);

    if exp < fmt.lower_exponent || exp >= fmt.upper_exponent {
        let m = trim_trailing_zeros(mantissa);
        let mut out = String::new();
        let _ = write!(out, "{m}e{exp}");
        return out;
    }

    // Plain decimal. The number of decimal places needed to show `sig` figures
    // follows from the exponent.
    let decimals = sig as i32 - 1 - exp;
    if decimals >= 0 {
        let plain = format!("{x:.*}", decimals as usize);
        return trim_trailing_zeros(&plain).to_string();
    }

    // More integer digits than significant figures, so the trailing ones are not
    // significant and must be zeroed: 1234.5678 to three figures is 1230, not
    // 1235. Formatting with zero decimal places would keep every digit, which is
    // why the already-rounded mantissa is reused instead.
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    format!(
        "{}{digits}{}",
        if negative { "-" } else { "" },
        "0".repeat(decimals.unsigned_abs() as usize)
    )
}

/// Split `1.234e-5` into its mantissa and exponent.
fn split_scientific(s: &str) -> (&str, i32) {
    match s.split_once('e') {
        Some((m, e)) => (m, e.parse().unwrap_or(0)),
        None => (s, 0),
    }
}

/// Drop trailing zeros after a decimal point, and a bare trailing point.
fn trim_trailing_zeros(s: &str) -> &str {
    if !s.contains('.') {
        return s;
    }
    let s = s.trim_end_matches('0');
    s.strip_suffix('.').unwrap_or(s)
}

#[cfg(test)]
// Formatting is exact by construction, so exact comparison is the right test.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn f(x: f64) -> String {
        format(x, &NumberFormat::default())
    }

    fn with_sig(x: f64, sig: usize) -> String {
        format(
            x,
            &NumberFormat {
                significant_figures: sig,
                ..Default::default()
            },
        )
    }

    #[test]
    fn whole_numbers_have_no_decimal_point() {
        assert_eq!(f(1.0), "1");
        assert_eq!(f(42.0), "42");
        assert_eq!(f(24525.0), "24525");
        assert_eq!(f(-7.0), "-7");
    }

    #[test]
    fn zero_and_negative_zero_are_both_zero() {
        assert_eq!(f(0.0), "0");
        assert_eq!(f(-0.0), "0");
    }

    #[test]
    fn significant_figures_are_respected() {
        assert_eq!(with_sig(std::f64::consts::PI, 3), "3.14");
        assert_eq!(with_sig(std::f64::consts::PI, 6), "3.14159");
        assert_eq!(with_sig(1234.5678, 3), "1230");
        assert_eq!(with_sig(1234.5678, 6), "1234.57");
    }

    #[test]
    fn the_cylinder_from_the_design_note() {
        // π · (5 cm)² · (12 cm) expressed in dm³.
        assert_eq!(with_sig(0.9424777960769379, 3), "0.942");
        assert_eq!(with_sig(0.9424777960769379, 6), "0.942478");
    }

    #[test]
    fn small_numbers_switch_to_scientific() {
        assert_eq!(f(0.0001), "0.0001");
        assert_eq!(f(0.00001), "1e-5");
        assert_eq!(f(1.5e-9), "1.5e-9");
    }

    #[test]
    fn large_numbers_switch_to_scientific() {
        assert_eq!(f(1e8), "100000000");
        assert_eq!(f(1e9), "1e9");
        assert_eq!(f(1.23e12), "1.23e12");
    }

    #[test]
    fn powers_of_ten_land_on_the_right_exponent() {
        // The case that would break if the exponent came from `log10`.
        for e in -3..=8 {
            // Through the vendored libm, like everything else: the CI guard
            // rightly refuses a direct `powi` here.
            let x = crate::math::powf(10.0, f64::from(e));
            let got = f(x);
            assert!(
                !got.contains('e'),
                "10^{e} formatted as {got}, expected plain decimal"
            );
        }
    }

    #[test]
    fn rounding_follows_the_stored_value_not_the_typed_one() {
        // 0.15 is not 0.15: the nearest f64 is a hair below, so one significant
        // figure is 0.1. That is the honest answer, and — far more importantly —
        // it is the same answer everywhere, which is the property that matters.
        assert_eq!(with_sig(0.15, 1), "0.1");
        assert_eq!(with_sig(-0.15, 1), "-0.1");
        // A value unambiguously above the boundary rounds up.
        assert_eq!(with_sig(0.16, 1), "0.2");
    }

    #[test]
    fn insignificant_integer_digits_are_zeroed_not_kept() {
        // The bug this guards: formatting with zero decimal places keeps every
        // digit, so 1234.5678 came out as 1235 rather than 1230.
        assert_eq!(with_sig(1234.5678, 3), "1230");
        assert_eq!(with_sig(-1234.5678, 3), "-1230");
        assert_eq!(with_sig(98765.0, 2), "99000");
        assert_eq!(with_sig(1234.5678, 1), "1000");
    }

    #[test]
    fn non_finite_values_are_readable() {
        assert_eq!(f(f64::NAN), "NaN");
        assert_eq!(f(f64::INFINITY), "∞");
        assert_eq!(f(f64::NEG_INFINITY), "-∞");
    }

    #[test]
    fn formatting_is_deterministic() {
        let x = 1.0 / 3.0;
        let first = f(x);
        for _ in 0..100 {
            assert_eq!(f(x), first);
        }
    }
}
