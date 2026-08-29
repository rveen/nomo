//! The importer, checked on a worksheet this project wrote.
//!
//! Everything else that exercises the importer needs the corpora, which are
//! third-party, fetched rather than committed, and absent on a machine that has
//! not run `scripts/fetch-corpora.sh` — including, at the time of writing, every
//! CI runner. `check-corpus.sh` skips what it cannot find, so on such a machine
//! the importer has no test at all.
//!
//! `fixtures/pipe.sm` closes that. It is an SMath 1.x worksheet written here, by
//! hand, in the format the reader documents: our numbers, our prose, ours to
//! commit and ours to show. It carries one of each thing that matters — a text
//! region, a global `≡`, positional `:` definitions with units, a computed
//! value, two stored answers to check against, and one construct that is out of
//! scope by decision so that a refusal is exercised too.
//!
//! It is also the worked example in `docs/smath.md`, so this test is what keeps
//! that document true.

use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pipe.sm")
}

fn worksheet() -> nomo_smath::Worksheet {
    let bytes = std::fs::read(fixture()).expect("the fixture should be readable");
    nomo_smath::read(&bytes).expect("the fixture should parse as a worksheet")
}

fn imported() -> String {
    nomo_smath::emit(&worksheet()).source
}

#[test]
fn the_fixture_reads_and_emits_a_worksheet_that_evaluates() {
    let source = imported();
    let sheet = nomo_core::Sheet::new(&source);
    assert!(
        !sheet.has_errors(),
        "the imported worksheet does not evaluate: {:?}\n{source}",
        sheet.diagnostics()
    );
}

#[test]
fn prose_units_and_globals_survive_the_crossing() {
    let source = imported();
    // Prose is carried, not discarded: a worksheet is a document.
    assert!(source.contains("' A pipe run, sized for a target velocity"));
    // `≡` is a global definition, and stays one.
    assert!(source.contains("global g = 9.81"), "{source}");
    // Units come across as units rather than as magnitudes.
    assert!(source.contains("d = 100 mm"), "{source}");
    assert!(source.contains("Q = 15*(L/s)"), "{source}");
}

#[test]
fn what_cannot_be_translated_is_visible() {
    // Never a silent drop. The Maxima call is out of scope by decision — design
    // note §8.12 — and the reader says so where it stood.
    let source = imported();
    assert!(
        source.contains("[import] unsupported") && source.contains("Maxima"),
        "the refusal should name the construct: {source}"
    );
}

#[test]
fn nomo_computes_what_smath_stored() {
    // The oracle, on one worksheet and without the corpora. SMath's own answers
    // are in the file — 1.90986 m/s and 7853.98 mm² — and Nomo has to reach
    // them from the definitions rather than from the stored values.
    use nomo_smath::Verdict;
    let (_, report) = nomo_smath::check(&worksheet());
    assert_eq!(report.count(&Verdict::Agreed), 2, "{:?}", report.checks);
    assert_eq!(report.count(&Verdict::Disagreed), 0, "{:?}", report.checks);
}
