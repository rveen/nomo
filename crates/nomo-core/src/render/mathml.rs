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
//! A name that spells a Greek letter is set as that letter — `sigma_allow` is
//! σ_allow — which is the rest of it. See [`greek`] for which names map and
//! why, and [`identifier`] for why that single change is also what gets the
//! italic and upright right.
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
use crate::render::{constant_symbol, escape, number, Renderer};
use crate::trace::{Trace, TraceNode};
use crate::value::EvalError;
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
    if binding(r, trace, sub) < least {
        format!("<mo>(</mo>{inner}<mo>)</mo>")
    } else {
        inner
    }
}

/// How tightly a node binds *as it will be drawn*.
///
/// [`precedence`] reads the expression; this reads the rendering, and they
/// differ in one place. A substituted name is an atom when it holds a bare
/// number, and a *product* when it holds a number and a unit — `50 mm` is two
/// things juxtaposed, so under a power it needs the brackets that say `(50 mm)²`
/// and not `50 mm²`, which is a different quantity.
///
/// The linear renderer has always known this: its `Piece` carries a precedence
/// that becomes `PRODUCT` for a value with a unit. Typeset output only needs it
/// now because a conversion no longer falls back to that renderer's text, which
/// arrived with the brackets already in it.
///
/// A complex value is the case this still gets wrong, exactly as before: it
/// stays `<mtext>`, so `(3 + 4i)²` draws without its brackets. Fixing that means
/// the substituted column knowing what a complex number is, which is the same
/// step as giving it a real `<mn>` and unit.
fn binding(r: &Renderer, trace: &Trace, sub: bool) -> u8 {
    if sub {
        if let TraceNode::Variable { .. } = &trace.node {
            // A power of ten is drawn as a product where the linear text is one
            // token, so it is the one case the two renderers see differently.
            if let Some((magnitude, _)) = r.substituted_parts(trace) {
                if scientific(&magnitude) {
                    return 4;
                }
            }
            return from_linear(r.substituted_binding(trace));
        }
    }
    // A number is an atom until it is written as a power of ten, and then it is
    // a product: `(2.5 × 10⁹)²` is not `2.5 × 10⁹²`.
    if let TraceNode::Number = &trace.node {
        if let Ok(Value::Scalar(q)) = &trace.value {
            if scientific(&number::format(q.value, &r.numbers)) {
                return 4;
            }
        }
    }
    precedence(&trace.node)
}

/// The linear renderer's precedence, in this one's levels.
///
/// Two scales for the same idea, because the two renderers bracket for
/// different reasons: a fraction bar and a radical group what they contain, so
/// division and exponentiation need no level here. This maps between them
/// rather than duplicating the judgement, which is what stops a complex value
/// losing the brackets in `(3 + 4i)²` — the linear renderer calls it a sum, and
/// now so does this one.
fn from_linear(prec: u8) -> u8 {
    match prec {
        super::prec::CONDITIONAL => 0,
        super::prec::LOGICAL => 1,
        super::prec::SUM => 3,
        super::prec::PRODUCT | super::prec::UNARY => 4,
        // A power and an atom are both drawn as one thing here.
        _ => 9,
    }
}

/// Whether this renderer's own number formatter wrote a power of ten.
fn scientific(formatted: &str) -> bool {
    matches!(formatted.split_once('e'), Some((m, e)) if !m.is_empty() && !e.is_empty())
}

fn node(r: &Renderer, trace: &Trace, linear: &str, sub: bool) -> String {
    // In the substituted column a name is replaced by what it holds, and what it
    // holds is a quantity rather than a formula — a number and a unit, which is
    // what `mtext` is for. Rendered by the same code as the text column, so the
    // two cannot disagree about a value.
    if sub {
        if let TraceNode::Variable { .. } = &trace.node {
            return match r.substituted_parts(trace) {
                Some((magnitude, symbol)) => quantity(&magnitude, &symbol),
                // A vector, a matrix, a string or a complex number is not a
                // magnitude and a unit, so it stays running text.
                None => format!("<mtext>{}</mtext>", escape(&r.substituted(trace))),
            };
        }
    }
    match &trace.node {
        // The node carries no value — the number is in the trace's result, the
        // way the linear renderer reads it — and it is formatted by the same
        // code, so a number cannot read one way typeset and another in text.
        TraceNode::Number => match &trace.value {
            Ok(Value::Scalar(q)) => decimal(&number::format(q.value, &r.numbers)),
            _ => String::from("<mo>?</mo>"),
        },
        // A built-in constant is a symbol, not a quantity, so it is upright:
        // ISO 80000-2 sets π, e and ∞ in roman, and that is also what tells the
        // constant apart from a variable someone named the same. The symbol
        // comes from the renderer's own table rather than a second one here, so
        // that the typeset column and the text column cannot disagree about it
        // — until now they did, and `pi` typeset as the *word* "pi" beside a
        // text column showing π.
        TraceNode::Constant(name) => format!(
            "<mi mathvariant=\"normal\">{}</mi>",
            escape(&constant_symbol(name))
        ),
        TraceNode::Variable { name, .. } | TraceNode::FnRef(name) => identifier(name),
        TraceNode::UnitRef(name) => format!("<mi mathvariant=\"normal\">{}</mi>", escape(name)),
        TraceNode::Text => format!("<mtext>{}</mtext>", escape(linear)),

        // `20 °C` — a magnitude and a unit, always, so it always takes the
        // space `unit_space` decides for the general case. Not the plane-angle
        // exception: an affine literal is a temperature scale, never an angle.
        TraceNode::AffineLiteral { magnitude, unit } => format!(
            "<mn>{}</mn><mo lspace=\"0\" rspace=\"0.167em\">&#8290;</mo>\
             <mi mathvariant=\"normal\">{}</mi>",
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
                "<msub><mrow>{}</mrow><mrow>{}</mrow></msub>",
                bracketed(r, base, linear, 9, sub),
                subscript.join("<mo lspace=\"0\" rspace=\"0.167em\">,</mo>")
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

        // A conversion is transparent, exactly as it is to the linear renderer
        // (`render/mod.rs`, which walks straight through it). `-> mm^2` says
        // what unit to *show the answer in*; it belongs to the result column
        // and is echoed there. Falling back for it dropped the whole expression
        // to running text — and since a worksheet writes `A_s = pi/4*d^2 ->
        // mm^2` far more often than it writes anything without a conversion,
        // that was most of what the typeset output ever fell back on: 135 of
        // the 342 whole-expression fallbacks across `examples/`.
        TraceNode::Convert { value, .. } => node(r, value, linear, sub),

        TraceNode::Conditional { .. } => cases(r, trace, linear, sub),

        // No typeset form here, and a hole would be worse than a sentence: the
        // linear rendering is what this worksheet has always shown.
        TraceNode::Malformed => format!("<mtext>{}</mtext>", escape(linear)),
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
        // Both children wrapped, because `<msup>` takes exactly two and a base
        // is very often more than one element: `(a+b)²` is five, and a
        // substituted `(50 mm)²` is seven. Without the wrapper the browser is
        // handed `<msup>` with six children and lays them out flat — `(a+b)²`
        // came out as `(a + b)²` with the bracket *inside* the superscript, and
        // `(50 mm)²` as `50 mm2`. Nothing errors; it simply reads wrong.
        BinaryOp::Pow => format!(
            "<msup><mrow>{}</mrow><mrow>{}</mrow></msup>",
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
                // Which takes a space — see `unit_space` below, because U+2062
                // is exactly zero wide and `5cm` is not what was written.
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
            } else if matches!(op, BinaryOp::ImplicitMul) {
                match unit_space(rhs) {
                    Some(width) => {
                        format!("<mo lspace=\"0\" rspace=\"{width}\">{symbol}</mo>")
                    }
                    None => format!("<mo>{symbol}</mo>"),
                }
            } else {
                let space = spacing(op);
                format!("<mo lspace=\"{space}\" rspace=\"{space}\">{symbol}</mo>")
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

/// The space an operator is set with, stated rather than left to the browser.
///
/// MathML's operator dictionary already gives these, and asking for them
/// explicitly changes nothing where the operands are ordinary markup: measured
/// in Chrome, a relation between `<mn>` operands is set with 5/18 em either
/// side, a sum with 4/18 and a product with 3/18, which are exactly the values
/// below and exactly the ones TeX uses.
///
/// It changes everything where an operand is `<mtext>`. That element is
/// *space-like* in MathML, and an operator whose siblings are all space-like
/// gets no spacing from the dictionary at all — which is why a comparison in
/// the substituted column read `160≥105.5`. Setting the values here means the
/// spacing no longer depends on what the operands happen to be made of, and a
/// string or a vector, which must stay running text, is set correctly too.
fn spacing(op: BinaryOp) -> &'static str {
    match op {
        // Relations, at TeX's thickmathspace.
        BinaryOp::Lt
        | BinaryOp::Gt
        | BinaryOp::Le
        | BinaryOp::Ge
        | BinaryOp::Equal
        | BinaryOp::NotEqual => "0.278em",
        // Sums, at mediummathspace.
        BinaryOp::Add | BinaryOp::Sub => "0.222em",
        // Products, at thinmathspace.
        BinaryOp::Mul => "0.167em",
        // Handled before this is reached: juxtaposition asks `unit_space`, and
        // a division and a power are drawn rather than spaced.
        BinaryOp::ImplicitMul | BinaryOp::Div | BinaryOp::Pow => "0",
        // Words, which have their own wider spacing at the call site.
        BinaryOp::And | BinaryOp::Or => "0.3em",
    }
}

/// The space between a number and the unit it is juxtaposed with, if any.
///
/// ISO 80000-1 §7.1.3: *"the numerical value always precedes the unit, and a
/// space is always used to separate the unit from the number"*. The renderer
/// was not doing it. `ImplicitMul` emits U+2062 INVISIBLE TIMES, which says
/// "multiply" and is exactly zero wide, so `d = 50 mm` typeset as `50mm` — and
/// only the *typeset* column did, because the substituted column goes through
/// `<mtext>` and carries the space the linear renderer already put there. A
/// worksheet therefore disagreed with itself across one line.
///
/// A thin space rather than a word space, at TeX's `\,` of 3/18 em, because
/// that is what a unit is set with everywhere it is set well.
///
/// The same juxtaposition is also ordinary algebra, and `2x` is correctly
/// tight, so this returns `None` for anything that is not a unit. What
/// distinguishes them is the *right* operand: a `UnitRef` is a unit and a
/// variable is not.
///
/// # The exception, which is in the standard
///
/// The same clause exempts the plane-angle symbols: `90°` takes no space, while
/// `20 °C` does. So `°` is matched by name. `deg` is not exempt — the exception
/// is about the symbol, and a unit spelled in letters reads as one.
fn unit_space(rhs: &Trace) -> Option<&'static str> {
    /// The unit a juxtaposition attaches, looking through what can wrap one.
    ///
    /// `2 m^2` puts the unit under a power and `2 (m/s)` puts it in a bracket;
    /// `5 N*m` and `2.5 kN/m` do not, because juxtaposition binds tighter than
    /// `*` and `/`, so the unit is already the right operand. Anything that
    /// starts with a number or a name — `2 (x+1)`, `2 x` — is algebra and gets
    /// nothing.
    fn unit_of(trace: &Trace) -> Option<&str> {
        match &trace.node {
            TraceNode::UnitRef(name) => Some(name),
            TraceNode::Paren(inner) => unit_of(inner),
            TraceNode::Binary {
                op: BinaryOp::Pow | BinaryOp::Mul | BinaryOp::ImplicitMul | BinaryOp::Div,
                lhs,
                ..
            } => unit_of(lhs),
            _ => None,
        }
    }
    match unit_of(rhs)? {
        "°" => None,
        _ => Some("0.167em"),
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

/// A conditional, drawn the way mathematics draws one: a brace over cases.
///
/// `if c then a else b` is a *choice between values*, and running it out as a
/// sentence — which is what the fallback did — is the one place typeset output
/// still read worse than the linear text it replaced. A brace and two rows says
/// the same thing in the shape a reader already knows.
///
/// # `else if` flattens
///
/// `if a then 1 else if b then 2 else 3` is three cases, not a case containing a
/// case. That is how the language chains them — `docs/language.md`'s `else` arm
/// "reaches as far as it can", which is exactly what makes the chain — and it is
/// how a table of cases is written. So the `otherwise` arm is unwrapped for as
/// long as it is another conditional.
///
/// # An arm that did not run is shown as written
///
/// The same rule the linear renderer follows, and for the same reason: the
/// substituted column should say which way the worksheet went, not pretend both
/// arms were computed. An arm carrying [`EvalError::NotTaken`] has no values in
/// it, so it is rendered symbolically even in the substituted column.
fn cases(r: &Renderer, trace: &Trace, linear: &str, sub: bool) -> String {
    // Value, then the condition that selects it — the column order of every
    // cases block ever set, because the reader is looking for the value.
    let mut rows = String::new();
    let mut current = trace;
    loop {
        let TraceNode::Conditional {
            cond,
            then,
            otherwise,
        } = &current.node
        else {
            // The tail of the chain: the value with no condition left to test.
            rows.push_str(&format!(
                "<mtr><mtd>{}</mtd><mtd><mtext>otherwise</mtext></mtd></mtr>",
                arm(r, current, linear, sub)
            ));
            return braced(&rows);
        };
        rows.push_str(&format!(
            "<mtr><mtd>{}</mtd><mtd><mtext>if&#160;</mtext>{}</mtd></mtr>",
            arm(r, then, linear, sub),
            arm(r, cond, linear, sub),
        ));
        current = otherwise;
    }
}

/// One arm of a conditional, substituted only if it was the arm that ran.
fn arm(r: &Renderer, trace: &Trace, linear: &str, sub: bool) -> String {
    let taken = !matches!(trace.value, Err(EvalError::NotTaken));
    node(r, trace, linear, sub && taken)
}

/// A table of cases under a brace that grows to fit it.
///
/// The `{` is stretchy in MathML's operator dictionary and takes its height from
/// the rest of the row, which is what the `<mrow>` around both is for. The font
/// carries the pieces it is assembled from: they are reached through the MATH
/// table's variant records rather than by character code, which is why
/// `web/font.mjs` does not enumerate them and does not have to.
///
/// Aligned by a class and CSS rather than by `columnalign`, which MathML Core
/// removed along with most of `mtable`'s attributes. Writing the attribute
/// anyway would have looked like alignment and done nothing — it was, until a
/// screenshot showed the columns still centred.
fn braced(rows: &str) -> String {
    format!("<mrow><mo>{{</mo><mtable class=\"cases\">{rows}</mtable></mrow>")
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
        // A function's name goes through the same table: `fn phi(x)` is φ(x).
        // `sin`, `max` and the rest are more than one character and so stay
        // upright, which is what ISO 80000-2 asks of a function name anyway.
        _ => format!(
            "<mi>{}</mi><mo>&#8289;</mo><mo>(</mo>{}<mo>)</mo>",
            symbol(name),
            // The comma's own dictionary spacing: nothing before it, a thin
            // space after. Stated for the same reason as the rest — an argument
            // that is running text would otherwise lose it.
            inner.join("<mo lspace=\"0\" rspace=\"0.167em\">,</mo>")
        ),
    }
}

/// A vector or matrix in square brackets, which is how this language writes one.
fn bracket_table(rows: &str) -> String {
    format!("<mo>[</mo><mtable>{rows}</mtable><mo>]</mo>")
}

/// A whole result as a `<math>` element, when it is a number and a unit.
///
/// The third column was the last plain text on a typeset line, which showed:
/// a result outside the range shown in full read `8.427e-5` beside a
/// substituted value on the same line reading 8.427 × 10⁻⁵.
pub(super) fn result(r: &Renderer, trace: &Trace) -> Option<String> {
    let (magnitude, symbol) = r.result_parts(trace)?;
    Some(format!(
        "<math display=\"inline\"><mrow>{}</mrow></math>",
        quantity(&magnitude, &symbol)
    ))
}

/// A substituted value, set as the number and unit it is rather than as text.
///
/// The substituted column used to be `<mtext>` of the string the linear
/// renderer produced, and that cost three things at once. `<mtext>` is a
/// *space-like* element in MathML, so an operator whose siblings are all
/// space-like loses the spacing the operator dictionary gives it — measured in
/// Chrome, a relation between `<mn>` operands is set with 5/18 em either side
/// and between `<mtext>` operands with none, which is why a comparison read
/// `160≥105.5`. The `²` in `8.427e-5 m²` was a literal superscript character
/// where the symbolic column beside it drew a real `<msup>`. And `e-5` is not
/// how a typeset document writes a power of ten.
///
/// `symbol` is empty for a dimensionless quantity.
fn quantity(magnitude: &str, symbol: &str) -> String {
    if symbol.is_empty() {
        return decimal(magnitude);
    }
    // The same ISO 80000-1 rule and the same exception as `unit_space`: a unit
    // stands off its number, except the plane-angle symbols. `join` in the
    // linear renderer applies it to `°` and `%` alike; here `%` is spaced,
    // because the standard's exception names the angle symbols and not percent.
    let space = if symbol.starts_with('°') {
        String::from("<mo>&#8290;</mo>")
    } else {
        String::from("<mo lspace=\"0\" rspace=\"0.167em\">&#8290;</mo>")
    };
    format!("{}{space}{}", decimal(magnitude), unit(symbol))
}

/// A unit symbol as markup.
///
/// A single name with an exponent — `mm^2`, `m^3` — becomes a real superscript.
/// Anything else is set upright as it stands, because `docs/language.md` makes
/// that a rule rather than an oversight: a conversion target is echoed *as it
/// was written*, so `kip*ft` keeps its `*` and `MN/m` its slash. A worksheet
/// checked against a specification that spells a unit a particular way should
/// show that spelling.
fn unit(symbol: &str) -> String {
    if let Some((name, exponent)) = symbol.split_once('^') {
        let simple = !name.is_empty()
            && !name.contains(['/', '*', '^'])
            && !exponent.is_empty()
            && exponent
                .strip_prefix('-')
                .unwrap_or(exponent)
                .chars()
                .all(|c| c.is_ascii_digit());
        if simple {
            return format!(
                "<msup><mi mathvariant=\"normal\">{}</mi><mn>{}</mn></msup>",
                escape(name),
                escape(&exponent.replace('-', "\u{2212}"))
            );
        }
    }
    format!(
        "<mi mathvariant=\"normal\">{}</mi>",
        escape(&super::superscript_exponents(symbol))
    )
}

/// A number, with a power of ten set as one.
///
/// `number::format` writes `8.427e-5` once a value leaves the range it shows in
/// full. That is the right thing in a text column and the wrong thing in a
/// typeset one, where the notation is 8.427 × 10⁻⁵. The mantissa and exponent
/// are split on the `e` this renderer's own formatter wrote, so the shape is
/// known rather than guessed; anything else — `NaN`, `∞` — passes through.
fn decimal(formatted: &str) -> String {
    // The same split `scientific` tests, so the two cannot drift apart: a
    // number drawn as a product must also *bind* as one.
    match formatted.split_once('e') {
        Some((mantissa, exponent)) if !mantissa.is_empty() && !exponent.is_empty() => format!(
            "<mn>{}</mn><mo lspace=\"0.167em\" rspace=\"0.167em\">&#215;</mo>\
             <msup><mn>10</mn><mn>{}</mn></msup>",
            escape(&mantissa.replace('-', "\u{2212}")),
            escape(&exponent.replace('-', "\u{2212}"))
        ),
        _ => format!("<mn>{}</mn>", escape(formatted)),
    }
}

/// The letter a spelled-out Greek name is conventionally written with.
///
/// A worksheet is typed on an ordinary keyboard, so `sigma_allow` is what an
/// engineer writes for σ_allow and `lambda` is what they write for the
/// slenderness ratio in `examples/column.nomo`. Setting those as the *words*
/// "sigma" and "lambda" is the difference between output that has been typeset
/// and output that looks like it.
///
/// This is a display convention and nothing more. The language already accepts
/// `σ` directly — `lex::is_ident_start` takes any alphabetic character — and
/// nothing here changes that: the two spellings remain two distinct names to
/// the graph, and a worksheet that binds both will draw them the same way.
///
/// # Which name maps, and to what
///
/// **The character a name maps to is the one Unicode gives that name**, so the
/// spelled-out form and the typed form agree. `phi` is U+03C6 GREEK SMALL
/// LETTER PHI, which is what a worksheet gets by typing `φ`; TeX's `\phi`
/// is the *symbol* form ϕ instead, and following TeX here would mean `phi` and
/// `φ` drawing as different letters, which is precisely the confusion this
/// table exists to remove. The `var` names take the symbol forms, as they do
/// in TeX.
///
/// **A name maps only where the Greek letter is distinct from the Latin letter
/// it would otherwise be set as.** So there is no `omicron` (ο is o) and no
/// `Alpha`, `Beta` or `Eta` (Α, Β, Η are A, B, H). Mapping those would change
/// the codepoint without changing the glyph, and would take the name away from
/// a worksheet using it as an ordinary variable — a cost with no visible
/// return. That one rule reproduces TeX's uppercase set exactly, and explains
/// the gap TeX leaves at omicron.
///
/// Units are not passed through here, and must not be: `psi` is pounds per
/// square inch in `unit.rs`, not ψ. A `UnitRef` has its own branch and its own
/// upright rendering.
fn greek(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        // The symbol forms, under the names TeX gives them.
        "varepsilon" => "ϵ",
        "vartheta" => "ϑ",
        "varkappa" => "ϰ",
        "varpi" => "ϖ",
        "varrho" => "ϱ",
        "varsigma" => "ς",
        "varphi" => "ϕ",
        // Uppercase, stopping where the glyph stops being Latin.
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        _ => return None,
    })
}

/// A name as it should be set: its Greek letter if it spells one, else itself.
fn symbol(name: &str) -> String {
    escape(greek(name).unwrap_or(name))
}

/// A name, with the underscore in it drawn as the subscript it stands for.
///
/// Nothing here asks for italic or upright, and that is the whole of the
/// discipline rather than an omission. MathML Core italicises a one-character
/// `<mi>` and leaves a longer one upright, which is ISO 80000-2's rule already:
/// a quantity symbol is a single italic letter, and a subscript that is a word
/// describing it — the `allow` in `σ_allow`, the `required` in `d_required` —
/// is upright because it is not a quantity.
///
/// So [`greek`] is also what makes the *stem* italic. As the word `sigma` it
/// was five characters and five characters are upright; as σ it is one.
pub(super) fn identifier(name: &str) -> String {
    // A subscripted name — `V_drop`, `sigma_allow` — is drawn as one, since that
    // is what the underscore was standing in for.
    match name.split_once('_') {
        Some((stem, sub)) if !stem.is_empty() && !sub.is_empty() => format!(
            "<msub><mi>{}</mi><mi>{}</mi></msub>",
            symbol(stem),
            symbol(sub)
        ),
        _ => format!("<mi>{}</mi>", symbol(name)),
    }
}
