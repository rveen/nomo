//! Rendering: turning an evaluated worksheet into something a person reads.
//!
//! # Three views of one expression
//!
//! This is why evaluation produces a trace rather than a value. The same
//! subtree is written out three ways:
//!
//! ```text
//! V = π·r²·h = π·(5 cm)²·(12 cm) = 0.942 dm³
//!     ───┬──   ────────┬───────    ────┬───
//!   symbolic      substituted        result
//! ```
//!
//! The symbolic form is the expression as typed. The substituted form replaces
//! each *variable* with its value, so a reader can check the arithmetic without
//! scrolling back — this is the column engineers actually audit. Constants such
//! as `π` stay symbolic, because expanding them adds noise and no information.
//!
//! # Parenthesisation
//!
//! Substitution can turn an atom into a compound: `r` becomes `5 cm`, which is a
//! product. Rendering therefore tracks precedence and adds parentheses where the
//! substituted text would otherwise regroup — `(5 cm)²`, never `5 cm²`.

pub mod html;
pub mod number;
pub mod plot;
pub mod text;

use crate::ast::{BinaryOp, UnaryOp};
use crate::complex::ComplexQuantity;
use crate::math;
use crate::plot::PlotValue;
use crate::quantity::Quantity;
use crate::trace::{DisplayTarget, Trace, TraceNode};
use crate::unit::UnitTable;
use crate::value::{EvalError, Value};
use number::NumberFormat;

/// How to render a worksheet.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub numbers: NumberFormat,
    /// Include the substituted-values column. Turning it off gives a terse
    /// `name = result` listing.
    pub show_substitution: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            numbers: NumberFormat::default(),
            show_substitution: true,
        }
    }
}

/// Which of the three views to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The expression as written.
    Symbolic,
    /// Variables replaced by their values.
    Substituted,
}

/// Operator precedence for output, matching the parser's.
///
/// Conversion is not listed: `->` belongs to the result column, not to the
/// expression, so the renderer never has to bracket around it.
mod prec {
    /// `if … then … else …`, which binds loosest of all: it swallows whatever
    /// follows into its final arm, so it needs brackets inside anything.
    pub const CONDITIONAL: u8 = 0;
    pub const LOGICAL: u8 = 1;
    pub const SUM: u8 = 2;
    pub const PRODUCT: u8 = 3;
    pub const UNARY: u8 = 4;
    pub const POWER: u8 = 5;
    pub const ATOM: u8 = 6;
}

/// An operator symbol with spaces around it, which every word-shaped and
/// comparison operator needs and no arithmetic one does.
fn spaced(symbol: &str) -> &'static str {
    match symbol {
        "<" => " < ",
        ">" => " > ",
        "≤" => " ≤ ",
        "≥" => " ≥ ",
        "==" => " == ",
        "≠" => " ≠ ",
        "and" => " and ",
        "or" => " or ",
        other => unreachable!("no spaced form for `{other}`"),
    }
}

/// A rendered fragment, carrying the precedence of its outermost operator so a
/// parent can decide whether it needs parentheses.
struct Piece {
    text: String,
    prec: u8,
}

impl Piece {
    fn atom(text: impl Into<String>) -> Piece {
        Piece {
            text: text.into(),
            prec: prec::ATOM,
        }
    }

    /// The text, parenthesised if it binds more loosely than the context needs.
    fn in_context(&self, needed: u8) -> String {
        if self.prec < needed {
            format!("({})", self.text)
        } else {
            self.text.clone()
        }
    }
}

pub struct Renderer<'a> {
    pub opts: &'a RenderOptions,
    /// How numbers are shown *now*.
    ///
    /// Owned and mutable rather than read from `opts`, because `digits n` moves
    /// it part way down a worksheet: the options say where the document starts
    /// and this says where the renderer has got to.
    pub numbers: NumberFormat,
    pub units: &'a UnitTable,
    /// The worksheet text. A conversion target is recorded as a span rather than
    /// a copied string, because the engine never sees a file and should not
    /// carry duplicates of text the caller already holds.
    pub source: &'a str,
}

impl<'a> Renderer<'a> {
    pub fn new(opts: &'a RenderOptions, units: &'a UnitTable, source: &'a str) -> Renderer<'a> {
        Renderer {
            opts,
            numbers: opts.numbers,
            units,
            source,
        }
    }

    /// Show results to `figures` significant figures from here down.
    pub fn set_significant_figures(&mut self, figures: u32) {
        self.numbers.significant_figures = figures as usize;
    }

    /// The expression as written.
    pub fn symbolic(&self, trace: &Trace) -> String {
        self.walk(trace, Mode::Symbolic).text
    }

    /// The expression with variables replaced by their values.
    pub fn substituted(&self, trace: &Trace) -> String {
        self.walk(trace, Mode::Substituted).text
    }

    /// Whether a power may be written as a superscript here.
    ///
    /// Not inside another power's exponent: `2^3^2` rendered as `2^3²` mixes two
    /// notations in one chain and reads as `(2³)²` about as easily as the truth.
    /// When any rung of the chain needs a caret, the whole chain uses carets.
    fn walk_exponent(&self, trace: &Trace, mode: Mode) -> Piece {
        match &trace.node {
            TraceNode::Binary { op, lhs, rhs } if *op == BinaryOp::Pow => {
                let l = self.walk_exponent(lhs, mode);
                let r = self.walk_exponent(rhs, mode);
                Piece {
                    text: format!(
                        "{}^{}",
                        l.in_context(prec::POWER + 1),
                        r.in_context(prec::POWER)
                    ),
                    prec: prec::POWER,
                }
            }
            _ => self.walk(trace, mode),
        }
    }

    /// The final value, in the unit the statement asked for.
    pub fn result(&self, trace: &Trace) -> String {
        match &trace.value {
            Err(_) => match trace.root_error() {
                Some((_, e)) => format!("[{e}]"),
                None => "[error]".into(),
            },
            Ok(v) => match self.target_of(trace) {
                Some(t) => self.value(v, Some(t)),
                // No explicit `->`, but the expression may still wear a unit
                // plainly: `r = 5 cm` should report centimetres, not the metres
                // the engine stores.
                None => match self.inferred_unit(trace) {
                    Some(u) => self.value_in_unit(v, &u),
                    None => self.value(v, None),
                },
            },
        }
    }

    /// The unit an expression wears plainly, when it has no explicit `->`.
    fn inferred_unit(&self, trace: &Trace) -> Option<crate::unit::Unit> {
        match &trace.node {
            TraceNode::Binary { op, rhs, .. } if op.is_mul() => match &rhs.node {
                TraceNode::UnitRef(name) => self.units.resolve(name).ok(),
                _ => None,
            },
            TraceNode::Paren(inner) => self.inferred_unit(inner),
            _ => None,
        }
    }

    fn value_in_unit(&self, value: &Value, unit: &crate::unit::Unit) -> String {
        let one = |q: &Quantity| match q.to_unit(unit) {
            Ok(m) => join(number::format(m, &self.numbers), &unit.symbol),
            Err(_) => self.quantity(q, None),
        };
        match value {
            Value::Scalar(q) => one(q),
            // Both parts go into the unit the author wrote, for the reason a
            // real one does: `Z = (3 + 4i) kΩ` should report kilohms, not the
            // ohms the engine stores.
            Value::Complex(c) => {
                match (
                    c.real_part().to_unit(unit),
                    c.imaginary_part().to_unit(unit),
                ) {
                    (Ok(re), Ok(im)) => {
                        let sign = if im.is_sign_negative() { "-" } else { "+" };
                        join(
                            format!(
                                "({} {sign} {}i)",
                                number::format(re, &self.numbers),
                                number::format(math::abs(im), &self.numbers)
                            ),
                            &unit.symbol,
                        )
                    }
                    _ => self.value(value, None),
                }
            }
            Value::Vector(v) => {
                let parts: Vec<String> = v.elements.iter().map(one).collect();
                format!("[{}]", parts.join(", "))
            }
            other => self.value(other, None),
        }
    }

    /// Whether the substituted column would say anything the symbolic one does
    /// not. For `r = 5 cm` the two are identical, and printing both is noise.
    pub fn substitution_is_informative(&self, trace: &Trace) -> bool {
        if !self.opts.show_substitution {
            return false;
        }
        // Substituting a bare name just restates the result in different units.
        let mut inner = trace;
        while let TraceNode::Convert { value, .. } | TraceNode::Paren(value) = &inner.node {
            inner = value;
        }
        if matches!(inner.node, TraceNode::Variable { .. }) {
            return false;
        }
        let symbolic = self.symbolic(trace);
        let substituted = self.substituted(trace);
        substituted != symbolic && substituted != self.result(trace)
    }

    /// True if the expression is a literal quantity — one magnitude wearing
    /// units, with no arithmetic that produces a new number.
    ///
    /// The symbolic form of such an expression *is* the answer, so restating it
    /// adds a column and no information: `g = 9.81 m/s²` should not be followed
    /// by `= 9.81 m·s⁻²`. But `x = 2 + 3` genuinely computes something, and
    /// suppressing its result would hide the answer — which is why this counts
    /// magnitudes rather than merely checking for the absence of names.
    pub fn is_literal_quantity(&self, trace: &Trace) -> bool {
        magnitudes(trace).is_some_and(|n| n <= 1)
    }

    fn target_of<'t>(&self, trace: &'t Trace) -> Option<&'t DisplayTarget> {
        match &trace.node {
            TraceNode::Convert { target, .. } => target.as_deref(),
            _ => None,
        }
    }

    /// Format a value, converted into `target` if one was requested.
    pub fn value(&self, value: &Value, target: Option<&DisplayTarget>) -> String {
        match value {
            Value::Scalar(q) => self.quantity(q, target),
            Value::Text(t) => format!("\"{t}\""),
            Value::Complex(c) => self.complex(c, target),
            // Never reachable from a worksheet: a dual exists only between the
            // seeding of a `derivative` call and the slope coming out of it.
            // Rendered as its value rather than as nothing, so that an escape
            // shows up as a number that is right rather than as a hole.
            Value::Dual(d) => self.quantity(&d.value, target),
            Value::Vector(v) => {
                let parts: Vec<String> = v
                    .elements
                    .iter()
                    .map(|q| self.quantity(q, target))
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Matrix(m) => {
                let rows: Vec<String> = (0..m.rows)
                    .map(|r| {
                        let cells: Vec<String> = (0..m.cols)
                            .map(|c| self.quantity(&m.get(r, c), target))
                            .collect();
                        format!("[{}]", cells.join(", "))
                    })
                    .collect();
                format!("[{}]", rows.join(", "))
            }
            Value::Plot(p) => self.plot_summary(p),
        }
    }

    /// A plot as one line of text.
    ///
    /// The same reasoning as an `[image …]` line: this is what the golden suite
    /// diffs, so it has to be stable and it has to say enough that a plot which
    /// moved shows up as a line that changed. The span and the extent are what
    /// move when a curve does; the sample count is there because changing it
    /// changes every drawing in the corpus and should be impossible to do
    /// quietly.
    pub fn plot_summary(&self, p: &PlotValue) -> String {
        let names: Vec<&str> = p.series.iter().map(|s| s.name.as_str()).collect();
        let unit_of = |dim: &crate::dim::Dimension| -> String {
            if dim.is_dimensionless() {
                String::new()
            } else {
                match self.units.preferred_for(dim) {
                    Some(u) => format!(" {}", u.symbol),
                    None => format!(" {dim}"),
                }
            }
        };
        let n = |x: f64| number::format(x, &self.numbers);
        let extent = match p.y_range() {
            Some((lo, hi)) => format!("{} to {}{}", n(lo), n(hi), unit_of(&p.y_dim)),
            None => String::from("nothing finite"),
        };
        let gaps = match p.gaps() {
            0 => String::new(),
            g => format!(", {g} gap(s)"),
        };
        // Every curve's count, not the first one's: sampled curves all carry
        // `SAMPLES`, but tables carry what they carry, and a table that lost a
        // row has to show up as a line that changed. Written once when they
        // agree, which is the common case.
        let mut counts: Vec<String> = p
            .series
            .iter()
            .map(|s| s.points.len().to_string())
            .collect();
        counts.dedup();
        // The scale and any chosen window belong in the summary because the
        // golden suite reads this line: a log plot and a linear one over the
        // same span hold different samples, and a snapshot that could not tell
        // them apart would let one turn into the other unnoticed.
        let scale = match (p.x_log, p.y_log) {
            (false, false) => String::new(),
            (true, false) => String::from(", log x"),
            (false, true) => String::from(", log y"),
            (true, true) => String::from(", log x and y"),
        };
        let window = match (p.x_limits, p.y_limits) {
            (None, None) => String::new(),
            (x, y) => {
                let side = |l: Option<(f64, f64)>| match l {
                    Some((lo, hi)) => format!("{} to {}", n(lo), n(hi)),
                    None => String::from("auto"),
                };
                format!(", window x {} y {}", side(x), side(y))
            }
        };
        format!(
            "[plot {}: {} to {}{}, {} points, {extent}{gaps}{scale}{window}]",
            names.join(", "),
            n(p.from),
            n(p.to),
            unit_of(&p.x_dim),
            counts.join(", "),
        )
    }

    /// Format a complex quantity: `3 - 4i`, or `(1 + 2i) Ω` with a dimension.
    ///
    /// The unit goes outside the brackets and is written once, because it
    /// belongs to the value and not to either part of it — an impedance is
    /// `(1 + 2i)·Ω`, which is how the corpus writes it and how design note item
    /// 29 records it. Writing it twice would read as two measurements, and
    /// writing it once *inside* — `1 + 2i Ω` — would say it applied to the
    /// imaginary part alone.
    ///
    /// The sign is part of the operator: `3 + -4i` is not how anyone writes a
    /// conjugate.
    pub fn complex(&self, c: &ComplexQuantity, target: Option<&DisplayTarget>) -> String {
        // Both parts share one dimension, so scaling them into the requested
        // unit is the same division twice, and the symbol is named once.
        let (re, im, symbol) = match target {
            Some(t) => {
                let symbol = t.unit.as_ref().map_or_else(
                    || t.span.text(self.source).to_string(),
                    |u| u.symbol.clone(),
                );
                (c.re / t.factor, c.im / t.factor, Some(symbol))
            }
            None if c.dim.is_dimensionless() => (c.re, c.im, None),
            None => (
                c.re,
                c.im,
                Some(match self.units.preferred_for(&c.dim) {
                    Some(u) => u.symbol.clone(),
                    None => c.dim.to_string(),
                }),
            ),
        };
        let sign = if im.is_sign_negative() { "-" } else { "+" };
        let parts = format!(
            "{} {sign} {}i",
            number::format(re, &self.numbers),
            number::format(math::abs(im), &self.numbers)
        );
        match symbol {
            // Bracketed so the unit plainly applies to the whole value.
            Some(symbol) => join(format!("({parts})"), &symbol),
            None => parts,
        }
    }

    /// Format one quantity with a unit.
    pub fn quantity(&self, q: &Quantity, target: Option<&DisplayTarget>) -> String {
        if let Some(t) = target {
            let magnitude = match &t.unit {
                // Only a named unit can carry an offset, so this is the only
                // path that can reach an offset temperature scale.
                Some(u) => q.to_unit(u).unwrap_or(f64::NAN),
                None => q.value / t.factor,
            };
            let symbol = t.unit.as_ref().map_or_else(
                || t.span.text(self.source).to_string(),
                |u| u.symbol.clone(),
            );
            return join(number::format(magnitude, &self.numbers), &symbol);
        }

        if q.is_dimensionless() {
            return number::format(q.value, &self.numbers);
        }
        // No unit was requested, so fall back to a coherent SI name if one
        // exists, and to raw base dimensions otherwise.
        match self.units.preferred_for(&q.dim) {
            Some(u) => join(number::format(q.value, &self.numbers), &u.symbol),
            None => join(number::format(q.value, &self.numbers), &q.dim.to_string()),
        }
    }

    fn walk(&self, trace: &Trace, mode: Mode) -> Piece {
        match &trace.node {
            TraceNode::Number => match &trace.value {
                Ok(Value::Scalar(q)) => Piece::atom(number::format(q.value, &self.numbers)),
                _ => Piece::atom("?"),
            },

            // Written back with its quotes, so that a line reads as the source
            // did and a string result is never mistaken for a name.
            TraceNode::Text => match &trace.value {
                Ok(Value::Text(t)) => Piece::atom(format!("\"{t}\"")),
                _ => Piece::atom("?"),
            },

            // Constants stay symbolic even when substituting: expanding `π` to
            // 3.14159 makes the line longer and tells the reader nothing.
            TraceNode::Constant(name) => Piece::atom(constant_symbol(name)),
            TraceNode::UnitRef(name) => Piece::atom(name.clone()),
            // A function passed by name is shown as the name, in every column.
            TraceNode::FnRef(name) => Piece::atom(name.clone()),

            TraceNode::Variable { name, unit } => match (mode, &trace.value) {
                (Mode::Substituted, Ok(v)) => {
                    // Shown in the unit the binding was written in, so the reader
                    // sees the numbers they typed.
                    let text = match (unit, v.as_scalar()) {
                        (Some(t), Some(q)) => {
                            // A named target converts through the unit table,
                            // which is the only route that can carry an offset
                            // scale; a compound one — `mm^2`, `MN/m` — is a
                            // factor and the text that was written.
                            let symbol = t.unit.as_ref().map_or_else(
                                || t.span.text(self.source).to_string(),
                                |u| u.symbol.clone(),
                            );
                            let magnitude = match &t.unit {
                                Some(u) => q.to_unit(u).unwrap_or(q.value / t.factor),
                                None => q.value / t.factor,
                            };
                            join(number::format(magnitude, &self.numbers), &symbol)
                        }
                        _ => self.value(v, None),
                    };
                    // A substituted value may itself be a product ("5 cm"), so
                    // it must be parenthesised inside a tighter context. A
                    // complex one is looser still: `3 + 4i` is a *sum*, and
                    // treating it as a product printed `z - w` as
                    // `3 + 4i - 1 - 2i`, which reads as a different number.
                    // With a unit it arrives already bracketed — `(3 + 4i) Ω` —
                    // and binds like `5 cm` again.
                    let prec = match v {
                        Value::Complex(c) if c.dim.is_dimensionless() => prec::SUM,
                        _ if text.contains(' ') => prec::PRODUCT,
                        _ => prec::ATOM,
                    };
                    Piece { text, prec }
                }
                _ => Piece::atom(name.clone()),
            },

            // `if c then a else b`. The arm that did not run has no values in
            // it, so it is always shown as written — which is also what a reader
            // wants: the substituted column should say which way the worksheet
            // went, not pretend both arms were computed.
            TraceNode::Conditional {
                cond,
                then,
                otherwise,
            } => {
                // An arm that did not run has no values in it, so it is always
                // shown as written. That is also what a reader wants: the
                // substituted column should say which way the worksheet went,
                // not pretend both arms were computed.
                let arm = |t: &Trace, context: u8| {
                    let mode = if matches!(t.value, Err(EvalError::NotTaken)) {
                        Mode::Symbolic
                    } else {
                        mode
                    };
                    self.walk(t, mode).in_context(context)
                };
                Piece {
                    text: format!(
                        "if {} then {} else {}",
                        arm(cond, prec::CONDITIONAL + 1),
                        arm(then, prec::CONDITIONAL + 1),
                        // The `else` arm reaches as far as it can, so a
                        // conditional inside it needs no brackets — that is what
                        // makes `else if` chain. The other two arms do need
                        // them: parsing either at the loosest power means a
                        // nested `if` there would swallow this one's `then` or
                        // `else`.
                        arm(otherwise, prec::CONDITIONAL),
                    ),
                    prec: prec::CONDITIONAL,
                }
            }

            TraceNode::AffineLiteral { magnitude, unit } => Piece {
                text: join(number::format(*magnitude, &self.numbers), unit),
                prec: prec::PRODUCT,
            },

            TraceNode::Unary { op, operand } => {
                let inner = self.walk(operand, mode);
                if *op == UnaryOp::Not {
                    return Piece {
                        text: format!("not {}", inner.in_context(prec::LOGICAL + 1)),
                        prec: prec::LOGICAL,
                    };
                }
                let symbol = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Pos => "+",
                    UnaryOp::Not => unreachable!("handled above"),
                };
                Piece {
                    text: format!("{symbol}{}", inner.in_context(prec::UNARY)),
                    prec: prec::UNARY,
                }
            }

            TraceNode::Binary { op, lhs, rhs } => {
                let (level, symbol) = match op {
                    BinaryOp::Add => (prec::SUM, " + "),
                    BinaryOp::Sub => (prec::SUM, " - "),
                    BinaryOp::Mul => (prec::PRODUCT, "·"),
                    // Juxtaposition stays juxtaposition: `5 cm`, not `5·cm`.
                    BinaryOp::ImplicitMul => (prec::PRODUCT, " "),
                    BinaryOp::Div => (prec::PRODUCT, "/"),
                    BinaryOp::Pow => (prec::POWER, "^"),
                    // Spelled the way `symbol()` spells them, so what the
                    // renderer shows and what the parser accepts stay one thing.
                    BinaryOp::Lt
                    | BinaryOp::Gt
                    | BinaryOp::Le
                    | BinaryOp::Ge
                    | BinaryOp::Equal
                    | BinaryOp::NotEqual => (prec::LOGICAL + 1, spaced(op.symbol())),
                    BinaryOp::And | BinaryOp::Or => (prec::LOGICAL, spaced(op.symbol())),
                };
                let l = self.walk(lhs, mode);
                let r = self.walk(rhs, mode);

                // A small whole-number exponent reads better as a superscript,
                // which is also how the design note writes it: `r²`, not `r^2`.
                if *op == BinaryOp::Pow {
                    if let Some(sup) = superscript(rhs) {
                        return Piece {
                            text: format!("{}{sup}", l.in_context(prec::POWER + 1)),
                            prec: prec::ATOM,
                        };
                    }
                    // The exponent needs a caret, so nothing below it may use a
                    // superscript either.
                    let r = self.walk_exponent(rhs, mode);
                    return Piece {
                        text: format!(
                            "{}^{}",
                            l.in_context(prec::POWER + 1),
                            r.in_context(prec::POWER)
                        ),
                        prec: prec::POWER,
                    };
                }
                // Left-associative operators need the right operand bracketed one
                // level tighter; `^` is right-associative, so it is the reverse.
                let (need_l, need_r) = match op {
                    BinaryOp::Pow => (level + 1, level),
                    _ => (level, level + 1),
                };
                Piece {
                    text: format!("{}{symbol}{}", l.in_context(need_l), r.in_context(need_r)),
                    prec: level,
                }
            }

            TraceNode::Call { name, args } => {
                let rendered: Vec<String> = args.iter().map(|a| self.walk(a, mode).text).collect();
                Piece::atom(format!("{name}({})", rendered.join(", ")))
            }

            TraceNode::Index { base, indices } => {
                let b = self.walk(base, mode);
                let idx: Vec<String> = indices.iter().map(|i| self.walk(i, mode).text).collect();
                Piece::atom(format!("{}[{}]", b.in_context(prec::ATOM), idx.join(", ")))
            }

            TraceNode::Vector(elements) => {
                let parts: Vec<String> = elements.iter().map(|e| self.walk(e, mode).text).collect();
                Piece::atom(format!("[{}]", parts.join(", ")))
            }

            TraceNode::Matrix(rows) => {
                let parts: Vec<String> = rows
                    .iter()
                    .map(|row| {
                        let cells: Vec<String> =
                            row.iter().map(|e| self.walk(e, mode).text).collect();
                        format!("[{}]", cells.join(", "))
                    })
                    .collect();
                Piece::atom(format!("[{}]", parts.join(", ")))
            }

            // The user's own parentheses are dropped: precedence-aware rendering
            // reinserts exactly the ones the structure needs, so keeping both
            // would double them up.
            TraceNode::Paren(inner) => self.walk(inner, mode),

            // The conversion target belongs to the result column, not here.
            TraceNode::Convert { value, .. } => self.walk(value, mode),

            TraceNode::Malformed => Piece::atom("?"),
        }
    }
}

/// How many independent magnitudes an expression contains, or `None` if it
/// performs arithmetic that yields a value not written in the source.
///
/// Multiplying and dividing by units does not create a new number — `9.81 m/s²`
/// still shows the 9.81 the author typed — but adding, calling a function, or
/// multiplying two numbers together does.
fn magnitudes(trace: &Trace) -> Option<usize> {
    match &trace.node {
        TraceNode::Number | TraceNode::AffineLiteral { .. } => Some(1),
        TraceNode::UnitRef(_) => Some(0),
        TraceNode::Paren(inner) | TraceNode::Unary { operand: inner, .. } => magnitudes(inner),
        TraceNode::Binary { op, lhs, rhs } => match op {
            BinaryOp::Mul | BinaryOp::ImplicitMul | BinaryOp::Div => {
                Some(magnitudes(lhs)? + magnitudes(rhs)?)
            }
            // A unit exponent is structural, not a magnitude — `s^2` in `m/s^2`
            // is part of the unit, not a number the reader is tracking.
            BinaryOp::Pow => match rhs.node {
                TraceNode::Number => magnitudes(lhs),
                _ => None,
            },
            BinaryOp::Add | BinaryOp::Sub => None,
            // A truth value is not a magnitude anybody is tracking through the
            // substituted column.
            op if op.is_logical() => None,
            _ => None,
        },
        // A literal collection counts as one magnitude however many entries it
        // has, since `[5, 10] Hz` is still just what the author wrote.
        TraceNode::Vector(elements) => elements
            .iter()
            .all(|e| matches!(e.node, TraceNode::Number))
            .then_some(1),
        TraceNode::Matrix(rows) => rows
            .iter()
            .all(|row| row.iter().all(|e| matches!(e.node, TraceNode::Number)))
            .then_some(1),
        _ => None,
    }
}

/// The symbol a built-in constant is conventionally written with.
fn constant_symbol(name: &str) -> String {
    match name {
        "pi" => "π".into(),
        "tau" => "τ".into(),
        "inf" => "∞".into(),
        other => other.into(),
    }
}

/// A Unicode superscript for a literal whole-number exponent, if it is small
/// enough to stay legible.
fn superscript(exponent: &Trace) -> Option<String> {
    if !matches!(exponent.node, TraceNode::Number) {
        return None;
    }
    let q = exponent.value.as_ref().ok()?.as_scalar()?;
    if !q.is_dimensionless() {
        return None;
    }
    let n = q.value;
    if n.fract() != 0.0 || !(-9.0..=9.0).contains(&n) {
        return None;
    }
    let n = n as i32;
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    let mut out = String::new();
    if n < 0 {
        out.push('⁻');
    }
    out.push(DIGITS[n.unsigned_abs() as usize]);
    Some(out)
}

/// Join a magnitude and a unit symbol.
///
/// Degree-style symbols attach directly — `30°`, not `30 °` — because that is
/// how they are written.
fn join(magnitude: String, symbol: &str) -> String {
    let symbol = superscript_exponents(symbol);
    if symbol.is_empty() {
        return magnitude;
    }
    if symbol.starts_with('°') || symbol == "%" {
        format!("{magnitude}{symbol}")
    } else {
        format!("{magnitude} {symbol}")
    }
}

/// Rewrite `m^3` as `m³` inside a unit symbol, so units match the superscripts
/// already used in expressions.
fn superscript_exponents(symbol: &str) -> String {
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    let mut out = String::with_capacity(symbol.len());
    let mut chars = symbol.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'-') {
            chars.next();
            out.push('⁻');
        }
        // Only a run of digits converts; anything else keeps the caret so that
        // nothing is silently mangled.
        let mut any = false;
        while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
            out.push(DIGITS[d as usize]);
            chars.next();
            any = true;
        }
        if !any {
            out.push('^');
        }
    }
    out
}
