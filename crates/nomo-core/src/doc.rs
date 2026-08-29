//! The worksheet document, and incremental recalculation over it.
//!
//! # The format is the source text
//!
//! A `.nomo` file is the syntax, not a serialisation of it. That is the payoff
//! of choosing a text language: worksheets go into version control and review
//! like code, and there is no second representation to keep in step with the
//! first.
//!
//! # Versioning, from the first commit
//!
//! An optional pragma on the first line records which version of the format a
//! worksheet was written against:
//!
//! ```text
//! ' nomo 1
//! ```
//!
//! It is an ordinary comment, so nothing downstream needs to know about it, and a
//! file without one is read as version 1. Migrations are pure functions from one
//! version's text to the next, each with its own golden test.
//!
//! EngineeringPaper.xyz is the cautionary example here: its `Sheet.ts` is full of
//! fields commented "early sheets did not have this property" because versioning
//! was retrofitted. The constraint it was defending — old worksheets must always
//! open, with no data loss — is the right one, adopted late.

use crate::ast::{Ast, Expr, Stmt};
use crate::diag::{Diagnostic, Severity};
use crate::eval::{Env, Outcome, OutcomeKind};
use crate::graph::DepGraph;
use crate::resource::Resources;
use crate::span::Span;
use std::collections::BTreeSet;

/// The format version this build writes and understands.
pub const CURRENT_VERSION: u32 = 1;

/// Diagnostic codes raised by the document layer.
pub mod doc_codes {
    pub const FROM_THE_FUTURE: &str = "SH301";
    pub const CYCLE: &str = "SH302";
    /// `use` naming a pack this build does not carry.
    pub const UNKNOWN_PACK: &str = "SH303";
    /// The same pack used twice in one worksheet.
    pub const PACK_TWICE: &str = "SH304";
}

/// Read the `' nomo <n>` pragma, if the first line carries one.
fn read_version(source: &str) -> Option<u32> {
    let first = source.lines().next()?.trim();
    let rest = first.strip_prefix('\'')?.trim();
    let n = rest.strip_prefix("nomo")?.trim();
    n.parse().ok()
}

/// `source` with a version pragma, added if it has none.
///
/// Saving goes through this so a worksheet on disk says what format it is in. A
/// file without a pragma still reads as version 1 — that is what `read_version`
/// falling back means — but a file that says so can be migrated by a later build
/// rather than guessed at, and guessing is what the `Sheet.ts` cautionary tale in
/// design note §7 is about.
///
/// It lives in the engine rather than the front end because the version number
/// and the pragma's spelling are facts about the format. A JavaScript function
/// writing `' nomo 1` would be a second description of it, and would go on
/// saying `1` long after [`CURRENT_VERSION`] said otherwise.
///
/// An already-stamped worksheet is returned unchanged, including one declaring a
/// version this build does not understand. Rewriting a future worksheet's pragma
/// to claim it is version 1 would turn "I cannot fully read this" into silent
/// corruption.
pub fn stamp_version(source: &str) -> String {
    if read_version(source).is_some() {
        return source.to_string();
    }
    if source.is_empty() {
        return format!("' nomo {CURRENT_VERSION}\n");
    }
    format!("' nomo {CURRENT_VERSION}\n{source}")
}

/// A parsed worksheet.
pub struct Document {
    pub version: u32,
    pub source: String,
    pub ast: Ast,
    /// Statement indices that came from a pack rather than from the worksheet.
    pub from_packs: BTreeSet<usize>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Document {
    pub fn parse(source: &str) -> Document {
        let declared = read_version(source);
        let version = declared.unwrap_or(CURRENT_VERSION);
        let source = migrate(source, version);
        let parsed = crate::parse(&source);
        let mut diagnostics = parsed.diagnostics;

        if version > CURRENT_VERSION {
            // Refusing outright would be worse than trying: the worksheet is
            // probably mostly readable, and the alternative is a user staring at
            // a file they cannot open.
            diagnostics.insert(
                0,
                Diagnostic::warning(
                    doc_codes::FROM_THE_FUTURE,
                    Span::new(0, 0),
                    format!(
                        "this worksheet declares format version {version}, but this build \
                         understands {CURRENT_VERSION}; reading it anyway"
                    ),
                ),
            );
        }

        let mut ast = parsed.ast;
        let from_packs = splice_packs(&mut ast, &mut diagnostics);

        Document {
            version,
            source,
            ast,
            from_packs,
            diagnostics,
        }
    }
}

/// Replace every `use` with the statements of the pack it names.
///
/// Returns the indices of the statements that came from a pack, which is what
/// keeps them out of the rendered output: a worksheet that shows its work should
/// show *its* work, not fourteen constants nobody typed.
///
/// The spliced statements take the span of the `use` line that brought them.
/// Their own spans point into the pack's source, which is a different string
/// from the one every consumer here slices — the editor draws diagnostics with
/// them and the highlighter indexes the worksheet with them, so a span from
/// another file is not merely wrong but out of bounds. Pointing at the `use`
/// line is also the right answer for a reader: that is the line they wrote.
fn splice_packs(ast: &mut Ast, diagnostics: &mut Vec<Diagnostic>) -> BTreeSet<usize> {
    if !ast.stmts.iter().any(|s| matches!(s, Stmt::Use { .. })) {
        return BTreeSet::new();
    }

    let mut out: Vec<Stmt> = Vec::with_capacity(ast.stmts.len());
    let mut from_packs = BTreeSet::new();
    let mut used: Vec<String> = Vec::new();

    for stmt in ast.stmts.drain(..) {
        let Stmt::Use { name, span } = &stmt else {
            out.push(stmt);
            continue;
        };
        let (name, span) = (name.clone(), *span);

        let Some(pack) = crate::packs::find(&name.text) else {
            diagnostics.push(Diagnostic::error(
                doc_codes::UNKNOWN_PACK,
                name.span,
                format!(
                    "there is no pack called `{}`; this build carries {}",
                    name.text,
                    crate::packs::names().join(", ")
                ),
            ));
            out.push(stmt);
            continue;
        };

        // Using one twice is harmless — the definitions are the same either way
        // — but it is almost certainly a mistake, and a silent one.
        if used.iter().any(|u| u == &name.text) {
            diagnostics.push(Diagnostic::warning(
                doc_codes::PACK_TWICE,
                name.span,
                format!("`{}` is already in use here", name.text),
            ));
            out.push(stmt);
            continue;
        }
        used.push(name.text.clone());

        out.push(stmt);
        for mut brought in crate::parse(pack.source).ast.stmts {
            set_span(&mut brought, span);
            from_packs.insert(out.len());
            out.push(brought);
        }
    }

    ast.stmts = out;
    from_packs
}

/// Point a statement, and everything inside it, at `span`.
fn set_span(stmt: &mut Stmt, span: Span) {
    match stmt {
        Stmt::Comment { span: s, .. }
        | Stmt::Assign { span: s, .. }
        | Stmt::GlobalDef { span: s, .. }
        | Stmt::Query { span: s, .. }
        | Stmt::UnitDecl { span: s, .. }
        | Stmt::Check { span: s, .. }
        | Stmt::Use { span: s, .. }
        | Stmt::FnDef { span: s, .. }
        | Stmt::Error { span: s } => *s = span,
    }
    match stmt {
        Stmt::Assign { name, value, .. }
        | Stmt::GlobalDef { name, value, .. }
        | Stmt::UnitDecl { name, value, .. } => {
            name.span = span;
            set_expr_span(value, span);
        }
        Stmt::FnDef {
            name, params, body, ..
        } => {
            name.span = span;
            for p in params {
                p.span = span;
            }
            set_expr_span(body, span);
        }
        Stmt::Query { expr, .. } | Stmt::Check { expr, .. } => set_expr_span(expr, span),
        Stmt::Use { name, .. } => name.span = span,
        Stmt::Comment { .. } | Stmt::Error { .. } => {}
    }
}

fn set_expr_span(expr: &mut Expr, span: Span) {
    use Expr::*;
    match expr {
        Number { span: s, .. }
        | Text { span: s, .. }
        | Unary { span: s, .. }
        | Binary { span: s, .. }
        | Call { span: s, .. }
        | Index { span: s, .. }
        | Vector { span: s, .. }
        | Matrix { span: s, .. }
        | If { span: s, .. }
        | Paren { span: s, .. }
        | Convert { span: s, .. }
        | Error { span: s } => *s = span,
        Ident(name) => name.span = span,
    }
    match expr {
        Unary { operand, .. } | Paren { inner: operand, .. } => set_expr_span(operand, span),
        Binary { lhs, rhs, .. }
        | Convert {
            value: lhs,
            unit: rhs,
            ..
        } => {
            set_expr_span(lhs, span);
            set_expr_span(rhs, span);
        }
        Call { callee, args, .. } => {
            callee.span = span;
            for a in args {
                set_expr_span(a, span);
            }
        }
        Index { base, indices, .. } => {
            set_expr_span(base, span);
            for i in indices {
                set_expr_span(i, span);
            }
        }
        Vector { elements, .. } => {
            for e in elements {
                set_expr_span(e, span);
            }
        }
        Matrix { rows, .. } => {
            for row in rows {
                for e in row {
                    set_expr_span(e, span);
                }
            }
        }
        If {
            cond,
            then,
            otherwise,
            ..
        } => {
            set_expr_span(cond, span);
            set_expr_span(then, span);
            set_expr_span(otherwise, span);
        }
        Number { .. } | Text { .. } | Ident(_) | Error { .. } => {}
    }
}

/// Bring a worksheet forward to [`CURRENT_VERSION`].
///
/// One pure function per step, applied in sequence. There is only one version so
/// far, so this is the identity; the shape is here because retrofitting it is
/// what goes wrong.
fn migrate(source: &str, from: u32) -> String {
    let mut text = source.to_string();
    for version in from..CURRENT_VERSION {
        text = migrate_step(&text, version);
    }
    text
}

/// One rung of the migration ladder: version `from_version` to the next.
///
/// Empty so far, because version 1 is the first format there has been. Each
/// future version adds a branch here together with a golden test for it.
fn migrate_step(text: &str, from_version: u32) -> String {
    let _ = from_version;
    text.to_string()
}

/// The verdicts a worksheet reached. See [`Sheet::checks`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Checks {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub undecided: usize,
}

/// What one call to [`Sheet::update`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Recalculation {
    /// Statements whose source changed.
    pub changed: Vec<usize>,
    /// Statements re-evaluated, in evaluation order. Always a superset of
    /// `changed`, extended with everything downstream.
    pub evaluated: Vec<usize>,
    /// True if the statement list itself changed shape, forcing a full pass.
    pub structural: bool,
}

/// A worksheet plus its evaluated state.
pub struct Sheet {
    doc: Document,
    graph: DepGraph,
    resources: Resources,
    outcomes: Vec<Outcome>,
    env: Env,
    diagnostics: Vec<Diagnostic>,
}

impl Sheet {
    /// Parse and fully evaluate a worksheet.
    pub fn new(source: &str) -> Sheet {
        let doc = Document::parse(source);
        let graph = DepGraph::build(&doc.ast);
        let resources = Resources::scan(&doc.ast);
        let mut sheet = Sheet {
            outcomes: Vec::new(),
            env: Env::new(),
            diagnostics: Vec::new(),
            graph,
            resources,
            doc,
        };
        sheet.evaluate_all();
        sheet
    }

    pub fn source(&self) -> &str {
        &self.doc.source
    }

    pub fn version(&self) -> u32 {
        self.doc.version
    }

    pub fn ast(&self) -> &Ast {
        &self.doc.ast
    }

    /// The unit table as it stands after evaluation, including any units the
    /// worksheet declared for itself.
    pub fn units(&self) -> &crate::unit::UnitTable {
        self.env.units()
    }

    pub fn graph(&self) -> &DepGraph {
        &self.graph
    }

    pub fn outcomes(&self) -> &[Outcome] {
        &self.outcomes
    }

    /// Whether outcome `index` is the version pragma.
    ///
    /// It is metadata written as a comment, and a renderer that shows it prints
    /// `nomo 1` at the head of every worksheet. That was cosmetic while each
    /// comment line rendered as its own paragraph; now that a run of them is one
    /// paragraph (`crate::prose`), the pragma would be swallowed into the first
    /// sentence of the document — `nomo 1 Complex numbers The imaginary unit is
    /// …` — so it is hidden the way the resource trailer is.
    ///
    /// Nothing else changes: the pragma is still an ordinary comment, still
    /// parsed as one, and a build that has never heard of it still opens the
    /// file. Only the first line can carry one, which is what `read_version`
    /// means, so this is an index test rather than a search.
    pub fn is_version_pragma(&self, index: usize) -> bool {
        index == 0
            && matches!(
                self.outcomes.first().map(|o| &o.kind),
                Some(OutcomeKind::Comment(_))
            )
            && read_version(self.source()).is_some()
    }

    /// The images this worksheet carries, and which of its statements are their
    /// data rather than something to show.
    pub fn resources(&self) -> &Resources {
        &self.resources
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Whether statement `index` came from a pack rather than from the author.
    ///
    /// Hidden from both renderers for the same reason the resource trailer is:
    /// a worksheet that shows its work should show the work its author did.
    pub fn is_from_pack(&self, index: usize) -> bool {
        self.doc.from_packs.contains(&index)
    }

    /// How many checks this worksheet states, and how many of them failed.
    ///
    /// A check that could not be decided counts in neither: it carries a
    /// diagnostic instead, so it is already reported as what it is — a
    /// worksheet that is wrong rather than a design that is.
    pub fn checks(&self) -> Checks {
        let mut checks = Checks::default();
        for outcome in &self.outcomes {
            if let OutcomeKind::Check { passed, .. } = &outcome.kind {
                checks.total += 1;
                match passed {
                    Some(true) => checks.passed += 1,
                    Some(false) => checks.failed += 1,
                    None => checks.undecided += 1,
                }
            }
        }
        checks
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    fn evaluate_all(&mut self) {
        let all: BTreeSet<usize> = (0..self.doc.ast.stmts.len()).collect();
        self.outcomes = self
            .doc
            .ast
            .stmts
            .iter()
            .map(|s| Outcome {
                span: s.span(),
                kind: OutcomeKind::NotEvaluated,
                diagnostics: vec![],
            })
            .collect();
        self.env = Env::new();
        self.evaluate(&self.graph.affected(&all).clone());
        self.collect_diagnostics();
    }

    /// Evaluate the given statements, in the order supplied.
    fn evaluate(&mut self, order: &[usize]) {
        for &i in order {
            let stmt = self.doc.ast.stmts[i].clone();
            self.outcomes[i] = self.env.eval_stmt(&stmt);
        }
    }

    fn collect_diagnostics(&mut self) {
        let mut diagnostics = self.doc.diagnostics.clone();
        for o in &self.outcomes {
            diagnostics.extend(o.diagnostics.iter().cloned());
        }
        for cycle in &self.graph.cycles {
            let names = if cycle.names.is_empty() {
                String::from("these statements")
            } else {
                cycle
                    .names
                    .iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(" and ")
            };
            diagnostics.push(Diagnostic::error(
                doc_codes::CYCLE,
                cycle.span,
                format!("{names} depend on each other, so neither can be computed"),
            ));
        }
        diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
        self.diagnostics = diagnostics;
    }

    /// Replace the source and recompute only what the change affects.
    ///
    /// Statements are matched by position. Editing a line in place therefore
    /// invalidates just that line and its dependents; inserting or deleting a
    /// line shifts everything below it and forces a full pass. That is correct
    /// but pessimistic, and fixing it needs stable per-statement identity rather
    /// than a better diff.
    pub fn update(&mut self, source: &str) -> Recalculation {
        let doc = Document::parse(source);
        let structural = doc.ast.stmts.len() != self.doc.ast.stmts.len();

        let changed: Vec<usize> = if structural {
            (0..doc.ast.stmts.len()).collect()
        } else {
            doc.ast
                .stmts
                .iter()
                .zip(&self.doc.ast.stmts)
                .enumerate()
                .filter(|(_, (new, old))| !same_statement(new, old, &doc.source, &self.doc.source))
                .map(|(i, _)| i)
                .collect()
        };

        self.doc = doc;
        self.graph = DepGraph::build(&self.doc.ast);
        self.resources = Resources::scan(&self.doc.ast);
        self.outcomes
            .resize_with(self.doc.ast.stmts.len(), || Outcome {
                span: Span::new(0, 0),
                kind: OutcomeKind::NotEvaluated,
                diagnostics: vec![],
            });

        let dirty: BTreeSet<usize> = changed.iter().copied().collect();
        let evaluated = self.graph.affected(&dirty);
        self.evaluate(&evaluated);
        self.collect_diagnostics();

        Recalculation {
            changed,
            evaluated,
            structural,
        }
    }
}

/// Whether two statements at the same position are the same statement.
///
/// Compares the text, not the tree, so that a change in spacing counts as a
/// change. Spans move when text above them changes, which would otherwise make
/// every statement below an edit look different.
fn same_statement(new: &Stmt, old: &Stmt, new_src: &str, old_src: &str) -> bool {
    new.span().text(new_src) == old.span().text(old_src)
}

/// Convenience: parse, evaluate, and hand back the results.
pub fn evaluate(source: &str) -> (Vec<Outcome>, Vec<Diagnostic>) {
    let sheet = Sheet::new(source);
    (sheet.outcomes.clone(), sheet.diagnostics.clone())
}

/// Whether a diagnostic list contains anything fatal.
pub fn is_fatal(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|d| d.severity == Severity::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every value a worksheet produced, line by line, or the error it gave.
    fn results(source: &str) -> Vec<Result<String, String>> {
        let (outcomes, _) = evaluate(source);
        outcomes
            .iter()
            .filter_map(|o| match &o.kind {
                OutcomeKind::Assign { trace, .. } | OutcomeKind::Query(trace) => Some(
                    trace
                        .value
                        .as_ref()
                        .map(|v| format!("{v:?}"))
                        .map_err(|e| e.to_string()),
                ),
                _ => None,
            })
            .collect()
    }

    /// The last statement's base-SI magnitude, or the error it gave.
    fn last(source: &str) -> Result<f64, String> {
        let (outcomes, _) = evaluate(source);
        let trace = outcomes
            .iter()
            .rev()
            .find_map(|o| match &o.kind {
                OutcomeKind::Assign { trace, .. } | OutcomeKind::Query(trace) => Some(trace),
                _ => None,
            })
            .expect("a statement with a value");
        match &trace.value {
            Ok(crate::value::Value::Scalar(q)) => Ok(q.value),
            Ok(other) => Err(format!("not a scalar: {other:?}")),
            Err(e) => Err(e.to_string()),
        }
    }

    /// The last statement's vector, as base-SI magnitudes.
    fn vector(source: &str) -> Result<Vec<f64>, String> {
        let (outcomes, _) = evaluate(source);
        let trace = outcomes
            .iter()
            .rev()
            .find_map(|o| match &o.kind {
                OutcomeKind::Assign { trace, .. } | OutcomeKind::Query(trace) => Some(trace),
                _ => None,
            })
            .expect("a statement with a value");
        match &trace.value {
            Ok(crate::value::Value::Vector(v)) => Ok(v.elements.iter().map(|q| q.value).collect()),
            Ok(other) => Err(format!("not a vector: {other:?}")),
            Err(e) => Err(e.to_string()),
        }
    }

    #[test]
    fn a_range_includes_its_end_when_the_step_lands_on_it() {
        // `range(1, 5)` has to mean 1 to 5 inclusive or it cannot index a
        // five-element vector, which is what it is mostly for.
        assert_eq!(vector("range(1, 5)\n"), Ok(vec![1.0, 2.0, 3.0, 4.0, 5.0]));
    }

    #[test]
    fn a_range_stops_before_an_end_it_cannot_land_on() {
        assert_eq!(vector("range(1, 6, 2)\n"), Ok(vec![1.0, 3.0, 5.0]));
    }

    #[test]
    fn a_range_carries_a_dimension() {
        assert_eq!(vector("range(0 m, 10 m, 5 m)\n"), Ok(vec![0.0, 5.0, 10.0]));
        // The implied step is one *of that dimension*, which is the only reading
        // that makes the two- and three-argument forms agree.
        assert_eq!(vector("range(0 m, 2 m)\n"), Ok(vec![0.0, 1.0, 2.0]));
    }

    #[test]
    fn a_range_is_built_by_multiplication_not_by_repeated_addition() {
        // Ten additions of 0.1 reach 0.9999999999999999; ten times 0.1 is
        // exactly 1. Both are defensible and only one gives the hundredth
        // element the same last bits as the second, which is the whole point of
        // this engine.
        let v = vector("range(0, 1, 0.1)\n").expect("a range");
        assert_eq!(v.len(), 11);
        // Exact equality is the assertion, not an oversight: the point is that
        // the last element is the bit pattern of 1.0 and not the 0.999… that
        // ten additions would give.
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(v[10], 1.0);
        }
    }

    #[test]
    fn a_range_with_mismatched_dimensions_is_an_error() {
        assert!(vector("range(0 m, 10 s)\n").is_err());
    }

    #[test]
    fn a_range_that_cannot_terminate_is_refused() {
        assert!(vector("range(1, 5, 0)\n").is_err());
        assert!(vector("range(5, 1)\n").is_err());
        // A browser tab has no way out of a hang, so size is capped too.
        assert!(vector("range(1, 1e9)\n").is_err());
    }

    #[test]
    fn map_applies_a_function_to_every_element() {
        assert_eq!(vector("map(sqrt, [1, 4, 9])\n"), Ok(vec![1.0, 2.0, 3.0]));
        assert_eq!(
            vector("fn double(x) = 2*x\nmap(double, range(1, 3))\n"),
            Ok(vec![2.0, 4.0, 6.0])
        );
    }

    #[test]
    fn map_keeps_the_units_its_function_produces() {
        assert_eq!(
            vector("fn area(s) = s^2\nmap(area, [2 m, 3 m])\n"),
            Ok(vec![4.0, 9.0])
        );
    }

    #[test]
    fn map_and_sum_replace_an_accumulating_loop() {
        // The pattern the corpus actually uses a `for` for.
        assert_eq!(
            last("fn double(x) = 2*x\nsum(map(double, range(1, 4)))\n"),
            Ok(20.0)
        );
    }

    #[test]
    fn iterate_applies_a_function_a_fixed_number_of_times() {
        // Newton-Raphson for the square root of two, which is what a `while`
        // loop in the corpus is doing.
        let root =
            last("fn step(x) = x - (x^2 - 2)/(2*x)\niterate(step, 1, 5)\n").expect("a value");
        assert!((root - 2f64.sqrt()).abs() < 1e-12, "{root}");
    }

    #[test]
    fn iterating_no_times_returns_what_it_started_with() {
        assert_eq!(last("fn double(x) = 2*x\niterate(double, 7, 0)\n"), Ok(7.0));
    }

    #[test]
    fn a_repetition_count_must_be_a_whole_number() {
        assert!(last("fn double(x) = 2*x\niterate(double, 1, 2.5)\n").is_err());
        assert!(last("fn double(x) = 2*x\niterate(double, 1, -1)\n").is_err());
        assert!(last("fn double(x) = 2*x\niterate(double, 1, 3 m)\n").is_err());
    }

    #[test]
    fn a_higher_order_builtin_needs_a_plain_name() {
        assert!(vector("map(1 + 2, [1, 2])\n").is_err());
    }

    #[test]
    fn a_function_that_does_not_exist_is_reported_by_name() {
        let out = vector("map(nosuchfn, [1, 2])\n");
        assert!(matches!(&out, Err(e) if e.contains("nosuchfn")), "{out:?}");
    }

    #[test]
    fn a_function_name_is_not_read_as_a_variable() {
        // `f` on its own is not a value in this language, so evaluating the
        // first argument would report an unknown name for a function that
        // plainly exists.
        let (outcomes, diagnostics) = evaluate("fn double(x) = 2*x\nmap(double, [1])\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(outcomes[1].diagnostics.is_empty());
    }

    #[test]
    fn a_comparison_answers_one_or_zero() {
        assert_eq!(last("3 m > 2 m\n"), Ok(1.0));
        assert_eq!(last("2 m > 3 m\n"), Ok(0.0));
    }

    #[test]
    fn comparison_converts_before_it_compares() {
        // Both sides are held in base SI, so this needs no conversion step of
        // its own — but it is the property everything else here rests on.
        assert_eq!(last("1 in < 1 m\n"), Ok(1.0));
        assert_eq!(last("1 kip > 1 lbf\n"), Ok(1.0));
    }

    #[test]
    fn comparing_different_dimensions_is_an_error() {
        assert!(last("1 m > 1 s\n").is_err());
    }

    #[test]
    fn a_condition_must_be_dimensionless() {
        // Otherwise `if x then …` with `x` in metres silently means "x is not
        // zero metres", which is not what anybody wrote.
        assert!(last("if 1 m then 2 else 3\n").is_err());
        assert_eq!(last("if 1 then 2 else 3\n"), Ok(2.0));
    }

    #[test]
    fn only_the_arm_that_is_taken_is_evaluated() {
        // The point of laziness here: the untaken arm would fail, and must not.
        assert_eq!(
            last("v = [1 m, 2 m]\nif 0 > 1 then v[9] else 5 m\n"),
            Ok(5.0)
        );
        assert_eq!(
            last("v = [1 m, 2 m]\nif 1 > 0 then v[1] else v[9]\n"),
            Ok(1.0)
        );
    }

    #[test]
    fn a_guard_written_with_and_actually_guards() {
        // `n > 0 and v[n] > 0 m` with n = 0 must not index at zero. Without
        // short-circuiting this is an out-of-bounds error on a line that is
        // correct.
        let out = last("v = [1 m]\nn = 0\nn > 0 and v[n] > 0 m\n");
        assert_eq!(out, Ok(0.0));
    }

    #[test]
    fn or_stops_once_it_is_decided() {
        let out = last("v = [1 m]\nn = 0\nn == 0 or v[n] > 0 m\n");
        assert_eq!(out, Ok(1.0));
    }

    #[test]
    fn a_failing_condition_blames_the_condition_and_neither_arm() {
        let (outcomes, _) = evaluate("if nosuchname then 1 else 2\n");
        let diags: Vec<&str> = outcomes[0]
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(diags, vec!["`nosuchname` is not defined"]);
    }

    #[test]
    fn an_untaken_arm_raises_no_diagnostic_of_its_own() {
        // The arm that did not run contains a name that does not exist. Nothing
        // evaluated it, so nothing may complain about it.
        let (outcomes, diagnostics) = evaluate("x = if 1 > 0 then 2 else nosuchname\n");
        assert!(outcomes[0].diagnostics.is_empty(), "{:?}", outcomes[0]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn not_inverts_a_truth_value() {
        assert_eq!(last("not 0\n"), Ok(1.0));
        assert_eq!(last("not 1\n"), Ok(0.0));
    }

    #[test]
    fn a_conditional_depends_on_both_arms() {
        // Which arm runs depends on values, and the graph is built before any
        // value exists — so both are dependencies. Under-approximating here
        // would leave a stale result when the unused arm's input changed.
        let mut sheet = Sheet::new("a = 1\nb = 2 m\nc = 3 m\nx = if a > 0 then b else c\n");
        // `c` is only used by the arm that does *not* run. Changing it must
        // still recalculate `x`, because the graph cannot know which arm wins.
        let recalc = sheet.update("a = 1\nb = 2 m\nc = 9 m\nx = if a > 0 then b else c\n");
        assert!(recalc.evaluated.contains(&3), "{recalc:?}");
    }

    #[test]
    fn a_failed_binding_does_not_become_a_unit() {
        // The bug this test exists for: `PF` is peta-farads to the unit table, so
        // a binding of it that failed used to leave the next line answering
        // 1e15 F with no diagnostic at all. Two worksheets in the SMath corpus
        // did this, one reporting a power factor and one a service ceiling.
        let out = results("PF = nosuchthing\nPF\n");
        assert!(out[0].is_err());
        assert_eq!(
            out[1],
            Err("`PF` has no value: the statement that defines it failed".into())
        );
    }

    #[test]
    fn a_failed_binding_hides_a_constant_too() {
        // `pi` is a constant rather than a unit, and the same argument applies:
        // the binding took the name over, so its failure cannot hand the name
        // back to something else.
        let out = results("pi = nosuchthing\npi\n");
        assert!(out[1].is_err());
    }

    #[test]
    fn a_name_nothing_binds_is_still_a_unit() {
        // The other half of the rule, and the reason this is not simply "unknown
        // name": a unit only loses to a binding that exists.
        let out = results("PF\n");
        assert!(out[0].is_ok(), "{out:?}");
    }

    #[test]
    fn a_failed_rebinding_takes_the_earlier_value_with_it() {
        // A use takes the nearest definition above it. If that one failed, the
        // answer is that it failed — not the value from two definitions ago,
        // which is not what the worksheet says on the line above.
        let out = results("x = 1 m\nx\nx = nosuchthing\nx\n");
        assert!(out[1].is_ok());
        assert!(out[3].is_err(), "{out:?}");
    }

    #[test]
    fn a_binding_that_recovers_is_available_again() {
        let out = results("x = nosuchthing\nx\nx = 2 m\nx\n");
        assert!(out[1].is_err());
        assert!(out[3].is_ok(), "{out:?}");
    }

    #[test]
    fn a_parameter_outranks_a_failed_binding_of_the_same_name() {
        // The function body sees the definition site's bindings, so a failed one
        // reaches it — but a parameter is a real binding and must win.
        let out = results("t = nosuchthing\nfn f(t) = t*2\nf(3)\n");
        assert!(out[1].is_ok(), "{out:?}");
    }

    #[test]
    fn stamping_adds_a_pragma_and_the_result_reads_back() {
        let stamped = stamp_version("r = 5 cm\n");
        assert_eq!(stamped, "' nomo 1\nr = 5 cm\n");
        // The round trip is the point: what is written must parse as what it
        // claims to be.
        assert_eq!(Document::parse(&stamped).version, CURRENT_VERSION);
    }

    #[test]
    fn stamping_twice_changes_nothing() {
        // Save, edit, save again must not stack pragmas up the top of the file.
        let once = stamp_version("r = 5 cm\n");
        assert_eq!(stamp_version(&once), once);
    }

    #[test]
    fn a_worksheet_from_the_future_keeps_its_own_pragma() {
        // Claiming a version 99 worksheet is version 1 would turn a warning into
        // silent corruption the next time a build tried to migrate it.
        let future = "' nomo 99\nx = 1\n";
        assert_eq!(stamp_version(future), future);
    }

    #[test]
    fn stamping_an_empty_worksheet_gives_a_valid_one() {
        let stamped = stamp_version("");
        assert_eq!(stamped, "' nomo 1\n");
        assert!(!is_fatal(&Document::parse(&stamped).diagnostics));
    }

    #[test]
    fn stamping_does_not_change_what_a_worksheet_computes() {
        // The pragma is a comment, so it adds one outcome and changes no result.
        // Saving a file must never alter its answers.
        let source = "r = 5 cm\nh = 12 cm\nV = pi*r^2*h\n";
        let plain = Sheet::new(source);
        let stamped = Sheet::new(&stamp_version(source));

        assert_eq!(stamped.outcomes().len(), plain.outcomes().len() + 1);

        let values = |s: &str| {
            let snap = crate::golden::snapshot("x", s);
            let start = snap.find("=== values ===").expect("values section");
            snap[start..].to_string()
        };
        assert_eq!(values(source), values(&stamp_version(source)));
    }

    #[test]
    fn a_file_without_a_pragma_is_the_current_version() {
        let d = Document::parse("x = 1");
        assert_eq!(d.version, CURRENT_VERSION);
        assert!(d.diagnostics.is_empty());
    }

    #[test]
    fn the_pragma_is_read_and_stays_an_ordinary_comment() {
        let d = Document::parse("' nomo 1\nx = 1");
        assert_eq!(d.version, 1);
        // It is still just a comment as far as the language is concerned.
        assert!(matches!(d.ast.stmts[0], Stmt::Comment { .. }));
    }

    #[test]
    fn a_worksheet_from_the_future_warns_but_still_opens() {
        let d = Document::parse("' nomo 99\nx = 1");
        assert_eq!(d.version, 99);
        assert_eq!(d.diagnostics.len(), 1);
        assert_eq!(d.diagnostics[0].code, doc_codes::FROM_THE_FUTURE);
        assert!(!d.diagnostics[0].is_error(), "it must still open");
        assert_eq!(d.ast.stmts.len(), 2);
    }

    #[test]
    fn an_ordinary_comment_is_not_mistaken_for_a_pragma() {
        assert_eq!(Document::parse("' Cylinder volume\nx = 1").version, 1);
        assert_eq!(Document::parse("' nomo calculations\nx = 1").version, 1);
    }

    #[test]
    fn cycles_become_a_diagnostic_not_a_hang() {
        let sheet = Sheet::new("global a = b + 1\nglobal b = a + 1");
        let cycle: Vec<_> = sheet
            .diagnostics()
            .iter()
            .filter(|d| d.code == doc_codes::CYCLE)
            .collect();
        assert_eq!(cycle.len(), 1, "{:#?}", sheet.diagnostics());
        assert!(cycle[0].message.contains("depend on each other"));
    }

    #[test]
    fn editing_one_line_recomputes_only_its_dependents() {
        let mut sheet = Sheet::new("a = 1\nb = a*2\nc = b+1\nd = 99");
        let r = sheet.update("a = 5\nb = a*2\nc = b+1\nd = 99");
        assert!(!r.structural);
        assert_eq!(r.changed, vec![0]);
        // `d` is independent and must not be touched.
        assert_eq!(r.evaluated, vec![0, 1, 2]);
    }

    #[test]
    fn editing_a_leaf_recomputes_only_the_leaf() {
        let mut sheet = Sheet::new("a = 1\nb = a*2\nd = 99");
        let r = sheet.update("a = 1\nb = a*2\nd = 100");
        assert_eq!(r.evaluated, vec![2]);
    }

    #[test]
    fn an_unchanged_worksheet_recomputes_nothing() {
        let src = "a = 1\nb = a*2";
        let mut sheet = Sheet::new(src);
        let r = sheet.update(src);
        assert!(r.changed.is_empty());
        assert!(r.evaluated.is_empty());
    }

    #[test]
    fn adding_a_line_forces_a_full_pass() {
        let mut sheet = Sheet::new("a = 1\nb = a*2");
        let r = sheet.update("a = 1\nb = a*2\nc = b+1");
        assert!(r.structural);
        assert_eq!(r.evaluated.len(), 3);
    }

    #[test]
    fn incremental_results_match_a_full_evaluation() {
        // The property that matters: whatever the update path skips, the answer
        // must be the one a fresh run would give.
        let before = "a = 2\nb = a*3\nc = b+1\nc";
        let after = "a = 7\nb = a*3\nc = b+1\nc";

        let mut incremental = Sheet::new(before);
        incremental.update(after);
        let fresh = Sheet::new(after);

        let value = |s: &Sheet| match &s.outcomes().last().unwrap().kind {
            OutcomeKind::Query(t) => t.value.clone().unwrap(),
            other => panic!("{other:?}"),
        };
        assert_eq!(value(&incremental), value(&fresh));
    }
}
