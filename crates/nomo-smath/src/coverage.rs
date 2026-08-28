//! Counting what the reader met and what it could not translate.
//!
//! The strategy in design note §8.8 is corpus-driven: read every worksheet that
//! can be collected, report what is unsupported, and let the counts decide the
//! order of the work. This module is that report. It is the deliverable of the
//! importer's first stage, before any Nomo document is written, because
//! deciding what `if` or `el` should become is wasted effort if the corpus turns
//! out to hinge on something else.
//!
//! Everything counted here is counted **on the input side**. A stored answer is
//! full of function calls too — `<result>` blocks alone account for the
//! difference between 817 and 454 uses of `el` — but those are SMath's rendering
//! of an answer, not work the importer has to be able to perform.

use std::collections::{BTreeMap, BTreeSet};

use crate::builtins;
use crate::expr::{Assign, Expr, Statement, Unsupported};
use crate::read::{Era, Payload, ResultKind, Worksheet};

/// One thing the reader could not handle, ready to be counted and located.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Issue {
    pub kind: IssueKind,
    pub what: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IssueKind {
    UnknownFunction,
    UnknownOperator,
    MalformedExpression,
    UnsupportedPayload,
}

#[derive(Debug, Default)]
pub struct Coverage {
    pub files: usize,
    pub failures: Vec<(String, String)>,
    pub versions: BTreeMap<String, usize>,
    pub eras: BTreeMap<&'static str, usize>,
    pub payloads: BTreeMap<&'static str, usize>,
    pub statements: BTreeMap<&'static str, usize>,

    /// Stored answers usable as assertions, by era.
    pub oracle_legacy: usize,
    pub oracle_modern: usize,
    pub oracle_files: BTreeSet<String>,
    /// `<result action="symbolic">`: provenance for a human, never an assertion.
    pub oracle_symbolic: usize,
    /// How many regions each language appears in, so the policy choice in
    /// [`crate::emit`] can be made against what the corpus actually holds.
    pub languages: BTreeMap<String, usize>,
    /// How many worksheets declare each plugin, from `<dependencies>`. The
    /// engine's own regions are filtered out: every file needs `SMath Core`, and
    /// saying so is noise.
    pub plugins: BTreeMap<String, usize>,
    /// Regions that already fail inside SMath. Not import failures.
    pub error_regions: usize,
    /// Stored answers that name the unit they are displayed in.
    pub oracle_with_contract: usize,

    pub builtin_calls: BTreeMap<String, usize>,
    pub local_calls: BTreeMap<String, usize>,
    pub unknown_calls: BTreeMap<String, usize>,
    pub unknown_operators: BTreeMap<String, usize>,
    pub units: BTreeMap<String, usize>,
    pub malformed: usize,
    pub unsupported_payloads: BTreeMap<String, usize>,
    /// Text regions written in more than one language. Which one an import keeps
    /// is a policy question nobody has answered yet.
    pub multilingual_text: usize,

    /// Files whose region order does not run down the page, with the index of
    /// the first region that goes backwards.
    pub order_anomalies: Vec<(String, usize)>,
}

impl Coverage {
    pub fn add(&mut self, name: &str, w: &Worksheet) {
        self.files += 1;
        *self.versions.entry(w.version.clone()).or_default() += 1;
        *self
            .eras
            .entry(match w.era {
                Era::Legacy => "legacy (pre-0.88)",
                Era::Modern => "modern (0.88+)",
            })
            .or_default() += 1;

        if let Some(&first) = w.order_anomalies().first() {
            self.order_anomalies.push((name.to_string(), first));
        }

        // A name this worksheet defines for itself needs no registry to be
        // recognised, so it is collected before anything is classified.
        const OWN: &[&str] = &[
            "SMath Core",
            "MathRegion",
            "TextRegion",
            "PictureRegion",
            "AreaRegion",
            "PlotRegion",
            "SpecialFunctions",
        ];
        for d in &w.dependencies {
            if !OWN.contains(&d.name.as_str()) {
                *self.plugins.entry(d.name.clone()).or_default() += 1;
            }
        }

        let local = local_functions(w);

        // Both blocks: a coverage report answers "what is in this corpus",
        // and a picture in a page header is still a picture the importer meets.
        for region in w.flat().into_iter().chain(w.flat_furniture()) {
            let label = match &region.payload {
                Payload::Math(_) => "math",
                Payload::Text { .. } => "text",
                // Counted apart: `<xyplot>` is a plugin region, and how much
                // of a corpus depends on one is a migration question.
                Payload::Plot { tag, .. } if tag == "xyplot" => "xyplot",
                Payload::Plot { .. } => "plot",
                Payload::Picture { .. } => "picture",
                Payload::Area { .. } => "area",
                Payload::Unsupported { tag } => {
                    *self
                        .unsupported_payloads
                        .entry(if tag.is_empty() {
                            "(empty region)".into()
                        } else {
                            tag.clone()
                        })
                        .or_default() += 1;
                    "unsupported"
                }
            };
            *self.payloads.entry(label).or_default() += 1;

            match &region.payload {
                Payload::Math(m) => {
                    *self
                        .statements
                        .entry(match &m.statement {
                            Statement::Define {
                                kind: Assign::Positional,
                                ..
                            } => "positional definition",
                            Statement::Define {
                                kind: Assign::Global,
                                ..
                            } => "global definition (≡)",
                            Statement::Show { .. } => "display with stored answer (=)",
                            Statement::Equation { .. } => "stated equation (≡, binds nothing)",
                            Statement::Bare(_) => "bare expression",
                        })
                        .or_default() += 1;

                    if let Statement::Show {
                        stored: Some(_), ..
                    } = &m.statement
                    {
                        self.oracle_legacy += 1;
                        self.oracle_files.insert(name.to_string());
                    }
                    // A symbolic result records what SMath derived, not a
                    // number: keeping it as provenance is useful, feeding it to
                    // the oracle is not, because a no-CAS engine will never
                    // reproduce it.
                    match (m.result.is_some(), m.result_kind) {
                        // `symbolic` is what SMath derived; `none` is an
                        // unevaluated equation. Neither is a number, so neither
                        // is an assertion — but both are worth counting, because
                        // they measure how much of a corpus a no-CAS engine
                        // cannot check itself against.
                        (true, Some(ResultKind::Symbolic | ResultKind::Other)) => {
                            self.oracle_symbolic += 1
                        }
                        (true, _) => {
                            self.oracle_modern += 1;
                            self.oracle_files.insert(name.to_string());
                            if m.contract.is_some() {
                                self.oracle_with_contract += 1;
                            }
                        }
                        _ => {}
                    }
                    if m.error.is_some() {
                        self.error_regions += 1;
                    }

                    m.statement.walk(&mut |e| self.note(e, &local));
                }
                Payload::Plot { expr, .. } => expr.walk(&mut |e| self.note(e, &local)),
                Payload::Text { variants } if variants.len() > 1 => {
                    self.multilingual_text += 1;
                    for (lang, _) in variants {
                        if let Some(l) = lang {
                            *self.languages.entry(l.clone()).or_default() += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn note(&mut self, e: &Expr, local: &BTreeSet<String>) {
        match e {
            Expr::Call { name, .. } => {
                let bucket = if local.contains(name) {
                    &mut self.local_calls
                } else if builtins::is_builtin(name) {
                    &mut self.builtin_calls
                } else {
                    &mut self.unknown_calls
                };
                *bucket.entry(name.clone()).or_default() += 1;
            }
            Expr::Op { glyph, .. } => {
                if !builtins::is_known_operator(glyph) {
                    *self.unknown_operators.entry(glyph.clone()).or_default() += 1;
                }
            }
            Expr::Unit(u) => *self.units.entry(u.clone()).or_default() += 1,
            Expr::Unsupported {
                what: Unsupported::Malformed,
                ..
            } => self.malformed += 1,
            Expr::Unsupported {
                what: Unsupported::Operator,
                detail,
                ..
            } => *self.unknown_operators.entry(detail.clone()).or_default() += 1,
            _ => {}
        }
    }

    /// Everything the importer cannot yet translate, ranked by how much of the
    /// corpus it accounts for. This is the list the work is meant to follow.
    pub fn ranked_gaps(&self) -> Vec<(IssueKind, String, usize)> {
        let mut out: Vec<(IssueKind, String, usize)> = Vec::new();
        for (name, n) in &self.unknown_calls {
            out.push((IssueKind::UnknownFunction, name.clone(), *n));
        }
        for (glyph, n) in &self.unknown_operators {
            out.push((IssueKind::UnknownOperator, glyph.clone(), *n));
        }
        for (tag, n) in &self.unsupported_payloads {
            out.push((IssueKind::UnsupportedPayload, tag.clone(), *n));
        }
        // Descending by count, then by name, so the report is stable.
        out.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(&b.1)));
        out
    }
}

/// Function names this worksheet binds for itself.
///
/// A definition looks like `f(x) ← …`: the target of a binding is a call rather
/// than a plain name.
fn local_functions(w: &Worksheet) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for m in w.math() {
        if let Statement::Define {
            target: Expr::Call { name, .. },
            ..
        } = &m.statement
        {
            out.insert(name.clone());
        }
    }
    out
}

fn ranked(map: &BTreeMap<String, usize>) -> Vec<(&String, &usize)> {
    let mut v: Vec<_> = map.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    v
}

impl std::fmt::Display for Coverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SMath import coverage")?;
        writeln!(f, "=====================")?;
        writeln!(f)?;
        writeln!(f, "{} worksheets read", self.files)?;
        for (e, n) in &self.eras {
            writeln!(f, "    {n:>5}  {e}")?;
        }
        // The manifest first, because the file states what it needs before a
        // token is parsed and that is the cheapest, surest thing a migration
        // report can tell someone (design note §8.7 item 21).
        if !self.plugins.is_empty() {
            writeln!(f, "\nPlugins the worksheets say they need")?;
            let mut ranked: Vec<_> = self.plugins.iter().collect();
            ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            for (name, n) in ranked {
                let scope = match name.as_str() {
                    "MaximaPlugin" => "  — computer algebra, out of scope (§8.12)",
                    _ => "",
                };
                writeln!(f, "    {n:>5}  {name}{scope}")?;
            }
        }
        if !self.failures.is_empty() {
            writeln!(f, "\n{} could not be read:", self.failures.len())?;
            for (name, why) in &self.failures {
                writeln!(f, "    {name}: {why}")?;
            }
        }

        writeln!(f, "\nRegions")?;
        for (k, n) in &self.payloads {
            writeln!(f, "    {n:>5}  {k}")?;
        }

        writeln!(f, "\nMath regions by kind")?;
        for (k, n) in &self.statements {
            writeln!(f, "    {n:>5}  {k}")?;
        }

        writeln!(f, "\nStored answers available as assertions")?;
        writeln!(
            f,
            "    {:>5}  legacy, as the second operand of `=`",
            self.oracle_legacy
        )?;
        writeln!(
            f,
            "    {:>5}  modern, as <result action=\"numeric\">",
            self.oracle_modern
        )?;
        writeln!(
            f,
            "    {:>5}  of those name their display unit in <contract>",
            self.oracle_with_contract
        )?;
        writeln!(
            f,
            "    {:>5}  total, across {} of {} files",
            self.oracle_legacy + self.oracle_modern,
            self.oracle_files.len(),
            self.files
        )?;
        if self.oracle_symbolic > 0 {
            writeln!(
                f,
                "    {:>5}  symbolic or unevaluated — provenance, never an assertion",
                self.oracle_symbolic
            )?;
        }
        if self.error_regions > 0 {
            writeln!(
                f,
                "    {:>5}  region(s) already failing inside SMath (not import failures)",
                self.error_regions
            )?;
        }

        writeln!(f, "\nFunction calls on the input side")?;
        writeln!(
            f,
            "    {:>5}  built in ({} distinct, registry holds {})",
            self.builtin_calls.values().sum::<usize>(),
            self.builtin_calls.len(),
            builtins::count()
        )?;
        writeln!(
            f,
            "    {:>5}  defined by their own worksheet ({} distinct)",
            self.local_calls.values().sum::<usize>(),
            self.local_calls.len()
        )?;
        writeln!(
            f,
            "    {:>5}  unknown ({} distinct)",
            self.unknown_calls.values().sum::<usize>(),
            self.unknown_calls.len()
        )?;

        writeln!(f, "\nUnits, by use")?;
        let units = ranked(&self.units);
        writeln!(f, "    {} distinct", units.len())?;
        let line: Vec<String> = units.iter().map(|(u, n)| format!("{u}({n})")).collect();
        for chunk in line.chunks(10) {
            writeln!(f, "    {}", chunk.join(" "))?;
        }

        if self.multilingual_text > 0 {
            writeln!(
                f,
                "\n{} text region(s) carry the same prose in more than one language",
                self.multilingual_text
            )?;
        }

        if self.malformed > 0 {
            writeln!(
                f,
                "\n{} expression(s) did not reduce to a single tree",
                self.malformed
            )?;
        }

        if !self.order_anomalies.is_empty() {
            writeln!(f, "\nRegion order does not run down the page in:")?;
            for (name, i) in &self.order_anomalies {
                writeln!(f, "    {name} (first at region {i})")?;
            }
        } else {
            writeln!(
                f,
                "\nRegion order runs down the page in every file, as the design note found."
            )?;
        }

        let gaps = self.ranked_gaps();
        writeln!(
            f,
            "\nGaps, ranked by how much of the corpus they account for"
        )?;
        writeln!(
            f,
            "--------------------------------------------------------"
        )?;
        if gaps.is_empty() {
            writeln!(f, "    none")?;
        }
        for (kind, what, n) in gaps.iter().take(40) {
            let label = match kind {
                IssueKind::UnknownFunction => "function",
                IssueKind::UnknownOperator => "operator",
                IssueKind::MalformedExpression => "expression",
                IssueKind::UnsupportedPayload => "region",
            };
            // A name a plugin supplies is a decision, not an unknown: say so,
            // so the ranking separates what is missing from what is declined.
            match builtins::plugin(what) {
                Some(p) => writeln!(f, "    {n:>5}  {label:<10} {what:<22} ({p})")?,
                None => writeln!(f, "    {n:>5}  {label:<10} {what}")?,
            }
        }
        if gaps.len() > 40 {
            writeln!(f, "    … and {} more", gaps.len() - 40)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<?application progid="SMath Studio" version="0.85"?>
<regions>
  <settings><calculation><precision>2</precision></calculation></settings>
  <region id="0" left="0" top="0"><math>
    <e type="operand">x</e><e type="function" args="1">myfn</e>
    <e type="operand">1</e><e type="operator" args="2">←</e>
  </math></region>
  <region id="1" left="0" top="10"><math>
    <e type="operand">2</e><e type="function" args="1">myfn</e>
    <e type="operand">3</e><e type="operator" args="2">=</e>
  </math></region>
  <region id="2" left="0" top="20"><math>
    <e type="operand">2</e><e type="function" args="1">whoIsThis</e>
    <e type="operand">4</e><e type="function" args="1">sqrt</e>
    <e type="operator" args="2">*</e>
  </math></region>
</regions>"#;

    fn coverage_of(src: &str) -> Coverage {
        let w = crate::read(src.as_bytes()).unwrap();
        let mut c = Coverage::default();
        c.add("test.sm", &w);
        c
    }

    #[test]
    fn a_worksheets_own_function_is_not_reported_as_unknown() {
        let c = coverage_of(DOC);
        assert_eq!(c.local_calls.get("myfn"), Some(&1));
        assert!(!c.unknown_calls.contains_key("myfn"));
    }

    #[test]
    fn a_name_that_is_neither_builtin_nor_defined_is_reported() {
        let c = coverage_of(DOC);
        assert_eq!(c.unknown_calls.get("whoIsThis"), Some(&1));
    }

    #[test]
    fn the_defining_region_is_not_itself_counted_as_a_call() {
        // `myfn(x) ← 1` defines; `myfn(2) = 3` calls. Both walk the same tree
        // shape, so a naive count would say two calls and no definition.
        let c = coverage_of(DOC);
        assert_eq!(c.statements.get("positional definition"), Some(&1));
    }

    #[test]
    fn a_stored_answer_is_counted_once_per_era() {
        let c = coverage_of(DOC);
        assert_eq!(c.oracle_legacy, 1);
        assert_eq!(c.oracle_modern, 0);
        assert_eq!(c.oracle_files.len(), 1);
    }

    #[test]
    fn gaps_come_back_ranked() {
        let mut c = Coverage::default();
        c.unknown_calls.insert("rare".into(), 2);
        c.unknown_calls.insert("common".into(), 9);
        let gaps = c.ranked_gaps();
        assert_eq!(gaps[0].1, "common");
        assert_eq!(gaps[1].1, "rare");
    }
}
