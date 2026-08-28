//! A worksheet's complete rendered output as one deterministic text document.
//!
//! This is the unit the golden-file suite diffs, and it lives in the engine
//! rather than in the CLI for a specific reason: in phase 7 the same function is
//! called from the WebAssembly build and the two outputs are compared byte for
//! byte. If the snapshot were assembled by the CLI, that comparison would be
//! testing the CLI instead of the numerics.
//!
//! It is therefore a pure function of `(name, source)`. Nothing here reads a
//! clock, a path or an environment variable, so the only way two machines can
//! produce different bytes is if they computed different numbers — which is the
//! bug the suite exists to catch.
//!
//! # What is captured
//!
//! The whole trace, not just final values: the expression as written, the
//! substituted form, and the result, for every statement. That pins substitution,
//! unit conversion and number formatting at the same time as the arithmetic, so a
//! change to any of them shows up in a diff beside the code that caused it. The
//! HTML body and the diagnostics are captured for the same reason — they are
//! output too, and output that nothing checks is output that drifts.
//!
//! # Why there is a separate `values` section
//!
//! The rendered columns show six significant figures, so on their own they
//! cannot see a difference in the last bits of a `f64` — which is precisely the
//! drift this project claims to have eliminated and therefore precisely what the
//! suite has to be able to catch. Perturbing π by one unit in the last place was
//! invisible in the rendered text.
//!
//! So the snapshot also records every result as its base-SI magnitude at full
//! round-trip precision. Rust's `{:?}` for `f64` prints the shortest decimal that
//! reads back as the same bits, which is exact without being a wall of hex. Two
//! machines that disagree by one bit now disagree in this section.

use crate::diag::Severity;
use crate::doc::Sheet;
use crate::eval::OutcomeKind;
use crate::quantity::{Kind, Quantity};
use crate::render::{html, text, RenderOptions};
use crate::value::Value;

/// Bumped when the layout of a snapshot changes.
///
/// The version is the first line so that a stale expected file fails with a
/// legible one-line header diff instead of a whole-body diff that has to be read
/// carefully before it means anything.
pub const FORMAT: u32 = 1;

/// The file extension for a committed snapshot.
pub const EXTENSION: &str = "snap";

/// Render `source` to its snapshot. `name` is the worksheet's stem, which is
/// also the HTML document title, so it is an input to the output and has to be
/// passed rather than inferred.
pub fn snapshot(name: &str, source: &str) -> String {
    let sheet = Sheet::new(source);
    let opts = RenderOptions::default();

    let mut out = String::new();
    out.push_str(&format!("# nomo snapshot v{FORMAT}\n"));
    out.push_str(&format!("# worksheet: {name}\n"));

    out.push_str("\n=== text ===\n");
    out.push_str(&text::render(&sheet, &opts));

    out.push_str("\n=== html ===\n");
    out.push_str(&html::body(&sheet, &opts));

    out.push_str("\n=== values ===\n");
    let mut any = false;
    for outcome in sheet.outcomes() {
        let (label, trace) = match &outcome.kind {
            OutcomeKind::Assign { name, trace } => (name.as_str(), trace),
            OutcomeKind::UnitDecl { name, trace } => (name.as_str(), trace),
            // A query has no name of its own; the expression it asked about is
            // already pinned in the text section above.
            OutcomeKind::Query(trace) => ("?", trace),
            _ => continue,
        };
        any = true;
        match &trace.value {
            Ok(value) => out.push_str(&format!("{label} = {}\n", exact(value))),
            Err(e) => out.push_str(&format!("{label} = error: {e}\n")),
        }
    }
    if !any {
        out.push_str("none\n");
    }

    out.push_str("\n=== diagnostics ===\n");
    if sheet.diagnostics().is_empty() {
        out.push_str("none\n");
    } else {
        // Engine order, not sorted: which diagnostic is reported first is
        // behaviour a user sees, so a change to it should fail the suite.
        for d in sheet.diagnostics() {
            let (line, col) = d.span.line_col(source);
            let severity = match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            out.push_str(&format!(
                "{severity}[{}] {line}:{col}: {}\n",
                d.code, d.message
            ));
        }
    }

    out
}

/// A value as base-SI magnitudes at full round-trip precision.
///
/// Deliberately not the display form: this section exists to see what six
/// significant figures cannot.
fn exact(value: &Value) -> String {
    match value {
        Value::Scalar(q) => quantity(q),
        Value::Complex(c) => format!("{:?}{:+?}i {}", c.re, c.im, c.dim),
        // Quoted and verbatim: a string has no magnitude to record at full
        // precision, and the bytes are the whole of what could drift.
        Value::Text(t) => format!("{t:?}"),
        // A dual lives only inside a `derivative` call and never becomes a
        // result, so nothing can put one here. Written out rather than left to
        // a catch-all so that the day one escapes, the snapshot says so.
        Value::Dual(d) => format!("{:?} d={:?} {}", d.value.value, d.d, d.value.dim),
        // Every sample at full precision, which is the whole point of this
        // section: the drawn SVG rounds to what a chart can show, so a
        // last-bit drift in the samples would be invisible in the HTML and
        // visible only here. Verbose on purpose — a golden file is read by a
        // diff, and a plot that moved must show which points moved.
        Value::Plot(p) => {
            let measured = p.extent == crate::plot::Extent::Measured;
            let kind = if measured { "measured" } else { "chosen" };
            let spans = format!(
                "{:?}..{:?} ({kind}) {} over {}",
                p.from, p.to, p.x_dim, p.y_dim
            );
            let curves: Vec<String> = p
                .series
                .iter()
                .map(|s| {
                    // A sampled curve's abscissae are `from + i*step` and the
                    // span above already pins them, so only the ordinates are
                    // written; a table's are data and both are. Writing both in
                    // either case would double the largest section of every
                    // snapshot to repeat what the span already says.
                    let points: Vec<String> = s
                        .points
                        .iter()
                        .map(|(x, y)| {
                            if measured {
                                format!("({x:?}, {y:?})")
                            } else {
                                format!("{y:?}")
                            }
                        })
                        .collect();
                    format!("{}: [{}]", s.name, points.join(", "))
                })
                .collect();
            format!("plot {spans} {{{}}}", curves.join("; "))
        }
        Value::Vector(v) => {
            let parts: Vec<String> = v.elements.iter().map(quantity).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Matrix(m) => {
            let rows: Vec<String> = (0..m.rows)
                .map(|r| {
                    let cells: Vec<String> = (0..m.cols).map(|c| quantity(&m.get(r, c))).collect();
                    format!("[{}]", cells.join(", "))
                })
                .collect();
            format!("[{}]", rows.join(", "))
        }
    }
}

fn quantity(q: &Quantity) -> String {
    // The dimension is part of the value: two results that agree numerically but
    // differ in dimension are not the same answer.
    let dim = q.dim.to_string();
    // A point on an offset scale is a different thing from an interval along it,
    // and the magnitude alone does not say which.
    let kind = match q.kind {
        Kind::Point => " point",
        Kind::Interval => "",
    };
    let magnitude = number(q.value);
    if dim.is_empty() {
        format!("{magnitude}{kind}")
    } else {
        format!("{magnitude} {dim}{kind}")
    }
}

/// One `f64` at full round-trip precision, with every NaN written the same way.
///
/// The NaN case is the reason this is a function rather than a `{:?}`.
/// WebAssembly leaves the payload bits of a NaN, and the sign of a NaN computed
/// from non-NaN operands, up to the implementation — they are the *only*
/// float nondeterminism the specification admits (design note §3). A NaN
/// produced natively and the same NaN produced in a browser may therefore differ
/// in bits while both being correct, and if those bits reached a snapshot the
/// golden suite would fail for something that is not a bug.
///
/// Rust's `Debug` for `f64` already prints `NaN` for every payload and both
/// signs, so this is currently what it would do anyway. It is written out
/// regardless: the guarantee is load-bearing for the cross-target comparison and
/// should not rest on a formatting detail of the standard library that no test
/// would notice changing. `is_nan` is a bit inspection, not arithmetic, so it
/// costs nothing in determinism.
fn number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    format!("{value:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CYLINDER: &str = "r = 5 cm\nh = 12 cm\nV = pi*r^2*h\nV -> dm^3\n";

    #[test]
    fn snapshot_is_stable_across_calls() {
        assert_eq!(
            snapshot("cylinder", CYLINDER),
            snapshot("cylinder", CYLINDER)
        );
    }

    #[test]
    fn every_section_is_present_and_in_order() {
        let snap = snapshot("cylinder", CYLINDER);
        let text = snap.find("=== text ===").expect("text section");
        let html = snap.find("=== html ===").expect("html section");
        let values = snap.find("=== values ===").expect("values section");
        let diags = snap
            .find("=== diagnostics ===")
            .expect("diagnostics section");
        assert!(text < html && html < values && values < diags);
        assert!(snap.starts_with("# nomo snapshot v1\n# worksheet: cylinder\n"));
    }

    #[test]
    fn a_difference_below_the_displayed_precision_is_still_visible() {
        // The reason the values section exists. Results show six significant
        // figures, so the rendered text cannot tell these two apart; a snapshot
        // that only captured the rendering would pass an engine that had drifted
        // in the last bits, which is the one failure this project must catch.
        let a = snapshot("t", "x = 3.141592653589793\n");
        let b = snapshot("t", "x = 3.141592653589792\n");

        let rendered = |s: &str| {
            let start = s.find("=== text ===").unwrap();
            s[start..s.find("=== html ===").unwrap()].to_string()
        };
        assert_eq!(
            rendered(&a),
            rendered(&b),
            "precondition: the display forms should agree"
        );
        assert_ne!(a, b, "the snapshots must not agree");
    }

    #[test]
    fn values_are_recorded_in_base_si_at_full_precision() {
        let snap = snapshot("cylinder", CYLINDER);
        // 300π cm³ in m³, to every digit that round-trips.
        assert!(
            snap.contains("V = 0.000942477796076938 m^3"),
            "expected a full-precision base-SI magnitude:\n{snap}"
        );
    }

    #[test]
    fn every_nan_is_written_the_same_way() {
        // WebAssembly does not pin NaN payload bits or the sign of a computed
        // NaN. If either reached a snapshot, the native and browser builds would
        // disagree without either being wrong.
        let quiet = f64::NAN;
        let negative = -f64::NAN;
        let with_payload = f64::from_bits(0x7ff8_0000_0000_00ff);
        let negative_payload = f64::from_bits(0xfff8_0000_0000_0abc);

        assert!(negative.is_sign_negative(), "precondition: sign differs");
        assert_ne!(
            quiet.to_bits(),
            with_payload.to_bits(),
            "precondition: payloads differ"
        );

        for nan in [quiet, negative, with_payload, negative_payload] {
            assert_eq!(number(nan), "NaN", "bits {:016x} leaked", nan.to_bits());
        }
    }

    #[test]
    fn the_infinities_and_signed_zero_are_kept_apart() {
        // Unlike a NaN payload these are exactly specified, so they are pinned
        // rather than normalised: collapsing them would hide real differences.
        assert_eq!(number(f64::INFINITY), "inf");
        assert_eq!(number(f64::NEG_INFINITY), "-inf");
        assert_eq!(number(-0.0), "-0.0");
        assert_eq!(number(0.0), "0.0");
    }

    #[test]
    fn a_failed_statement_records_its_error() {
        let snap = snapshot("t", "x = 1 m + 1 s\n");
        assert!(snap.contains("x = error:"), "{snap}");
    }

    #[test]
    fn the_whole_trace_is_captured_not_just_the_result() {
        let snap = snapshot("cylinder", CYLINDER);
        // Symbolic, substituted and result forms all appear.
        assert!(snap.contains("π·r²·h"), "symbolic form missing:\n{snap}");
        assert!(
            snap.contains("(5 cm)²"),
            "substituted form missing:\n{snap}"
        );
        assert!(snap.contains("0.942478 dm³"), "result missing:\n{snap}");
    }

    #[test]
    fn a_clean_worksheet_says_none_rather_than_nothing() {
        // An empty section and a section that was never written look the same in
        // a diff; "none" distinguishes them.
        assert!(snapshot("cylinder", CYLINDER).ends_with("=== diagnostics ===\nnone\n"));
    }

    #[test]
    fn diagnostics_are_captured_with_position() {
        let snap = snapshot("broken", "x = 1 +\n");
        assert!(
            snap.contains("error[SH004] 1:8:"),
            "expected a positioned diagnostic:\n{snap}"
        );
    }

    #[test]
    fn the_name_reaches_the_output() {
        // The stem is the HTML title, so it is part of what is being pinned.
        let snap = snapshot("beam", CYLINDER);
        assert!(snap.contains("# worksheet: beam"));
    }

    #[test]
    fn no_trailing_whitespace_anywhere() {
        // Trailing spaces survive an editor's "strip on save" only by accident,
        // and make a golden file fail for a reason unrelated to the engine.
        let snap = snapshot("cylinder", CYLINDER);
        for (n, line) in snap.lines().enumerate() {
            assert_eq!(line.trim_end(), line, "line {} has trailing space", n + 1);
        }
    }
}
