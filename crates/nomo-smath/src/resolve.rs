//! Deciding which unit-styled operands are actually units.
//!
//! `<e type="operand" style="unit">` is a **display style, not a semantic
//! marker**, and taking it at face value is the quietest mistake an SMath
//! importer can make. In the mechanics corpus ten of the 25 symbols carrying it
//! are not built-in units, and eight are ordinary variables — several of them the
//! unknowns the worksheet exists to solve for. `4.2.sm` writes
//! `F.A := mat(A.x, A.y, A.z, 3, 1)` with the components styled as units and
//! `4.3.sm` writes the identical line with them unstyled, so the attribute is not
//! even consistent for the same symbols in the same book.
//!
//! An importer that trusts it invents units that do not exist *and* loses the
//! variables, and both failures are silent: the emitted worksheet parses.
//!
//! So a styled symbol is a unit only if it **resolves** to one — a unit the
//! engine knows, or one this document declares. Everything else becomes an
//! ordinary name.
//!
//! # What counts as a declaration
//!
//! Both forms found in the corpora, and only at a region root:
//!
//! ```text
//! VA : W              alias — apparent power as another name for the watt
//! ΔF ← 0.5555 K       magnitude — the Fahrenheit degree interval
//! a.0 := 1 m          magnitude — a length scale the worksheet then works in
//! ```
//!
//! The right-hand side must be *unit-like*: literals, units, and the operators
//! that combine them. `n := rows(X)` also has a unit-styled target, but a symbol
//! bound to a computed value is a variable however it is drawn, and declaring it
//! a unit would put a matrix row count into the unit namespace.

use std::collections::BTreeSet;

use nomo_core::unit::UnitTable;

use crate::expr::{Assign, Expr, Statement};
use crate::read::{Math, Payload, Region, Worksheet};

/// Rewrite every unit-styled operand that is not a unit into a plain name.
pub fn units(w: &mut Worksheet) {
    let declared = declarations(w);
    let table = UnitTable::new();
    let known = |name: &str| declared.contains(name) || table.contains(name);
    each_math(&mut w.regions, &mut |m| rewrite_math(m, &known));
    // The header is not the document, but its math is still math and is still
    // shown; leaving it un-rewritten would print a unit-styled operand raw.
    each_math(&mut w.furniture, &mut |m| rewrite_math(m, &known));
}

/// The unit symbols this document declares for itself.
fn declarations(w: &Worksheet) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for m in w.math() {
        if let Statement::Define {
            kind: Assign::Positional,
            target: Expr::Unit(name),
            value,
        } = &m.statement
        {
            if unit_like(value) {
                out.insert(name.clone());
            }
        }
    }
    out
}

/// Whether an expression is built only from magnitudes and units.
fn unit_like(e: &Expr) -> bool {
    match e {
        Expr::Number(_) | Expr::Unit(_) => true,
        Expr::Op { glyph, args } if matches!(glyph.as_str(), "*" | "/" | "^" | "-") => {
            args.iter().all(unit_like)
        }
        _ => false,
    }
}

fn rewrite_math(m: &mut Math, known: &impl Fn(&str) -> bool) {
    match &mut m.statement {
        Statement::Define { target, value, .. } => {
            rewrite(target, known);
            rewrite(value, known);
        }
        Statement::Equation { left, right } => {
            rewrite(left, known);
            rewrite(right, known);
        }
        Statement::Show { expr, stored } => {
            rewrite(expr, known);
            if let Some(s) = stored {
                rewrite(s, known);
            }
        }
        Statement::Bare(e) => rewrite(e, known),
    }
    // A stored answer and its contract are token streams like any other, and the
    // same styled-but-not-a-unit symbols appear in them.
    if let Some(r) = &mut m.result {
        rewrite(r, known);
    }
    if let Some(c) = &mut m.contract {
        rewrite(c, known);
    }
}

fn rewrite(e: &mut Expr, known: &impl Fn(&str) -> bool) {
    match e {
        Expr::Unit(name) => {
            if !known(name) {
                *e = Expr::Name(std::mem::take(name));
            }
        }
        Expr::Op { args, .. } | Expr::Call { args, .. } => {
            for a in args {
                rewrite(a, known);
            }
        }
        Expr::Unsupported { inside, .. } => {
            for a in inside {
                rewrite(a, known);
            }
        }
        Expr::Number(_) | Expr::Name(_) | Expr::Text(_) => {}
    }
}

/// Apply `f` to every math payload in the tree, nested regions included.
fn each_math(regions: &mut [Region], f: &mut impl FnMut(&mut Math)) {
    for r in regions {
        if let Payload::Math(m) = &mut r.payload {
            f(m);
        }
        each_math(&mut r.children, f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read;

    fn read_str(s: &str) -> Worksheet {
        let mut w = read::worksheet(s.as_bytes()).unwrap();
        units(&mut w);
        w
    }

    fn wrap(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<?application progid="SMath Studio" version="0.96"?>
<regions>{body}</regions>"#
        )
    }

    fn region(tokens: &str) -> String {
        format!(r#"<region id="0" left="0" top="0"><math><input>{tokens}</input></math></region>"#)
    }

    const UNIT_A_X: &str = r#"<e type="operand">F.A</e><e type="operand" style="unit">A.x</e>
        <e type="operator" args="2">:</e>"#;

    #[test]
    fn a_styled_symbol_that_is_no_unit_becomes_a_name() {
        let w = read_str(&wrap(&region(UNIT_A_X)));
        let m: Vec<_> = w.math().collect();
        match &m[0].statement {
            Statement::Define { value, .. } => {
                assert_eq!(
                    *value,
                    Expr::Name("A.x".into()),
                    "A.x is an unknown, not a unit"
                )
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_styled_symbol_the_engine_knows_stays_a_unit() {
        let w = read_str(&wrap(&region(
            r#"<e type="operand">L</e><e type="operand">3</e>
               <e type="operand" style="unit">m</e><e type="operator" args="2">*</e>
               <e type="operator" args="2">:</e>"#,
        )));
        let m: Vec<_> = w.math().collect();
        let Statement::Define { value, .. } = &m[0].statement else {
            panic!()
        };
        let Expr::Op { args, .. } = value else {
            panic!()
        };
        assert_eq!(args[1], Expr::Unit("m".into()));
    }

    #[test]
    fn a_document_may_declare_its_own_units_in_both_forms() {
        // `VA : W` aliases the watt; `a.0 : 1 m` names a length scale.
        let w = read_str(&wrap(&format!(
            "{}{}",
            region(
                r#"<e type="operand" style="unit">VA</e><e type="operand" style="unit">W</e>
                   <e type="operator" args="2">:</e>"#
            ),
            region(
                r#"<e type="operand" style="unit">a.0</e><e type="operand">1</e>
                   <e type="operand" style="unit">m</e><e type="operator" args="2">*</e>
                   <e type="operator" args="2">:</e>"#
            ),
        )));
        let m: Vec<_> = w.math().collect();
        for s in &m {
            let Statement::Define { target, .. } = &s.statement else {
                panic!()
            };
            assert!(matches!(target, Expr::Unit(_)), "{target:?}");
        }
    }

    #[test]
    fn a_styled_symbol_bound_to_a_computation_is_a_variable() {
        // `n : rows(X)` — a row count is not a unit however it is drawn.
        let w = read_str(&wrap(&region(
            r#"<e type="operand" style="unit">n</e><e type="operand">X</e>
               <e type="function" args="1">rows</e><e type="operator" args="2">:</e>"#,
        )));
        let m: Vec<_> = w.math().collect();
        let Statement::Define { target, .. } = &m[0].statement else {
            panic!()
        };
        assert_eq!(*target, Expr::Name("n".into()));
    }
}
