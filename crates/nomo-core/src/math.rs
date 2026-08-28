//! Deterministic floating-point mathematics.
//!
//! **Nothing in this crate may call `f64::sin`, `f64::powf` or their siblings
//! directly. Everything goes through here.**
//!
//! # Why this module exists
//!
//! A worksheet must produce identical results on every machine. WebAssembly
//! gives most of that away for free: its specification mandates IEEE 754 with
//! round-to-nearest-ties-to-even, there is no x87 excess precision to worry
//! about, and there is no implicit fused multiply-add because the deterministic
//! core has no fused multiply-add instruction at all. General floating-point
//! arithmetic does not appear in the specification's own enumeration of
//! nondeterminism sources.
//!
//! Transcendentals are the exception, and the whole reason for this module.
//! IEEE 754 does not specify `sin`, `exp`, `ln` or `pow`, so every platform's
//! `libm` differs in the last bits — the drift CalcpadCE documents in its own
//! build, where rendered decimals must be compared with tolerance because
//! "AVX2 CPU extensions of different architectures/CPU manufacturers behave
//! slightly different at the edge of precision".
//!
//! WebAssembly defines *no* transcendental instructions whatsoever. Its entire
//! floating-point instruction set is `fadd`, `fsub`, `fmul`, `fdiv`, `fsqrt`,
//! `fabs`, `fneg`, `fceil`, `ffloor`, `ftrunc`, `fnearest`, `fmin`, `fmax`,
//! `fcopysign` and comparisons. So every transcendental must come from
//! somewhere, and the choice of *where* is the entire determinism question:
//!
//! * the host's `libm`, or JavaScript's `Math.sin` — differs between machines;
//! * a library compiled into our own artifact — identical everywhere.
//!
//! We take the second. `libm` is a pure-Rust implementation, so the same source
//! produces the same bits on every target.
//!
//! # What may bypass this module
//!
//! The five operations IEEE 754 *does* specify exactly — addition, subtraction,
//! multiplication, division and square root — plus sign and rounding
//! manipulation. Those are bit-reproducible by specification, so `a + b` and
//! `x.sqrt()` are written directly. Everything else is here.

/// `x` raised to the power `y`.
pub fn powf(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

/// Square root. Exactly specified by IEEE 754, so the intrinsic is safe; routed
/// through here anyway so callers need not remember which is which.
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}

pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}

pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}

pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}

pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

pub fn log10(x: f64) -> f64 {
    libm::log10(x)
}

pub fn log2(x: f64) -> f64 {
    libm::log2(x)
}

pub fn sinh(x: f64) -> f64 {
    libm::sinh(x)
}

pub fn cosh(x: f64) -> f64 {
    libm::cosh(x)
}

pub fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

pub fn cbrt(x: f64) -> f64 {
    libm::cbrt(x)
}

pub fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

/// Absolute value. IEEE-exact.
pub fn abs(x: f64) -> f64 {
    x.abs()
}

/// Round half away from zero. IEEE-exact.
pub fn round(x: f64) -> f64 {
    x.round()
}

pub fn floor(x: f64) -> f64 {
    x.floor()
}

pub fn ceil(x: f64) -> f64 {
    x.ceil()
}

/// Normalise a NaN to a single canonical bit pattern.
///
/// NaN payload bits, and the sign of a NaN produced from non-NaN inputs, are the
/// documented nondeterminism in WebAssembly floating point — the one hole the
/// specification leaves open. Any value crossing a boundary where its bits could
/// be observed (the document format, a golden file, the renderer) passes through
/// here first, so that two machines cannot disagree about a NaN's representation
/// even in principle.
pub fn canonicalize(x: f64) -> f64 {
    if x.is_nan() {
        f64::NAN
    } else {
        x
    }
}

#[cfg(test)]
// Exact float comparison is the point here, not an oversight: this engine
// promises bit-reproducible results, so its tests assert bit equality.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn identities_hold() {
        assert!((sin(0.0)).abs() < 1e-15);
        assert!((cos(0.0) - 1.0).abs() < 1e-15);
        assert!((exp(0.0) - 1.0).abs() < 1e-15);
        assert!((ln(1.0)).abs() < 1e-15);
        assert!((sqrt(4.0) - 2.0).abs() < 1e-15);
        assert!((powf(2.0, 10.0) - 1024.0).abs() < 1e-12);
    }

    #[test]
    fn sin_squared_plus_cos_squared() {
        for i in 0..100 {
            let x = f64::from(i) * 0.1;
            let s = sin(x);
            let c = cos(x);
            assert!((s * s + c * c - 1.0).abs() < 1e-12, "failed at {x}");
        }
    }

    #[test]
    fn exact_powers_are_exact() {
        // These must be bit-exact, not merely close, on every target.
        assert_eq!(powf(2.0, 2.0), 4.0);
        assert_eq!(powf(10.0, 3.0), 1000.0);
        assert_eq!(powf(5.0, 0.0), 1.0);
    }

    #[test]
    fn canonicalize_leaves_ordinary_values_alone() {
        assert_eq!(canonicalize(1.5).to_bits(), 1.5f64.to_bits());
        assert_eq!(canonicalize(0.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(canonicalize(-0.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(canonicalize(f64::INFINITY), f64::INFINITY);
    }

    #[test]
    fn canonicalize_collapses_nan_payloads() {
        // Two NaNs with different payloads must become one bit pattern.
        let a = f64::from_bits(0x7ff8_0000_0000_0001);
        let b = f64::from_bits(0xfff8_0000_dead_beef);
        assert!(a.is_nan() && b.is_nan());
        assert_eq!(canonicalize(a).to_bits(), canonicalize(b).to_bits());
    }
}
