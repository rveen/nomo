//! Pratt parser.
//!
//! Hand-written and small, which is the payoff of choosing a text syntax:
//! EngineeringPaper.xyz needs roughly 7,000 generated lines plus a 2,600-line
//! visitor because it consumes LaTeX emitted by a visual editor.
//!
//! # Precedence
//!
//! Loosest to tightest:
//!
//! | Level | Operators | Associativity |
//! |-------|-----------|---------------|
//! | 1 | `->` conversion | left |
//! | 2 | `+` `-` | left |
//! | 3 | `*` `/` and juxtaposition | left |
//! | 4 | unary `-` `+` | prefix |
//! | 5 | `^` | right |
//! | 6 | call `f(x)`, index `x[i]` | postfix |
//!
//! Juxtaposition sits at the same level as explicit `*` and `/`, left
//! associative. That is what makes unit expressions work without the grammar
//! knowing anything about units: `9.81 m/s^2` parses as `((9.81 * m) / (s^2))`,
//! and `m` and `s` are ordinary identifiers that happen to resolve to units. It
//! also makes `1/2 m` mean `(1/2) * m`, which is what a reader expects.
//!
//! # Two deliberate ambiguity rules
//!
//! `f(...)` is always a call, never juxtaposition — write `x*(a+b)` to multiply.
//! `x[...]` is always an index, never juxtaposition — write `x*[1,2]` to multiply
//! by a vector literal. Both avoid making whitespace significant, which would be
//! worse.

use crate::ast::{Ast, BinaryOp, Expr, Name, Stmt, UnaryOp};
use crate::diag::{codes, Diagnostic};
use crate::lex::{lex, Token, TokenKind};
use crate::span::Span;

pub struct Parsed {
    pub ast: Ast,
    pub diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }
}

/// The deepest an expression may nest.
///
/// A fixed number, like [`crate::eval::MAX_DEPTH`] and for the same reason: the
/// answer must not depend on the machine, and neither must the refusal. Every
/// descent into a sub-expression — a bracket, a call argument, an index, a
/// vector element, an operand, an `if` arm — is one level.
///
/// **Chosen from the tightest target, which is WebAssembly.** Measured
/// 2026-08-29 on `x = ((((…1…))))`: the native build aborts on its guard page
/// somewhere between 5 000 and 7 000 levels with an 8 MB stack, and the
/// WebAssembly build traps between 750 and 800 with its 1 MB one. The trap is
/// the worse failure by far — it leaves the instance's allocator in an
/// undefined state, so in the browser every *later* edit fails too and the
/// editor stops recalculating for the life of the tab.
///
/// 128 sits an order of magnitude below that cliff and an order of magnitude
/// above anything real: the deepest expression under `examples/` is 13 levels
/// (`plots.nomo`, `llc.nomo`) and the deepest across all 114 worksheets the
/// SMath importer emits is 14. A worksheet reaching this limit was generated,
/// not written.
pub const MAX_NEST: usize = 128;

pub fn parse(source: &str) -> Parsed {
    let lexed = lex(source);
    let mut p = Parser {
        source,
        tokens: lexed.tokens,
        pos: 0,
        diags: lexed.diagnostics,
        depth: 0,
        too_deep: false,
    };
    let ast = p.parse_worksheet();
    let mut diagnostics = p.diags;
    // Lexing runs to completion before parsing begins, so without this the two
    // sets interleave by phase rather than by position. Readers expect source
    // order. `sort_by_key` is stable, so diagnostics on the same span keep the
    // order they were reported in.
    diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
    Parsed { ast, diagnostics }
}

struct Parser<'s> {
    source: &'s str,
    tokens: Vec<Token>,
    pos: usize,
    diags: Vec<Diagnostic>,
    /// How many sub-expressions deep the parser currently is. Bounded by
    /// [`MAX_NEST`], which is what keeps the recursion off the stack's edge.
    depth: usize,
    /// Set when this statement hit [`MAX_NEST`], and cleared at the next one.
    ///
    /// It silences the diagnostics that follow. Unwinding out of 128 frames of
    /// bracket passes 128 unclosed delimiters on the way, and reporting all of
    /// them would bury the one message that says what actually happened.
    too_deep: bool,
}

/// Binding powers for infix operators, as `(left, right)`. A right-associative
/// operator has `right < left`.
fn infix_bp(kind: &TokenKind) -> Option<(u8, u8)> {
    Some(match kind {
        TokenKind::Arrow => (1, 2),
        TokenKind::KwOr => (3, 4),
        TokenKind::KwAnd => (5, 6),
        TokenKind::Lt
        | TokenKind::Gt
        | TokenKind::Le
        | TokenKind::Ge
        | TokenKind::EqEq
        | TokenKind::Ne => (7, 8),
        TokenKind::Plus | TokenKind::Minus => (9, 10),
        TokenKind::Star | TokenKind::Slash => (11, 12),
        TokenKind::Caret => (14, 13),
        _ => return None,
    })
}

/// Binding power of juxtaposition, matching explicit `*`.
const IMPLICIT_MUL_BP: (u8, u8) = (11, 12);

/// Right binding power of a prefix operator. Looser than `^` so that `-x^2`
/// means `-(x^2)`, tighter than `*` so that `-x * y` means `(-x) * y`.
const PREFIX_BP: u8 = 12;

/// Right binding power of `not`. Tighter than `and` so that `not a and b` is
/// `(not a) and b`, looser than a comparison so that `not x == y` is
/// `not (x == y)` — which is what it reads as.
const NOT_BP: u8 = 7;

impl<'s> Parser<'s> {
    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_token(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn at(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, kind: &TokenKind) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn error(&mut self, code: &'static str, span: Span, msg: impl Into<String>) {
        // Everything after a nesting refusal is a consequence of it. See
        // `too_deep`.
        if self.too_deep {
            return;
        }
        self.diags.push(Diagnostic::error(code, span, msg));
    }

    /// Skip forward to the next statement boundary, so one bad line does not
    /// cascade into every line after it.
    fn recover_to_line_end(&mut self) {
        while !matches!(self.peek(), TokenKind::Newline | TokenKind::Eof) {
            self.bump();
        }
    }

    fn parse_worksheet(&mut self) -> Ast {
        let mut stmts = Vec::new();
        loop {
            while self.eat(&TokenKind::Newline).is_some() {}
            if self.at(&TokenKind::Eof) {
                break;
            }
            let before = self.pos;
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
            }
            // Guarantee forward progress even if a branch failed to consume.
            if self.pos == before {
                self.bump();
            }
        }
        Ast { stmts }
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        // One statement's nesting refusal must not silence the next statement's
        // diagnostics. `depth` is already back to zero here — it unwinds — but
        // the suppression flag has to be cleared deliberately.
        self.too_deep = false;
        match self.peek() {
            TokenKind::Comment => {
                let t = self.bump();
                // Strip the leading `'` and one following space, which is the
                // conventional way people write these.
                let raw = t.text(self.source);
                let text = raw
                    .strip_prefix('\'')
                    .unwrap_or(raw)
                    .strip_prefix(' ')
                    .unwrap_or_else(|| raw.strip_prefix('\'').unwrap_or(raw))
                    .to_string();
                Some(Stmt::Comment { text, span: t.span })
            }
            TokenKind::KwUnit => self.parse_unit_decl(),
            TokenKind::KwFn => self.parse_fn_def(),
            TokenKind::KwGlobal => self.parse_global_def(),
            _ => self.parse_assign_or_query(),
        }
    }

    fn parse_name(&mut self, context: &str) -> Option<Name> {
        if self.at(&TokenKind::Ident) {
            let t = self.bump();
            Some(Name {
                text: t.text(self.source).to_string(),
                span: t.span,
            })
        } else {
            let span = self.peek_token().span;
            self.error(
                codes::EXPECTED_TOKEN,
                span,
                format!("expected a name after `{context}`"),
            );
            None
        }
    }

    fn expect_eq(&mut self, context: &str) -> bool {
        if self.eat(&TokenKind::Eq).is_some() {
            true
        } else {
            let span = self.peek_token().span;
            self.error(
                codes::EXPECTED_TOKEN,
                span,
                format!("expected `=` in {context}"),
            );
            false
        }
    }

    /// Consume `kind`, or report what was wanted and where it was wanted.
    fn expect(&mut self, kind: &TokenKind, what: &str, after: &str) -> bool {
        if self.eat(kind).is_some() {
            return true;
        }
        let span = self.peek_token().span;
        self.error(
            codes::EXPECTED_TOKEN,
            span,
            format!("expected {what} after {after}"),
        );
        false
    }

    fn parse_unit_decl(&mut self) -> Option<Stmt> {
        let kw = self.bump();
        let Some(name) = self.parse_name("unit") else {
            self.recover_to_line_end();
            return Some(Stmt::Error { span: kw.span });
        };
        if !self.expect_eq("a unit declaration") {
            self.recover_to_line_end();
            return Some(Stmt::Error {
                span: kw.span.to(name.span),
            });
        }
        let value = self.parse_expr(0);
        let span = kw.span.to(value.span());
        self.finish_line();
        Some(Stmt::UnitDecl { name, value, span })
    }

    fn parse_global_def(&mut self) -> Option<Stmt> {
        let kw = self.bump();
        let Some(name) = self.parse_name("global") else {
            self.recover_to_line_end();
            return Some(Stmt::Error { span: kw.span });
        };
        if !self.expect_eq("a global definition") {
            self.recover_to_line_end();
            return Some(Stmt::Error {
                span: kw.span.to(name.span),
            });
        }
        let value = self.parse_expr(0);
        let span = kw.span.to(value.span());
        self.finish_line();
        Some(Stmt::GlobalDef { name, value, span })
    }

    fn parse_fn_def(&mut self) -> Option<Stmt> {
        let kw = self.bump();
        let Some(name) = self.parse_name("fn") else {
            self.recover_to_line_end();
            return Some(Stmt::Error { span: kw.span });
        };

        let mut params = Vec::new();
        if self.eat(&TokenKind::LParen).is_none() {
            let span = self.peek_token().span;
            self.error(
                codes::EXPECTED_TOKEN,
                span,
                "expected `(` to begin the parameter list",
            );
            self.recover_to_line_end();
            return Some(Stmt::Error {
                span: kw.span.to(name.span),
            });
        }
        if !self.at(&TokenKind::RParen) {
            while let Some(p) = self.parse_name("parameter list") {
                params.push(p);
                if self.eat(&TokenKind::Comma).is_none() {
                    break;
                }
            }
        }
        if self.eat(&TokenKind::RParen).is_none() {
            let span = self.peek_token().span;
            self.error(
                codes::UNCLOSED_DELIMITER,
                span,
                "expected `)` to close the parameter list",
            );
        }

        if !self.expect_eq("a function definition") {
            self.recover_to_line_end();
            return Some(Stmt::Error {
                span: kw.span.to(name.span),
            });
        }
        let body = self.parse_expr(0);
        let span = kw.span.to(body.span());
        self.finish_line();
        Some(Stmt::FnDef {
            name,
            params,
            body,
            span,
        })
    }

    fn parse_assign_or_query(&mut self) -> Option<Stmt> {
        let first = self.parse_expr(0);

        if self.at(&TokenKind::Eq) {
            let eq = self.bump();
            let value = self.parse_expr(0);
            let span = first.span().to(value.span());
            return match first {
                Expr::Ident(name) => {
                    self.finish_line();
                    Some(Stmt::Assign { name, value, span })
                }
                other => {
                    self.error(
                        codes::INVALID_ASSIGN_TARGET,
                        other.span().to(eq.span),
                        "only a name can be assigned to; \
                         write `name = expression`",
                    );
                    self.recover_to_line_end();
                    Some(Stmt::Error { span })
                }
            };
        }

        let span = first.span();
        self.finish_line();
        Some(Stmt::Query { expr: first, span })
    }

    /// Consume the statement terminator, complaining about anything left over.
    fn finish_line(&mut self) {
        if matches!(self.peek(), TokenKind::Newline | TokenKind::Eof) {
            return;
        }
        let span = self.peek_token().span;
        self.error(
            codes::TRAILING_INPUT,
            span,
            format!(
                "unexpected `{}` after the end of this line",
                span.text(self.source)
            ),
        );
        self.recover_to_line_end();
    }

    /// Every descent into a sub-expression passes through here — `parse_prefix`,
    /// `parse_primary`'s bracket, `parse_call`, `parse_index`, `parse_bracketed`
    /// and `parse_conditional` all recurse by calling it — so counting here
    /// counts the whole recursion, and there is one place to get right rather
    /// than six.
    fn parse_expr(&mut self, min_bp: u8) -> Expr {
        if self.depth >= MAX_NEST {
            return self.refuse_nesting();
        }
        self.depth += 1;
        let e = self.parse_expr_inner(min_bp);
        self.depth -= 1;
        e
    }

    /// Report the nesting limit and abandon the rest of the line.
    ///
    /// Abandoning it is not tidiness: returning an error node without consuming
    /// anything would leave every enclosing loop looking at the same token it
    /// already refused, and juxtaposition would spin on it forever. Skipping to
    /// the end of the line is the existing recovery and it guarantees progress.
    fn refuse_nesting(&mut self) -> Expr {
        let span = self.peek_token().span;
        self.error(
            codes::NESTING_TOO_DEEP,
            span,
            format!("this expression nests more than {MAX_NEST} levels deep"),
        );
        self.too_deep = true;
        self.recover_to_line_end();
        Expr::Error { span }
    }

    fn parse_expr_inner(&mut self, min_bp: u8) -> Expr {
        let mut lhs = self.parse_prefix();

        loop {
            // Explicit infix operators.
            if let Some((lbp, rbp)) = infix_bp(self.peek()) {
                if lbp < min_bp {
                    break;
                }
                let op_tok = self.bump();
                let rhs = self.parse_expr(rbp);
                let span = lhs.span().to(rhs.span());
                lhs = if op_tok.kind == TokenKind::Arrow {
                    Expr::Convert {
                        value: Box::new(lhs),
                        unit: Box::new(rhs),
                        span,
                    }
                } else {
                    let op = match op_tok.kind {
                        TokenKind::Plus => BinaryOp::Add,
                        TokenKind::Minus => BinaryOp::Sub,
                        TokenKind::Star => BinaryOp::Mul,
                        TokenKind::Slash => BinaryOp::Div,
                        TokenKind::Caret => BinaryOp::Pow,
                        TokenKind::Lt => BinaryOp::Lt,
                        TokenKind::Gt => BinaryOp::Gt,
                        TokenKind::Le => BinaryOp::Le,
                        TokenKind::Ge => BinaryOp::Ge,
                        TokenKind::EqEq => BinaryOp::Equal,
                        TokenKind::Ne => BinaryOp::NotEqual,
                        TokenKind::KwAnd => BinaryOp::And,
                        TokenKind::KwOr => BinaryOp::Or,
                        _ => unreachable!("infix_bp admitted a non-infix token"),
                    };
                    Expr::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                        span,
                    }
                };
                continue;
            }

            // Postfix: call and index bind tighter than anything else. A `(`
            // after a bare name is a call; after anything else it falls through
            // to juxtaposition below, so `(a+b)(c+d)` multiplies.
            if self.at(&TokenKind::LParen) && matches!(lhs, Expr::Ident(_)) {
                let Expr::Ident(name) = lhs else {
                    unreachable!("guarded by matches! above")
                };
                lhs = self.parse_call(name);
                continue;
            }
            if self.at(&TokenKind::LBracket) {
                lhs = self.parse_index(lhs);
                continue;
            }

            // Juxtaposition: two primaries in a row multiply.
            if self.starts_primary() {
                let (lbp, rbp) = IMPLICIT_MUL_BP;
                if lbp < min_bp {
                    break;
                }
                let rhs = self.parse_expr(rbp);
                let span = lhs.span().to(rhs.span());
                lhs = Expr::Binary {
                    op: BinaryOp::ImplicitMul,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span,
                };
                continue;
            }

            break;
        }

        lhs
    }

    /// True if the current token could begin a primary expression, which is what
    /// makes juxtaposition detectable.
    fn starts_primary(&self) -> bool {
        matches!(
            self.peek(),
            TokenKind::Number | TokenKind::Ident | TokenKind::LParen | TokenKind::LBracket
        )
    }

    fn parse_prefix(&mut self) -> Expr {
        match self.peek() {
            TokenKind::KwIf => self.parse_conditional(),
            TokenKind::KwNot => {
                let t = self.bump();
                let operand = self.parse_expr(NOT_BP);
                let span = t.span.to(operand.span());
                Expr::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                    span,
                }
            }
            TokenKind::Minus | TokenKind::Plus => {
                let t = self.bump();
                let op = if t.kind == TokenKind::Minus {
                    UnaryOp::Neg
                } else {
                    UnaryOp::Pos
                };
                let operand = self.parse_expr(PREFIX_BP);
                let span = t.span.to(operand.span());
                Expr::Unary {
                    op,
                    operand: Box::new(operand),
                    span,
                }
            }
            _ => self.parse_primary(),
        }
    }

    /// `if c then a else b`.
    ///
    /// Both branches are required. An `if` with no `else` would have to mean
    /// something when the condition is false, and in a language whose every
    /// expression has a value there is no honest answer — so it is a syntax
    /// error rather than an invented zero.
    ///
    /// The branches parse at the loosest binding power, so the `else` arm
    /// extends as far as it can and `if a then b else c + 1` puts the `+ 1`
    /// inside the arm. Chaining follows from that: `else if` is just another
    /// conditional in the arm.
    fn parse_conditional(&mut self) -> Expr {
        let start = self.bump().span;
        let cond = self.parse_expr(0);
        if !self.expect(&TokenKind::KwThen, "`then`", "a condition") {
            return Expr::Error {
                span: start.to(cond.span()),
            };
        }
        let then = self.parse_expr(0);
        if !self.expect(&TokenKind::KwElse, "`else`", "the `then` arm") {
            return Expr::Error {
                span: start.to(then.span()),
            };
        }
        let otherwise = self.parse_expr(0);
        let span = start.to(otherwise.span());
        Expr::If {
            cond: Box::new(cond),
            then: Box::new(then),
            otherwise: Box::new(otherwise),
            span,
        }
    }

    fn parse_primary(&mut self) -> Expr {
        match self.peek() {
            TokenKind::Text => {
                let t = self.bump();
                // The span covers the quotes and the value does not. The lexer
                // has already established that both are there.
                let quoted = t.text(self.source);
                Expr::Text {
                    value: quoted[1..quoted.len() - 1].to_string(),
                    span: t.span,
                }
            }

            TokenKind::Number => {
                let t = self.bump();
                let text = t.text(self.source);
                match text.parse::<f64>() {
                    Ok(value) => Expr::Number {
                        value,
                        span: t.span,
                    },
                    Err(_) => {
                        // The lexer already reported malformed literals; avoid a
                        // second diagnostic for the same characters.
                        if !self
                            .diags
                            .iter()
                            .any(|d| d.span == t.span && d.code == codes::MALFORMED_NUMBER)
                        {
                            self.error(
                                codes::MALFORMED_NUMBER,
                                t.span,
                                format!("`{text}` is not a valid number"),
                            );
                        }
                        Expr::Error { span: t.span }
                    }
                }
            }
            TokenKind::Ident => {
                let t = self.bump();
                Expr::Ident(Name {
                    text: t.text(self.source).to_string(),
                    span: t.span,
                })
            }
            TokenKind::LParen => {
                let open = self.bump();
                let inner = self.parse_expr(0);
                let close = if let Some(t) = self.eat(&TokenKind::RParen) {
                    t.span
                } else {
                    let span = self.peek_token().span;
                    self.error(codes::UNCLOSED_DELIMITER, span, "expected `)`");
                    inner.span()
                };
                Expr::Paren {
                    inner: Box::new(inner),
                    span: open.span.to(close),
                }
            }
            TokenKind::LBracket => self.parse_bracketed(),
            _ => {
                let t = self.peek_token().clone();
                self.error(
                    codes::EXPECTED_EXPRESSION,
                    t.span,
                    match t.kind {
                        TokenKind::Eof => "expected an expression, found end of input".to_string(),
                        TokenKind::Newline => {
                            "expected an expression, found end of line".to_string()
                        }
                        _ => format!("expected an expression, found `{}`", t.text(self.source)),
                    },
                );
                Expr::Error { span: t.span }
            }
        }
    }

    fn parse_call(&mut self, callee: Name) -> Expr {
        self.bump(); // `(`
        let mut args = Vec::new();
        if !self.at(&TokenKind::RParen) {
            loop {
                args.push(self.parse_expr(0));
                if self.eat(&TokenKind::Comma).is_none() {
                    break;
                }
            }
        }
        let close = if let Some(t) = self.eat(&TokenKind::RParen) {
            t.span
        } else {
            let span = self.peek_token().span;
            self.error(
                codes::UNCLOSED_DELIMITER,
                span,
                format!("expected `)` to close the call to `{}`", callee.text),
            );
            span
        };
        let span = callee.span.to(close);
        Expr::Call { callee, args, span }
    }

    fn parse_index(&mut self, base: Expr) -> Expr {
        self.bump(); // `[`
        let mut indices = Vec::new();
        if !self.at(&TokenKind::RBracket) {
            loop {
                indices.push(self.parse_expr(0));
                if self.eat(&TokenKind::Comma).is_none() {
                    break;
                }
            }
        }
        let close = if let Some(t) = self.eat(&TokenKind::RBracket) {
            t.span
        } else {
            let span = self.peek_token().span;
            self.error(
                codes::UNCLOSED_DELIMITER,
                span,
                "expected `]` to close an index",
            );
            span
        };
        let span = base.span().to(close);
        Expr::Index {
            base: Box::new(base),
            indices,
            span,
        }
    }

    /// `[ ... ]` — a vector literal, or a matrix when every element is itself a
    /// bracketed row.
    fn parse_bracketed(&mut self) -> Expr {
        let open = self.bump();
        let mut elements = Vec::new();
        if !self.at(&TokenKind::RBracket) {
            loop {
                elements.push(self.parse_expr(0));
                if self.eat(&TokenKind::Comma).is_none() {
                    break;
                }
            }
        }
        let close = if let Some(t) = self.eat(&TokenKind::RBracket) {
            t.span
        } else {
            let span = self.peek_token().span;
            self.error(
                codes::UNCLOSED_DELIMITER,
                span,
                "expected `]` to close this literal",
            );
            span
        };
        let span = open.span.to(close);

        let all_rows =
            !elements.is_empty() && elements.iter().all(|e| matches!(e, Expr::Vector { .. }));
        if all_rows {
            let rows: Vec<Vec<Expr>> = elements
                .into_iter()
                .map(|e| match e {
                    Expr::Vector { elements, .. } => elements,
                    _ => unreachable!("checked above"),
                })
                .collect();
            let width = rows[0].len();
            if rows.iter().any(|r| r.len() != width) {
                self.error(
                    codes::RAGGED_MATRIX,
                    span,
                    "every row of a matrix must have the same number of elements",
                );
            }
            return Expr::Matrix { rows, span };
        }

        Expr::Vector { elements, span }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render an expression as an s-expression so that structural assertions read
    /// as one line rather than a page of nested patterns. Implicit multiplication
    /// prints as `·` to keep it distinguishable from explicit `*`.
    fn sexpr(e: &Expr) -> String {
        match e {
            Expr::Number { value, .. } => format!("{value}"),
            Expr::Text { value, .. } => format!("{value:?}"),
            Expr::Ident(n) => n.text.clone(),
            Expr::Unary { op, operand, .. } => {
                let o = match op {
                    UnaryOp::Neg => "neg",
                    UnaryOp::Pos => "pos",
                    UnaryOp::Not => "not",
                };
                format!("({o} {})", sexpr(operand))
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                let o = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::ImplicitMul => "·",
                    BinaryOp::Div => "/",
                    BinaryOp::Pow => "^",
                    other => other.symbol(),
                };
                format!("({o} {} {})", sexpr(lhs), sexpr(rhs))
            }
            Expr::If {
                cond,
                then,
                otherwise,
                ..
            } => format!("(if {} {} {})", sexpr(cond), sexpr(then), sexpr(otherwise)),
            Expr::Call { callee, args, .. } => {
                let a: Vec<String> = args.iter().map(sexpr).collect();
                format!("(call {} {})", callee.text, a.join(" "))
            }
            Expr::Index { base, indices, .. } => {
                let a: Vec<String> = indices.iter().map(sexpr).collect();
                format!("(index {} {})", sexpr(base), a.join(" "))
            }
            Expr::Vector { elements, .. } => {
                let a: Vec<String> = elements.iter().map(sexpr).collect();
                format!("[{}]", a.join(" "))
            }
            Expr::Matrix { rows, .. } => {
                let r: Vec<String> = rows
                    .iter()
                    .map(|row| {
                        let c: Vec<String> = row.iter().map(sexpr).collect();
                        format!("[{}]", c.join(" "))
                    })
                    .collect();
                format!("[{}]", r.join(" "))
            }
            Expr::Paren { inner, .. } => format!("(group {})", sexpr(inner)),
            Expr::Convert { value, unit, .. } => {
                format!("(-> {} {})", sexpr(value), sexpr(unit))
            }
            Expr::Error { .. } => "<error>".to_string(),
        }
    }

    /// Parse a single expression statement and render it.
    fn expr(src: &str) -> String {
        let p = parse(src);
        assert!(
            !p.has_errors(),
            "unexpected diagnostics for {src:?}: {:?}",
            p.diagnostics
        );
        match p.ast.stmts.as_slice() {
            [Stmt::Query { expr, .. }] => sexpr(expr),
            other => panic!("expected one query statement, got {other:?}"),
        }
    }

    // ---- conditionals and comparisons -------------------------------------

    #[test]
    fn a_conditional_is_an_expression() {
        assert_eq!(expr("if a then b else c"), "(if a b c)");
    }

    #[test]
    fn the_else_arm_reaches_as_far_as_it_can() {
        // `if a then b else c + 1` puts the `+ 1` inside the arm. The other
        // reading — `(if a then b else c) + 1` — would make every chained
        // conditional need brackets.
        assert_eq!(expr("if a then b else c + 1"), "(if a b (+ c 1))");
        assert_eq!(expr("if a then b + 1 else c"), "(if a (+ b 1) c)");
    }

    #[test]
    fn conditionals_chain_without_brackets() {
        assert_eq!(
            expr("if a then b else if c then d else e"),
            "(if a b (if c d e))"
        );
    }

    #[test]
    fn a_conditional_can_sit_inside_arithmetic() {
        assert_eq!(expr("1 + (if a then b else c)"), "(+ 1 (group (if a b c)))");
    }

    #[test]
    fn comparison_is_looser_than_arithmetic() {
        // `a + b < c` compares the sum, which is the only reading anyone means.
        assert_eq!(expr("a + b < c"), "(< (+ a b) c)");
        assert_eq!(expr("a < b + c"), "(< a (+ b c))");
    }

    #[test]
    fn and_binds_tighter_than_or() {
        assert_eq!(expr("a and b or c"), "(or (and a b) c)");
        assert_eq!(expr("a or b and c"), "(or a (and b c))");
    }

    #[test]
    fn logical_connectives_are_looser_than_comparison() {
        assert_eq!(expr("a < b and c > d"), "(and (< a b) (> c d))");
    }

    #[test]
    fn not_takes_the_whole_comparison() {
        // `not x == y` reads as "not (x equals y)", so that is what it parses as.
        assert_eq!(expr("not x == y"), "(not (== x y))");
        // But it does not reach across `and`.
        assert_eq!(expr("not a and b"), "(and (not a) b)");
    }

    #[test]
    fn the_two_character_comparisons_lex_as_one_token() {
        // `<=` must never become `<` followed by `=`, which would report
        // something about assignment and send the reader nowhere useful.
        assert_eq!(expr("a <= b"), "(≤ a b)");
        assert_eq!(expr("a >= b"), "(≥ a b)");
        assert_eq!(expr("a != b"), "(≠ a b)");
        assert_eq!(expr("a == b"), "(== a b)");
    }

    #[test]
    fn the_typeset_comparisons_mean_the_same_as_the_typed_ones() {
        // `π` and `°` are already ordinary characters, so refusing `≤` would be
        // an odd place to draw a line.
        assert_eq!(expr("a ≤ b"), expr("a <= b"));
        assert_eq!(expr("a ≥ b"), expr("a >= b"));
        assert_eq!(expr("a ≠ b"), expr("a != b"));
    }

    #[test]
    fn a_conditional_without_an_else_is_refused() {
        // There is no honest value for the missing arm in a language where
        // every expression has one, so this is an error rather than a zero.
        let p = parse("x = if a then b\n");
        assert!(p.has_errors());
    }

    #[test]
    fn a_conditional_without_a_then_is_refused() {
        let p = parse("x = if a b else c\n");
        assert!(p.has_errors());
    }

    #[test]
    fn a_lone_bang_says_what_to_write_instead() {
        let p = parse("x = a ! b\n");
        assert!(p.has_errors());
        assert!(
            p.diagnostics[0].message.contains("!="),
            "{:?}",
            p.diagnostics[0]
        );
    }

    // ---- precedence -------------------------------------------------------

    #[test]
    fn juxtaposition_is_multiplication() {
        assert_eq!(expr("5 cm"), "(· 5 cm)");
        assert_eq!(expr("2 x"), "(· 2 x)");
    }

    #[test]
    fn unit_expressions_group_correctly() {
        // The case the whole precedence design exists to get right.
        assert_eq!(expr("9.81 m/s^2"), "(/ (· 9.81 m) (^ s 2))");
        assert_eq!(expr("kg m/s^2"), "(/ (· kg m) (^ s 2))");
    }

    #[test]
    fn implicit_and_explicit_multiplication_associate_alike() {
        // `1/2 m` reads as (1/2)·m, not 1/(2·m).
        assert_eq!(expr("1/2 m"), "(· (/ 1 2) m)");
        assert_eq!(expr("1/2*m"), "(* (/ 1 2) m)");
    }

    #[test]
    fn power_is_right_associative() {
        assert_eq!(expr("2^3^2"), "(^ 2 (^ 3 2))");
    }

    #[test]
    fn unary_minus_sits_between_power_and_product() {
        assert_eq!(expr("-x^2"), "(neg (^ x 2))");
        assert_eq!(expr("-x * y"), "(* (neg x) y)");
    }

    #[test]
    fn addition_is_loosest_arithmetic() {
        assert_eq!(expr("a + b*c"), "(+ a (* b c))");
        assert_eq!(expr("a*b + c"), "(+ (* a b) c)");
    }

    #[test]
    fn conversion_binds_loosest_of_all() {
        assert_eq!(expr("a + b -> mm"), "(-> (+ a b) mm)");
        assert_eq!(expr("V -> dm^3"), "(-> V (^ dm 3))");
    }

    #[test]
    fn parentheses_are_preserved_for_round_tripping() {
        assert_eq!(expr("(a + b)*c"), "(* (group (+ a b)) c)");
    }

    // ---- postfix ----------------------------------------------------------

    #[test]
    fn call_and_index() {
        assert_eq!(expr("f(x)"), "(call f x)");
        assert_eq!(expr("area(50 mm)"), "(call area (· 50 mm))");
        assert_eq!(expr("x[3]"), "(index x 3)");
        assert_eq!(expr("K[2,1]"), "(index K 2 1)");
    }

    #[test]
    fn paren_after_non_name_is_multiplication_not_a_call() {
        assert_eq!(expr("(a+b)(c+d)"), "(· (group (+ a b)) (group (+ c d)))");
        assert_eq!(expr("2(x+1)"), "(· 2 (group (+ x 1)))");
    }

    // ---- literals ---------------------------------------------------------

    #[test]
    fn vector_and_matrix_literals() {
        assert_eq!(expr("[1, 2, 3]"), "[1 2 3]");
        assert_eq!(expr("[[1, 2], [3, 4]]"), "[[1 2] [3 4]]");
    }

    #[test]
    fn vector_with_units() {
        assert_eq!(expr("[5, 10, 15] Hz"), "(· [5 10 15] Hz)");
    }

    #[test]
    fn ragged_matrix_is_reported() {
        let p = parse("[[1, 2], [3]]");
        assert!(p.diagnostics.iter().any(|d| d.code == codes::RAGGED_MATRIX));
    }

    // ---- statements -------------------------------------------------------

    #[test]
    fn assignment() {
        let p = parse("r = 5 cm");
        assert!(!p.has_errors());
        match p.ast.stmts.as_slice() {
            [Stmt::Assign { name, value, .. }] => {
                assert_eq!(name.text, "r");
                assert_eq!(sexpr(value), "(· 5 cm)");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn unit_declaration() {
        let p = parse("unit kip = 1000 lbf");
        assert!(!p.has_errors());
        match p.ast.stmts.as_slice() {
            [Stmt::UnitDecl { name, value, .. }] => {
                assert_eq!(name.text, "kip");
                assert_eq!(sexpr(value), "(· 1000 lbf)");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn function_definition() {
        let p = parse("fn area(d) = pi*d^2/4");
        assert!(!p.has_errors());
        match p.ast.stmts.as_slice() {
            [Stmt::FnDef {
                name, params, body, ..
            }] => {
                assert_eq!(name.text, "area");
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].text, "d");
                assert_eq!(sexpr(body), "(/ (* pi (^ d 2)) 4)");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn function_with_several_parameters() {
        let p = parse("fn f(a, b, c) = a+b+c");
        match p.ast.stmts.as_slice() {
            [Stmt::FnDef { params, .. }] => assert_eq!(params.len(), 3),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn comments_become_statements() {
        let p = parse("' Shaker specifications\nk = 4");
        assert!(!p.has_errors());
        match p.ast.stmts.as_slice() {
            [Stmt::Comment { text, .. }, Stmt::Assign { .. }] => {
                assert_eq!(text, "Shaker specifications");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn worksheet_of_several_statements() {
        let src = "' Cylinder\nr = 5 cm\nh = 12 cm\nV = pi*r^2*h\nV -> dm^3";
        let p = parse(src);
        assert!(!p.has_errors(), "{:?}", p.diagnostics);
        assert_eq!(p.ast.stmts.len(), 5);
    }

    // ---- error recovery ---------------------------------------------------

    #[test]
    fn a_bad_line_does_not_swallow_the_next() {
        let p = parse("x = 1 +\ny = 2\nz = 3");
        assert!(p.has_errors());
        // The two good lines still parse.
        let assigns = p
            .ast
            .stmts
            .iter()
            .filter(|s| matches!(s, Stmt::Assign { .. }))
            .count();
        assert!(assigns >= 2, "got {:?}", p.ast.stmts);
    }

    #[test]
    fn assigning_to_a_non_name_is_rejected() {
        let p = parse("x + 1 = 2");
        assert!(p
            .diagnostics
            .iter()
            .any(|d| d.code == codes::INVALID_ASSIGN_TARGET));
    }

    #[test]
    fn unclosed_delimiters_are_reported() {
        assert!(parse("f(x")
            .diagnostics
            .iter()
            .any(|d| d.code == codes::UNCLOSED_DELIMITER));
        assert!(parse("[1, 2")
            .diagnostics
            .iter()
            .any(|d| d.code == codes::UNCLOSED_DELIMITER));
        assert!(parse("(a + b")
            .diagnostics
            .iter()
            .any(|d| d.code == codes::UNCLOSED_DELIMITER));
    }

    #[test]
    fn parsing_always_terminates_on_junk() {
        // No panics, no hangs, whatever the input.
        for src in [
            "", "\n\n", "=", "]", ")", "@@@", "1.2.3", "unit", "fn", "fn f(", "->",
        ] {
            let _ = parse(src);
        }
    }

    #[test]
    fn diagnostics_come_back_in_source_order() {
        // The `@` is a lexer error on the last line; the others are parse errors
        // on earlier lines. Reported together, they must read top to bottom.
        let p = parse("x = 1 +\ny = (2\nw + 1 = 4\nq = 5 @ 6");
        let starts: Vec<u32> = p.diagnostics.iter().map(|d| d.span.start).collect();
        let mut sorted = starts.clone();
        sorted.sort_unstable();
        assert_eq!(starts, sorted, "diagnostics were {:?}", p.diagnostics);
    }

    #[test]
    fn spans_point_at_the_offending_text() {
        let src = "x = 1 + @";
        let p = parse(src);
        let d = p
            .diagnostics
            .iter()
            .find(|d| d.code == crate::diag::codes::UNEXPECTED_CHAR)
            .expect("expected an unexpected-character diagnostic");
        assert_eq!(d.span.text(src), "@");
    }

    /// `x = ((( … 1 … )))`, nested `n` deep.
    fn nested(n: usize) -> String {
        format!("x = {}1{}\n", "(".repeat(n), ")".repeat(n))
    }

    #[test]
    fn nesting_up_to_the_limit_parses() {
        // One level is the expression itself, so MAX_NEST brackets is the
        // deepest thing that must still work. Real worksheets reach 14.
        let p = parse(&nested(MAX_NEST - 1));
        assert!(
            p.diagnostics.is_empty(),
            "diagnostics at the limit: {:?}",
            p.diagnostics
        );
    }

    #[test]
    fn nesting_past_the_limit_is_refused_once() {
        let p = parse(&nested(MAX_NEST + 50));
        let errors: Vec<&str> = p.diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(
            errors,
            vec![codes::NESTING_TOO_DEEP],
            "one refusal and no cascade, got {:?}",
            p.diagnostics
        );
        assert!(p.diagnostics[0].message.contains("nests more than 128"));
    }

    #[test]
    fn a_refused_line_does_not_silence_the_next_one() {
        // The suppression is per statement. A worksheet whose second line is
        // machine-generated nonsense still reports what is wrong with its
        // fourth.
        let src = format!("a = 1\n{}b = 2 +\n", nested(MAX_NEST + 1));
        let p = parse(&src);
        let codes_seen: Vec<&str> = p.diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(
            codes_seen,
            vec![codes::NESTING_TOO_DEEP, codes::EXPECTED_EXPRESSION],
            "{:?}",
            p.diagnostics
        );
        // And the good line before it survived as a statement.
        assert!(matches!(p.ast.stmts.first(), Some(Stmt::Assign { .. })));
    }

    #[test]
    fn every_way_of_nesting_is_counted() {
        // The guard sits in `parse_expr`, so each of these must trip it. If one
        // of them ever stops routing through there it becomes a way to build a
        // deep tree without being counted, which is the crash again.
        let deep = MAX_NEST + 5;
        for src in [
            format!("x = {}1{}\n", "(".repeat(deep), ")".repeat(deep)),
            format!("x = {}1{}\n", "sin(".repeat(deep), ")".repeat(deep)),
            format!("x = {}1{}\n", "[".repeat(deep), "]".repeat(deep)),
            format!("x = {}1{}\n", "-".repeat(deep), "".repeat(deep)),
            format!(
                "x = {}1{}\n",
                "if 1 then ".repeat(deep),
                " else 2".repeat(deep)
            ),
        ] {
            let p = parse(&src);
            assert!(
                p.diagnostics
                    .iter()
                    .any(|d| d.code == codes::NESTING_TOO_DEEP),
                "not counted: {:?}",
                &src[..40.min(src.len())]
            );
        }
    }
}
