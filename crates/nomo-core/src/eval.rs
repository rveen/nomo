//! Evaluation.
//!
//! Produces a [`Trace`] rather than a value; see the `trace` module for why.
//!
//! # Name resolution
//!
//! An identifier is looked up as a variable, then a constant, then a unit. User
//! bindings therefore shadow units, which is what people expect. Shadowing a
//! multi-letter unit warns; see `shadowing_is_worth_reporting` for why
//! single-letter ones deliberately do not.
//!
//! # Reduction order
//!
//! Sums and products reduce strictly left to right, and nothing here evaluates in
//! parallel. Any other order would change the last bits of a result, and this
//! engine promises identical output on every machine.

use crate::ast::{BinaryOp, Expr, Stmt, UnaryOp};
use crate::complex::ComplexQuantity;
use crate::diag::Diagnostic;
use crate::dual::{DualError, DualQuantity};
use crate::math;
use crate::quantity::Quantity;
use crate::span::Span;
use crate::trace::{DisplayTarget, Trace, TraceNode};
use crate::unit::{Unit, UnitError, UnitTable};
use crate::value::{EvalError, MatrixValue, Value, VectorValue};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// Diagnostic codes raised during evaluation.
pub mod eval_codes {
    pub const EVAL_ERROR: &str = "SH201";
    pub const SHADOWS_UNIT: &str = "SH202";
    pub const REDEFINED: &str = "SH203";
}

/// A user-defined function.
#[derive(Debug, Clone)]
struct Function {
    params: Vec<String>,
    body: Expr,
}

/// Bindings, functions and units in scope.
pub struct Env {
    vars: BTreeMap<String, Value>,
    /// Names this worksheet binds whose binding produced no value.
    ///
    /// A binding hides a unit of the same name "for the rest of the worksheet",
    /// which is what the `SH202` warning promises — and it has to keep that
    /// promise when it fails, or a name the author clearly meant as a variable
    /// quietly becomes a unit and the worksheet answers with confidence. `PF`
    /// reads as peta-farads, `Zs` as zetta-seconds; the two-letter space of
    /// every SI prefix against every unit symbol is large enough that ordinary
    /// names fall into it. Two worksheets in the SMath corpus did exactly this,
    /// which is how it was found.
    failed: BTreeSet<String>,
    /// The unit each binding was written in, for the substituted column.
    hints: BTreeMap<String, Unit>,
    funcs: BTreeMap<String, Function>,
    units: UnitTable,
    /// How many user functions are on the stack below this one. See
    /// [`MAX_DEPTH`].
    depth: usize,
    /// Calls left to spend on this statement. See [`MAX_CALLS`].
    ///
    /// Shared with every child scope rather than copied, because the thing it
    /// bounds is the *total* work a statement does: a call that branches
    /// spends the same budget twice over, and a per-frame counter would not
    /// notice.
    budget: Rc<Cell<usize>>,
    /// How deep evaluation is nested right now. See [`MAX_EVAL_NEST`].
    ///
    /// Shared for the same reason the budget is, and a sharper one: a called
    /// function's body runs in a child scope but on the *same* native stack, so
    /// a counter that started again at zero per scope would bound nothing.
    nesting: Rc<Cell<usize>>,
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

/// Every built-in function, for callers that need to recognise one without
/// calling it. The editor colours a call differently from a variable, and
/// deciding that in TypeScript would be a second list of what the language
/// contains — the mistake invariant 1 exists to prevent.
///
/// This is still a second list next to the dispatch in `call_builtin`, which is
/// exactly the kind of thing that drifts, so `builtins_match_the_dispatch` fails
/// if it ever does.
pub const BUILTINS: &[&str] = &[
    "Im",
    "Re",
    "abs",
    "acos",
    "arg",
    "asin",
    "atan",
    "atan2",
    "augment",
    "ceil",
    "col",
    "cols",
    "conj",
    "cos",
    "cosh",
    "cross",
    "derivative",
    "det",
    "diag",
    "dot",
    "exp",
    "floor",
    "identity",
    "integral",
    "inv",
    "iterate",
    "length",
    "ln",
    "log10",
    "log2",
    "map",
    "max",
    "min",
    "norm",
    "plot",
    "range",
    "root",
    "roots",
    "round",
    "row",
    "rows",
    "sign",
    "sin",
    "sinh",
    "solve_linear",
    "sqrt",
    "stack",
    "sum",
    "tan",
    "tanh",
    "transpose",
];

/// Whether `name` is a built-in constant.
pub fn is_constant(name: &str) -> bool {
    constant(name).is_some() || imaginary_constant(name).is_some()
}

/// Built-in constants. Dimensionless by construction.
/// The imaginary unit, if that is what this name is.
///
/// A constant rather than a literal suffix, and `i` rather than `j`, because
/// design note item 29 found it in the corpus as an operand spelled `i` with
/// units attached — `(1 + 2i)·Ω`. It needs no lexer rule: juxtaposition is
/// already multiplication, so `2i` is `2*i` and `4i Ω` is `4*i*Ω`, which is
/// how `2e` already reads as `2*e`.
///
/// A binding wins over it, exactly as one wins over `e`, so a worksheet that
/// uses `i` for something of its own — an index, a current — keeps it by
/// saying so.
fn imaginary_constant(name: &str) -> Option<ComplexQuantity> {
    (name == "i").then(ComplexQuantity::i)
}

fn constant(name: &str) -> Option<f64> {
    Some(match name {
        "pi" | "π" => core::f64::consts::PI,
        "e" => core::f64::consts::E,
        "tau" | "τ" => core::f64::consts::TAU,
        "inf" => f64::INFINITY,
        _ => return None,
    })
}

impl Env {
    pub fn new() -> Env {
        Env {
            vars: BTreeMap::new(),
            failed: BTreeSet::new(),
            hints: BTreeMap::new(),
            funcs: BTreeMap::new(),
            units: UnitTable::new(),
            depth: 0,
            budget: Rc::new(Cell::new(MAX_CALLS)),
            nesting: Rc::new(Cell::new(0)),
        }
    }

    /// Give the next statement its own budget.
    ///
    /// Called once per statement, so that a long worksheet is not refused for
    /// the work its earlier lines did — the ceiling is on one result, not on a
    /// document.
    fn refresh_budget(&mut self) {
        self.budget.set(MAX_CALLS);
        // Balanced increments already leave this at zero between statements.
        // Set it anyway: a leak here would refuse later lines for work an
        // earlier one did, and that failure would be very hard to read.
        self.nesting.set(0);
    }

    pub fn units(&self) -> &UnitTable {
        &self.units
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    pub fn set(&mut self, name: &str, value: Value) {
        self.vars.insert(name.to_string(), value);
    }

    // ---- expressions ---------------------------------------------------

    /// Evaluate an expression, counting how deep the engine's own recursion is.
    ///
    /// The counter, not the match below, is what keeps this off the end of the
    /// stack. Everything recursive routes through here — operands, arguments,
    /// elements, the arms of an `if`, and a called function's body — so one
    /// place counts the whole descent. See [`MAX_EVAL_NEST`] for why counting
    /// calls was not enough.
    pub fn eval(&self, expr: &Expr) -> Trace {
        let outer = self.nesting.get();
        if outer >= MAX_EVAL_NEST {
            return Trace::new(expr.span(), TraceNode::Malformed, Err(EvalError::TooNested));
        }
        self.nesting.set(outer + 1);
        let trace = self.eval_expr(expr);
        // Restored rather than decremented: the value is a depth, and putting
        // back what was there cannot drift however this returned.
        self.nesting.set(outer);
        trace
    }

    fn eval_expr(&self, expr: &Expr) -> Trace {
        match expr {
            Expr::Error { span } => {
                Trace::new(*span, TraceNode::Malformed, Err(EvalError::Malformed))
            }

            Expr::Text { value, span } => {
                Trace::new(*span, TraceNode::Text, Ok(Value::Text(value.clone())))
            }

            Expr::Number { value, span } => Trace::new(
                *span,
                TraceNode::Number,
                Ok(Value::scalar(math::canonicalize(*value))),
            ),

            Expr::Ident(name) => self.eval_ident(&name.text, name.span),

            Expr::Paren { inner, span } => {
                let inner = self.eval(inner);
                let value = clone_or_poison(&inner);
                Trace::new(*span, TraceNode::Paren(Box::new(inner)), value)
            }

            Expr::Unary { op, operand, span } => {
                let operand = self.eval(operand);
                let value = match &operand.value {
                    Err(_) => Err(EvalError::Poisoned),
                    Ok(v) => match op {
                        UnaryOp::Pos => Ok(v.clone()),
                        UnaryOp::Neg => v.neg(),
                        UnaryOp::Not => truth(v).map(|t| truth_value(!t)),
                    },
                };
                Trace::new(
                    *span,
                    TraceNode::Unary {
                        op: *op,
                        operand: Box::new(operand),
                    },
                    value,
                )
            }

            Expr::Binary { op, lhs, rhs, span } => self.eval_binary(*op, lhs, rhs, *span),

            Expr::If {
                cond,
                then,
                otherwise,
                span,
            } => self.eval_conditional(cond, then, otherwise, *span),

            Expr::Vector { elements, span } => {
                let traces: Vec<Trace> = elements.iter().map(|e| self.eval(e)).collect();
                let value = collect_scalars(&traces)
                    .map(|elements| Value::Vector(VectorValue { elements }));
                Trace::new(*span, TraceNode::Vector(traces), value)
            }

            Expr::Matrix { rows, span } => {
                let traces: Vec<Vec<Trace>> = rows
                    .iter()
                    .map(|row| row.iter().map(|e| self.eval(e)).collect())
                    .collect();
                let n_rows = traces.len();
                let n_cols = traces.first().map_or(0, Vec::len);
                let flat: Vec<&Trace> = traces.iter().flatten().collect();
                let value = collect_scalars_ref(&flat)
                    .map(|data| Value::Matrix(MatrixValue::new(n_rows, n_cols, data)));
                Trace::new(*span, TraceNode::Matrix(traces), value)
            }

            Expr::Call { callee, args, span } => self.eval_call(&callee.text, args, *span),

            Expr::Index {
                base,
                indices,
                span,
            } => self.eval_index(base, indices, *span),

            Expr::Convert { value, unit, span } => self.eval_convert(value, unit, *span),
        }
    }

    fn eval_ident(&self, name: &str, span: Span) -> Trace {
        if let Some(v) = self.vars.get(name) {
            return Trace::new(
                span,
                TraceNode::Variable {
                    name: name.into(),
                    unit: self.hints.get(name).cloned(),
                },
                Ok(v.clone()),
            );
        }
        // Checked before constants and units, because a binding takes precedence
        // over both and a failed binding is still a binding.
        if self.failed.contains(name) {
            return Trace::new(
                span,
                TraceNode::Variable {
                    name: name.into(),
                    unit: None,
                },
                Err(EvalError::DefinitionFailed(name.into())),
            );
        }
        if let Some(x) = constant(name) {
            return Trace::new(span, TraceNode::Constant(name.into()), Ok(Value::scalar(x)));
        }
        if let Some(z) = imaginary_constant(name) {
            return Trace::new(
                span,
                TraceNode::Constant(name.into()),
                Ok(Value::Complex(z)),
            );
        }
        match self.units.resolve(name) {
            Ok(u) if u.is_affine() => Trace::new(
                span,
                TraceNode::UnitRef(name.into()),
                // An offset scale has no value of "one" to stand in for.
                Err(EvalError::BareAffineUnit(name.into())),
            ),
            Ok(u) => Trace::new(
                span,
                TraceNode::UnitRef(name.into()),
                Ok(Value::Scalar(Quantity::new(u.factor, u.dim))),
            ),
            Err(_) => Trace::new(
                span,
                TraceNode::Variable {
                    name: name.into(),
                    unit: None,
                },
                Err(EvalError::UnknownName(name.into())),
            ),
        }
    }

    /// The unit an expression was written in, if it wore one plainly.
    ///
    /// Recognises the two shapes that carry an author's intent: a magnitude
    /// applied to a unit (`5 cm`), and an explicit conversion (`... -> dm^3`).
    /// Anything more involved has no single answer, so nothing is recorded and
    /// the renderer falls back to a coherent SI unit.
    fn unit_written_in(&self, expr: &Expr) -> Option<Unit> {
        match expr {
            Expr::Convert { unit, .. } => self.named_unit(unit),
            Expr::Binary { op, rhs, .. } if op.is_mul() => self.named_unit(rhs),
            Expr::Paren { inner, .. } => self.unit_written_in(inner),
            _ => None,
        }
    }

    /// The unit a bare identifier names, if it names one and is not shadowed.
    fn named_unit(&self, expr: &Expr) -> Option<Unit> {
        let Expr::Ident(name) = expr else { return None };
        if self.vars.contains_key(&name.text) || is_constant(&name.text) {
            return None;
        }
        self.units.resolve(&name.text).ok()
    }

    /// The unit a bare identifier names, if it names one and is not shadowed.
    fn affine_unit_named_by(&self, expr: &Expr) -> Option<Unit> {
        let Expr::Ident(name) = expr else { return None };
        if self.vars.contains_key(&name.text) || is_constant(&name.text) {
            return None;
        }
        self.units.resolve(&name.text).ok().filter(Unit::is_affine)
    }

    /// `if c then a else b`, evaluating only the arm that is taken.
    ///
    /// Laziness is not an optimisation here. It is what lets a conditional guard
    /// something that would otherwise fail — `if n > 0 then v[n] else 0 m` must
    /// not index a vector at zero — and a worksheet full of guards that evaluate
    /// anyway would be full of diagnostics about work nobody asked for.
    fn eval_conditional(&self, cond: &Expr, then: &Expr, otherwise: &Expr, span: Span) -> Trace {
        let cond_trace = self.eval(cond);
        let taken = match &cond_trace.value {
            Err(_) => Err(EvalError::Poisoned),
            Ok(v) => truth(v),
        };
        let (then_trace, else_trace, value) = match taken {
            // The condition is unusable, so neither arm runs and neither is
            // blamed: the diagnostic belongs on the condition.
            Err(e) => (sketch(then), sketch(otherwise), Err(e)),
            Ok(true) => {
                let t = self.eval(then);
                let v = clone_or_poison(&t);
                (t, sketch(otherwise), v)
            }
            Ok(false) => {
                let o = self.eval(otherwise);
                let v = clone_or_poison(&o);
                (sketch(then), o, v)
            }
        };
        Trace::new(
            span,
            TraceNode::Conditional {
                cond: Box::new(cond_trace),
                then: Box::new(then_trace),
                otherwise: Box::new(else_trace),
            },
            value,
        )
    }

    /// `and` and `or`, which decide on the left operand where they can.
    ///
    /// `n > 0 and v[n] > 3 m` is the reason: the guard has to actually guard.
    fn eval_logical(&self, op: BinaryOp, lhs: &Expr, rhs: &Expr, span: Span) -> Trace {
        let lhs_trace = self.eval(lhs);
        let left = match &lhs_trace.value {
            Err(_) => Err(EvalError::Poisoned),
            Ok(v) => truth(v),
        };
        let (rhs_trace, value) = match left {
            Err(e) => (sketch(rhs), Err(e)),
            // `false and _` is false; `true or _` is true. Either way the right
            // operand is never looked at.
            Ok(decided) if decided == matches!(op, BinaryOp::Or) => {
                (sketch(rhs), Ok(truth_value(decided)))
            }
            Ok(_) => {
                let r = self.eval(rhs);
                let value = match &r.value {
                    Err(_) => Err(EvalError::Poisoned),
                    Ok(v) => truth(v).map(truth_value),
                };
                (r, value)
            }
        };
        Trace::new(
            span,
            TraceNode::Binary {
                op,
                lhs: Box::new(lhs_trace),
                rhs: Box::new(rhs_trace),
            },
            value,
        )
    }

    fn eval_binary(&self, op: BinaryOp, lhs: &Expr, rhs: &Expr, span: Span) -> Trace {
        // `20°C` reaches here as a multiplication, but it is not one: an offset
        // scale cannot be scaled, so there is nothing to multiply by. Recognise
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return self.eval_logical(op, lhs, rhs, span);
        }

        // the shape and build the quantity directly.
        if op.is_mul() {
            if let Some(unit) = self.affine_unit_named_by(rhs) {
                return self.eval_affine_literal(lhs, &unit, span);
            }
        }

        let lhs = self.eval(lhs);
        let rhs = self.eval(rhs);
        let value = match (&lhs.value, &rhs.value) {
            (Err(_), _) | (_, Err(_)) => Err(EvalError::Poisoned),
            (Ok(a), Ok(b)) => match op {
                BinaryOp::Add => a.add(b),
                BinaryOp::Sub => a.sub(b),
                BinaryOp::Mul | BinaryOp::ImplicitMul => a.mul(b),
                BinaryOp::Div => a.div(b),
                BinaryOp::Pow => a.pow(b),
                BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::Le
                | BinaryOp::Ge
                | BinaryOp::Equal
                | BinaryOp::NotEqual => compare(op, a, b),
                // Handled before this point, because their right operand may
                // never be evaluated at all.
                BinaryOp::And | BinaryOp::Or => unreachable!("short-circuited above"),
            },
        };
        Trace::new(
            span,
            TraceNode::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            value,
        )
    }

    fn eval_affine_literal(&self, magnitude: &Expr, unit: &Unit, span: Span) -> Trace {
        let mag = self.eval(magnitude);
        let value = match &mag.value {
            Err(_) => Err(EvalError::Poisoned),
            Ok(Value::Scalar(q)) if q.is_dimensionless() && !q.is_point() => {
                Ok(Value::Scalar(Quantity::from_unit(q.value, unit)))
            }
            Ok(other) => Err(EvalError::TypeMismatch {
                op: "an offset scale",
                lhs: other.type_name(),
                rhs: "a plain number",
            }),
        };
        let magnitude_value = mag.value.as_ref().ok().and_then(Value::as_scalar);
        Trace::new(
            span,
            TraceNode::AffineLiteral {
                magnitude: magnitude_value.map_or(f64::NAN, |q| q.value),
                unit: unit.symbol.clone(),
            },
            value,
        )
    }

    /// Builtins whose first argument is the *name* of a function rather than a
    /// value.
    ///
    /// This is as close to a higher-order function as the language gets, and
    /// deliberately: there are no lambdas, no closures and no function values,
    /// only these two builtins accepting a name that is already defined. That is
    /// enough for what worksheets actually do with loops — see the module docs —
    /// and it keeps `Expr::Call`'s callee a name, which the whole evaluator
    /// assumes.
    const HIGHER_ORDER: &'static [&'static str] = &[
        "map",
        "iterate",
        "root",
        "roots",
        "integral",
        "plot",
        "derivative",
        "solve_linear",
    ];

    fn eval_call(&self, name: &str, args: &[Expr], span: Span) -> Trace {
        if Self::HIGHER_ORDER.contains(&name) && !args.is_empty() {
            return self.eval_higher_order(name, args, span);
        }
        let traces: Vec<Trace> = args.iter().map(|a| self.eval(a)).collect();
        if traces.iter().any(|t| t.value.is_err()) {
            return Trace::new(
                span,
                TraceNode::Call {
                    name: name.into(),
                    args: traces,
                },
                Err(EvalError::Poisoned),
            );
        }
        let values: Vec<Value> = traces
            .iter()
            .map(|t| t.value.clone().expect("checked above"))
            .collect();

        let value = if let Some(f) = self.funcs.get(name) {
            self.call_user_function(name, f, &values)
        } else {
            self.call_builtin(name, &values)
        };

        Trace::new(
            span,
            TraceNode::Call {
                name: name.into(),
                args: traces,
            },
            value,
        )
    }

    fn call_user_function(
        &self,
        name: &str,
        f: &Function,
        args: &[Value],
    ) -> Result<Value, EvalError> {
        if f.params.len() != args.len() {
            return Err(EvalError::WrongArity {
                name: name.into(),
                expected: f.params.len(),
                found: args.len(),
            });
        }
        // A definition that never reaches its base case would otherwise run the
        // stack out and abort the process. Checked before the frame is built
        // rather than inside it, so the error names the call that was refused.
        if self.depth >= MAX_DEPTH {
            return Err(EvalError::TooDeep(name.into()));
        }
        // Depth alone does not bound the work: `fn f(x) = f(x) + f(x)` never
        // nests deeper than the ceiling and still calls itself 2^64 times.
        // A budget the whole statement shares is what makes evaluation finish.
        match self.budget.get() {
            0 => return Err(EvalError::TooDeep(name.into())),
            left => self.budget.set(left - 1),
        }
        // Arguments bind in a child scope; the body sees the definition site's
        // bindings plus its own parameters.
        let mut inner = Env {
            vars: self.vars.clone(),
            failed: self.failed.clone(),
            hints: self.hints.clone(),
            funcs: self.funcs.clone(),
            units: self.units.clone(),
            depth: self.depth + 1,
            budget: Rc::clone(&self.budget),
            nesting: Rc::clone(&self.nesting),
        };
        for (p, a) in f.params.iter().zip(args) {
            inner.vars.insert(p.clone(), a.clone());
            // A parameter is a real binding and outranks a failed one of the
            // same name from the definition site.
            inner.failed.remove(p);
        }
        inner.eval(&f.body).value
    }

    /// How many leading arguments name a function rather than carry a value.
    ///
    /// One everywhere but `plot`, which takes a curve per name and then the two
    /// ends of the span: `plot(gain, loss, 30 kHz, 200 kHz)`. Counted from the
    /// end rather than by sniffing which of the names happen to be defined
    /// functions, so that what a call means does not depend on what is in
    /// scope: `plot(f, a, b)` reads the same whether or not something called
    /// `a` exists.
    ///
    /// The same arithmetic answers zero for `plot(measured)`, which is a plot
    /// of a table and takes no names at all — the two kinds of plot are told
    /// apart by whether a span was written, and never by looking at what a name
    /// happens to hold.
    fn named_arity(name: &str, argc: usize) -> usize {
        match name {
            // A plot of one or two arguments has no span in it, so it is a plot
            // of tables: every argument is a value and none is a name.
            "plot" if argc < 3 => 0,
            "plot" => argc - 2,
            _ => 1,
        }
    }

    /// `map(f, v)`, `iterate(f, x, n)`, and `plot(f, g, …, a, b)`.
    ///
    /// A leading argument is read as a name and never evaluated: `f` on its own
    /// is not a value in this language, so evaluating it would report an unknown
    /// name for a function that exists.
    fn eval_higher_order(&self, name: &str, args: &[Expr], span: Span) -> Trace {
        let named = Self::named_arity(name, args.len());
        let mut callees: Vec<&str> = Vec::with_capacity(named);
        let mut traces: Vec<Trace> = Vec::with_capacity(args.len());
        for arg in &args[..named] {
            let Expr::Ident(callee) = arg else {
                return Trace::new(
                    span,
                    TraceNode::Call {
                        name: name.into(),
                        args: args.iter().map(sketch).collect(),
                    },
                    Err(EvalError::Singular(if named == 1 {
                        "the first argument names a function, so it must be a plain name"
                    } else {
                        "a plot's curves are named functions, so every argument \
                         before the span must be a plain name"
                    })),
                );
            };
            // The named function is recorded as a trace leaf so the rendered
            // line still reads `map(step, xs)` rather than losing the name.
            traces.push(Trace::new(
                callee.span,
                TraceNode::FnRef(callee.text.clone()),
                // Marked as not evaluated, which it was not: `Trace::children`
                // then keeps it out of error search, and nothing tries to print
                // a value for a name that has none.
                Err(EvalError::NotTaken),
            ));
            callees.push(&callee.text);
        }
        traces.extend(args[named..].iter().map(|a| self.eval(a)));

        let value = if traces[named..].iter().any(|t| t.value.is_err()) {
            Err(EvalError::Poisoned)
        } else {
            let rest: Vec<Value> = traces[named..]
                .iter()
                .map(|t| t.value.clone().expect("checked above"))
                .collect();
            // What the worksheet called each value argument, where it called it
            // anything: `plot(measured, model)` should say so in its legend
            // rather than "table 1, table 2". Only a label — the argument was
            // evaluated as a value like any other, and nothing about what the
            // call *means* depends on this.
            let labels: Vec<Option<&str>> = args[named..]
                .iter()
                .map(|a| match a {
                    Expr::Ident(name) => Some(name.text.as_str()),
                    _ => None,
                })
                .collect();
            self.apply_higher_order(name, &callees, &rest, &labels)
        };
        Trace::new(
            span,
            TraceNode::Call {
                name: name.into(),
                args: traces,
            },
            value,
        )
    }

    fn apply_higher_order(
        &self,
        name: &str,
        callees: &[&str],
        args: &[Value],
        labels: &[Option<&str>],
    ) -> Result<Value, EvalError> {
        let apply = |callee: &str, x: &Value| -> Result<Value, EvalError> {
            match self.funcs.get(callee) {
                Some(f) => self.call_user_function(callee, f, std::slice::from_ref(x)),
                None if BUILTINS.contains(&callee) => {
                    self.call_builtin(callee, std::slice::from_ref(x))
                }
                None => Err(EvalError::UnknownFunction(callee.into())),
            }
        };
        // Everything but `plot` takes exactly one name, so it applies the first
        // and there is no second to apply.
        let call_one = |x: &Value| apply(callees[0], x);
        // The same, for the one function here whose callee takes several
        // arguments at once: a system of equations in several unknowns.
        let call_all = |xs: &[Value]| -> Result<Value, EvalError> {
            match self.funcs.get(callees[0]) {
                Some(f) => self.call_user_function(callees[0], f, xs),
                None if BUILTINS.contains(&callees[0]) => self.call_builtin(callees[0], xs),
                None => Err(EvalError::UnknownFunction(callees[0].into())),
            }
        };

        match (name, args) {
            ("map", [Value::Vector(v)]) => {
                let mut out = Vec::with_capacity(v.elements.len());
                for e in &v.elements {
                    match call_one(&Value::Scalar(*e))? {
                        Value::Scalar(q) => out.push(q),
                        other => {
                            return Err(EvalError::ShapeMismatch {
                                op: "map",
                                lhs: other.shape_name(),
                                rhs: String::from("a scalar"),
                            })
                        }
                    }
                }
                Ok(Value::Vector(VectorValue { elements: out }))
            }
            ("map", [other]) => Err(EvalError::ShapeMismatch {
                op: "map",
                lhs: other.shape_name(),
                rhs: String::from("a vector"),
            }),
            // A root of `f` bracketed by `a` and `b`, by bisection.
            //
            // Bisection rather than Newton, for the same reason `iterate` takes
            // a count rather than a tolerance: it needs no derivative, it cannot
            // diverge, it takes the same number of steps on every machine, and
            // every step is a comparison and a midpoint — so the answer is
            // bit-identical wherever it runs. The cost is that it needs a
            // bracket, which is what the worksheets supply anyway: SMath's
            // `solve` is always given one.
            ("root", [a, b]) => {
                let (Some(mut lo), Some(mut hi)) = (a.as_scalar(), b.as_scalar()) else {
                    return Err(EvalError::ShapeMismatch {
                        op: "root",
                        lhs: a.shape_name(),
                        rhs: String::from("a scalar"),
                    });
                };
                if lo.dim != hi.dim {
                    return Err(crate::unit::UnitError::DimensionMismatch {
                        lhs: lo.dim,
                        rhs: hi.dim,
                    }
                    .into());
                }
                if lo.value > hi.value {
                    core::mem::swap(&mut lo, &mut hi);
                }
                let at = |x: Quantity| -> Result<Quantity, EvalError> {
                    match call_one(&Value::Scalar(x))? {
                        Value::Scalar(q) => Ok(q),
                        other => Err(EvalError::ShapeMismatch {
                            op: "root",
                            lhs: other.shape_name(),
                            rhs: String::from("a scalar"),
                        }),
                    }
                };
                let (mut f_lo, f_hi) = (at(lo)?, at(hi)?);
                if f_lo.dim != f_hi.dim {
                    return Err(crate::unit::UnitError::DimensionMismatch {
                        lhs: f_lo.dim,
                        rhs: f_hi.dim,
                    }
                    .into());
                }
                // An endpoint that is already the root is the answer, and saying
                // so is better than halving 100 times towards it.
                if f_lo.value == 0.0 {
                    return Ok(Value::Scalar(lo));
                }
                if f_hi.value == 0.0 {
                    return Ok(Value::Scalar(hi));
                }
                // Same sign at both ends means the bracket does not contain an
                // odd number of roots, and bisection would return a confident
                // wrong answer. Refuse instead — a silent wrong root in a
                // structural calculation is the worst outcome available.
                if (f_lo.value > 0.0) == (f_hi.value > 0.0) {
                    return Err(EvalError::Singular(
                        "root needs the function to change sign between the two ends",
                    ));
                }
                // 100 halvings exhausts a binary64 interval whatever its
                // exponent, so this is "until there is nothing left to halve"
                // written as a count that provably terminates.
                for _ in 0..100 {
                    let mid = Quantity {
                        value: (lo.value + hi.value) / 2.0,
                        ..lo
                    };
                    // Exact comparison on purpose: the midpoint coinciding with
                    // an endpoint *is* the definition of an interval with
                    // nothing left to halve, and a tolerance here would be a
                    // second, machine-dependent stopping rule next to the
                    // machine-independent one.
                    #[allow(clippy::float_cmp)]
                    let exhausted = mid.value == lo.value || mid.value == hi.value;
                    if exhausted {
                        break;
                    }
                    let f_mid = at(mid)?;
                    if f_mid.value == 0.0 {
                        return Ok(Value::Scalar(mid));
                    }
                    if (f_mid.value > 0.0) == (f_lo.value > 0.0) {
                        lo = mid;
                        f_lo = f_mid;
                    } else {
                        hi = mid;
                    }
                }
                Ok(Value::Scalar(Quantity {
                    value: (lo.value + hi.value) / 2.0,
                    ..lo
                }))
            }

            // `solve_linear(f, kinds)`: the unknowns that make a **linear**
            // system of equations balance.
            //
            // # A linear system needs no algebra
            //
            // This is the shape most of the SMath corpus's symbolic solves have
            // — statics, where ΣF = 0 and ΣM = 0 are solved for the reactions —
            // and it looked like it needed a computer algebra system because
            // that is how SMath does it. It does not. A system that is linear in
            // its unknowns *is* its coefficients, and those come out of
            // evaluating the residual: `b = −r(0)`, and column `j` of the matrix
            // is `r(eⱼ) − r(0)`. No expression is manipulated and no derivative
            // is needed — the Jacobian of an affine map is recoverable by
            // subtraction (design note §8.34).
            //
            // # `f` is a residual, and `kinds` says what the unknowns are
            //
            // `f` takes the unknowns and answers with what is left over when
            // they are substituted: zero when they are right. `kinds` is a
            // vector whose **dimensions** name each unknown — a vector of
            // forces for a statics problem. Its magnitudes are never read: the
            // engine needs to know that the first unknown is a force, not what
            // force it might be.
            //
            // # Dimensions come off and go back on exactly
            //
            // A moment equation beside two force equations makes the
            // coefficients dimensionally mixed by row, which `inv` refuses and
            // is right to. So the matrix is solved in magnitudes and the
            // dimensions are reattached from `kinds` — and that costs nothing,
            // because everything here is in base SI, so scaling by one unit of
            // each dimension multiplies by exactly 1.0 and changes no bits. The
            // dimensional check is not lost either: it is done by the residual
            // subtractions, which refuse to take a force from a moment.
            //
            // # Linearity is checked on the answer, not assumed
            //
            // The coefficients are only the system's coefficients if the system
            // is linear, so the answer is put back into the equations: a linear
            // system leaves nothing, and anything else leaves a residual. That
            // is a stronger statement than a property test — it says *this is
            // the solution* rather than *this looked affine at three points* —
            // and it is what lets a caller who cannot verify linearity, such as
            // the importer reading somebody else's worksheet, use this without
            // guessing.
            ("solve_linear", [kinds]) => {
                let dims: Vec<crate::dim::Dimension> = match kinds {
                    Value::Scalar(q) => vec![q.dim],
                    Value::Vector(v) => v.elements.iter().map(|q| q.dim).collect(),
                    other => {
                        return Err(EvalError::ShapeMismatch {
                            op: "solve_linear",
                            lhs: other.shape_name(),
                            rhs: String::from("a vector naming each unknown's dimension"),
                        })
                    }
                };
                let n = dims.len();
                let at = |args: &[Value]| -> Result<Vec<Quantity>, EvalError> {
                    let out = call_all(args)?;
                    let elements = match &out {
                        Value::Scalar(q) => vec![*q],
                        Value::Vector(v) => v.elements.clone(),
                        other => {
                            return Err(EvalError::ShapeMismatch {
                                op: "solve_linear",
                                lhs: other.shape_name(),
                                rhs: format!("{n} residual(s)"),
                            })
                        }
                    };
                    if elements.len() != n {
                        return Err(EvalError::ShapeMismatch {
                            op: "solve_linear",
                            lhs: format!("{} residual(s)", elements.len()),
                            rhs: format!("{n}, one per unknown"),
                        });
                    }
                    Ok(elements)
                };
                let zeros: Vec<Value> = dims
                    .iter()
                    .map(|d| Value::Scalar(Quantity::new(0.0, *d)))
                    .collect();
                let r0 = at(&zeros)?;
                // Column by column, one unit of one unknown at a time.
                let mut columns: Vec<Vec<f64>> = Vec::with_capacity(n);
                for (j, d) in dims.iter().enumerate() {
                    let mut probe = zeros.clone();
                    probe[j] = Value::Scalar(Quantity::new(1.0, *d));
                    let rj = at(&probe)?;
                    let mut column = Vec::with_capacity(n);
                    for (i, r) in rj.iter().enumerate() {
                        // The subtraction is where dimensional coherence is
                        // checked: a residual that changed dimension between two
                        // evaluations is not a linear system, and says so.
                        column.push(r.sub(&r0[i])?.value);
                    }
                    columns.push(column);
                }
                let mut data = Vec::with_capacity(n * n);
                for i in 0..n {
                    for column in &columns {
                        data.push(Quantity::scalar(column[i]));
                    }
                }
                let b = Value::Vector(VectorValue {
                    elements: r0.iter().map(|q| Quantity::scalar(-q.value)).collect(),
                });
                // One unknown is one division, and `reshape` would have made the
                // 1×1 matrix into the scalar it is — which `inv` refuses, and is
                // right to. Taken first rather than worked around.
                let magnitudes: Vec<Quantity> = if n == 1 {
                    // One unknown is one division, and `reshape` would have made
                    // the 1×1 matrix into the scalar it is — which `inv` refuses,
                    // and is right to. Taken separately rather than worked
                    // around, and it rejoins the verification below.
                    #[allow(clippy::float_cmp)]
                    let singular = columns[0][0] == 0.0;
                    if singular {
                        return Err(EvalError::Singular(
                            "these equations do not determine their unknowns",
                        ));
                    }
                    vec![Quantity::scalar(-r0[0].value / columns[0][0])]
                } else {
                    let matrix = reshape(n, n, data);
                    // `inv` refuses a singular matrix, which is the honest
                    // answer when the equations do not determine the unknowns.
                    match matrix.inv()?.mul(&b)? {
                        Value::Vector(v) => v.elements,
                        other => {
                            return Err(EvalError::ShapeMismatch {
                                op: "solve_linear",
                                lhs: other.shape_name(),
                                rhs: format!("{n} unknown(s)"),
                            })
                        }
                    }
                };
                let answer: Vec<Quantity> = magnitudes
                    .iter()
                    .zip(&dims)
                    .map(|(q, d)| Quantity::new(q.value, *d))
                    .collect();
                // Put it back. Each row is judged against its own scale — the
                // largest term that went into it — because an equation in
                // meganewtons and one in millimetres cannot share an absolute
                // yardstick. One part in 10⁹ is far above the rounding of a
                // handful of multiplications and far below any nonlinearity
                // that would change an engineering answer.
                const SATISFIED: f64 = 1e-9;
                let check = at(&answer.iter().map(|q| Value::Scalar(*q)).collect::<Vec<_>>())?;
                for (i, left) in check.iter().enumerate() {
                    let mut scale = r0[i].value.abs();
                    for (j, column) in columns.iter().enumerate() {
                        scale = scale.max((column[i] * answer[j].value).abs());
                    }
                    if left.value.abs() > scale * SATISFIED {
                        return Err(EvalError::Singular(
                            "the answer does not satisfy these equations, so they are not \
                             linear in their unknowns",
                        ));
                    }
                }
                if n == 1 {
                    Ok(Value::Scalar(answer[0]))
                } else {
                    Ok(Value::Vector(VectorValue { elements: answer }))
                }
            }

            // `derivative(f, x)`: the slope of `f` at `x`, exactly.
            //
            // # It is a number, not an expression
            //
            // SMath's `diff` is symbolic: it hands the expression to a computer
            // algebra system and gets another expression back. This is the
            // other thing a worksheet might mean by a derivative, and by far
            // the commoner one — the *value* at a point, so that a root search
            // or a plot can have it. No algebra is involved: the parameter is
            // seeded with a slope of one and the arithmetic carries the chain
            // rule alongside every value it was already computing (see
            // [`crate::dual`]). There is no step size, so there is nothing to
            // tune and no truncation error to trade against cancellation: the
            // slope comes out correct to the same rounding as the value.
            //
            // # The dimension is arithmetic too
            //
            // `d(f)/d(x)` has dimension `f/x` — the derivative of a gain with
            // respect to a frequency is a per-hertz — and that falls out here
            // rather than being a rule about differentiation.
            //
            // A function that never reads its argument has slope zero, and says
            // so rather than refusing: a constant is a perfectly good function
            // to differentiate, and `0` is the answer.
            ("derivative", [a]) | ("derivative", [a, _]) => {
                // A second derivative needs a dual of a dual, and nothing here
                // nests. Named rather than left to the shape mismatch, because
                // "a number being differentiated is not a scalar" is true and
                // tells a reader nothing about what to do.
                if matches!(a, Value::Dual(_)) {
                    return Err(EvalError::NotImplemented("a derivative of a derivative"));
                }
                let Some(at) = a.as_scalar() else {
                    return Err(EvalError::ShapeMismatch {
                        op: "derivative",
                        lhs: a.shape_name(),
                        rhs: String::from("a scalar"),
                    });
                };
                // `derivative(f, x, n)`: the nth derivative, and `n` is 1 or 2.
                // Higher would need a third component on the dual and a third
                // column in every rule; both are ordinary work and neither is
                // written, so the ceiling is a number rather than a surprise.
                let order = match &args[1..] {
                    [] => 1usize,
                    [n] => match whole_count("derivative", n)? {
                        order @ (1 | 2) => order,
                        _ => {
                            return Err(EvalError::Singular(
                                "a derivative of order above the second is not implemented yet",
                            ))
                        }
                    },
                    _ => unreachable!("arity is checked before evaluation"),
                };
                let seeded = Value::Dual(DualQuantity::seed(at));
                // `d(f)/d(x)ⁿ` has dimension `f/xⁿ`, which falls out of dividing
                // by the variable's dimension once per order.
                let slope = |value: Quantity, d: f64| {
                    let mut dim = value.dim;
                    for _ in 0..order {
                        dim = dim.div(&at.dim);
                    }
                    Ok(Value::Scalar(Quantity::new(d, dim)))
                };
                match call_one(&seeded)? {
                    Value::Dual(out) => {
                        let d = if order == 1 { out.d } else { out.dd };
                        slope(out.value, d)
                    }
                    Value::Scalar(q) => slope(q, 0.0),
                    other => Err(EvalError::ShapeMismatch {
                        op: "derivative",
                        lhs: other.shape_name(),
                        rhs: String::from("a scalar"),
                    }),
                }
            }

            // `roots(f, a, b)`: every root of `f` between `a` and `b` that a
            // fixed scan can see. One value when there is one, a vector in
            // increasing order when there are several, an error when there are
            // none.
            //
            // # Why this exists beside `root`
            //
            // `root` asks the worksheet to bracket the answer, and refuses when
            // the two ends do not straddle it. That is the right primitive when
            // the author knows where the answer is and wants to be told when
            // they are wrong. It is the wrong one for the question engineers
            // actually ask a window — *what does this cross zero at, between
            // here and here* — where there may be two answers, or none, and the
            // author does not know which.
            //
            // This is also the shape SMath's `solve(expr, x, a, b)` has, and
            // reading its implementation is what settled that the two limits are
            // a search range rather than a bracket (design note §8.24). A
            // faithful import needs a function that searches; that is a reason
            // for the feature to exist, not a licence to copy an algorithm, and
            // the three departures below are all in the direction of an answer
            // that cannot depend on anything but the arithmetic.
            //
            // # Fixed counts, no tolerances
            //
            // 200 intervals across the range, both ends included, and every
            // sign change between neighbours bisected to exhaustion. The scan
            // count is a count for the reason every other limit here is: it
            // decides *which* roots are found, so a machine that scanned a
            // different number of points would answer a different question.
            // Nodes are computed as `a + i·(b−a)/n` rather than by repeated
            // addition, for the reason `range` and `plot` compute theirs that
            // way.
            //
            // What a scan cannot see, it does not claim: two roots inside one
            // interval cancel each other's sign change and are missed. That is
            // a property of the method rather than a bug, it is why the count
            // is documented, and it is why `root` is still here for the case
            // where the author can bracket the answer themselves.
            ("roots", [a, b]) => {
                let (Some(mut lo), Some(mut hi)) = (a.as_scalar(), b.as_scalar()) else {
                    return Err(EvalError::ShapeMismatch {
                        op: "roots",
                        lhs: a.shape_name(),
                        rhs: String::from("a scalar"),
                    });
                };
                if lo.dim != hi.dim {
                    return Err(crate::unit::UnitError::DimensionMismatch {
                        lhs: lo.dim,
                        rhs: hi.dim,
                    }
                    .into());
                }
                if lo.value > hi.value {
                    core::mem::swap(&mut lo, &mut hi);
                }
                // A window of no width holds no scan, and sampling it 201 times
                // would put every sample in one place. Exact, because "are these
                // the same point" is a question about the bits.
                #[allow(clippy::float_cmp)]
                let empty = lo.value == hi.value;
                if empty {
                    return Err(EvalError::Singular(
                        "roots needs two different ends to search between",
                    ));
                }
                let mut at = |x: Quantity| -> Result<Quantity, EvalError> {
                    match call_one(&Value::Scalar(x))? {
                        Value::Scalar(q) => Ok(q),
                        other => Err(EvalError::ShapeMismatch {
                            op: "roots",
                            lhs: other.shape_name(),
                            rhs: String::from("a scalar"),
                        }),
                    }
                };
                const SCAN: usize = 200;
                let width = hi.value - lo.value;
                let mut found: Vec<Quantity> = Vec::new();
                let mut y_dim: Option<crate::dim::Dimension> = None;
                // The previous sample, when it was a finite value with a sign.
                // `None` breaks the chain: a sample that came back NaN or
                // infinite says nothing about which side of zero the function
                // was on, so no bracket is drawn across it.
                let mut previous: Option<(Quantity, f64)> = None;
                for i in 0..=SCAN {
                    let x = Quantity {
                        value: lo.value + (i as f64) * width / (SCAN as f64),
                        ..lo
                    };
                    let y = at(x)?;
                    // One dimension down the whole scan, for `plot`'s reason: a
                    // function that answers in metres here and seconds there has
                    // no zero to look for.
                    match y_dim {
                        None => y_dim = Some(y.dim),
                        Some(d) if d != y.dim => {
                            return Err(crate::unit::UnitError::DimensionMismatch {
                                lhs: d,
                                rhs: y.dim,
                            }
                            .into())
                        }
                        Some(_) => {}
                    }
                    if !y.value.is_finite() {
                        previous = None;
                        continue;
                    }
                    #[allow(clippy::float_cmp)]
                    let exact = y.value == 0.0;
                    if exact {
                        push_root(&mut found, x);
                        // The neighbouring interval is not bracketed against a
                        // sample that is already the answer: it would bisect
                        // straight back to this point.
                        previous = None;
                        continue;
                    }
                    if let Some((px, py)) = previous {
                        if (py > 0.0) != (y.value > 0.0) {
                            let r = bisect(px, py, x, &mut at)?;
                            push_root(&mut found, r);
                        }
                    }
                    previous = Some((x, y.value));
                }
                match found.len() {
                    0 => Err(EvalError::Singular(
                        "roots found no sign change anywhere in the range",
                    )),
                    1 => Ok(Value::Scalar(found[0])),
                    _ => Ok(Value::Vector(VectorValue { elements: found })),
                }
            }

            // The definite integral of `f` from `a` to `b`, by composite
            // Simpson's rule over a fixed number of panels.
            //
            // Fixed rather than adaptive, again for reproducibility: an adaptive
            // scheme's subdivision depends on comparisons against a tolerance,
            // and two builds that disagree in the last bit would then disagree
            // in how many panels they used and by much more than a bit. 1024
            // panels is exact for anything up to a cubic and lands near the
            // limit of binary64 for the smooth functions worksheets integrate.
            //
            // The dimension falls out of the arithmetic — `f(x)·dx` — so
            // integrating a load in kN/m over metres gives kN with no rule about
            // integration needed.
            ("integral", [a, b]) => {
                let (Some(lo), Some(hi)) = (a.as_scalar(), b.as_scalar()) else {
                    return Err(EvalError::ShapeMismatch {
                        op: "integral",
                        lhs: a.shape_name(),
                        rhs: String::from("a scalar"),
                    });
                };
                if lo.dim != hi.dim {
                    return Err(crate::unit::UnitError::DimensionMismatch {
                        lhs: lo.dim,
                        rhs: hi.dim,
                    }
                    .into());
                }
                const PANELS: usize = 1024;
                let at = |x: Quantity| -> Result<Quantity, EvalError> {
                    match call_one(&Value::Scalar(x))? {
                        Value::Scalar(q) => Ok(q),
                        other => Err(EvalError::ShapeMismatch {
                            op: "integral",
                            lhs: other.shape_name(),
                            rhs: String::from("a scalar"),
                        }),
                    }
                };
                let span = hi.value - lo.value;
                let h = span / PANELS as f64;
                // Nodes as `lo + i*h`, never by repeated addition — the same
                // rule `range` follows, and for the same reason.
                let node = |i: usize| Quantity {
                    value: lo.value + (i as f64) * h,
                    ..lo
                };
                let mut acc = at(node(0))?;
                let last = at(node(PANELS))?;
                acc = acc.add(&last)?;
                // Ends, then every odd node at weight 4, then every even one at
                // 2. Summing in a fixed order by weight keeps the result
                // reproducible and keeps the two large groups from being
                // interleaved, which costs accuracy for nothing.
                let mut odd = Quantity { value: 0.0, ..acc };
                for i in (1..PANELS).step_by(2) {
                    odd = odd.add(&at(node(i))?)?;
                }
                let mut even = Quantity { value: 0.0, ..acc };
                for i in (2..PANELS).step_by(2) {
                    even = even.add(&at(node(i))?)?;
                }
                acc = acc.add(&odd.mul(&Quantity::scalar(4.0))?)?;
                acc = acc.add(&even.mul(&Quantity::scalar(2.0))?)?;
                let step = Quantity {
                    value: h / 3.0,
                    ..lo
                };
                Ok(Value::Scalar(acc.mul(&step)?))
            }

            // `plot(m)`, or `plot(m, n)`: a plot of tables. An n×2 matrix of
            // measured points, x in the first column and y in the second —
            // which is the shape `augment(x, y)` builds, and the shape the
            // SMath corpus plots.
            //
            // No span is written and none is needed: the points brought their
            // own x, and the axis is fitted to them.
            // The guard, not the shape, is what tells the two kinds apart: a
            // span is two values as well, and `named_arity` has already decided
            // which this call is by whether one was written.
            ("plot", tables @ ([_] | [_, _])) if callees.is_empty() => {
                let mut drawn = crate::plot::PlotValue {
                    from: 0.0,
                    to: 0.0,
                    extent: crate::plot::Extent::Measured,
                    x_dim: crate::dim::Dimension::DIMENSIONLESS,
                    y_dim: crate::dim::Dimension::DIMENSIONLESS,
                    series: Vec::new(),
                };
                let mut dims: Option<(crate::dim::Dimension, crate::dim::Dimension)> = None;
                for (i, table) in tables.iter().enumerate() {
                    let Value::Matrix(m) = table else {
                        return Err(EvalError::ShapeMismatch {
                            op: "plot",
                            lhs: table.shape_name(),
                            rhs: String::from("a table of points, two columns wide"),
                        });
                    };
                    if m.cols != 2 {
                        return Err(EvalError::ShapeMismatch {
                            op: "plot",
                            lhs: format!("a {}×{} matrix", m.rows, m.cols),
                            rhs: String::from("a table of points, two columns wide"),
                        });
                    }
                    let mut points = Vec::with_capacity(m.rows);
                    for r in 0..m.rows {
                        let (x, y) = (m.get(r, 0), m.get(r, 1));
                        // One axis, one dimension — along a column and across
                        // the tables alike. A table whose x is a time under one
                        // whose x is a length has no chart.
                        match dims {
                            None => dims = Some((x.dim, y.dim)),
                            Some((dx, dy)) => {
                                if dx != x.dim {
                                    return Err(crate::unit::UnitError::DimensionMismatch {
                                        lhs: dx,
                                        rhs: x.dim,
                                    }
                                    .into());
                                }
                                if dy != y.dim {
                                    return Err(crate::unit::UnitError::DimensionMismatch {
                                        lhs: dy,
                                        rhs: y.dim,
                                    }
                                    .into());
                                }
                            }
                        }
                        points.push((x.value, y.value));
                    }
                    drawn.series.push(crate::plot::Series {
                        // What the worksheet called it, or which argument it
                        // was when it was written out in place. One table needs
                        // no legend at all.
                        name: match labels.get(i).copied().flatten() {
                            Some(label) => label.to_string(),
                            None => format!("table {}", i + 1),
                        },
                        points,
                    });
                }
                if let Some((x_dim, y_dim)) = dims {
                    drawn.x_dim = x_dim;
                    drawn.y_dim = y_dim;
                }
                // The extent of the data, which is the only span a table has.
                // A table with nothing finite in it has none, and the renderer
                // says so where the picture would have been rather than drawing
                // an axis from an invented number.
                if let Some((lo, hi)) = drawn.x_range() {
                    drawn.from = lo;
                    drawn.to = hi;
                }
                Ok(Value::Plot(Box::new(drawn)))
            }

            // `plot(f, a, b)`, or `plot(f, g, …, a, b)`: sample each named
            // function across the span and hand the samples to the renderer. A
            // fixed number of points, for `integral`'s reason — a fixed amount
            // of work rather than a tolerance test, so the drawing terminates
            // and is the same drawing everywhere.
            ("plot", [a, b]) => {
                let (Some(from), Some(to)) = (a.as_scalar(), b.as_scalar()) else {
                    return Err(EvalError::ShapeMismatch {
                        op: "plot",
                        lhs: a.shape_name(),
                        rhs: String::from("a scalar"),
                    });
                };
                if from.dim != to.dim {
                    return Err(crate::unit::UnitError::DimensionMismatch {
                        lhs: from.dim,
                        rhs: to.dim,
                    }
                    .into());
                }
                // A span of no width has no chart in it, and dividing by the
                // step would put every sample in one place. Exact, because
                // "are these the same point" is a question about the bits.
                #[allow(clippy::float_cmp)]
                let empty = from.value == to.value;
                if empty {
                    return Err(EvalError::Singular(
                        "plot needs two different ends to draw between",
                    ));
                }
                let mut sampled = crate::plot::PlotValue {
                    from: from.value,
                    to: to.value,
                    // The worksheet named both ends, so the axis will be
                    // exactly them rather than something rounded outwards.
                    extent: crate::plot::Extent::Chosen,
                    x_dim: from.dim,
                    y_dim: from.dim,
                    series: Vec::new(),
                };
                // One vertical axis for the whole plot, so one dimension —
                // across the curves as well as along each. A gain beside a
                // frequency has no chart, and drawing it anyway would put two
                // meanings on one axis; this is where the plot value's promise
                // that its series share `y_dim` is kept.
                let mut y_dim = None;
                for callee in callees {
                    let mut points = Vec::with_capacity(crate::plot::SAMPLES);
                    for i in 0..crate::plot::SAMPLES {
                        let x = Quantity {
                            value: sampled.x_at(i),
                            ..from
                        };
                        let y = match apply(callee, &Value::Scalar(x))? {
                            Value::Scalar(q) => q,
                            other => {
                                return Err(EvalError::ShapeMismatch {
                                    op: "plot",
                                    lhs: other.shape_name(),
                                    rhs: String::from("a scalar"),
                                })
                            }
                        };
                        match y_dim {
                            None => y_dim = Some(y.dim),
                            Some(d) if d != y.dim => {
                                return Err(crate::unit::UnitError::DimensionMismatch {
                                    lhs: d,
                                    rhs: y.dim,
                                }
                                .into())
                            }
                            Some(_) => {}
                        }
                        points.push((x.value, y.value));
                    }
                    sampled.series.push(crate::plot::Series {
                        name: (*callee).to_string(),
                        points,
                    });
                }
                sampled.y_dim = y_dim.unwrap_or(from.dim);
                Ok(Value::Plot(Box::new(sampled)))
            }

            ("iterate", [start, n]) => {
                let count = whole_count("iterate", n)?;
                let mut x = start.clone();
                // Left to right, one application at a time. Reduction order is
                // part of the language (design note §3), and an iteration is the
                // place where reassociating would be most tempting and most
                // visible in the last bits.
                for _ in 0..count {
                    x = call_one(&x)?;
                }
                Ok(x)
            }
            _ => Err(EvalError::WrongArity {
                name: name.into(),
                expected: if name == "map" { 2 } else { 3 },
                // The names were taken off the front before the values got
                // here, so what the worksheet wrote is both counts together.
                found: args.len() + callees.len(),
            }),
        }
    }

    fn call_builtin(&self, name: &str, args: &[Value]) -> Result<Value, EvalError> {
        let arity = |n: usize| -> Result<(), EvalError> {
            if args.len() == n {
                Ok(())
            } else {
                Err(EvalError::WrongArity {
                    name: name.into(),
                    expected: n,
                    found: args.len(),
                })
            }
        };

        // Element-wise over a dimensionless argument: the trigonometric and
        // exponential family. `rad` is dimensionless, so `sin(30°)` works.
        let unary_dimensionless = |f: fn(f64) -> f64| -> Result<Value, EvalError> {
            arity(1)?;
            args[0].map_quantities(|q| {
                if !q.is_dimensionless() {
                    return Err(crate::unit::UnitError::ExpectedDimensionless { found: q.dim });
                }
                Ok(Quantity::scalar(f(q.value)))
            })
        };

        // A dual argument means this call is inside a `derivative(f, x)`, so
        // the function has to carry the chain rule as well as compute its
        // value. Only the rules written below answer; everything else falls
        // through to the ordinary path, where a dual is refused rather than
        // having its slope quietly dropped. See [`crate::dual`].
        if let [Value::Dual(u)] = args {
            if let Some(slope) = differentiated(name, u) {
                return Ok(Value::Dual(slope?));
            }
        }

        match name {
            "sin" => unary_dimensionless(math::sin),
            "cos" => unary_dimensionless(math::cos),
            "tan" => unary_dimensionless(math::tan),
            "asin" => unary_dimensionless(math::asin),
            "acos" => unary_dimensionless(math::acos),
            "atan" => unary_dimensionless(math::atan),
            "sinh" => unary_dimensionless(math::sinh),
            "cosh" => unary_dimensionless(math::cosh),
            "tanh" => unary_dimensionless(math::tanh),
            "exp" => unary_dimensionless(math::exp),
            "ln" => unary_dimensionless(math::ln),
            "log10" => unary_dimensionless(math::log10),
            "log2" => unary_dimensionless(math::log2),

            "sqrt" => {
                arity(1)?;
                args[0].map_quantities(Quantity::sqrt)
            }
            // The one function that spans both: for a real it is the magnitude
            // ignoring sign, for a complex it is the modulus, and those are the
            // same question asked of a number with one component or two.
            "abs" => {
                arity(1)?;
                if let Value::Complex(z) = &args[0] {
                    return Ok(Value::Scalar(z.abs()));
                }
                args[0].map_quantities(|q| {
                    Ok(Quantity {
                        value: math::abs(q.value),
                        ..*q
                    })
                })
            }

            // The four that take a complex number apart. Each accepts a real
            // too, because a real *is* a complex number with no imaginary part
            // and a worksheet that writes `Re(x)` of something that happens to
            // have come out real should not have to care.
            "Re" | "Im" | "conj" | "arg" => {
                arity(1)?;
                let z = match &args[0] {
                    Value::Complex(z) => *z,
                    Value::Scalar(q) => ComplexQuantity::promote(q).map_err(EvalError::Unit)?,
                    // Design note item 29 wants `Re` element-wise over a matrix.
                    // That needs collections that can hold a complex element,
                    // which is the same gap `complex_pair` names.
                    other => {
                        return Err(EvalError::ShapeMismatch {
                            op: match name {
                                "Re" => "Re",
                                "Im" => "Im",
                                "conj" => "conj",
                                _ => "arg",
                            },
                            lhs: other.shape_name(),
                            rhs: String::from("a scalar"),
                        })
                    }
                };
                Ok(match name {
                    "Re" => Value::Scalar(z.real_part()),
                    "Im" => Value::Scalar(z.imaginary_part()),
                    "conj" => Value::Complex(z.conj()),
                    _ => Value::Scalar(z.arg()),
                })
            }
            "round" | "floor" | "ceil" => {
                arity(1)?;
                let f = match name {
                    "round" => math::round,
                    "floor" => math::floor,
                    _ => math::ceil,
                };
                args[0].map_quantities(|q| {
                    Ok(Quantity {
                        value: f(q.value),
                        ..*q
                    })
                })
            }
            "atan2" => {
                arity(2)?;
                let (a, b) = (scalar_arg(&args[0])?, scalar_arg(&args[1])?);
                Ok(Value::scalar(math::atan2(a.value, b.value)))
            }

            // Reaching here means one of these was called with no arguments at
            // all; with any, `eval_call` routes them to `eval_higher_order`
            // before evaluation. Answering "wrong arity" rather than "unknown
            // function" is both truthful and what keeps `BUILTINS` and this
            // dispatch provably in step.
            "map" => {
                arity(2)?;
                unreachable!("map with two arguments is handled before evaluation")
            }
            "iterate" => {
                arity(3)?;
                unreachable!("iterate with three arguments is handled before evaluation")
            }
            "derivative" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(EvalError::WrongArity {
                        name: name.into(),
                        expected: 2,
                        found: args.len(),
                    });
                }
                unreachable!("derivative is handled before evaluation")
            }
            "solve_linear" => {
                arity(2)?;
                unreachable!("solve_linear is handled before evaluation")
            }
            "root" | "roots" | "integral" | "plot" => {
                arity(3)?;
                unreachable!(
                    "root, roots, integral and plot with three arguments are handled before \
                     evaluation"
                )
            }
            "length" => {
                arity(1)?;
                Ok(Value::scalar(args[0].len() as f64))
            }
            "sum" => {
                arity(1)?;
                fold(name, &args[0], Quantity::add)
            }
            "range" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(EvalError::WrongArity {
                        name: name.into(),
                        expected: 2,
                        found: args.len(),
                    });
                }
                range(args)
            }
            "min" | "max" => {
                arity(1)?;
                let want_max = name == "max";
                let elements = args[0].elements();
                let Some(first) = elements.first().copied() else {
                    return Err(EvalError::Singular("min and max need a non-empty value"));
                };
                let mut best = first;
                for q in &elements[1..] {
                    if q.dim != best.dim {
                        return Err(crate::unit::UnitError::DimensionMismatch {
                            lhs: best.dim,
                            rhs: q.dim,
                        }
                        .into());
                    }
                    // Strict comparison, so an equal element never displaces the
                    // earlier one and the result does not depend on scan order.
                    // NaN compares false either way and is therefore never
                    // selected, which is also deterministic.
                    let better = if want_max {
                        q.value > best.value
                    } else {
                        q.value < best.value
                    };
                    if better {
                        best = *q;
                    }
                }
                Ok(Value::Scalar(best))
            }
            "transpose" => {
                arity(1)?;
                args[0].transpose()
            }

            // `diag(v)`: the square matrix with `v` down its diagonal.
            //
            // The companion to `identity`, and what a worksheet reaches for
            // when it builds a mass or a scaling matrix. One direction only:
            // this makes a matrix out of a vector, and does *not* also read a
            // diagonal back out of a matrix. Systems that overload it both ways
            // decide by argument shape, which means `diag(diag(v))` quietly
            // changes meaning with the shape of `v`; and SMath — where the name
            // comes from — describes only this direction.
            //
            // The zeros carry the diagonal's dimension, so the result is a
            // matrix that can be added to and multiplied by other matrices. A
            // dimensionless zero beside `3 m` would make every later operation
            // report a mismatch, which is why a vector of mixed dimensions is
            // refused here rather than one line further on.
            "diag" => {
                arity(1)?;
                let elements = match &args[0] {
                    Value::Vector(v) => v.elements.clone(),
                    Value::Scalar(q) => vec![*q],
                    other => {
                        return Err(EvalError::ShapeMismatch {
                            op: "diag",
                            lhs: other.shape_name(),
                            rhs: String::from("a vector"),
                        })
                    }
                };
                let n = elements.len();
                if n.saturating_mul(n) > MAX_RANGE {
                    return Err(EvalError::Singular(
                        "a diagonal matrix of more than a million elements is refused",
                    ));
                }
                let dim = elements[0].dim;
                if elements.iter().any(|q| q.dim != dim) {
                    return Err(EvalError::Singular(
                        "a diagonal matrix has one dimension, and this vector has several",
                    ));
                }
                let mut data = vec![Quantity::new(0.0, dim); n * n];
                for (i, q) in elements.iter().enumerate() {
                    data[i * n + i] = *q;
                }
                Ok(reshape(n, n, data))
            }

            // `identity(n)`: the n×n matrix with ones down the diagonal.
            //
            // Dimensionless, because that is the only thing it can be: it
            // exists to be multiplied by something else, and `S - λ*identity(2)`
            // — an eigenvalue problem, which is what the corpus uses it for —
            // needs the ones to take their dimension from `λ`.
            //
            // The cap is `MAX_RANGE` on the *elements*, not on `n`, for that
            // constant's own reason: a browser tab has no way out of building a
            // trillion-element matrix, and `identity(1000)` is already past
            // where this tool is for.
            "identity" => {
                arity(1)?;
                let n = whole_count("identity", &args[0])?;
                if n == 0 {
                    return Err(EvalError::Singular(
                        "an identity matrix needs at least one row",
                    ));
                }
                if n.saturating_mul(n) > MAX_RANGE {
                    return Err(EvalError::Singular(
                        "an identity matrix of more than a million elements is refused",
                    ));
                }
                let mut data = vec![Quantity::scalar(0.0); n * n];
                for i in 0..n {
                    data[i * n + i] = Quantity::scalar(1.0);
                }
                Ok(reshape(n, n, data))
            }

            // Shape. A vector answers as the column it is: `rows` counts its
            // elements and `cols` is 1, which is what indexing it already
            // assumes, and a scalar is a 1×1.
            "rows" => {
                arity(1)?;
                Ok(Value::Scalar(Quantity::scalar(shape_of(&args[0]).0 as f64)))
            }
            "cols" => {
                arity(1)?;
                Ok(Value::Scalar(Quantity::scalar(shape_of(&args[0]).1 as f64)))
            }

            // Extracting a row or a column. One-based, like every other index.
            "row" | "col" => {
                arity(2)?;
                let (rows, cols) = shape_of(&args[0]);
                let n = as_index(&args[1])?;
                let limit = if name == "row" { rows } else { cols };
                if n < 1 || n as usize > limit {
                    return Err(EvalError::IndexOutOfBounds {
                        index: n,
                        len: limit,
                    });
                }
                let n = n as usize - 1;
                let cells = args[0].elements();
                let picked: Vec<Quantity> = if name == "row" {
                    (0..cols).map(|c| cells[n * cols + c]).collect()
                } else {
                    (0..rows).map(|r| cells[r * cols + n]).collect()
                };
                Ok(Value::Vector(VectorValue { elements: picked }))
            }

            // Joining. `augment` puts operands side by side and `stack` puts one
            // above the other, which is how the corpus builds a table out of the
            // columns it computed separately. Both take two or more operands,
            // because that is how they are written in the worksheets that use
            // them, and both refuse a mismatch rather than padding: a table with
            // a short column is a mistake, not a shape to be repaired.
            "augment" | "stack" => {
                if args.len() < 2 {
                    return Err(EvalError::WrongArity {
                        name: name.into(),
                        expected: 2,
                        found: args.len(),
                    });
                }
                let horizontal = name == "augment";
                let mut acc: Option<(usize, usize, Vec<Quantity>)> = None;
                for a in args {
                    let (r, c) = shape_of(a);
                    let cells = a.elements();
                    acc = Some(match acc {
                        None => (r, c, cells),
                        Some((ar, ac, adata)) => {
                            let fits = if horizontal { ar == r } else { ac == c };
                            if !fits {
                                return Err(EvalError::ShapeMismatch {
                                    op: if horizontal { "augment" } else { "stack" },
                                    lhs: format!("a {ar}×{ac}"),
                                    rhs: format!("a {r}×{c}"),
                                });
                            }
                            if horizontal {
                                let mut data = Vec::with_capacity(adata.len() + cells.len());
                                for row in 0..ar {
                                    data.extend_from_slice(&adata[row * ac..(row + 1) * ac]);
                                    data.extend_from_slice(&cells[row * c..(row + 1) * c]);
                                }
                                (ar, ac + c, data)
                            } else {
                                let mut data = adata;
                                data.extend(cells);
                                (ar + r, ac, data)
                            }
                        }
                    });
                }
                let (rows, cols, data) = acc.expect("at least two operands");
                Ok(reshape(rows, cols, data))
            }

            "sign" => {
                arity(1)?;
                // Dimensionless by construction: the sign of a length is a
                // number, not a length. `sign(0)` is 0 and NaN stays NaN rather
                // than being called positive.
                args[0].map_quantities(|q| {
                    let x = q.value;
                    let s = if x.is_nan() {
                        f64::NAN
                    } else if x > 0.0 {
                        1.0
                    } else if x < 0.0 {
                        -1.0
                    } else {
                        0.0
                    };
                    Ok(Quantity::scalar(s))
                })
            }
            "det" => {
                arity(1)?;
                args[0].det()
            }
            "inv" => {
                arity(1)?;
                args[0].inv()
            }
            // Only defined in three dimensions, which is the whole of its use:
            // engineering worksheets reach for it to write a moment as r × F and
            // a surface normal as one tangent crossed with another. A seven-
            // dimensional cross product exists in mathematics and in no
            // worksheet, so requiring three is a real check rather than a
            // limitation, and it catches the common mistake — crossing vectors
            // that were meant to be dotted — instead of returning a number.
            "cross" => {
                arity(2)?;
                let (a, b) = (args[0].elements(), args[1].elements());
                if a.len() != 3 || b.len() != 3 {
                    return Err(EvalError::ShapeMismatch {
                        op: "cross",
                        lhs: args[0].shape_name(),
                        rhs: args[1].shape_name(),
                    });
                }
                // Each component is a difference of two products, so units
                // combine exactly as they do in `dot` — m × N gives N·m — and
                // `sub` rejects the mixed-dimension vectors that would otherwise
                // produce a nonsense moment.
                let term = |i: usize, j: usize| a[i].mul(&b[j]);
                let out = vec![
                    term(1, 2)?.sub(&term(2, 1)?)?,
                    term(2, 0)?.sub(&term(0, 2)?)?,
                    term(0, 1)?.sub(&term(1, 0)?)?,
                ];
                Ok(Value::Vector(VectorValue { elements: out }))
            }

            // The Euclidean norm. Units survive it — the norm of a vector of
            // stresses is a stress — because the squares and the square root
            // cancel in the dimension exactly as they do in the number.
            "norm" => {
                arity(1)?;
                let mut acc: Option<Quantity> = None;
                for x in &args[0].elements() {
                    let sq = x.mul(x)?;
                    acc = Some(match acc {
                        None => sq,
                        Some(sum) => sum.add(&sq)?,
                    });
                }
                let total = acc.unwrap_or_else(|| Quantity::scalar(0.0));
                Ok(Value::Scalar(Quantity::sqrt(&total)?))
            }

            "dot" => {
                arity(2)?;
                let (a, b) = (args[0].elements(), args[1].elements());
                if a.len() != b.len() {
                    return Err(EvalError::ShapeMismatch {
                        op: "dot",
                        lhs: args[0].shape_name(),
                        rhs: args[1].shape_name(),
                    });
                }
                let mut acc: Option<Quantity> = None;
                for (x, y) in a.iter().zip(&b) {
                    let term = x.mul(y)?;
                    acc = Some(match acc {
                        None => term,
                        Some(sum) => sum.add(&term)?,
                    });
                }
                Ok(Value::Scalar(acc.unwrap_or_else(|| Quantity::scalar(0.0))))
            }

            _ => Err(EvalError::UnknownFunction(name.into())),
        }
    }

    fn eval_index(&self, base: &Expr, indices: &[Expr], span: Span) -> Trace {
        let base_trace = self.eval(base);
        let index_traces: Vec<Trace> = indices.iter().map(|i| self.eval(i)).collect();

        let value = (|| -> Result<Value, EvalError> {
            let Ok(base_value) = &base_trace.value else {
                return Err(EvalError::Poisoned);
            };
            let mut resolved = Vec::new();
            for t in &index_traces {
                let Ok(v) = &t.value else {
                    return Err(EvalError::Poisoned);
                };
                resolved.push(as_index(v)?);
            }

            match base_value {
                // One-based, matching how engineers number rows and columns and
                // how the SMath worksheets this must eventually import do.
                Value::Vector(v) => match resolved.len() {
                    1 => element_at(&v.elements, resolved[0]),
                    // A vector is a column of n. That is not a new decision
                    // here: `rows` and `cols` answer `n` and `1` for one,
                    // `augment` and `stack` line them up on it, and `reshape`
                    // turns any single row or column into one. Indexing was the
                    // odd corner that did not know it, so `v[i, 1]` — the shape
                    // every SMath worksheet writes as `el(v, i, 1)` — was an
                    // error while `rows(v)` cheerfully said how many there were.
                    2 => {
                        let c = resolved[1];
                        if c != 1 {
                            return Err(EvalError::IndexOutOfBounds { index: c, len: 1 });
                        }
                        element_at(&v.elements, resolved[0])
                    }
                    n => Err(EvalError::WrongIndexCount {
                        expected: 1,
                        found: n,
                    }),
                },
                Value::Matrix(m) => match resolved.len() {
                    2 => {
                        let (r, c) = (resolved[0], resolved[1]);
                        if r < 1 || r as usize > m.rows {
                            return Err(EvalError::IndexOutOfBounds {
                                index: r,
                                len: m.rows,
                            });
                        }
                        if c < 1 || c as usize > m.cols {
                            return Err(EvalError::IndexOutOfBounds {
                                index: c,
                                len: m.cols,
                            });
                        }
                        Ok(Value::Scalar(m.get(r as usize - 1, c as usize - 1)))
                    }
                    1 => element_at(&m.data, resolved[0]),
                    n => Err(EvalError::WrongIndexCount {
                        expected: 2,
                        found: n,
                    }),
                },
                other => Err(EvalError::TypeMismatch {
                    op: "indexing",
                    lhs: other.type_name(),
                    rhs: "a vector or matrix",
                }),
            }
        })();

        Trace::new(
            span,
            TraceNode::Index {
                base: Box::new(base_trace),
                indices: index_traces,
            },
            value,
        )
    }

    fn eval_convert(&self, value: &Expr, unit: &Expr, span: Span) -> Trace {
        let value_trace = self.eval(value);

        // A bare affine name is the only way to reach an offset scale: a
        // compound expression has no way to carry an offset.
        if let Some(u) = self.affine_unit_named_by(unit) {
            let result = match &value_trace.value {
                Err(_) => Err(EvalError::Poisoned),
                Ok(v) => match v.as_scalar() {
                    Some(q) => q.to_unit(&u).map(|_| v.clone()).map_err(EvalError::Unit),
                    None => Err(EvalError::TypeMismatch {
                        op: "->",
                        lhs: v.type_name(),
                        rhs: "an offset scale",
                    }),
                },
            };
            let target = DisplayTarget {
                span: unit.span(),
                factor: u.factor,
                unit: Some(u),
            };
            return Trace::new(
                span,
                TraceNode::Convert {
                    value: Box::new(value_trace),
                    target: Some(target),
                },
                result,
            );
        }

        let unit_trace = self.eval(unit);
        let (result, target) = match (&value_trace.value, &unit_trace.value) {
            (Err(_), _) | (_, Err(_)) => (Err(EvalError::Poisoned), None),
            (Ok(v), Ok(u)) => match u.as_scalar() {
                None => (
                    Err(EvalError::TypeMismatch {
                        op: "->",
                        lhs: v.type_name(),
                        rhs: "a unit",
                    }),
                    None,
                ),
                Some(uq) => {
                    // Conversion changes only how a value is shown, so the value
                    // passes through; what matters here is that the dimensions
                    // agree, and that is checked by dividing.
                    match v.elements().first() {
                        Some(first) if first.dim != uq.dim => (
                            Err(EvalError::Unit(crate::unit::UnitError::DimensionMismatch {
                                lhs: first.dim,
                                rhs: uq.dim,
                            })),
                            None,
                        ),
                        _ => (
                            Ok(v.clone()),
                            Some(DisplayTarget {
                                span: unit_trace.span,
                                unit: None,
                                factor: uq.value,
                            }),
                        ),
                    }
                }
            },
        };

        Trace::new(
            span,
            TraceNode::Convert {
                value: Box::new(value_trace),
                target,
            },
            result,
        )
    }

    // ---- statements ------------------------------------------------------

    /// Evaluate one statement, updating the environment.
    ///
    /// Public because the document layer drives evaluation in dependency order
    /// rather than reading order; there is deliberately no second driver here
    /// that walks statements top to bottom, because two evaluation paths for one
    /// language is exactly the duplication this project set out to avoid.
    pub fn eval_stmt(&mut self, stmt: &Stmt) -> Outcome {
        // Each statement gets the whole budget: the ceiling is on one result,
        // not on how long a worksheet is.
        self.refresh_budget();
        match stmt {
            Stmt::Comment { text, span } => Outcome {
                span: *span,
                kind: OutcomeKind::Comment(text.clone()),
                diagnostics: vec![],
            },

            Stmt::GlobalDef { name, value, span } | Stmt::Assign { name, value, span } => {
                let trace = self.eval(value);
                let mut diagnostics = diagnose(&trace);
                if shadowing_is_worth_reporting(&name.text)
                    && self.units.contains(&name.text)
                    && !self.vars.contains_key(&name.text)
                {
                    diagnostics.push(Diagnostic::warning(
                        eval_codes::SHADOWS_UNIT,
                        name.span,
                        format!(
                            "`{}` is also a unit; this binding hides it for the rest of \
                             the worksheet",
                            name.text
                        ),
                    ));
                }
                match &trace.value {
                    Ok(v) => {
                        self.vars.insert(name.text.clone(), v.clone());
                        self.failed.remove(&name.text);
                        match self.unit_written_in(value) {
                            Some(u) => self.hints.insert(name.text.clone(), u),
                            None => self.hints.remove(&name.text),
                        };
                    }
                    // The binding still happened; it simply has no value. Any
                    // earlier value must go with it, because a use below takes
                    // the nearest definition above it and that is this one.
                    Err(_) => {
                        self.vars.remove(&name.text);
                        self.hints.remove(&name.text);
                        self.failed.insert(name.text.clone());
                    }
                }
                Outcome {
                    span: *span,
                    kind: OutcomeKind::Assign {
                        name: name.text.clone(),
                        trace,
                    },
                    diagnostics,
                }
            }

            Stmt::Query { expr, span } => {
                let trace = self.eval(expr);
                let diagnostics = diagnose(&trace);
                Outcome {
                    span: *span,
                    kind: OutcomeKind::Query(trace),
                    diagnostics,
                }
            }

            Stmt::Check { expr, span } => {
                let trace = self.eval(expr);
                let mut diagnostics = diagnose(&trace);
                // A condition, and nothing else. Comparisons and the logical
                // connectives answer exactly 1 or 0, so this is not a tolerance
                // question. Anything else — a length, a vector, a string, 0.5 —
                // is refused rather than read as true: a check that passes
                // because `5 m` is "truthy" is worse than no check at all, and
                // the mistake it hides is the one a check exists to catch.
                //
                // Exact comparison, and deliberately so: a comparison answers
                // the bits 1.0 or 0.0, nothing near them. A tolerance here
                // would be a tolerance on *whether a condition holds*, which is
                // the one place this engine must not have one.
                #[allow(clippy::float_cmp)]
                let passed = match trace.value.as_ref() {
                    Ok(Value::Scalar(q))
                        if q.is_dimensionless() && (q.value == 0.0 || q.value == 1.0) =>
                    {
                        Some(q.value == 1.0)
                    }
                    Ok(_) => {
                        diagnostics.push(Diagnostic::error(
                            eval_codes::EVAL_ERROR,
                            *span,
                            "a check needs a condition, such as `check sigma <= sigma_allow`",
                        ));
                        None
                    }
                    // The failure is already reported by `diagnose`.
                    Err(_) => None,
                };
                Outcome {
                    span: *span,
                    kind: OutcomeKind::Check { trace, passed },
                    diagnostics,
                }
            }

            Stmt::UnitDecl { name, value, span } => {
                let trace = self.eval(value);
                let mut diagnostics = diagnose(&trace);
                match trace.value.as_ref().ok().and_then(Value::as_scalar) {
                    Some(q) if !q.is_point() => {
                        self.units.declare(&name.text, q.value, q.dim);
                    }
                    Some(_) => diagnostics.push(Diagnostic::error(
                        eval_codes::EVAL_ERROR,
                        *span,
                        "a unit cannot be declared from an offset temperature scale",
                    )),
                    None => {
                        if trace.value.is_ok() {
                            diagnostics.push(Diagnostic::error(
                                eval_codes::EVAL_ERROR,
                                *span,
                                "a unit must be declared from a single number",
                            ));
                        }
                    }
                }
                Outcome {
                    span: *span,
                    kind: OutcomeKind::UnitDecl {
                        name: name.text.clone(),
                        trace,
                    },
                    diagnostics,
                }
            }

            Stmt::FnDef {
                name, params, body, ..
            } => {
                let diagnostics = if self.funcs.contains_key(&name.text) {
                    vec![Diagnostic::warning(
                        eval_codes::REDEFINED,
                        name.span,
                        format!(
                            "`{}` was already defined; the later definition wins",
                            name.text
                        ),
                    )]
                } else {
                    vec![]
                };
                self.funcs.insert(
                    name.text.clone(),
                    Function {
                        params: params.iter().map(|p| p.text.clone()).collect(),
                        body: body.clone(),
                    },
                );
                Outcome {
                    span: stmt.span(),
                    kind: OutcomeKind::FnDef(name.text.clone()),
                    diagnostics,
                }
            }

            Stmt::Error { span } => Outcome {
                span: *span,
                kind: OutcomeKind::Malformed,
                diagnostics: vec![],
            },
        }
    }
}

/// What one statement produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub span: Span,
    pub kind: OutcomeKind,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub enum OutcomeKind {
    Comment(String),
    Assign {
        name: String,
        trace: Trace,
    },
    Query(Trace),
    /// `check …` — the condition, and whether it held.
    ///
    /// `passed` is `None` when the condition could not be evaluated at all, or
    /// was not a condition: a check that cannot be decided is neither a pass nor
    /// a failure, and reporting it as either would be the quietly-wrong answer
    /// this engine refuses everywhere else. That case carries a diagnostic; a
    /// plain `Some(false)` does not, because a design that does not meet its
    /// limit is not a worksheet that is wrong.
    Check {
        trace: Trace,
        passed: Option<bool>,
    },
    UnitDecl {
        name: String,
        trace: Trace,
    },
    FnDef(String),
    /// The statement could not be parsed.
    Malformed,
    /// The statement parsed but was never evaluated, because it takes part in a
    /// dependency cycle. Distinct from `Malformed`: nothing is wrong with the
    /// line itself, so saying so would send the reader to the wrong place.
    NotEvaluated,
}

/// One diagnostic per failed statement, pointing at the subexpression that
/// actually failed rather than at the whole line.
/// `range(a, b)` and `range(a, b, step)`.
///
/// All three share one dimension, so `range(0 m, 10 m, 2 m)` tabulates a length
/// as readily as `range(1, 5)` counts. The step defaults to one *of that
/// dimension*, which is the only reading that makes the two-argument form and
/// the three-argument form agree.
///
/// Elements are computed as `a + i*step` rather than by repeated addition. Both
/// are defensible; only one gives the same last bits for the hundredth element
/// as for the second, and reproducibility is the point of this engine.
fn range(args: &[Value]) -> Result<Value, EvalError> {
    let scalar = |v: &Value| match v {
        Value::Scalar(q) if !q.is_point() => Ok(*q),
        other => Err(EvalError::ShapeMismatch {
            op: "range",
            lhs: other.shape_name(),
            rhs: String::from("a scalar"),
        }),
    };
    let from = scalar(&args[0])?;
    let to = scalar(&args[1])?;
    let step = match args.get(2) {
        Some(v) => scalar(v)?,
        None => Quantity::new(1.0, from.dim),
    };
    if from.dim != to.dim || from.dim != step.dim {
        return Err(EvalError::Unit(UnitError::DimensionMismatch {
            lhs: from.dim,
            rhs: to.dim,
        }));
    }
    if step.value == 0.0 {
        return Err(EvalError::Singular("a range needs a step that is not zero"));
    }
    let span = (to.value - from.value) / step.value;
    if span < 0.0 {
        return Err(EvalError::Singular("a range's step runs away from its end"));
    }
    // The end is included when it lands on a step, which is what `range(1, 5)`
    // has to mean for indexing to work.
    let count = span.floor() as usize + 1;
    if count > MAX_RANGE {
        return Err(EvalError::Singular(
            "a range of more than a million elements is refused",
        ));
    }
    Ok(Value::Vector(VectorValue {
        elements: (0..count)
            .map(|i| Quantity::new(from.value + i as f64 * step.value, from.dim))
            .collect(),
    }))
}

/// How deep calls may nest before evaluation gives up.
///
/// Recursion is not a mistake here: the conditional is lazy, so
/// `fn fact(n) = if n <= 1 then 1 else n*fact(n - 1)` terminates and answers.
/// One that never reaches its base case is the mistake, and without a ceiling
/// it does not produce a wrong answer — it exhausts the stack and takes the
/// process with it, which in the browser is a tab that dies with nothing on the
/// page to say why.
///
/// 64 because the ceiling has to hold on *every* target, and the targets differ
/// by an order of magnitude: measured on this worksheet, the WebAssembly build
/// stops answering between 190 and 200 calls deep, the native release build
/// between 800 and 1600, and the debug build somewhere between 300 and 500. A
/// worksheet that recursed 200 deep therefore computed an answer natively and
/// trapped in the browser — a cross-target difference of exactly the kind this
/// engine exists to rule out. The limit is set below the smallest of them with
/// room to spare, and far above anything an engineering worksheet nests: the
/// deepest chain of calls in either SMath corpus is under ten.
pub const MAX_DEPTH: usize = 64;

/// How deeply evaluation may nest, counting every recursive step.
///
/// [`MAX_DEPTH`] counts *calls* and [`crate::parse::MAX_NEST`] counts brackets,
/// and neither bounds the stack on its own, because the two multiply:
///
/// ```text
/// fn f(x) = ((((( … f(x) … )))))
/// ```
///
/// is 120 brackets inside 64 calls, which is some 7 700 nested evaluations —
/// both limits respected, and the WebAssembly build traps. Found by
/// `tests/robustness.rs` and confirmed against the shipped release module, so
/// it was reachable from a worksheet rather than only in theory.
///
/// This counts what actually consumes the stack, and the number comes from the
/// tightest target as usual. Measured 2026-08-29 on the release WebAssembly
/// build, varying calls against bracket depth: it answers to roughly 900 nested
/// evaluations and traps beyond about 1 000, and the cliff sits at the same
/// place however the nesting is composed — 64 calls of 8 brackets, 8 calls of
/// 120, and 16 of 64 all fail together. 512 is half of that, which is the
/// margin available rather than a generous one: the ceiling is pressed from
/// below as well, since [`MAX_DEPTH`] recursion of an ordinary definition costs
/// four or five nested evaluations per call and must keep working. A worksheet
/// hits this only by combining deep brackets with deep recursion, and the
/// deepest expression in either SMath corpus is 14 with call chains under ten.
pub const MAX_EVAL_NEST: usize = 512;

/// How many user-function calls one statement may make.
///
/// [`MAX_DEPTH`] bounds the stack and this bounds the clock. They are different
/// failures: `fn f(x) = f(x) + f(x)` never nests deeper than the ceiling and
/// still asks for 2^64 calls, which is a tab that hangs with no way out — the
/// same thing [`MAX_RANGE`] exists to prevent, arrived at from the other side.
///
/// A hundred thousand, and the number is chosen by what it costs rather than by
/// what sounds generous: a call copies the environment it runs in, so the
/// budget is also the worst case a reader waits for when a worksheet is wrong —
/// a second or so, not a minute. Fixed rather than timed, because a time limit
/// would make the answer depend on the machine.
///
/// It bounds *user* functions only. `map(sin, …)` over the largest vector
/// [`MAX_RANGE`] allows costs nothing here; `map(f, …)` over more than a
/// hundred thousand elements is refused, which is the one honest cost of this
/// and is well past what a worksheet does.
pub const MAX_CALLS: usize = 100_000;

/// A ceiling on generated vectors.
///
/// Not a performance tuning knob: `range(1, 1e18)` in a browser tab is a hang
/// with no way out, and a worksheet that wants a million elements has already
/// gone somewhere this tool is not for.
const MAX_RANGE: usize = 1_000_000;

/// Read a repetition count: dimensionless, whole, and not negative.
fn whole_count(name: &'static str, v: &Value) -> Result<usize, EvalError> {
    match v {
        Value::Scalar(q) if q.is_dimensionless() && !q.is_point() => {
            if q.value < 0.0 || q.value.fract() != 0.0 {
                return Err(EvalError::Singular(
                    "a repetition count must be a whole number that is not negative",
                ));
            }
            if q.value > MAX_RANGE as f64 {
                return Err(EvalError::Singular(
                    "more than a million repetitions is refused",
                ));
            }
            Ok(q.value as usize)
        }
        other => Err(EvalError::ShapeMismatch {
            op: name,
            lhs: other.shape_name(),
            rhs: String::from("a dimensionless whole number"),
        }),
    }
}

/// A truth value: the dimensionless 1 or 0.
///
/// Nomo has no boolean type, and deliberately. Adding one would touch every
/// arm of the value tower to buy an error message, while SMath — whose
/// worksheets this language has to be able to receive — already computes with
/// comparisons as numbers. What is enforced instead is the part that catches
/// real mistakes: a condition must be *dimensionless*, so `if x then …` with `x`
/// in metres is an error rather than a coin toss.
/// A dual seen as the value it carries, for the operations that ask about the
/// value alone. See [`compare`].
fn undual(v: &Value) -> Value {
    match v {
        Value::Dual(d) => Value::Scalar(d.value),
        other => other.clone(),
    }
}

fn truth_value(t: bool) -> Value {
    Value::scalar(if t { 1.0 } else { 0.0 })
}

/// Read a value as a condition. Anything non-zero is true.
fn truth(v: &Value) -> Result<bool, EvalError> {
    match v {
        Value::Scalar(q) if q.is_point() => Err(EvalError::Singular(
            "a temperature on an offset scale is not a condition",
        )),
        Value::Scalar(q) if q.is_dimensionless() => Ok(q.value != 0.0),
        Value::Scalar(q) => Err(EvalError::Unit(UnitError::ExpectedDimensionless {
            found: q.dim,
        })),
        other => Err(EvalError::ShapeMismatch {
            op: "condition",
            lhs: other.shape_name(),
            rhs: String::from("a dimensionless number"),
        }),
    }
}

/// Compare two quantities of the same dimension.
///
/// `==` is exact, deliberately. Clippy's suggestion to compare within a margin
/// is right for most programs and wrong for this one: every result here is
/// reproducible to the bit, the golden-file suite compares with no tolerance at
/// all, and a hidden epsilon in the one operator that asks "are these the same
/// number" would make the language quietly disagree with its own test suite. A
/// worksheet that wants a tolerance can write one.
#[allow(clippy::float_cmp)]
fn compare(op: BinaryOp, a: &Value, b: &Value) -> Result<Value, EvalError> {
    // A comparison inside a `derivative` call is a question about the value and
    // not about the slope: the derivative of a piecewise definition is the
    // derivative of whichever branch its condition selects. Exact everywhere
    // except at the switch itself, where the function has no derivative to be
    // exact about — a clamp differentiates to the slope of the side it is on.
    let (a, b) = (&undual(a), &undual(b));
    // Two strings compare for equality and for nothing else. `<` on words would
    // have to pick a collation — by code point, by locale, by length — and every
    // choice is a decision a worksheet cannot state, so only the question with
    // one answer is answered.
    if let (Value::Text(x), Value::Text(y)) = (a, b) {
        return match op {
            BinaryOp::Equal => Ok(truth_value(x == y)),
            BinaryOp::NotEqual => Ok(truth_value(x != y)),
            _ => Err(EvalError::NotImplemented("ordering two strings")),
        };
    }
    let (Value::Scalar(x), Value::Scalar(y)) = (a, b) else {
        return Err(EvalError::ShapeMismatch {
            op: op.symbol(),
            lhs: a.shape_name(),
            rhs: b.shape_name(),
        });
    };
    if x.dim != y.dim {
        return Err(EvalError::Unit(UnitError::DimensionMismatch {
            lhs: x.dim,
            rhs: y.dim,
        }));
    }
    // Both sides are held in base SI, so comparing magnitudes compares the
    // quantities: `1 in < 1 m` needs no conversion step of its own.
    Ok(truth_value(match op {
        BinaryOp::Lt => x.value < y.value,
        BinaryOp::Gt => x.value > y.value,
        BinaryOp::Le => x.value <= y.value,
        BinaryOp::Ge => x.value >= y.value,
        BinaryOp::Equal => x.value == y.value,
        BinaryOp::NotEqual => x.value != y.value,
        _ => unreachable!("compare called with {op:?}"),
    }))
}

/// The shape of an expression as a trace, with nothing evaluated.
///
/// Used for the arm of a conditional that was not taken. The renderer still has
/// to show it — a worksheet shows its work, and "which arm" is part of the work
/// — so the structure has to survive even though no value does. Numeric literals
/// keep their value because a literal needs no evaluating to be known, and
/// without them the unrendered arm would print as a row of question marks.
fn sketch(expr: &Expr) -> Trace {
    let node = match expr {
        Expr::Number { value, span } => {
            return Trace::new(*span, TraceNode::Number, Ok(Value::scalar(*value)))
        }
        Expr::Text { value, span } => {
            return Trace::new(*span, TraceNode::Text, Ok(Value::Text(value.clone())))
        }
        Expr::Ident(name) => TraceNode::Variable {
            name: name.text.clone(),
            unit: None,
        },
        Expr::Unary { op, operand, .. } => TraceNode::Unary {
            op: *op,
            operand: Box::new(sketch(operand)),
        },
        Expr::Binary { op, lhs, rhs, .. } => TraceNode::Binary {
            op: *op,
            lhs: Box::new(sketch(lhs)),
            rhs: Box::new(sketch(rhs)),
        },
        Expr::Call { callee, args, .. } => TraceNode::Call {
            name: callee.text.clone(),
            args: args.iter().map(sketch).collect(),
        },
        Expr::Index { base, indices, .. } => TraceNode::Index {
            base: Box::new(sketch(base)),
            indices: indices.iter().map(sketch).collect(),
        },
        Expr::Vector { elements, .. } => TraceNode::Vector(elements.iter().map(sketch).collect()),
        Expr::Matrix { rows, .. } => TraceNode::Matrix(
            rows.iter()
                .map(|r| r.iter().map(sketch).collect())
                .collect(),
        ),
        Expr::Paren { inner, .. } => TraceNode::Paren(Box::new(sketch(inner))),
        Expr::If {
            cond,
            then,
            otherwise,
            ..
        } => TraceNode::Conditional {
            cond: Box::new(sketch(cond)),
            then: Box::new(sketch(then)),
            otherwise: Box::new(sketch(otherwise)),
        },
        Expr::Convert { value, .. } => TraceNode::Convert {
            value: Box::new(sketch(value)),
            target: None,
        },
        Expr::Error { .. } => TraceNode::Malformed,
    };
    Trace::new(expr.span(), node, Err(EvalError::NotTaken))
}

fn diagnose(trace: &Trace) -> Vec<Diagnostic> {
    match trace.root_error() {
        None => vec![],
        Some((_, EvalError::Malformed)) => vec![],
        Some((span, e)) => vec![Diagnostic::error(
            eval_codes::EVAL_ERROR,
            span,
            e.to_string(),
        )],
    }
}

/// Whether binding `name` should warn about hiding a unit of the same name.
///
/// Single-character unit symbols collide with the most ordinary engineering
/// variable names there are: `V` is volume as often as volts, `h` a height as
/// often as an hour, and `A`, `F`, `P`, `T`, `L`, `W` are all both. Warning on
/// those would fire on nearly every real worksheet, and a warning that is usually
/// wrong teaches people to ignore warnings.
///
/// Multi-character collisions — `min`, `psi`, `bar`, `rad` — are rarer and more
/// likely to surprise, so those are still reported.
///
/// The precise rule is to warn only when the hidden unit is *used* as a unit
/// later in the same worksheet. That needs a whole-document view, which arrives
/// with the dependency graph.
fn shadowing_is_worth_reporting(name: &str) -> bool {
    name.chars().count() > 1
}

fn clone_or_poison(t: &Trace) -> Result<Value, EvalError> {
    t.value.clone().map_err(|_| EvalError::Poisoned)
}

fn scalar_arg(v: &Value) -> Result<Quantity, EvalError> {
    v.as_scalar().ok_or(EvalError::TypeMismatch {
        op: "this function",
        lhs: v.type_name(),
        rhs: "a number",
    })
}

fn fold(
    name: &str,
    v: &Value,
    f: impl Fn(&Quantity, &Quantity) -> Result<Quantity, crate::unit::UnitError>,
) -> Result<Value, EvalError> {
    let elements = v.elements();
    let Some(first) = elements.first().copied() else {
        return Err(EvalError::WrongArity {
            name: name.into(),
            expected: 1,
            found: 0,
        });
    };
    // Left to right, as specified.
    let mut acc = first;
    for q in &elements[1..] {
        acc = f(&acc, q)?;
    }
    Ok(Value::Scalar(acc))
}

/// The shape of a value as (rows, cols), with a vector read as a column and a
/// scalar as a 1×1. Every function here that asks about shape asks this, so the
/// three cases are answered once rather than at each call site.
/// The derivative rule for a one-argument builtin, if it has one.
///
/// `None` means no rule is written, which the caller turns into a refusal
/// rather than a slope of zero. The list is the elementary functions whose
/// derivatives are themselves elementary: what is missing is deliberate —
/// `floor`, `round` and `sign` are constant almost everywhere and undefined
/// exactly where a worksheet would care, and answering `0` for them would be
/// true almost everywhere and useless.
fn differentiated(name: &str, u: &DualQuantity) -> Option<Result<DualQuantity, DualError>> {
    let chain =
        |f: fn(f64) -> f64, df: fn(f64) -> f64, ddf: fn(f64) -> f64| Some(u.chain(f, df, ddf));
    // Each line is the function, its slope and its curvature. Written out
    // rather than derived, because there is nothing here to derive with.
    match name {
        "sin" => chain(math::sin, math::cos, |x| -math::sin(x)),
        "cos" => chain(math::cos, |x| -math::sin(x), |x| -math::cos(x)),
        "tan" => chain(
            math::tan,
            |x| 1.0 / (math::cos(x) * math::cos(x)),
            |x| 2.0 * math::tan(x) / (math::cos(x) * math::cos(x)),
        ),
        "asin" => chain(
            math::asin,
            |x| 1.0 / math::sqrt(1.0 - x * x),
            |x| x / math::powf(1.0 - x * x, 1.5),
        ),
        "acos" => chain(
            math::acos,
            |x| -1.0 / math::sqrt(1.0 - x * x),
            |x| -x / math::powf(1.0 - x * x, 1.5),
        ),
        "atan" => chain(
            math::atan,
            |x| 1.0 / (1.0 + x * x),
            |x| -2.0 * x / ((1.0 + x * x) * (1.0 + x * x)),
        ),
        "sinh" => chain(math::sinh, math::cosh, math::sinh),
        "cosh" => chain(math::cosh, math::sinh, math::cosh),
        "tanh" => chain(
            math::tanh,
            |x| 1.0 / (math::cosh(x) * math::cosh(x)),
            |x| -2.0 * math::tanh(x) / (math::cosh(x) * math::cosh(x)),
        ),
        "exp" => chain(math::exp, math::exp, math::exp),
        "ln" => chain(math::ln, |x| 1.0 / x, |x| -1.0 / (x * x)),
        "log10" => chain(
            math::log10,
            |x| 1.0 / (x * math::ln(10.0)),
            |x| -1.0 / (x * x * math::ln(10.0)),
        ),
        "log2" => chain(
            math::log2,
            |x| 1.0 / (x * math::ln(2.0)),
            |x| -1.0 / (x * x * math::ln(2.0)),
        ),
        // These two keep a dimension rather than demanding none, so they are
        // not `chain`: `sqrt` halves it and `abs` leaves it alone.
        "sqrt" => Some(u.sqrt()),
        "abs" => Some(Ok(u.abs())),
        _ => None,
    }
}

/// Bisect a bracket until there is nothing left to halve.
///
/// `f_lo` is `f(lo)`, already computed, and `f(lo)` and `f(hi)` differ in sign.
/// 100 halvings exhausts a binary64 interval whatever its exponent, so this is
/// "until there is nothing left to halve" written as a count that provably
/// terminates — and the midpoint coinciding with an endpoint *is* the
/// definition of an exhausted interval, which is why that comparison is exact.
/// A tolerance here would be a second, machine-dependent stopping rule next to
/// the machine-independent one.
fn bisect(
    mut lo: Quantity,
    mut f_lo: f64,
    mut hi: Quantity,
    at: &mut dyn FnMut(Quantity) -> Result<Quantity, EvalError>,
) -> Result<Quantity, EvalError> {
    for _ in 0..100 {
        let mid = Quantity {
            value: (lo.value + hi.value) / 2.0,
            ..lo
        };
        #[allow(clippy::float_cmp)]
        let exhausted = mid.value == lo.value || mid.value == hi.value;
        if exhausted {
            break;
        }
        let f_mid = at(mid)?.value;
        #[allow(clippy::float_cmp)]
        let hit = f_mid == 0.0;
        if hit {
            return Ok(mid);
        }
        if (f_mid > 0.0) == (f_lo > 0.0) {
            lo = mid;
            f_lo = f_mid;
        } else {
            hi = mid;
        }
    }
    Ok(Quantity {
        value: (lo.value + hi.value) / 2.0,
        ..lo
    })
}

/// Add a root to the list unless it is already there.
///
/// Exact equality, and no tolerance: two brackets that converge to the same bits
/// found the same root, and two that do not found two roots this scan can tell
/// apart. Deciding otherwise would need a distance, and a distance here would be
/// a number nobody could derive from the worksheet.
fn push_root(found: &mut Vec<Quantity>, r: Quantity) {
    #[allow(clippy::float_cmp)]
    let seen = found.iter().any(|q| q.value == r.value);
    if !seen {
        found.push(r);
    }
}

fn shape_of(v: &Value) -> (usize, usize) {
    match v {
        Value::Matrix(m) => (m.rows, m.cols),
        Value::Vector(x) => (x.elements.len(), 1),
        _ => (1, 1),
    }
}

/// Row-major cells back into the smallest value that holds them: a single row or
/// column is a vector, which is what indexing and the rest of the language
/// expect, and a 1×1 is a scalar.
fn reshape(rows: usize, cols: usize, data: Vec<Quantity>) -> Value {
    match (rows, cols) {
        (1, 1) => Value::Scalar(data.into_iter().next().expect("1×1 has one cell")),
        (1, _) | (_, 1) => Value::Vector(VectorValue { elements: data }),
        _ => Value::Matrix(MatrixValue::new(rows, cols, data)),
    }
}

fn as_index(v: &Value) -> Result<i64, EvalError> {
    let Some(q) = v.as_scalar() else {
        return Err(EvalError::BadIndex(v.type_name().into()));
    };
    if !q.is_dimensionless() {
        return Err(EvalError::BadIndex(format!("a value in {}", q.dim)));
    }
    if q.value.fract() != 0.0 || !q.value.is_finite() {
        return Err(EvalError::BadIndex(format!("{}", q.value)));
    }
    Ok(q.value as i64)
}

fn element_at(elements: &[Quantity], i: i64) -> Result<Value, EvalError> {
    if i < 1 || i as usize > elements.len() {
        return Err(EvalError::IndexOutOfBounds {
            index: i,
            len: elements.len(),
        });
    }
    Ok(Value::Scalar(elements[i as usize - 1]))
}

fn collect_scalars(traces: &[Trace]) -> Result<Vec<Quantity>, EvalError> {
    collect_scalars_ref(&traces.iter().collect::<Vec<_>>())
}

fn collect_scalars_ref(traces: &[&Trace]) -> Result<Vec<Quantity>, EvalError> {
    let mut out = Vec::with_capacity(traces.len());
    for t in traces {
        match &t.value {
            Err(_) => return Err(EvalError::Poisoned),
            Ok(v) => match v.as_scalar() {
                Some(q) => out.push(q),
                None => {
                    return Err(EvalError::TypeMismatch {
                        op: "a literal",
                        lhs: v.type_name(),
                        rhs: "a number",
                    })
                }
            },
        }
    }
    Ok(out)
}

/// Parse-then-evaluate, for callers that want the whole pipeline.
///
/// Goes through the document layer, so it sees dependency ordering, global
/// definitions and cycle detection rather than a simpler top-to-bottom walk.
pub fn run_source(source: &str) -> (Vec<Outcome>, Vec<Diagnostic>) {
    crate::doc::evaluate(source)
}

#[cfg(test)]
mod builtin_tests {
    use super::*;
    use crate::value::Value;

    /// `BUILTINS` and `call_builtin`'s dispatch must name the same functions.
    ///
    /// Checked by calling each one with no arguments: a name the dispatch knows
    /// answers "wrong arity", and a name it does not answers "unknown function".
    /// That distinguishes the two without depending on what any of them compute.
    #[test]
    fn builtins_match_the_dispatch() {
        let env = Env::new();

        for name in BUILTINS {
            if let Err(EvalError::UnknownFunction(_)) = env.call_builtin(name, &[]) {
                panic!("`{name}` is listed in BUILTINS but the dispatch does not know it");
            }
        }

        for name in ["frobnicate", "integrate", "solve", "chart"] {
            assert!(
                matches!(
                    env.call_builtin(name, &[Value::Scalar(crate::Quantity::scalar(1.0))]),
                    Err(EvalError::UnknownFunction(_))
                ),
                "`{name}` is not a builtin but the dispatch accepted it"
            );
        }
    }

    #[test]
    fn the_list_is_sorted_and_free_of_duplicates() {
        // Sorted so that adding a function has one obvious place to put it, and
        // so a duplicate is visible rather than silently harmless.
        let mut sorted = BUILTINS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.as_slice(), BUILTINS);
    }

    #[test]
    fn constants_are_recognised_without_being_evaluated() {
        for name in ["pi", "π", "e", "tau", "τ", "inf"] {
            assert!(is_constant(name), "`{name}` should be a constant");
        }
        assert!(!is_constant("x"));
        assert!(!is_constant("sin"));
    }
}
