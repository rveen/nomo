//! The syntax tree.
//!
//! There is exactly one of these. Diagnostics, the formatter, the future
//! highlighter and the evaluator all consume this tree; none of them re-parses.
//! The design note records why (§10): CalcpadCE maintains two independent
//! parsers for one language, and they can disagree about what a document means.
//!
//! Two consequences shape the shapes below. Every node carries a `Span`, because
//! evaluation carries spans into the trace so a worksheet can show its work
//! against the exact source the user typed. And syntax that is semantically
//! redundant — parentheses, implicit versus explicit multiplication — is still
//! represented, so that formatting round-trips and the symbolic render mode can
//! reproduce what was written.

use crate::span::Span;

/// An identifier occurrence: variable, unit, or function name.
#[derive(Debug, Clone, PartialEq)]
pub struct Name {
    pub text: String,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Pos,
    /// `not x`. Dimensionless in, dimensionless out: 1 or 0.
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    /// Multiplication written by juxtaposition: `5 cm`, `2 x`, `kg m/s^2`.
    ///
    /// Evaluates identically to [`BinaryOp::Mul`]; kept distinct so the
    /// formatter does not rewrite `5 cm` as `5 * cm`. This is also precisely how
    /// SMath encodes units — an operand carrying a unit style, attached by
    /// multiplication — so an importer maps onto it directly.
    ImplicitMul,
    Div,
    Pow,
    /// Comparisons. Both operands must share a dimension; the result is the
    /// dimensionless 1 or 0.
    Lt,
    Gt,
    Le,
    Ge,
    /// `==` and `!=`. Written with two characters because `=` binds a name.
    Equal,
    NotEqual,
    /// `and` and `or`, on dimensionless operands. Both short-circuit, so a guard
    /// like `n > 0 and v[n] > 3` never indexes out of bounds.
    And,
    Or,
}

impl BinaryOp {
    /// True for the comparisons and the logical connectives, whose result is a
    /// truth value rather than a quantity.
    pub fn is_logical(self) -> bool {
        matches!(
            self,
            BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::Le
                | BinaryOp::Ge
                | BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::And
                | BinaryOp::Or
        )
    }
}

impl BinaryOp {
    /// True for the two multiplication spellings, which differ only in how they
    /// are written.
    pub fn is_mul(self) -> bool {
        matches!(self, BinaryOp::Mul | BinaryOp::ImplicitMul)
    }

    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::ImplicitMul => " ",
            BinaryOp::Div => "/",
            BinaryOp::Pow => "^",
            BinaryOp::Lt => "<",
            BinaryOp::Gt => ">",
            // The typeset spellings, since this is what the renderer prints and
            // a worksheet is meant to be read.
            BinaryOp::Le => "≤",
            BinaryOp::Ge => "≥",
            BinaryOp::Equal => "==",
            BinaryOp::NotEqual => "≠",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number {
        value: f64,
        span: Span,
    },
    Ident(Name),
    /// `"a verdict in words"`. The text without its quotes.
    Text {
        value: String,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    /// `f(x, y)`. The callee is a name; higher-order functions are not in scope.
    Call {
        callee: Name,
        args: Vec<Expr>,
        span: Span,
    },
    /// `x[3]`, `K[2, 1]`.
    Index {
        base: Box<Expr>,
        indices: Vec<Expr>,
        span: Span,
    },
    /// `[1, 2, 3]`
    Vector {
        elements: Vec<Expr>,
        span: Span,
    },
    /// `[[1, 2], [3, 4]]`
    Matrix {
        rows: Vec<Vec<Expr>>,
        span: Span,
    },
    /// `if c then a else b`.
    ///
    /// An expression rather than a statement, so a conditional can appear inside
    /// arithmetic and a function body can be piecewise without any new statement
    /// form. Only the branch that is taken is evaluated.
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        otherwise: Box<Expr>,
        span: Span,
    },
    /// Explicit grouping. Retained so formatting round-trips.
    Paren {
        inner: Box<Expr>,
        span: Span,
    },
    /// `value -> unit`, a conversion for display or coercion.
    Convert {
        value: Box<Expr>,
        unit: Box<Expr>,
        span: Span,
    },
    /// A placeholder left where the parser could not understand the input. Its
    /// presence means a diagnostic was emitted; evaluation refuses to run.
    Error {
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Number { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Call { span, .. }
            | Expr::Index { span, .. }
            | Expr::Vector { span, .. }
            | Expr::Matrix { span, .. }
            | Expr::Paren { span, .. }
            | Expr::If { span, .. }
            | Expr::Convert { span, .. }
            | Expr::Text { span, .. }
            | Expr::Error { span } => *span,
            Expr::Ident(name) => name.span,
        }
    }

    /// True if this subtree contains a parse error placeholder.
    pub fn has_error(&self) -> bool {
        match self {
            Expr::Error { .. } => true,
            Expr::Number { .. } | Expr::Ident(_) | Expr::Text { .. } => false,
            Expr::Unary { operand, .. } => operand.has_error(),
            Expr::Paren { inner, .. } => inner.has_error(),
            Expr::Binary { lhs, rhs, .. } => lhs.has_error() || rhs.has_error(),
            Expr::If {
                cond,
                then,
                otherwise,
                ..
            } => cond.has_error() || then.has_error() || otherwise.has_error(),
            Expr::Convert { value, unit, .. } => value.has_error() || unit.has_error(),
            Expr::Call { args, .. } => args.iter().any(Expr::has_error),
            Expr::Index { base, indices, .. } => {
                base.has_error() || indices.iter().any(Expr::has_error)
            }
            Expr::Vector { elements, .. } => elements.iter().any(Expr::has_error),
            Expr::Matrix { rows, .. } => rows.iter().any(|row| row.iter().any(Expr::has_error)),
        }
    }
}

/// What an `axis` line asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum AxisSetting {
    /// `log` or `linear`.
    Log(bool),
    /// `0 kN, 100 kN` — the window to draw, whatever was sampled.
    Limits(Expr, Expr),
    /// `auto` — back to the extent the data or the span implies, and linear.
    Auto,
}

/// A top-level worksheet statement.
///
/// There are two kinds of binding. A positional one resolves in reading order; a
/// global one is collected before anything runs and is visible throughout. The
/// design note (§6) records why both are needed.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `' prose`. Carried through to the renderer as documentation.
    Comment { text: String, span: Span },
    /// `r = 5 cm`. Visible to statements below it.
    Assign { name: Name, value: Expr, span: Span },
    /// `global g = 9.81 m/s^2`. Visible everywhere, regardless of position.
    ///
    /// This exists because SMath's `≡` declares exactly such a binding, verified
    /// by 39 forward references in the surveyed corpus, so any worksheet imported
    /// from it may depend on the behaviour.
    GlobalDef { name: Name, value: Expr, span: Span },
    /// A bare expression, displayed with its result: `V` or `V -> dm^3`.
    Query { expr: Expr, span: Span },
    /// `unit kip = 1000 lbf`
    UnitDecl { name: Name, value: Expr, span: Span },
    /// `check sigma <= sigma_allow`. A condition, and a verdict on it.
    ///
    /// It binds nothing and nothing reads it, which is what separates it from
    /// every other statement here: it exists to be *reported*. A worksheet whose
    /// check fails is not a worksheet that is wrong — the arithmetic is fine and
    /// the design is not — so a failed check produces no diagnostic and does not
    /// make `has_errors` true. What it produces is a verdict, counted and
    /// answered for in the exit code.
    Check { expr: Expr, span: Span },
    /// `use steel` — bring in a pack of definitions.
    ///
    /// The pack's own statements are spliced in where this stands and hidden
    /// from the output; this node is what the reader sees, and what a
    /// diagnostic about the pack points at. See [`crate::packs`].
    Use { name: Name, span: Span },
    /// `digits 3` — show results to three significant figures, from here down.
    ///
    /// Presentation, not arithmetic: it changes what is printed and nothing
    /// about what was computed, which is why it may sit anywhere and why the
    /// snapshot's values section ignores it.
    Digits { figures: u32, span: Span },
    /// `axis x log`, `axis y 0 kN, 100 kN`, `axis y auto`.
    ///
    /// How the plots below this line are drawn — and, for a logarithmic
    /// horizontal axis, how they are *sampled*: a decade sweep spaced linearly
    /// has almost no points in its first decade.
    Axis {
        vertical: bool,
        setting: AxisSetting,
        span: Span,
    },
    /// `fn area(d) = pi*d^2/4`
    FnDef {
        name: Name,
        params: Vec<Name>,
        body: Expr,
        span: Span,
    },
    /// A statement that could not be parsed. A diagnostic accompanies it.
    Error { span: Span },
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Comment { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::GlobalDef { span, .. }
            | Stmt::Query { span, .. }
            | Stmt::UnitDecl { span, .. }
            | Stmt::Check { span, .. }
            | Stmt::Use { span, .. }
            | Stmt::Digits { span, .. }
            | Stmt::Axis { span, .. }
            | Stmt::FnDef { span, .. }
            | Stmt::Error { span } => *span,
        }
    }
}

/// A parsed worksheet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ast {
    pub stmts: Vec<Stmt>,
}
