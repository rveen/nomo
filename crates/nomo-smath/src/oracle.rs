//! Checking an imported worksheet against the answers SMath stored in it.
//!
//! This is the reason the corpus exists (design note §8.2 finding 2, §8.9): 553
//! regions across 51 of 54 worksheets carry the number SMath itself computed, so
//! importer and engine can be validated together against real files with no
//! hand-written expectations anywhere.
//!
//! # The comparison, and why it is not a simple equality
//!
//! A stored answer is what SMath **displayed**, not what it held. `96` shown to
//! no decimal places in inches says only that the true value lies within half an
//! inch of 96 in — so the tolerance is half a unit in the last displayed place,
//! measured in the unit it was displayed in.
//!
//! That last clause is the part that is easy to get wrong, and there are two
//! traps in it. Nomo holds quantities in base SI, so a computed `2.4384 m`
//! against a stored `96` needs the size of one display unit before the stored
//! precision means anything. And SMath writes a large answer in scientific form,
//! `1.8491*10^5`, where its `precision` setting counts decimals **of the
//! mantissa** — so reading that setting as decimals of the value makes the
//! tolerance a hundred thousand times too tight, and the corpus then reports
//! correct answers as disagreements.
//!
//! Both are avoided by not consulting the setting at all. The stored literal
//! states its own precision: `1.8491` means ±0.00005 of a mantissa, and scaling
//! by `expected / 1.8491` carries that through the exponent and the units at
//! once, whatever they are.
//!
//! This is the one place in the project where a tolerance is legitimate. The
//! golden-file suite compares bit-exactly and must keep doing so — there, both
//! sides are Nomo's own output and any difference is a bug. Here the other side
//! is a decimal string written by a different program a decade ago, and
//! demanding bit equality of it would be demanding that SMath had stored more
//! digits than it displayed.

use nomo_core::{OutcomeKind, Value};

use crate::emit::{self, Emitted};
use crate::read::Worksheet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    /// Nomo's answer matches SMath's, within the precision SMath displayed.
    Agreed,
    /// Both computed a value and they differ. The interesting outcome.
    Disagreed,
    /// The imported line did not evaluate — an unsupported construct, an unknown
    /// name, a unit Nomo does not have.
    LineFailed,
    /// The stored answer itself did not evaluate, so there is nothing to compare
    /// against — a unit Nomo has no spelling for, or a shape it cannot read.
    AnswerUnreadable,
    /// The two sides are different shapes — a number against a table, or two
    /// tables of different sizes — so there is nothing to compare element by
    /// element.
    ShapeDiffers,
}

#[derive(Debug, Clone)]
pub struct Checked {
    pub line: usize,
    pub verdict: Verdict,
    /// What the two sides were, in base SI, when both produced a number.
    pub computed: Option<f64>,
    pub expected: Option<f64>,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub checks: Vec<Checked>,
}

impl Report {
    pub fn count(&self, v: &Verdict) -> usize {
        self.checks.iter().filter(|c| c.verdict == *v).count()
    }

    /// Of the assertions that could be compared at all, the share that agreed.
    ///
    /// Reported separately from coverage on purpose: "how much can be imported"
    /// and "is what was imported right" are different questions, and averaging
    /// them hides both.
    pub fn agreement(&self) -> Option<f64> {
        let comparable = self.count(&Verdict::Agreed) + self.count(&Verdict::Disagreed);
        (comparable > 0).then(|| self.count(&Verdict::Agreed) as f64 / comparable as f64)
    }
}

/// Import a worksheet, evaluate it, and check every stored answer.
pub fn check(w: &Worksheet) -> (Emitted, Report) {
    let emitted = emit::emit(w);
    let report = check_emitted(&emitted);
    (emitted, report)
}

pub fn check_emitted(emitted: &Emitted) -> Report {
    let (outcomes, _) = nomo_core::run_source(&emitted.source);
    let line_of = LineIndex::new(&emitted.source);

    let mut report = Report::default();
    for assertion in &emitted.assertions {
        let outcome = outcomes
            .iter()
            .find(|o| line_of.line(o.span.start as usize) == assertion.line);

        let computed = match outcome.map(|o| &o.kind) {
            Some(OutcomeKind::Query(t)) | Some(OutcomeKind::Assign { trace: t, .. }) => {
                t.value.as_ref().ok()
            }
            _ => None,
        };
        let Some(computed) = computed else {
            report.checks.push(Checked {
                line: assertion.line,
                verdict: Verdict::LineFailed,
                computed: None,
                expected: None,
                detail: outcome
                    .map(|o| {
                        o.diagnostics
                            .first()
                            .map(|d| d.message.clone())
                            .unwrap_or_else(|| "did not evaluate".into())
                    })
                    .unwrap_or_else(|| "no statement on that line".into()),
            });
            continue;
        };

        let Some(stored) = evaluate(&assertion.expected) else {
            report.checks.push(Checked {
                line: assertion.line,
                verdict: Verdict::AnswerUnreadable,
                computed: None,
                expected: None,
                detail: format!("`{}` did not evaluate", assertion.expected),
            });
            continue;
        };

        // One pair per element, so that a scalar and a table of roots go through
        // the same comparison. The mantissas come alongside: a stored vector
        // writes each element to its own significant places, so each element
        // states its own tolerance.
        let (mine, theirs) = (elements_of(computed), elements_of(&stored));
        let (Some(mine), Some(theirs)) = (mine, theirs) else {
            report.checks.push(Checked {
                line: assertion.line,
                verdict: Verdict::ShapeDiffers,
                computed: None,
                expected: None,
                detail: "not a number or a table of them".into(),
            });
            continue;
        };
        if mine.len() != theirs.len() {
            report.checks.push(Checked {
                line: assertion.line,
                verdict: Verdict::ShapeDiffers,
                computed: None,
                expected: None,
                detail: format!("{} value(s) against {} stored", mine.len(), theirs.len()),
            });
            continue;
        }

        let mut tolerances = Vec::with_capacity(theirs.len());
        for (i, t) in theirs.iter().enumerate() {
            // A stored zero states no precision — there is no last significant
            // place to be half of — so there is nothing to derive a tolerance
            // from, and the answer is unreadable rather than wrong.
            let literal = assertion
                .elements
                .get(i)
                .unwrap_or(&assertion.mantissa)
                .as_str();
            let Some(tolerance) = tolerance_for(t.value, literal) else {
                tolerances.clear();
                break;
            };
            tolerances.push(tolerance);
        }
        if tolerances.len() != theirs.len() {
            report.checks.push(Checked {
                line: assertion.line,
                verdict: Verdict::AnswerUnreadable,
                computed: None,
                expected: None,
                detail: format!("`{}` states no precision", assertion.expected),
            });
            continue;
        }

        // The first element that does not match is what a reader needs to see;
        // when they all match, the first pair is as good a witness as any.
        let mut off = None;
        let mut dimensions_differ = false;
        for (i, (q, t)) in mine.iter().zip(theirs.iter()).enumerate() {
            // Half a unit in the last displayed place, and *inclusive*: SMath
            // rounds half away from zero, so a value exactly half a unit below
            // the stored digits is what those digits mean — `7.2.sm` computes
            // `337.5 mm²` where SMath stored `338 mm²`, and they are the same
            // answer. Both sides arrive here as binary approximations of decimal
            // strings, so that exact boundary lands a few parts in 10¹³ outside
            // the tolerance. One part in 10¹² covers it and nothing else: the
            // nearest value that would genuinely round to different digits is a
            // whole unit away, which is twice the tolerance.
            const BOUNDARY: f64 = 1e-12;
            let matches =
                q.dim == t.dim && (q.value - t.value).abs() <= tolerances[i] * (1.0 + BOUNDARY);
            if !matches {
                dimensions_differ = q.dim != t.dim;
                off = Some(i);
                break;
            }
        }
        let witness = off.unwrap_or(0);
        report.checks.push(Checked {
            line: assertion.line,
            verdict: if off.is_none() {
                Verdict::Agreed
            } else {
                Verdict::Disagreed
            },
            computed: Some(mine[witness].value),
            expected: Some(theirs[witness].value),
            detail: if dimensions_differ {
                String::from("dimensions differ")
            } else if mine.len() > 1 && off.is_some() {
                format!("element {} of {}", witness + 1, mine.len())
            } else {
                String::new()
            },
        });
    }
    report
}

/// A value as a flat list of quantities, in reading order, or `None` for
/// something that is not a number or a table of them.
///
/// A vector and a one-column matrix are the same answer written two ways —
/// SMath stores several roots as `mat(a, b, 2, 1)` and Nomo computes a vector —
/// so both flatten here rather than being told apart by shape.
fn elements_of(v: &Value) -> Option<Vec<nomo_core::Quantity>> {
    match v {
        Value::Scalar(q) => Some(vec![*q]),
        Value::Vector(v) => Some(v.elements.clone()),
        Value::Matrix(m) => Some(m.data.clone()),
        // Real part then imaginary, which is the order SMath writes them and
        // therefore the order their literals arrive in. A complex answer
        // against a real one then reports as one value against two, which is
        // what it is: an impedance is not a resistance.
        Value::Complex(c) => Some(vec![c.real_part(), c.imaginary_part()]),
        _ => None,
    }
}

/// How close a match has to be, given the literal the answer was written as.
///
/// Half a unit in the last place the mantissa was written to, scaled by
/// everything between the mantissa and the base-SI value: the power of ten, the
/// unit, or both.
fn tolerance_for(expected: f64, mantissa: &str) -> Option<f64> {
    let m: f64 = mantissa.parse().ok()?;
    if m == 0.0 {
        return None;
    }
    let half_ulp = 0.5 * 10f64.powi(-(decimals_of(mantissa) as i32));
    Some(half_ulp * (expected / m).abs())
}

/// Digits written after the decimal point.
fn decimals_of(literal: &str) -> u32 {
    match literal.split_once('.') {
        Some((_, after)) => after.chars().filter(char::is_ascii_digit).count() as u32,
        None => 0,
    }
}

/// Evaluate a stored answer on its own. It is always a literal — a stored
/// answer never refers to a worksheet variable — so it needs no context.
fn evaluate(source: &str) -> Option<Value> {
    let (outcomes, _) = nomo_core::run_source(source);
    for o in outcomes {
        if let OutcomeKind::Query(t) = o.kind {
            if let Ok(v) = t.value {
                return Some(v);
            }
        }
    }
    None
}

/// Byte offset to 1-based line, precomputed so that a worksheet with hundreds of
/// assertions does not rescan its own source for each one.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(source: &str) -> LineIndex {
        let mut starts = vec![0];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        LineIndex { starts }
    }

    fn line(&self, offset: usize) -> usize {
        match self.starts.binary_search(&offset) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::Assertion;

    fn emitted(source: &str, assertions: Vec<Assertion>) -> Emitted {
        Emitted {
            source: source.into(),
            notes: Vec::new(),
            assertions,
        }
    }

    fn assertion(line: usize, expected: &str, mantissa: &str) -> Assertion {
        Assertion {
            line,
            expected: expected.into(),
            mantissa: mantissa.into(),
            elements: Vec::new(),
        }
    }

    /// A stored answer with one literal per element, which is what a table of
    /// roots arrives as.
    fn table(line: usize, expected: &str, elements: &[&str]) -> Assertion {
        Assertion {
            line,
            expected: expected.into(),
            mantissa: elements.first().copied().unwrap_or("0").into(),
            elements: elements.iter().map(|e| (*e).to_string()).collect(),
        }
    }

    #[test]
    fn a_table_of_roots_is_compared_element_by_element() {
        // SMath stores several roots as an n×1 matrix, and this construct's
        // characteristic answer is exactly that: `CEE3500Test01Problem03.sm`
        // computes [1.17307, 3.57795] against a stored mat(1.1731, 3.5779).
        let e = emitted(
            "x = [1.173071, 3.577912]\n",
            vec![table(1, "[1.1731, 3.5779]", &["1.1731", "3.5779"])],
        );
        assert_eq!(check_emitted(&e).checks[0].verdict, Verdict::Agreed);
        // Each element states its own precision. `1.74` allows ±0.005 and
        // `0.757` allows ±0.0005, so the same absolute error is inside one and
        // outside the other.
        let e = emitted(
            "x = [0.7576, 1.742]\n",
            vec![table(1, "[0.757, 1.74]", &["0.757", "1.74"])],
        );
        assert_eq!(check_emitted(&e).checks[0].verdict, Verdict::Disagreed);
        let e = emitted(
            "x = [0.7573, 1.742]\n",
            vec![table(1, "[0.757, 1.74]", &["0.757", "1.74"])],
        );
        assert_eq!(check_emitted(&e).checks[0].verdict, Verdict::Agreed);
    }

    #[test]
    fn a_complex_answer_is_compared_part_by_part() {
        // `ElecEngExample.sm` stores `(14.88 - 9.47i) A` for a current Nomo
        // computes as `(14.8824 - 9.47059i) A`. Both parts are inside the
        // precision SMath displayed, and neither would be if one mantissa had
        // to speak for the pair: `196.18 - 20.29i` is five significant figures
        // beside four.
        let e = emitted(
            "x = (14.8824 - 9.47059*i) A\n",
            vec![table(1, "(14.88 - 9.47*i) A", &["14.88", "9.47"])],
        );
        assert_eq!(check_emitted(&e).checks[0].verdict, Verdict::Agreed);
        // An imaginary part outside its own last displayed place is a
        // disagreement, and the witness says which part it was.
        let e = emitted(
            "x = (14.8824 - 9.5*i) A\n",
            vec![table(1, "(14.88 - 9.47*i) A", &["14.88", "9.47"])],
        );
        let r = check_emitted(&e);
        assert_eq!(r.checks[0].verdict, Verdict::Disagreed);
        assert!(
            r.checks[0].detail.contains("element 2 of 2"),
            "{:?}",
            r.checks[0]
        );
    }

    #[test]
    fn a_complex_answer_against_a_real_one_is_a_shape_difference() {
        // An impedance is not a resistance, and reporting one value against two
        // says so rather than comparing a real part and calling it agreement.
        let e = emitted(
            "x = 14.88 A\n",
            vec![assertion(1, "(14.88 - 9.47*i) A", "14.88")],
        );
        let r = check_emitted(&e);
        assert_eq!(r.checks[0].verdict, Verdict::ShapeDiffers);
        assert!(
            r.checks[0].detail.contains("1 value(s) against 2"),
            "{:?}",
            r.checks[0]
        );
    }

    #[test]
    fn two_tables_of_different_sizes_are_not_compared() {
        let e = emitted("x = [1, 2, 3]\n", vec![table(1, "[1, 2]", &["1", "2"])]);
        let r = check_emitted(&e);
        assert_eq!(r.checks[0].verdict, Verdict::ShapeDiffers);
        assert!(r.checks[0].detail.contains("3 value(s) against 2"));
    }

    #[test]
    fn a_value_exactly_half_a_unit_from_the_display_agrees() {
        // `7.2.sm` computes 337.5 mm² where SMath stored 338 mm². Half a unit
        // in the last displayed place is what those digits mean, and both sides
        // are binary approximations of decimals, so the boundary has to be
        // reachable.
        let e = emitted("x = 337.5 mm^2\n", vec![assertion(1, "338 mm^2", "338")]);
        assert_eq!(check_emitted(&e).checks[0].verdict, Verdict::Agreed);
        // A whole unit away is a different display and stays a disagreement.
        let e = emitted("x = 337 mm^2\n", vec![assertion(1, "338 mm^2", "338")]);
        assert_eq!(check_emitted(&e).checks[0].verdict, Verdict::Disagreed);
    }

    #[test]
    fn an_answer_within_the_displayed_precision_agrees() {
        // 489/sqrt(2) is 345.7757…, and SMath stored 345.78 to two places.
        let e = emitted("x = 489/sqrt(2)\n", vec![assertion(1, "345.78", "345.78")]);
        let r = check_emitted(&e);
        assert_eq!(r.checks[0].verdict, Verdict::Agreed, "{:?}", r.checks[0]);
    }

    #[test]
    fn an_answer_outside_it_disagrees() {
        let e = emitted("x = 500\n", vec![assertion(1, "345.78", "345.78")]);
        let r = check_emitted(&e);
        assert_eq!(r.checks[0].verdict, Verdict::Disagreed);
    }

    #[test]
    fn the_tolerance_is_measured_in_the_unit_it_was_displayed_in() {
        // 96 in displayed to no decimal places. The true value may be anywhere
        // within half an inch — 0.0127 m — and 2.44 m is inside that while
        // 2.5 m is not. Comparing ±0.5 in *metres* would accept both.
        let inside = emitted("x = 2.44 m\n", vec![assertion(1, "96 in", "96")]);
        let outside = emitted("x = 2.5 m\n", vec![assertion(1, "96 in", "96")]);
        assert_eq!(check_emitted(&inside).checks[0].verdict, Verdict::Agreed);
        assert_eq!(
            check_emitted(&outside).checks[0].verdict,
            Verdict::Disagreed
        );
    }

    #[test]
    fn the_right_number_with_the_wrong_dimension_is_not_agreement() {
        let e = emitted("x = 96 kg\n", vec![assertion(1, "96 in", "96")]);
        let r = check_emitted(&e);
        assert_eq!(r.checks[0].verdict, Verdict::Disagreed);
        assert_eq!(r.checks[0].detail, "dimensions differ");
    }

    #[test]
    fn a_line_that_does_not_evaluate_is_not_counted_as_a_disagreement() {
        // Coverage and correctness are separate questions; folding an import gap
        // into the agreement rate would hide both.
        let e = emitted(
            "x = nosuchname + 1\n",
            vec![assertion(1, "345.78", "345.78")],
        );
        let r = check_emitted(&e);
        assert_eq!(r.checks[0].verdict, Verdict::LineFailed);
        assert_eq!(r.agreement(), None);
    }

    #[test]
    fn an_unreadable_stored_answer_is_reported_rather_than_failed() {
        // A unit Nomo has no spelling for: the stored answer does not evaluate
        // at all, so there is nothing to compare against and nothing to blame
        // the engine for.
        let e = emitted("x = 1\n", vec![assertion(1, "14.88 nosuchunit", "14.88")]);
        let r = check_emitted(&e);
        assert_eq!(r.checks[0].verdict, Verdict::AnswerUnreadable);
    }

    #[test]
    fn a_complex_stored_answer_is_a_shape_this_cannot_compare() {
        // It evaluates — that is the difference from the case above — and there
        // is no element-by-element comparison for it yet. Reporting the shape
        // rather than the reading says which of the two it is.
        let e = emitted("x = 1\n", vec![assertion(1, "14.88 - 9.47*i", "14.88")]);
        let r = check_emitted(&e);
        assert_eq!(r.checks[0].verdict, Verdict::ShapeDiffers);
    }

    #[test]
    fn agreement_counts_only_what_could_be_compared() {
        let e = emitted(
            "a = 1\nb = 2\nc = nosuch\n",
            vec![
                assertion(1, "1", "1"),
                assertion(2, "99", "99"),
                assertion(3, "3", "3"),
            ],
        );
        let r = check_emitted(&e);
        assert_eq!(r.count(&Verdict::Agreed), 1);
        assert_eq!(r.count(&Verdict::Disagreed), 1);
        assert_eq!(r.count(&Verdict::LineFailed), 1);
        assert_eq!(r.agreement(), Some(0.5));
    }

    #[test]
    fn a_scientific_answer_is_not_held_to_the_precision_of_its_mantissa() {
        // SMath stores 184914.14 as `1.8491*10^5`. The mantissa's four decimals
        // mean ±5 in the value, not ±0.00005: reading the document's
        // `precision` as decimals of the value reported this as a disagreement.
        let e = emitted(
            "x = 184914.14359029522\n",
            vec![assertion(1, "1.8491*10^5", "1.8491")],
        );
        assert_eq!(check_emitted(&e).checks[0].verdict, Verdict::Agreed);

        // And it is still a real check: 184960 is outside ±5.
        let e = emitted(
            "x = 184960.0\n",
            vec![assertion(1, "1.8491*10^5", "1.8491")],
        );
        assert_eq!(check_emitted(&e).checks[0].verdict, Verdict::Disagreed);
    }

    #[test]
    fn decimals_are_counted_from_the_literal() {
        assert_eq!(decimals_of("345.78"), 2);
        assert_eq!(decimals_of("96"), 0);
        assert_eq!(decimals_of("1.8491"), 4);
    }

    #[test]
    fn lines_are_found_by_offset_including_the_first_and_last() {
        let index = LineIndex::new("a\nbb\nccc\n");
        assert_eq!(index.line(0), 1);
        assert_eq!(index.line(2), 2);
        assert_eq!(index.line(5), 3);
    }
}
