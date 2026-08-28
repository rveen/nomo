//! The SMath expression tree, and the stack reduction that builds it.
//!
//! `.sm` stores math as a postfix token stream, so there is no grammar to write
//! and no parser: push operands, and when an operator or function arrives, pop
//! its `args` operands and push the node. The `args` attribute is the only thing
//! that distinguishes unary from binary `-`, so arity is never inferred from the
//! glyph.
//!
//! Reduction is total. A malformed stream — too few operands, or more than one
//! left at the end — produces an [`Expr::Unsupported`] node rather than an error,
//! because the coverage report needs to count what a worksheet contains, and a
//! reader that stops at the first surprise cannot do that.

use crate::read::{Style, Token, TokenKind};

/// A node in an SMath expression.
///
/// Numbers keep their literal text rather than becoming `f64` here. What a
/// decimal literal means is a question for whatever emits Nomo source, and the
/// reader has no business answering it early: some of these worksheets carry
/// 65-digit integers that no `f64` can hold.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(String),
    Name(String),
    /// An operand carrying `style="unit"`. Units attach to a magnitude by
    /// multiplication, so this appears as the right operand of a `*`.
    Unit(String),
    /// An operand carrying `style="string"`.
    Text(String),
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Op {
        glyph: String,
        args: Vec<Expr>,
    },
    /// Something the reader could not make sense of, kept in place so that it is
    /// counted and reported rather than dropped (design note §8.7 item 23).
    ///
    /// `inside` holds whatever *had* reduced successfully when the trouble was
    /// hit. Keeping it is the difference between "this region is malformed" and
    /// "this region is malformed, and here are the four functions it calls" —
    /// and the second is what a coverage report is for.
    Unsupported {
        what: Unsupported,
        detail: String,
        inside: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unsupported {
    /// An operator glyph the importer has no meaning for.
    Operator,
    /// A token stream that does not reduce to exactly one expression.
    Malformed,
}

/// How a name was bound, which is the most important semantic distinction in the
/// whole format (design note §8.7 items 9–11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assign {
    /// `←` in the older versions, `:` in the newer ones. Resolves in reading
    /// order.
    Positional,
    /// `≡`, whose scope ignores position entirely: it is visible above its own
    /// definition. Must be collected in a pre-pass.
    Global,
}

/// A whole math region, classified by what sits at the root of its tree.
///
/// The distinction only exists at the root. `≡` nested inside an expression is
/// not a global definition but an equality *test* — 81 of the corpus's 304 uses
/// are nested, inside `if` conditions next to `&`, `¬` and `|` — so a reader that
/// treats the glyph as a definition wherever it appears mistranslates all of
/// them. That is why classification happens here, on the reduced tree, rather
/// than in the reduction.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Define {
        kind: Assign,
        target: Expr,
        value: Expr,
    },
    /// The older era's `=`: a display whose second operand is the answer SMath
    /// stored. Never an equation to solve.
    Show { expr: Expr, stored: Option<Expr> },
    /// A `≡` at a region root whose left side is an *expression* rather than a
    /// name or a call: an equation written out for the reader, which binds
    /// nothing. 86 of the 289 root-level uses across both corpora are this, and
    /// they are the reason `≡` cannot be collected as a definition on sight —
    /// doing so invents 86 variables whose names are whole expressions.
    ///
    /// Kept as a statement of its own rather than folded into `Bare` so that a
    /// coverage report can say how much of a worksheet is documentation and how
    /// much is calculation.
    Equation { left: Expr, right: Expr },
    /// An expression with no binding operator at its root. In the newer era this
    /// is the normal shape of a displayed result: `<input>` holds the expression
    /// and the answer lives in a sibling `<result>`.
    Bare(Expr),
}

/// Reduce a postfix token stream to a single expression.
pub fn reduce(tokens: &[Token]) -> Expr {
    let mut stack: Vec<Expr> = Vec::new();

    for token in tokens {
        match token.kind {
            // Postfix is already unambiguous; brackets exist only to round-trip
            // how the expression looked on screen.
            TokenKind::Bracket => continue,

            TokenKind::Operand => stack.push(match token.style {
                Some(Style::Unit) => Expr::Unit(token.text.clone()),
                Some(Style::Str) => Expr::Text(token.text.clone()),
                None => {
                    if is_number(&token.text) {
                        Expr::Number(token.text.clone())
                    } else {
                        Expr::Name(token.text.clone())
                    }
                }
            }),

            TokenKind::Function | TokenKind::Operator => {
                let arity = token.args.unwrap_or(0);
                let Some(args) = pop_n(&mut stack, arity) else {
                    // Everything already reduced is folded into the marker so
                    // that nothing is lost, and reduction continues.
                    let detail = format!(
                        "`{}` wants {arity} operand(s), {} on the stack",
                        token.text,
                        stack.len()
                    );
                    let inside = std::mem::take(&mut stack);
                    stack.push(Expr::Unsupported {
                        what: Unsupported::Malformed,
                        detail,
                        inside,
                    });
                    continue;
                };
                stack.push(match token.kind {
                    TokenKind::Function => Expr::Call {
                        name: token.text.clone(),
                        args,
                    },
                    _ => Expr::Op {
                        glyph: token.text.clone(),
                        args,
                    },
                });
            }
        }
    }

    match stack.len() {
        1 => stack.pop().expect("length checked"),
        // An empty `<input>` is legal and common: SMath saves a region the moment
        // it is created, before anything is typed into it.
        0 => Expr::Unsupported {
            what: Unsupported::Malformed,
            detail: "empty token stream".into(),
            inside: Vec::new(),
        },
        n => Expr::Unsupported {
            what: Unsupported::Malformed,
            detail: format!("{n} expressions left on the stack"),
            inside: stack,
        },
    }
}

/// Classify a reduced tree by its root operator.
pub fn classify(expr: Expr) -> Statement {
    let Expr::Op { glyph, args } = &expr else {
        return Statement::Bare(expr);
    };
    if args.len() != 2 {
        return Statement::Bare(expr);
    }
    let kind = match glyph.as_str() {
        "←" | ":" => Assign::Positional,
        "≡" => Assign::Global,
        "=" => {
            let Expr::Op { args, .. } = expr else {
                unreachable!("matched above")
            };
            let mut args = args.into_iter();
            let shown = args.next().expect("arity 2");
            let stored = args.next().expect("arity 2");
            return Statement::Show {
                expr: shown,
                stored: Some(stored),
            };
        }
        _ => return Statement::Bare(expr),
    };
    let Expr::Op { args, .. } = expr else {
        unreachable!("matched above")
    };
    let mut args = args.into_iter();
    let target = args.next().expect("arity 2");
    let value = args.next().expect("arity 2");

    // `≡` dispatches on the *shape* of its left side, not on its position. A
    // name is a global definition and a call is a function definition, but an
    // expression on the left is an equation the author wrote for a reader —
    // `z1 + p1/γ + V1²/(2g) ≡ z2 + p2/γ + V2²/(2g)` is Bernoulli, not a binding.
    // Positional `:` and `←` are not treated this way: their left side is a
    // target by construction, and the handful that are not reduce to a target
    // the emitter reports rather than to an equation.
    if kind == Assign::Global && !matches!(target, Expr::Name(_) | Expr::Call { .. }) {
        return Statement::Equation {
            left: target,
            right: value,
        };
    }

    Statement::Define {
        kind,
        target,
        value,
    }
}

fn pop_n(stack: &mut Vec<Expr>, n: usize) -> Option<Vec<Expr>> {
    if stack.len() < n {
        return None;
    }
    // Operands were pushed left to right, so the top of the stack is the
    // rightmost argument. Restore source order.
    let at = stack.len() - n;
    Some(stack.split_off(at))
}

/// Whether an operand's text is a numeric literal rather than a name.
///
/// Deliberately narrow: SMath writes decimals with a `.` and no exponent, sign or
/// separator, because a sign is a unary `-` operator token of its own. Anything
/// else is a name — including `#number`, which is how a function parameter
/// placeholder is spelled.
fn is_number(text: &str) -> bool {
    let mut chars = text.chars();
    let mut seen_digit = false;
    let mut seen_dot = false;
    for c in chars.by_ref() {
        match c {
            '0'..='9' => seen_digit = true,
            '.' if !seen_dot => seen_dot = true,
            _ => return false,
        }
    }
    seen_digit
}

impl Expr {
    /// Visit this node and every node beneath it, in source order.
    pub fn walk(&self, f: &mut impl FnMut(&Expr)) {
        f(self);
        match self {
            Expr::Call { args, .. } | Expr::Op { args, .. } => {
                for a in args {
                    a.walk(f);
                }
            }
            Expr::Unsupported { inside, .. } => {
                for a in inside {
                    a.walk(f);
                }
            }
            _ => {}
        }
    }
}

impl Statement {
    /// Visit the expressions this statement asks the engine to *compute*.
    ///
    /// A stored answer is deliberately not visited. It is SMath's rendering of a
    /// result, so counting the calls inside it would measure how SMath displays
    /// numbers rather than what an importer has to be able to evaluate — the
    /// difference is large enough to mislead, `el` alone appearing 817 times
    /// across the corpus but only 454 times on the input side.
    pub fn walk(&self, f: &mut impl FnMut(&Expr)) {
        match self {
            Statement::Define { target, value, .. } => {
                // `f(x) ← …` puts a call shape in target position, but it is a
                // signature, not a call: the parameters are being bound, not
                // passed. Walking it as a call makes every user-defined function
                // appear to call itself once, which is exactly the kind of quiet
                // double count that makes a coverage report untrustworthy.
                match target {
                    Expr::Call { args, .. } => {
                        for a in args {
                            a.walk(f);
                        }
                    }
                    other => other.walk(f),
                }
                value.walk(f);
            }
            Statement::Equation { left, right } => {
                left.walk(f);
                right.walk(f);
            }
            Statement::Show { expr, .. } => expr.walk(f),
            Statement::Bare(e) => e.walk(f),
        }
    }

    /// The answer SMath stored for this statement, if it stored one here. The
    /// newer era keeps it in a sibling element instead; see [`crate::read::Math`].
    pub fn stored(&self) -> Option<&Expr> {
        match self {
            Statement::Show { stored, .. } => stored.as_ref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::{Style, Token, TokenKind};

    fn operand(text: &str) -> Token {
        Token {
            kind: TokenKind::Operand,
            text: text.into(),
            args: None,
            style: None,
        }
    }
    fn styled(text: &str, style: Style) -> Token {
        Token {
            style: Some(style),
            ..operand(text)
        }
    }
    fn op(glyph: &str, args: usize) -> Token {
        Token {
            kind: TokenKind::Operator,
            text: glyph.into(),
            args: Some(args),
            style: None,
        }
    }
    fn call(name: &str, args: usize) -> Token {
        Token {
            kind: TokenKind::Function,
            text: name.into(),
            args: Some(args),
            style: None,
        }
    }

    #[test]
    fn a_unit_attaches_by_multiplication() {
        // Van := 230 V, exactly as design note §8.1 quotes it.
        let stream = [
            operand("Van"),
            operand("230"),
            styled("V", Style::Unit),
            op("*", 2),
            op(":", 2),
        ];
        let stmt = classify(reduce(&stream));
        assert_eq!(
            stmt,
            Statement::Define {
                kind: Assign::Positional,
                target: Expr::Name("Van".into()),
                value: Expr::Op {
                    glyph: "*".into(),
                    args: vec![Expr::Number("230".into()), Expr::Unit("V".into())],
                },
            }
        );
    }

    #[test]
    fn arity_and_not_the_glyph_decides_unary_from_binary() {
        let unary = reduce(&[operand("x"), op("-", 1)]);
        let binary = reduce(&[operand("x"), operand("y"), op("-", 2)]);
        assert_eq!(
            unary,
            Expr::Op {
                glyph: "-".into(),
                args: vec![Expr::Name("x".into())]
            }
        );
        match binary {
            Expr::Op { args, .. } => assert_eq!(args.len(), 2),
            other => panic!("expected an operator, got {other:?}"),
        }
    }

    #[test]
    fn operands_keep_their_source_order() {
        // Subtraction is not commutative, so a reversed pop would be silent and
        // wrong rather than obviously wrong.
        let e = reduce(&[operand("a"), operand("b"), op("-", 2)]);
        assert_eq!(
            e,
            Expr::Op {
                glyph: "-".into(),
                args: vec![Expr::Name("a".into()), Expr::Name("b".into())],
            }
        );
    }

    #[test]
    fn brackets_are_display_hints_and_leave_no_trace() {
        let with = reduce(&[
            operand("a"),
            operand("b"),
            op("+", 2),
            Token {
                kind: TokenKind::Bracket,
                text: "(".into(),
                args: None,
                style: None,
            },
        ]);
        let without = reduce(&[operand("a"), operand("b"), op("+", 2)]);
        assert_eq!(with, without);
    }

    #[test]
    fn the_older_eras_equals_carries_the_answer() {
        // Ling_rms_N = 345.78, the region design note §8.2 finding 2 recomputed.
        let stmt = classify(reduce(&[
            operand("Ling_rms_N"),
            operand("345.78"),
            op("=", 2),
        ]));
        assert_eq!(
            stmt,
            Statement::Show {
                expr: Expr::Name("Ling_rms_N".into()),
                stored: Some(Expr::Number("345.78".into())),
            }
        );
    }

    #[test]
    fn identity_at_the_root_is_a_global_definition() {
        let stmt = classify(reduce(&[operand("Cb"), operand("2"), op("≡", 2)]));
        assert!(matches!(
            stmt,
            Statement::Define {
                kind: Assign::Global,
                ..
            }
        ));
    }

    #[test]
    fn identity_inside_an_expression_is_not_a_definition() {
        // `if(ntcp ≡ "No notches", 1, 2)` — the shape that 81 of the corpus's
        // 304 uses of the glyph actually have. Classifying this as a global
        // binding would invent a variable and lose the condition.
        let stmt = classify(reduce(&[
            operand("ntcp"),
            styled("No notches", Style::Str),
            op("≡", 2),
            operand("1"),
            operand("2"),
            call("if", 3),
        ]));
        assert!(matches!(stmt, Statement::Bare(Expr::Call { .. })));
    }

    #[test]
    fn a_short_stack_becomes_a_marker_rather_than_a_panic() {
        let e = reduce(&[operand("a"), op("+", 2)]);
        assert!(matches!(
            e,
            Expr::Unsupported {
                what: Unsupported::Malformed,
                ..
            }
        ));
    }

    #[test]
    fn a_matrix_literal_takes_its_elements_and_its_shape() {
        // mat(a, b, rows, cols) — arity is elements + 2.
        let e = reduce(&[
            operand("1"),
            operand("2"),
            operand("2"),
            operand("1"),
            call("mat", 4),
        ]);
        match e {
            Expr::Call { name, args } => {
                assert_eq!(name, "mat");
                assert_eq!(args.len(), 4);
                assert_eq!(args[2], Expr::Number("2".into()));
            }
            other => panic!("expected a call, got {other:?}"),
        }
    }

    #[test]
    fn a_placeholder_parameter_is_a_name_not_a_number() {
        assert_eq!(reduce(&[operand("#number")]), Expr::Name("#number".into()));
        assert_eq!(reduce(&[operand("2.89")]), Expr::Number("2.89".into()));
    }
}
