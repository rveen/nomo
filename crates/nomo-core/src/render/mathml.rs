//! The symbolic column as typeset mathematics.
//!
//! # Why this exists
//!
//! This project bets that engineers will accept a *text syntax* — that typing
//! `w*L^2/8` beats hunting for a fraction template with a mouse. It does not
//! bet that they will accept text *output*: a worksheet is signed and filed and
//! read by somebody who did not write it, and `w·L²/8` on a page is a worse
//! thing to hand them than the fraction they would have drawn by hand.
//!
//! So the input stays text and the output learns to be a formula. MathML rather
//! than an image or a layout engine of our own, for the same reason the plots
//! are SVG the engine draws: it is markup, it is in the artifact, it needs no
//! script and no font to be fetched, and a browser knows how to line it up.
//!
//! # What it does and does not do
//!
//! Division becomes a fraction, a power becomes a superscript, `sqrt` becomes a
//! radical, and a bracket that only existed to say "divide this whole thing" is
//! dropped because the fraction says it now. That is most of what makes a
//! formula look like one.
//!
//! Everything it has no typeset form for — a conditional, a conversion, a
//! comparison, a plot — falls back to the linear text this renderer has always
//! produced, wrapped in `<mtext>`. A fallback rather than a refusal because the
//! alternative is a hole in the middle of a worksheet, and because these are the
//! constructs a reader is least surprised to see written out.
//!
//! # It is off unless asked for
//!
//! [`crate::RenderOptions::mathml`] defaults to false, so nothing changes for
//! anyone until they ask. Chrome is checked by `scripts/check-mathml.mjs`;
//! Firefox and Safari implement MathML Core and are **not** checked here,
//! because this machine has one browser. That is a gap in the evidence rather
//! than a claim about those browsers.

use crate::ast::{BinaryOp, UnaryOp};
use crate::render::{escape, number, Renderer};
use crate::trace::{Trace, TraceNode};
use crate::value::Value;

/// One expression as a MathML element.
///
/// `linear` is what the same expression looks like as text, for the constructs
/// that have no typeset form — passed in rather than recomputed so that the two
/// columns cannot disagree about what an expression says.
pub fn render(r: &Renderer, trace: &Trace, linear: &str, substituted: bool) -> String {
    format!(
        "<math display=\"inline\"><mrow>{}</mrow></math>",
        node(r, trace, linear, substituted)
    )
}

/// Where a bracket is still needed once fractions and superscripts have taken
/// their arguments.
///
/// Lower binds looser. These match the parser's own table, minus the levels a
/// typeset form removes: nothing inside a fraction or a radical needs a bracket,
/// because the rule that made one necessary is drawn instead.
fn precedence(node: &TraceNode) -> u8 {
    match node {
        TraceNode::Binary { op, .. } => match op {
            BinaryOp::And | BinaryOp::Or => 1,
            BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::Le
            | BinaryOp::Ge
            | BinaryOp::Equal
            | BinaryOp::NotEqual => 2,
            BinaryOp::Add | BinaryOp::Sub => 3,
            BinaryOp::Mul | BinaryOp::ImplicitMul => 4,
            // A division is a fraction and a power is a superscript; both bind
            // as tightly as an atom once drawn.
            BinaryOp::Div | BinaryOp::Pow => 9,
        },
        TraceNode::Unary { .. } => 4,
        TraceNode::Conditional { .. } | TraceNode::Convert { .. } => 0,
        _ => 9,
    }
}

fn bracketed(r: &Renderer, trace: &Trace, linear: &str, least: u8, sub: bool) -> String {
    let inner = node(r, trace, linear, sub);
    if precedence(&trace.node) < least {
        format!("<mo>(</mo>{inner}<mo>)</mo>")
    } else {
        inner
    }
}

fn node(r: &Renderer, trace: &Trace, linear: &str, sub: bool) -> String {
    // In the substituted column a name is replaced by what it holds, and what it
    // holds is a quantity rather than a formula — a number and a unit, which is
    // what `mtext` is for. Rendered by the same code as the text column, so the
    // two cannot disagree about a value.
    if sub {
        if let TraceNode::Variable { .. } = &trace.node {
            return format!("<mtext>{}</mtext>", escape(&r.substituted(trace)));
        }
    }
    match &trace.node {
        // The node carries no value — the number is in the trace's result, the
        // way the linear renderer reads it — and it is formatted by the same
        // code, so a number cannot read one way typeset and another in text.
        TraceNode::Number => match &trace.value {
            Ok(Value::Scalar(q)) => {
                format!("<mn>{}</mn>", escape(&number::format(q.value, &r.numbers)))
            }
            _ => String::from("<mo>?</mo>"),
        },
        TraceNode::Constant(name) => identifier(name),
        TraceNode::Variable { name, .. } | TraceNode::FnRef(name) => identifier(name),
        TraceNode::UnitRef(name) => format!("<mi mathvariant=\"normal\">{}</mi>", escape(name)),
        TraceNode::Text => format!("<mtext>{}</mtext>", escape(linear)),

        TraceNode::AffineLiteral { magnitude, unit } => format!(
            "<mn>{}</mn><mo>&#8290;</mo><mi mathvariant=\"normal\">{}</mi>",
            escape(&number::format(*magnitude, &r.numbers)),
            escape(unit)
        ),

        TraceNode::Paren(inner) => {
            // A bracket that exists only to group what a fraction or a radical
            // now encloses is dropped: `(a+b)/c` typesets as a fraction, and
            // drawing its brackets as well would be saying the same thing twice.
            match &inner.node {
                TraceNode::Binary {
                    op: BinaryOp::Div | BinaryOp::Pow,
                    ..
                } => node(r, inner, linear, sub),
                _ => format!("<mo>(</mo>{}<mo>)</mo>", node(r, inner, linear, sub)),
            }
        }

        TraceNode::Unary { op, operand } => {
            let sign = match op {
                UnaryOp::Neg => "<mo>&#8722;</mo>",
                UnaryOp::Pos => "<mo>+</mo>",
                UnaryOp::Not => "<mo lspace=\"0\" rspace=\"0.2em\">not</mo>",
            };
            format!("{sign}{}", bracketed(r, operand, linear, 4, sub))
        }

        TraceNode::Binary { op, lhs, rhs } => binary(r, *op, lhs, rhs, linear, sub),

        TraceNode::Call { name, args } => call(r, name, args, linear, sub),

        TraceNode::Index { base, indices } => {
            let subscript: Vec<String> = indices
                .iter()
                .map(|i| node(r, i, linear, sub))
                .collect::<Vec<_>>();
            format!(
                "<msub>{}<mrow>{}</mrow></msub>",
                bracketed(r, base, linear, 9, sub),
                subscript.join("<mo>,</mo>")
            )
        }

        TraceNode::Vector(elements) => {
            let rows: Vec<String> = elements
                .iter()
                .map(|e| format!("<mtr><mtd>{}</mtd></mtr>", node(r, e, linear, sub)))
                .collect();
            bracket_table(&rows.join(""))
        }

        TraceNode::Matrix(rows) => {
            let body: Vec<String> = rows
                .iter()
                .map(|row| {
                    let cells: Vec<String> = row
                        .iter()
                        .map(|c| format!("<mtd>{}</mtd>", node(r, c, linear, sub)))
                        .collect();
                    format!("<mtr>{}</mtr>", cells.join(""))
                })
                .collect();
            bracket_table(&body.join(""))
        }

        // No typeset form here, and a hole would be worse than a sentence: the
        // linear rendering is what this worksheet has always shown.
        TraceNode::Conditional { .. } | TraceNode::Convert { .. } | TraceNode::Malformed => {
            format!("<mtext>{}</mtext>", escape(linear))
        }
    }
}

fn binary(r: &Renderer, op: BinaryOp, lhs: &Trace, rhs: &Trace, linear: &str, sub: bool) -> String {
    match op {
        // The two that are the point of doing this at all.
        //
        // A fraction bar groups what is above and below it, so a bracket that
        // was there to say exactly that is dropped: `M/(2 MPa)` typesets with
        // `2 MPa` under the bar and no parentheses, which is how it would be
        // written by hand. This is the whole visual difference between typeset
        // output and the linear text with a bar drawn through it.
        BinaryOp::Div => format!(
            "<mfrac><mrow>{}</mrow><mrow>{}</mrow></mfrac>",
            ungrouped(r, lhs, linear, sub),
            ungrouped(r, rhs, linear, sub)
        ),
        BinaryOp::Pow => format!(
            "<msup>{}<mrow>{}</mrow></msup>",
            bracketed(r, lhs, linear, 9, sub),
            node(r, rhs, linear, sub)
        ),
        _ => {
            let (level, symbol) = match op {
                BinaryOp::Add => (3, "+"),
                BinaryOp::Sub => (3, "&#8722;"),
                BinaryOp::Mul => (4, "&#183;"),
                // Juxtaposition is invisible multiplication, and it is what
                // attaches a unit to a number: `5 cm` should look like `5 cm`.
                BinaryOp::ImplicitMul => (4, "&#8290;"),
                BinaryOp::Lt => (2, "&lt;"),
                BinaryOp::Gt => (2, "&gt;"),
                BinaryOp::Le => (2, "&#8804;"),
                BinaryOp::Ge => (2, "&#8805;"),
                BinaryOp::Equal => (2, "="),
                BinaryOp::NotEqual => (2, "&#8800;"),
                BinaryOp::And => (1, "and"),
                BinaryOp::Or => (1, "or"),
                BinaryOp::Div | BinaryOp::Pow => unreachable!("handled above"),
            };
            let word = matches!(op, BinaryOp::And | BinaryOp::Or);
            let operator = if word {
                format!("<mo lspace=\"0.3em\" rspace=\"0.3em\">{symbol}</mo>")
            } else {
                format!("<mo>{symbol}</mo>")
            };
            format!(
                "{}{operator}{}",
                bracketed(r, lhs, linear, level, sub),
                // The right operand of a subtraction or a division needs a
                // bracket at its own level: `a - (b - c)` is not `a - b - c`.
                bracketed(
                    r,
                    rhs,
                    linear,
                    level + u8::from(matches!(op, BinaryOp::Sub)),
                    sub
                )
            )
        }
    }
}

/// A node with its outermost bracket removed, for a place that groups already.
///
/// A fraction and a radical both enclose what they contain, so a `Paren` around
/// their argument is drawing the same statement twice.
fn ungrouped(r: &Renderer, trace: &Trace, linear: &str, sub: bool) -> String {
    match &trace.node {
        TraceNode::Paren(inner) => node(r, inner, linear, sub),
        _ => node(r, trace, linear, sub),
    }
}

fn call(r: &Renderer, name: &str, args: &[Trace], linear: &str, sub: bool) -> String {
    let inner: Vec<String> = args.iter().map(|a| node(r, a, linear, sub)).collect();
    match (name, args.len()) {
        // A radical, which is the third thing worth drawing.
        ("sqrt", 1) => format!("<msqrt>{}</msqrt>", ungrouped(r, &args[0], linear, sub)),
        ("abs", 1) => format!(
            "<mo>|</mo>{}<mo>|</mo>",
            ungrouped(r, &args[0], linear, sub)
        ),
        _ => format!(
            "<mi>{}</mi><mo>&#8289;</mo><mo>(</mo>{}<mo>)</mo>",
            escape(name),
            inner.join("<mo>,</mo>")
        ),
    }
}

/// A vector or matrix in square brackets, which is how this language writes one.
fn bracket_table(rows: &str) -> String {
    format!("<mo>[</mo><mtable>{rows}</mtable><mo>]</mo>")
}

fn identifier(name: &str) -> String {
    // A subscripted name — `V_drop`, `sigma_allow` — is drawn as one, since that
    // is what the underscore was standing in for.
    match name.split_once('_') {
        Some((stem, sub)) if !stem.is_empty() && !sub.is_empty() => format!(
            "<msub><mi>{}</mi><mi>{}</mi></msub>",
            escape(stem),
            escape(sub)
        ),
        _ => format!("<mi>{}</mi>", escape(name)),
    }
}
