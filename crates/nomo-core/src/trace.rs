//! Evaluation results, as an annotated mirror of the syntax tree.
//!
//! # Why evaluation does not return a value
//!
//! The entire reason a worksheet exists is that it shows its work:
//!
//! ```text
//! V = π·r²·h = π·(5 cm)²·(12 cm) = 0.942 dm³
//!     ^           ^                 ^
//!     symbolic    substituted       result
//! ```
//!
//! Rendering that line needs the original expression structure, every leaf's
//! value and unit, and the final result, all still present at render time. A
//! signature of `eval(expr) -> Value` throws away two of the three. So evaluation
//! produces a [`Trace`]: the same shape as the input, with a value attached to
//! every node. The renderer walks it three times, once per column above.
//!
//! # Errors are local
//!
//! Each node holds a `Result`, so one bad subexpression does not erase the rest
//! of the tree. Only the node that actually failed carries a real error; its
//! ancestors carry [`EvalError::Poisoned`], so a single mistake yields a single
//! diagnostic rather than one per level.

use crate::ast::{BinaryOp, UnaryOp};
use crate::span::Span;
use crate::unit::Unit;
use crate::value::{EvalError, Value};

/// A unit requested with `->`, recorded for the renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayTarget {
    /// Where the target was written, e.g. the `dm^3` in `V -> dm^3`.
    ///
    /// The renderer holds the source and slices this, rather than the engine
    /// carrying a copy of the text: `nomo-core` never sees a file, and threading
    /// the source through evaluation purely to reproduce a substring the caller
    /// already has would be an odd dependency to acquire.
    pub span: Span,
    /// Present when the target was a single named unit. An offset scale can only
    /// be reached this way, since a compound expression cannot carry an offset.
    pub unit: Option<Unit>,
    /// Base-SI magnitude of one of the target unit, so a renderer divides by it.
    pub factor: f64,
    /// What the target measures.
    ///
    /// Carried so that a *hint* can be checked against the value it would
    /// describe. `M = 500 N*m` records `m` as the unit the binding was written
    /// in — the right operand of the multiplication — and that is a length
    /// where the value is a moment. Dividing by it anyway printed `500 m`,
    /// which is the quietly-wrong kind of answer this engine exists to avoid.
    pub dim: crate::dim::Dimension,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TraceNode {
    Number,
    /// A string literal. Its own node rather than a `Number`, because the
    /// renderer writes it back with its quotes and a number without.
    Text,
    /// A name that resolved to a bound variable.
    ///
    /// `unit` is the unit the binding was *written* in, when it had one. The
    /// substituted column reads `π·(5 cm)²·(12 cm)`, not `π·(0.05 m)²·(0.12 m)`:
    /// an engineer checking the arithmetic wants to see the numbers they typed,
    /// not the base-SI values the engine happens to store.
    Variable {
        name: String,
        /// How the binding was written, when that says something the base units
        /// do not: `A_s = … -> mm^2` should substitute as `84.27 mm²` wherever
        /// `A_s` is used, not as `8.427e-5 m²`.
        ///
        /// A [`DisplayTarget`] rather than a [`Unit`] so that a *compound*
        /// target carries too. `mm^2`, `MN/m` and `N*m` are not names in the
        /// unit table and cannot become a `Unit`; they are a span and a factor,
        /// which is exactly what this already holds for `->`.
        /// Boxed: a `DisplayTarget` is the largest thing a `TraceNode` can
        /// hold, and evaluation returns a `Trace` by value at every level of a
        /// recursion — so its size is stack the engine spends on every node,
        /// including the great majority that carry no target at all.
        unit: Option<Box<DisplayTarget>>,
    },
    /// A name that resolved to a unit.
    UnitRef(String),
    /// A name that resolved to a built-in constant such as `pi`.
    Constant(String),
    /// A name passed to `map` or `iterate` as the function to apply. It is not
    /// evaluated — a function is not a value in this language — so it renders as
    /// itself in every column.
    FnRef(String),
    Unary {
        op: UnaryOp,
        operand: Box<Trace>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Trace>,
        rhs: Box<Trace>,
    },
    /// A number applied to an offset scale, as in `20°C`.
    ///
    /// Distinct from multiplication because it is not multiplication: an offset
    /// scale has no meaningful value of "one" to multiply by.
    AffineLiteral {
        magnitude: f64,
        unit: String,
    },
    Call {
        name: String,
        args: Vec<Trace>,
    },
    Index {
        base: Box<Trace>,
        indices: Vec<Trace>,
    },
    Vector(Vec<Trace>),
    Matrix(Vec<Vec<Trace>>),
    Paren(Box<Trace>),
    /// `if c then a else b`. Exactly one arm carries a value; the other holds
    /// `EvalError::NotTaken` and exists so the line can still be shown as it was
    /// written.
    Conditional {
        cond: Box<Trace>,
        then: Box<Trace>,
        otherwise: Box<Trace>,
    },
    Convert {
        value: Box<Trace>,
        target: Option<Box<DisplayTarget>>,
    },
    /// The syntax tree had an error here.
    Malformed,
}

/// One node of an evaluated expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    pub span: Span,
    pub node: TraceNode,
    pub value: Result<Value, EvalError>,
}

impl Trace {
    pub fn new(span: Span, node: TraceNode, value: Result<Value, EvalError>) -> Trace {
        Trace { span, node, value }
    }

    pub fn is_ok(&self) -> bool {
        self.value.is_ok()
    }

    /// The error that actually caused a failure, ignoring poisoned ancestors.
    ///
    /// Walking to the deepest genuine error is what keeps one mistake to one
    /// diagnostic, and points it at the subexpression the user must fix.
    pub fn root_error(&self) -> Option<(Span, &EvalError)> {
        if self.value.is_ok() {
            return None;
        }
        for child in self.children() {
            if let Some(found) = child.root_error() {
                return Some(found);
            }
        }
        match &self.value {
            Err(e) => Some((self.span, e)),
            Ok(_) => None,
        }
    }

    /// The children that were actually evaluated.
    ///
    /// An arm of a conditional that was not taken is left out, so error search
    /// never descends into work that did not happen. That single rule covers
    /// short-circuited `and`/`or` too, since their skipped operand is marked the
    /// same way.
    pub fn children(&self) -> Vec<&Trace> {
        self.all_children()
            .into_iter()
            .filter(|c| !matches!(c.value, Err(EvalError::NotTaken)))
            .collect()
    }

    /// Every child, evaluated or not. For rendering, which must show the whole
    /// expression whatever ran.
    pub fn all_children(&self) -> Vec<&Trace> {
        match &self.node {
            TraceNode::Number
            | TraceNode::Text
            | TraceNode::Variable { .. }
            | TraceNode::UnitRef(_)
            | TraceNode::Constant(_)
            | TraceNode::FnRef(_)
            | TraceNode::AffineLiteral { .. }
            | TraceNode::Malformed => vec![],
            TraceNode::Unary { operand, .. } => vec![operand],
            TraceNode::Binary { lhs, rhs, .. } => vec![lhs, rhs],
            TraceNode::Conditional {
                cond,
                then,
                otherwise,
            } => vec![cond, then, otherwise],
            TraceNode::Call { args, .. } => args.iter().collect(),
            TraceNode::Index { base, indices } => {
                let mut v: Vec<&Trace> = vec![base];
                v.extend(indices.iter());
                v
            }
            TraceNode::Vector(elements) => elements.iter().collect(),
            TraceNode::Matrix(rows) => rows.iter().flatten().collect(),
            TraceNode::Paren(inner) => vec![inner],
            TraceNode::Convert { value, .. } => vec![value],
        }
    }
}
