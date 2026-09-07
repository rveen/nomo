//! The import, as one JSON payload a host can render.
//!
//! This exists because the importer acquired a second caller. `src/bin/import.rs`
//! writes prose for a terminal and aggregates a corpus; the browser needs the
//! same facts as data, for one worksheet, so that it can put a marker beside the
//! line it belongs to and let a reader click it.
//!
//! It lives here rather than in `nomo-wasm` for the same reason
//! [`nomo_core::api::vocabulary_json`] lives in the engine: the crate that owns
//! [`NoteKind`] and [`Verdict`] is the crate that should decide what they are
//! called on the wire. `nomo-wasm` stays what its own documentation says it is —
//! the edge, moving bytes and deciding nothing.
//!
//! # Everything is reported, including the failure to read
//!
//! A `.sm` that will not parse is a *user's* mistake — the wrong file, a
//! truncated download — not a caller's, so it comes back as a payload with an
//! `error` field rather than as a null pointer the host can only describe as
//! "the engine rejected its input". The rule that nothing is silently dropped
//! applies to the reason a worksheet was rejected as much as to a construct
//! inside one.

use nomo_core::api::push_string;

use crate::emit::{Emitted, NoteKind};
use crate::oracle::{self, Report, Verdict};

/// The payload version, so a host can refuse what it does not understand.
///
/// Separate from `nomo_core::golden::FORMAT`: the engine's snapshot format and
/// this report change for unrelated reasons, and tying them together would make
/// every importer change look like an engine change to a cautious host.
pub const FORMAT: u32 = 1;

/// Read, translate and check `bytes`, as JSON.
///
/// `language` picks which translation of a multilingual worksheet to keep, and
/// is the same policy argument [`crate::emit_in`] takes — the design note is
/// explicit that first-match is not a decision anyone made (§8.9).
pub fn import_json(bytes: &[u8], language: Option<&str>) -> String {
    let worksheet = match crate::read(bytes) {
        Ok(w) => w,
        Err(e) => {
            let mut out = format!("{{\"format\":{FORMAT},\"error\":");
            push_string(&mut out, &e.to_string());
            out.push('}');
            return out;
        }
    };

    let emitted = crate::emit_in(&worksheet, language);
    // The oracle, not just the emitter. Translating a worksheet and *checking it
    // against the answers that worksheet already carries* are one action from a
    // reader's point of view: the second is what makes the first believable, and
    // splitting them would put the reassurance behind a second click nobody
    // presses. It is also why the importer belongs in this module rather than in
    // a separate lazily-loaded one — the check needs the whole evaluator, so a
    // second module would carry a second copy of it.
    let report = oracle::check_emitted(&emitted);
    payload(&emitted, &report)
}

fn payload(emitted: &Emitted, report: &Report) -> String {
    let mut out = format!("{{\"format\":{FORMAT},\"source\":");
    push_string(&mut out, &emitted.source);

    out.push_str(",\"notes\":[");
    for (i, note) in emitted.notes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("{{\"line\":{},\"kind\":", note.line));
        push_string(&mut out, name_of_note(note.kind));
        out.push_str(",\"detail\":");
        push_string(&mut out, &note.detail);
        out.push('}');
    }

    out.push_str("],\"checks\":[");
    for (i, check) in report.checks.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("{{\"line\":{},\"verdict\":", check.line));
        push_string(&mut out, name_of_verdict(&check.verdict));
        out.push_str(",\"detail\":");
        push_string(&mut out, &check.detail);
        // Both sides in base SI, and only when both exist. A null here is not a
        // zero: it says the comparison never got as far as two numbers.
        out.push_str(",\"computed\":");
        push_number(&mut out, check.computed);
        out.push_str(",\"expected\":");
        push_number(&mut out, check.expected);
        out.push('}');
    }
    out.push(']');

    out.push('}');
    out
}

/// A float as JSON, or `null`.
///
/// JSON has no infinity and no NaN, and a host that received the bare token
/// `NaN` would fail to parse the whole payload rather than the one field. Both
/// become null, which is what they mean here: there is no number to compare.
fn push_number(out: &mut String, value: Option<f64>) {
    match value {
        Some(v) if v.is_finite() => out.push_str(&format!("{v:?}")),
        _ => out.push_str("null"),
    }
}

/// The wire name of a note kind.
///
/// Deliberately not the prose `src/bin/import.rs` prints. That is a sentence for
/// a person reading a terminal and it should stay free to be reworded; this is
/// an identifier a host branches on, and it must not move when the prose does.
fn name_of_note(kind: NoteKind) -> &'static str {
    match kind {
        NoteKind::Unsupported => "unsupported",
        NoteKind::Carried => "carried",
        NoteKind::Renamed => "renamed",
        NoteKind::Collision => "collision",
        NoteKind::ScopeFlattened => "scope-flattened",
    }
}

fn name_of_verdict(verdict: &Verdict) -> &'static str {
    match verdict {
        Verdict::Agreed => "agreed",
        Verdict::Disagreed => "disagreed",
        Verdict::LineFailed => "line-failed",
        Verdict::AnswerUnreadable => "answer-unreadable",
        Verdict::ShapeDiffers => "shape-differs",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest worksheet that still has a stored answer, written here
    /// rather than taken from a corpus: the corpora are fetched, not committed,
    /// and a unit test that only runs on a machine that has them is not a test.
    const WORKSHEET: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<regions>
  <region><math><input><e type="operand">2</e><e type="operand">3</e><e type="operator" args="2">+</e></input><result action="numeric"><e type="operand">5</e></result></math></region>
</regions>"#;

    #[test]
    fn reports_source_and_a_checked_answer() {
        let json = import_json(WORKSHEET, None);
        assert!(json.starts_with("{\"format\":1,\"source\":"));
        assert!(json.contains("\"verdict\":\"agreed\""), "{json}");
        assert!(json.contains("\"checks\":["));
    }

    #[test]
    fn a_file_that_is_not_a_worksheet_reports_why() {
        let json = import_json(b"not xml at all", None);
        assert!(json.contains("\"error\":"), "{json}");
        // And nothing else: a host must not find an empty `source` here and
        // load it into an editor as though the import had succeeded.
        assert!(!json.contains("\"source\":"), "{json}");
    }

    #[test]
    fn invalid_utf8_is_an_error_rather_than_a_panic() {
        let json = import_json(&[0xff, 0xfe, 0x00], None);
        assert!(json.contains("\"error\":"), "{json}");
    }

    /// A non-finite value must not reach the payload as a bare `NaN` token: one
    /// unparseable field would cost the host the whole import.
    #[test]
    fn non_finite_numbers_become_null() {
        let mut out = String::new();
        push_number(&mut out, Some(f64::NAN));
        push_number(&mut out, Some(f64::INFINITY));
        push_number(&mut out, None);
        assert_eq!(out, "nullnullnull");
    }
}
