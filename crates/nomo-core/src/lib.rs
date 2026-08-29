//! Nomo engine core.
//!
//! This crate is deliberately free of I/O, threads, clocks and randomness: it is
//! compiled unchanged to `wasm32-unknown-unknown` for the browser and linked into
//! the native CLI. Anything that reads a file or asks the operating system a
//! question belongs in `nomo-cli`, not here.
//!
//! Invariants this crate exists to uphold (see `docs/design-note.md`):
//!
//! * One grammar, one AST. Diagnostics, formatting and highlighting all consume
//!   the same syntax tree the evaluator does.
//! * Evaluation yields an annotated tree, never a bare value, so that a worksheet
//!   can show its work.
//! * Transcendentals come from a vendored `libm`, never from the host.

pub mod api;
pub mod ast;
pub mod complex;
pub mod diag;
pub mod dim;
pub mod doc;
pub mod dual;
pub mod eval;
pub mod golden;
pub mod graph;
pub mod lex;
pub mod math;
pub mod packs;
pub mod parse;
pub mod plot;
pub mod prose;
pub mod quantity;
pub mod render;
pub mod resource;
pub mod span;
pub mod trace;
pub mod unit;
pub mod value;

pub use api::{analysis_json, ClassifiedToken, TokenClass};
pub use complex::ComplexQuantity;
pub use diag::{Diagnostic, Severity};
pub use dim::{Dimension, Ratio};
pub use doc::{Document, Recalculation, Sheet};
pub use eval::{run_source, Env, Outcome, OutcomeKind};
pub use golden::snapshot;
pub use graph::DepGraph;
pub use plot::{PlotValue, Series};
pub use prose::Block;
pub use quantity::{Kind, Quantity};
pub use render::{RenderOptions, Renderer};
pub use resource::{Image, Reference, Resources, Size};
pub use span::Span;
pub use trace::{Trace, TraceNode};
pub use unit::{Unit, UnitError, UnitTable};
pub use value::{EvalError, Value};

/// Parse a worksheet source into a syntax tree plus any diagnostics.
///
/// Parsing never fails outright: a document with errors still yields a tree
/// covering everything that could be understood, because the editor needs
/// diagnostics on input that is mid-keystroke.
pub fn parse(source: &str) -> parse::Parsed {
    parse::parse(source)
}
