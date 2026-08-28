//! Reads SMath Studio `.sm` worksheets.
//!
//! This crate reads; it does not evaluate and it does not yet write Nomo
//! documents. It exists so that the coverage report described in the design note
//! §8.8 can be produced before any semantics are committed to: run the reader
//! over every worksheet available, count what it cannot handle, and let that
//! ranking decide what gets built.
//!
//! # The format has two eras
//!
//! This is the single most important structural fact about `.sm`, and it is not
//! in the design note, which was written from two samples that happened to be
//! read only for their token streams. Measured across the 54-worksheet corpus,
//! the format changes shape at **version 0.88**:
//!
//! | | 0.82–0.85 (35 files) | 0.88–0.98 (19 files) |
//! |---|---|---|
//! | Math body | `<e>` directly under `<math>` | `<e>` under `<math><input>` |
//! | Stored answer | second operand of a binary `=` | `<result action="numeric">` |
//! | Display unit of an answer | carried in the expression | `<contract>` |
//! | `=` operators in the corpus | 247 | **0** |
//!
//! An importer written to the older shape alone finds *no math at all* in 19 of
//! 54 files, because `<e>` is not a child of `<math>` there. It would report a
//! clean import of an empty worksheet, which is the failure mode the design note
//! (item 23) most wants to avoid.
//!
//! The container changed again at 1.x — a `<worksheet>` root, an XML namespace,
//! and nesting moved from `<area>` to `<text>`. See [`read`] for that table; the
//! shape of the *math* is what [`read::Era`] records, and the two changed
//! independently.
//!
//! # Both eras carry an oracle, and together they carry more than was thought
//!
//! The design note counts 247 stored results across 32 files. That is the older
//! mechanism only. Adding `<result>` gives **553 stored answers across 51 of the
//! 54 files** — and the newer form is the richer of the two, because it is a full
//! postfix expression that can carry units and complex values rather than a bare
//! rounded scalar. The 1.x mechanics corpus adds **877 more, across all 60 of its
//! files**, 477 of them naming a display unit.
//!
//! Only `action="numeric"` counts. A `symbolic` result records what SMath's CAS
//! derived and a `none` result holds an unevaluated equation; 174 of those are
//! kept as provenance for whoever reviews the migration and are never asserted,
//! because a no-CAS engine will not reproduce them.
//!
//! # Reading is total
//!
//! Nothing here returns an error for an unrecognised construct. Anything the
//! reader does not understand becomes an explicit marker in the tree —
//! [`Expr::Unsupported`], [`Payload::Unsupported`] — carrying enough context to
//! be counted, located and reported. A migration tool that drops what it cannot
//! read is worse than one that refuses to run.

pub mod builtins;
pub mod coverage;
pub mod emit;
pub mod expr;
pub mod oracle;
pub mod read;
pub mod resolve;

pub use coverage::{Coverage, Issue, IssueKind};
pub use emit::{emit, emit_in, Assertion, Emitted, Note, NoteKind};
pub use expr::{Assign, Expr};
pub use oracle::{check, Checked, Verdict};
pub use read::{Dependency, Era, Math, Payload, Region, ResultKind, Settings, Worksheet};

/// Read a worksheet from the bytes of a `.sm` file.
///
/// The BOM and CRLF line endings that SMath writes are handled here rather than
/// by the caller.
pub fn read(bytes: &[u8]) -> Result<Worksheet, read::ReadError> {
    let mut w = read::worksheet(bytes)?;
    // Unit resolution belongs here rather than in the reader or in one consumer:
    // `style="unit"` is a display style, and a worksheet that took it literally
    // is wrong for the coverage report, the emitter and the oracle alike. See
    // [`resolve`].
    resolve::units(&mut w);
    Ok(w)
}
