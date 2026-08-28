//! The value tower: what an expression evaluates to.
//!
//! # Shape
//!
//! A value is a scalar, a complex scalar, a vector, or a matrix. Collections hold
//! [`Quantity`] elements rather than bare `f64`, so `[5 mm, 2 in]` is
//! representable and unit consistency is enforced element by element. That costs
//! memory on large matrices — a dimension is seven rational exponents — and the
//! representation can be narrowed later without changing this API.
//!
//! # Complex
//!
//! A complex scalar is [`ComplexQuantity`], and the arithmetic is in
//! [`crate::complex`]. Any operation with a complex operand on either side
//! promotes the other and answers complex — the promotion is one-way, for the
//! reason that module gives. Complex *collections* are not built yet: a vector
//! or matrix holds [`Quantity`], so a complex element says so rather than
//! silently dropping its imaginary part. That is the remaining half of design
//! note item 29, which also wants `Re` applied element-wise to a matrix.

pub use crate::complex::ComplexQuantity;
use crate::dual::{DualError, DualQuantity};

use crate::dim::{Dimension, Ratio};
use crate::math;
use crate::quantity::{Kind, Quantity};
use crate::unit::UnitError;

#[derive(Debug, Clone, PartialEq)]
pub struct VectorValue {
    pub elements: Vec<Quantity>,
}

/// A dense matrix in row-major order.
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixValue {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<Quantity>,
}

impl MatrixValue {
    pub fn get(&self, row: usize, col: usize) -> Quantity {
        self.data[row * self.cols + col]
    }

    pub fn new(rows: usize, cols: usize, data: Vec<Quantity>) -> MatrixValue {
        debug_assert_eq!(rows * cols, data.len());
        MatrixValue { rows, cols, data }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Scalar(Quantity),
    Complex(ComplexQuantity),
    Vector(VectorValue),
    Matrix(MatrixValue),
    /// A string: a label, a verdict, a lookup key. It is not a quantity — it has
    /// no dimension and no arithmetic — so it is a value of its own rather than
    /// a `Quantity` with a flag. Comparison for equality is the whole of what
    /// can be done with one, which is what the worksheets that carry strings
    /// actually do with them: choose one with `if`, and show it.
    Text(String),
    /// A quantity and its derivative, alive only inside a `derivative(f, x)`
    /// call: the parameter is seeded with one, the body computes with it, and
    /// [`crate::eval`] takes the slope out at the end. It never reaches a
    /// worksheet line, a renderer or a snapshot — every other match on a value
    /// refuses it, which is what makes a missing differentiation rule report
    /// itself instead of quietly answering zero. See [`crate::dual`].
    Dual(crate::dual::DualQuantity),
    /// A computed plot. Boxed because it carries hundreds of samples and every
    /// other value in the tower is a handful of words; an enum sized for this
    /// one would make every scalar as expensive.
    ///
    /// It is a value because it is what an expression evaluated to, and it goes
    /// no further than that: there is no arithmetic on a plot, so every
    /// operation refuses it by name rather than doing something surprising with
    /// its samples.
    Plot(Box<crate::plot::PlotValue>),
}

/// Everything that can go wrong while evaluating.
#[derive(Debug, Clone, PartialEq)]
pub enum EvalError {
    Unit(UnitError),
    UnknownName(String),
    UnknownFunction(String),
    NotAFunction(String),
    WrongArity {
        name: String,
        expected: usize,
        found: usize,
    },
    /// An operation was applied to operand shapes it does not accept.
    TypeMismatch {
        op: &'static str,
        lhs: &'static str,
        rhs: &'static str,
    },
    ShapeMismatch {
        op: &'static str,
        lhs: String,
        rhs: String,
    },
    IndexOutOfBounds {
        index: i64,
        len: usize,
    },
    /// An index that is not a whole number, or carries a unit.
    BadIndex(String),
    WrongIndexCount {
        expected: usize,
        found: usize,
    },
    /// `°C` used as a value in its own right.
    BareAffineUnit(String),
    /// A name was assigned that also names a unit.
    Singular(&'static str),
    NotImplemented(&'static str),
    /// A subexpression failed; the real diagnostic is on that node.
    Poisoned,
    /// This branch of a conditional was not taken, so it was never evaluated.
    ///
    /// Not a failure. It marks the part of the tree that still has to be
    /// *shown* — a worksheet shows its work, and the work includes which arm was
    /// chosen — while keeping it out of everything that looks for errors.
    NotTaken,
    /// Calls nested deeper than [`crate::eval::MAX_DEPTH`].
    ///
    /// In practice: a definition that calls itself without getting closer to an
    /// answer. Recursion *works* here — the conditional is lazy, so
    /// `fn fact(n) = if n <= 1 then 1 else n*fact(n - 1)` terminates — so this
    /// is a ceiling rather than a ban, and it names the function the ceiling
    /// was hit in.
    TooDeep(String),
    /// The name is bound by this worksheet, but the statement that binds it did
    /// not produce a value.
    ///
    /// Distinct from `UnknownName`, and the distinction is the whole point: a
    /// name nothing binds may still be a unit, and one the worksheet *does* bind
    /// may not. Without this, `PF = <error>` followed by `PF` reads as
    /// peta-farads and answers 1e15 F.
    DefinitionFailed(String),
    /// The syntax tree contained a parse error.
    Malformed,
}

impl From<UnitError> for EvalError {
    fn from(e: UnitError) -> Self {
        EvalError::Unit(e)
    }
}

impl From<DualError> for EvalError {
    fn from(e: DualError) -> Self {
        match e {
            DualError::Unit(u) => EvalError::Unit(u),
            // The value exists and the slope does not, which is a different
            // complaint from a unit rule and reads as one.
            DualError::NoSlope => EvalError::Singular(
                "a power with a varying exponent has no slope unless its base is positive",
            ),
        }
    }
}

impl core::fmt::Display for EvalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EvalError::Unit(e) => write!(f, "{e}"),
            EvalError::UnknownName(n) => write!(f, "`{n}` is not defined"),
            EvalError::UnknownFunction(n) => write!(f, "`{n}` is not a known function"),
            EvalError::NotAFunction(n) => write!(f, "`{n}` is not a function"),
            EvalError::TooDeep(n) => write!(
                f,
                "`{n}` is nested more than {} calls deep, so it never reaches an answer",
                crate::eval::MAX_DEPTH
            ),
            EvalError::WrongArity {
                name,
                expected,
                found,
            } => write!(
                f,
                "`{name}` takes {expected} argument{}, but {found} were given",
                if *expected == 1 { "" } else { "s" }
            ),
            EvalError::TypeMismatch { op, lhs, rhs } => {
                write!(f, "cannot apply `{op}` to {lhs} and {rhs}")
            }
            EvalError::ShapeMismatch { op, lhs, rhs } => {
                write!(f, "cannot apply `{op}` to {lhs} and {rhs}")
            }
            EvalError::IndexOutOfBounds { index, len } => {
                write!(f, "index {index} is outside 1..={len}")
            }
            EvalError::BadIndex(what) => write!(f, "an index must be a whole number, found {what}"),
            EvalError::WrongIndexCount { expected, found } => {
                write!(f, "expected {expected} index/indices, found {found}")
            }
            EvalError::BareAffineUnit(n) => write!(
                f,
                "`{n}` is an offset scale and has no value on its own; \
                 write a number before it, as in `20 {n}`"
            ),
            EvalError::Singular(what) => write!(f, "{what}"),
            EvalError::NotImplemented(what) => write!(f, "{what} is not implemented yet"),
            EvalError::Poisoned => write!(f, "a subexpression could not be evaluated"),
            EvalError::NotTaken => write!(f, "this branch was not taken"),
            EvalError::DefinitionFailed(n) => {
                write!(
                    f,
                    "`{n}` has no value: the statement that defines it failed"
                )
            }
            EvalError::Malformed => write!(f, "this expression could not be parsed"),
        }
    }
}

type R<T> = Result<T, EvalError>;

impl Value {
    /// A short noun for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Scalar(_) => "a number",
            Value::Complex(_) => "a complex number",
            Value::Vector(_) => "a vector",
            Value::Matrix(_) => "a matrix",
            Value::Plot(_) => "a plot",
            Value::Text(_) => "a string",
            Value::Dual(_) => "a number being differentiated",
        }
    }

    pub fn shape_name(&self) -> String {
        match self {
            Value::Scalar(_) => "a number".into(),
            Value::Complex(_) => "a complex number".into(),
            Value::Vector(v) => format!("a vector of {}", v.elements.len()),
            Value::Matrix(m) => format!("a {}×{} matrix", m.rows, m.cols),
            Value::Plot(p) => format!("a plot of {}", p.series.len()),
            Value::Text(_) => "a string".into(),
            Value::Dual(_) => "a number being differentiated".into(),
        }
    }

    pub fn scalar(x: f64) -> Value {
        Value::Scalar(Quantity::scalar(x))
    }

    /// The scalar quantity, if this is one.
    pub fn as_scalar(&self) -> Option<Quantity> {
        match self {
            Value::Scalar(q) => Some(*q),
            _ => None,
        }
    }

    /// Both operands as complex quantities, when either of them is one.
    ///
    /// `None` means neither side is complex and the caller should carry on with
    /// the real path — which keeps every real worksheet on exactly the code it
    /// was on before complex numbers existed.
    fn complex_pair(&self, other: &Value) -> Option<R<(ComplexQuantity, ComplexQuantity)>> {
        use Value::*;
        let promote = |v: &Value| match v {
            Complex(c) => Some(Ok(*c)),
            Scalar(q) => Some(ComplexQuantity::promote(q).map_err(EvalError::Unit)),
            // A complex vector is the unbuilt half of the value tower, and
            // saying so is better than dropping an imaginary part into a
            // collection that cannot hold one.
            // A complex derivative is a coherent thing and not this one: the
            // two towers are built for different questions and combining them
            // would need rules nobody here has written.
            Vector(_) | Matrix(_) | Plot(_) | Dual(_) | Text(_) => None,
        };
        if !matches!((self, other), (Complex(_), _) | (_, Complex(_))) {
            return None;
        }
        Some(match (promote(self), promote(other)) {
            (Some(Ok(a)), Some(Ok(b))) => Ok((a, b)),
            (Some(Err(e)), _) | (_, Some(Err(e))) => Err(e),
            _ => Err(EvalError::NotImplemented(
                "a complex element in a vector or matrix",
            )),
        })
    }

    /// The two sides of an operation as duals, when either side is one.
    ///
    /// The same shape as [`Value::complex_pair`], and for the same reason: a
    /// second numeric tower joins the arithmetic at one place rather than at
    /// every operator. Anything on the other side that is not a number has no
    /// derivative rule and says so — differentiating through a vector is a
    /// coherent thing to want and not a thing anybody here has written.
    fn dual_pair(&self, other: &Value) -> Option<R<(DualQuantity, DualQuantity)>> {
        use Value::*;
        if !matches!((self, other), (Dual(_), _) | (_, Dual(_))) {
            return None;
        }
        let promote = |v: &Value| match v {
            Dual(d) => Some(*d),
            // Everything the body reads that is not the variable is a constant
            // for this derivative, whatever it is elsewhere.
            Scalar(q) => Some(DualQuantity::constant(*q)),
            Complex(_) | Vector(_) | Matrix(_) | Plot(_) | Text(_) => None,
        };
        Some(match (promote(self), promote(other)) {
            (Some(a), Some(b)) => Ok((a, b)),
            _ => Err(EvalError::NotImplemented(
                "differentiating through this shape",
            )),
        })
    }

    /// Apply a quantity-level operation element-wise, broadcasting scalars.
    fn zip_with(
        &self,
        other: &Value,
        op: &'static str,
        f: impl Fn(&Quantity, &Quantity) -> Result<Quantity, UnitError> + Copy,
    ) -> R<Value> {
        use Value::*;
        match (self, other) {
            (Scalar(a), Scalar(b)) => Ok(Scalar(f(a, b)?)),

            (Vector(a), Vector(b)) => {
                if a.elements.len() != b.elements.len() {
                    return Err(EvalError::ShapeMismatch {
                        op,
                        lhs: self.shape_name(),
                        rhs: other.shape_name(),
                    });
                }
                let elements = a
                    .elements
                    .iter()
                    .zip(&b.elements)
                    .map(|(x, y)| f(x, y))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Vector(VectorValue { elements }))
            }

            (Matrix(a), Matrix(b)) => {
                if a.rows != b.rows || a.cols != b.cols {
                    return Err(EvalError::ShapeMismatch {
                        op,
                        lhs: self.shape_name(),
                        rhs: other.shape_name(),
                    });
                }
                let data = a
                    .data
                    .iter()
                    .zip(&b.data)
                    .map(|(x, y)| f(x, y))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Matrix(MatrixValue::new(a.rows, a.cols, data)))
            }

            (Scalar(s), Vector(v)) => {
                let elements = v
                    .elements
                    .iter()
                    .map(|x| f(s, x))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Vector(VectorValue { elements }))
            }
            (Vector(v), Scalar(s)) => {
                let elements = v
                    .elements
                    .iter()
                    .map(|x| f(x, s))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Vector(VectorValue { elements }))
            }
            (Scalar(s), Matrix(m)) => {
                let data = m
                    .data
                    .iter()
                    .map(|x| f(s, x))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Matrix(MatrixValue::new(m.rows, m.cols, data)))
            }
            (Matrix(m), Scalar(s)) => {
                let data = m
                    .data
                    .iter()
                    .map(|x| f(x, s))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Matrix(MatrixValue::new(m.rows, m.cols, data)))
            }

            _ => Err(EvalError::TypeMismatch {
                op,
                lhs: self.type_name(),
                rhs: other.type_name(),
            }),
        }
    }

    pub fn add(&self, other: &Value) -> R<Value> {
        if let Some(pair) = self.dual_pair(other) {
            let (a, b) = pair?;
            return Ok(Value::Dual(a.add(&b)?));
        }
        if let Some(pair) = self.complex_pair(other) {
            let (a, b) = pair?;
            return Ok(Value::Complex(a.add(&b)?));
        }
        self.zip_with(other, "+", Quantity::add)
    }

    pub fn sub(&self, other: &Value) -> R<Value> {
        if let Some(pair) = self.dual_pair(other) {
            let (a, b) = pair?;
            return Ok(Value::Dual(a.sub(&b)?));
        }
        if let Some(pair) = self.complex_pair(other) {
            let (a, b) = pair?;
            return Ok(Value::Complex(a.sub(&b)?));
        }
        self.zip_with(other, "-", Quantity::sub)
    }

    /// Multiplication.
    ///
    /// Element-wise between two vectors of equal length, which is what a
    /// tabulated calculation wants — `acc / (2·pi·f)^2` over parallel columns.
    /// Between matrices, and between a matrix and a vector, it is the matrix
    /// product. Use `dot` for an inner product.
    pub fn mul(&self, other: &Value) -> R<Value> {
        use Value::*;
        if let Some(pair) = self.dual_pair(other) {
            let (a, b) = pair?;
            return Ok(Value::Dual(a.mul(&b)?));
        }
        if let Some(pair) = self.complex_pair(other) {
            let (a, b) = pair?;
            return Ok(Complex(a.mul(&b)));
        }
        match (self, other) {
            (Matrix(a), Matrix(b)) => Self::matmul(a, b),
            (Matrix(a), Vector(v)) => {
                let b = MatrixValue::new(v.elements.len(), 1, v.elements.clone());
                let product = Self::matmul(a, &b)?;
                Ok(Self::demote_column(product))
            }
            (Vector(v), Matrix(b)) => {
                let a = MatrixValue::new(1, v.elements.len(), v.elements.clone());
                let product = Self::matmul(&a, b)?;
                Ok(Self::demote_row(product))
            }
            _ => self.zip_with(other, "*", Quantity::mul),
        }
    }

    pub fn div(&self, other: &Value) -> R<Value> {
        if let Some(pair) = self.dual_pair(other) {
            let (a, b) = pair?;
            return Ok(Value::Dual(a.div(&b)?));
        }
        if let Some(pair) = self.complex_pair(other) {
            let (a, b) = pair?;
            return Ok(Value::Complex(a.div(&b)));
        }
        self.zip_with(other, "/", Quantity::div)
    }

    pub fn pow(&self, other: &Value) -> R<Value> {
        if let Some(pair) = self.dual_pair(other) {
            let (a, b) = pair?;
            return Ok(Value::Dual(a.pow(&b)?));
        }
        match (self, other) {
            (Value::Matrix(_), _) => Err(EvalError::NotImplemented("raising a matrix to a power")),
            // A complex *exponent* needs a complex logarithm, which needs a
            // branch cut nothing here can choose; see `ComplexQuantity::pow`.
            (_, Value::Complex(_)) => Err(EvalError::NotImplemented("a complex exponent")),
            (Value::Complex(z), _) => {
                let Some(e) = other.as_scalar() else {
                    return Err(EvalError::TypeMismatch {
                        op: "^",
                        lhs: self.type_name(),
                        rhs: other.type_name(),
                    });
                };
                use crate::complex::PowError;
                z.pow(&e).map(Value::Complex).map_err(|e| match e {
                    PowError::Dimension(u) => EvalError::Unit(u),
                    PowError::Fractional => {
                        EvalError::NotImplemented("a fractional power of a complex number")
                    }
                    PowError::TooManySteps => {
                        EvalError::NotImplemented("a complex power that large")
                    }
                })
            }
            _ => self.zip_with(other, "^", Quantity::pow),
        }
    }

    pub fn neg(&self) -> R<Value> {
        if let Value::Complex(z) = self {
            return Ok(Value::Complex(z.neg()));
        }
        if let Value::Dual(d) = self {
            return Ok(Value::Dual(d.neg()?));
        }
        self.map_quantities(Quantity::neg)
    }

    /// Apply a fallible quantity function to every element.
    pub fn map_quantities(&self, f: impl Fn(&Quantity) -> Result<Quantity, UnitError>) -> R<Value> {
        Ok(match self {
            // Every real-only elementwise function: `sin`, `floor`, `round`.
            // A complex argument is refused by name rather than silently taking
            // the real part.
            Value::Complex(_) => {
                return Err(EvalError::NotImplemented(
                    "this function of a complex number",
                ))
            }
            // A string has no arithmetic and no functions of it: comparing two
            // for equality is the whole of what can be done with one, and every
            // other operation says so rather than inventing a meaning.
            Value::Text(_) => return Err(EvalError::NotImplemented("arithmetic on a string")),
            // A function with no differentiation rule, met while differentiating.
            // Refused rather than applied to the value alone, because that would
            // answer with a slope of zero and be believed. See [`crate::dual`].
            Value::Dual(_) => {
                return Err(EvalError::NotImplemented("the derivative of this function"))
            }
            // There is no arithmetic on a plot. Refused by name, so a worksheet
            // that writes `2*plot(f, 0, 1)` is told what it did rather than
            // being handed a scaled copy of something it did not ask to scale.
            Value::Plot(_) => return Err(EvalError::NotImplemented("arithmetic on a plot")),
            Value::Scalar(q) => Value::Scalar(f(q)?),
            Value::Vector(v) => Value::Vector(VectorValue {
                elements: v.elements.iter().map(&f).collect::<Result<Vec<_>, _>>()?,
            }),
            Value::Matrix(m) => Value::Matrix(MatrixValue::new(
                m.rows,
                m.cols,
                m.data.iter().map(&f).collect::<Result<Vec<_>, _>>()?,
            )),
        })
    }

    fn matmul(a: &MatrixValue, b: &MatrixValue) -> R<Value> {
        if a.cols != b.rows {
            return Err(EvalError::ShapeMismatch {
                op: "*",
                lhs: format!("a {}×{} matrix", a.rows, a.cols),
                rhs: format!("a {}×{} matrix", b.rows, b.cols),
            });
        }
        let mut data = Vec::with_capacity(a.rows * b.cols);
        for i in 0..a.rows {
            for j in 0..b.cols {
                // Summed left to right, as the language specifies. Any other
                // order would give different last bits.
                let mut acc: Option<Quantity> = None;
                for k in 0..a.cols {
                    let term = a.get(i, k).mul(&b.get(k, j))?;
                    acc = Some(match acc {
                        None => term,
                        Some(sum) => sum.add(&term)?,
                    });
                }
                data.push(acc.unwrap_or_else(|| Quantity::scalar(0.0)));
            }
        }
        Ok(Value::Matrix(MatrixValue::new(a.rows, b.cols, data)))
    }

    fn demote_column(v: Value) -> Value {
        match v {
            Value::Matrix(m) if m.cols == 1 => Value::Vector(VectorValue { elements: m.data }),
            other => other,
        }
    }

    fn demote_row(v: Value) -> Value {
        match v {
            Value::Matrix(m) if m.rows == 1 => Value::Vector(VectorValue { elements: m.data }),
            other => other,
        }
    }

    /// Number of elements, for `length`.
    pub fn len(&self) -> usize {
        match self {
            Value::Scalar(_) | Value::Complex(_) | Value::Dual(_) | Value::Text(_) => 1,
            Value::Vector(v) => v.elements.len(),
            Value::Matrix(m) => m.data.len(),
            // Not its sample count: `length` asks how many elements a value
            // has, and a plot is one thing however many points were taken to
            // draw it.
            Value::Plot(_) => 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every element in row-major order, for aggregation.
    pub fn elements(&self) -> Vec<Quantity> {
        match self {
            Value::Scalar(q) => vec![*q],
            Value::Complex(_) | Value::Plot(_) | Value::Dual(_) | Value::Text(_) => vec![],
            Value::Vector(v) => v.elements.clone(),
            Value::Matrix(m) => m.data.clone(),
        }
    }

    pub fn transpose(&self) -> R<Value> {
        match self {
            Value::Matrix(m) => {
                let mut data = Vec::with_capacity(m.data.len());
                for c in 0..m.cols {
                    for r in 0..m.rows {
                        data.push(m.get(r, c));
                    }
                }
                Ok(Value::Matrix(MatrixValue::new(m.cols, m.rows, data)))
            }
            // A vector transposes to a one-row matrix.
            Value::Vector(v) => Ok(Value::Matrix(MatrixValue::new(
                1,
                v.elements.len(),
                v.elements.clone(),
            ))),
            other => Ok(other.clone()),
        }
    }

    /// Determinant by Gaussian elimination with partial pivoting.
    ///
    /// Row operations run in a fixed order so the result is reproducible;
    /// pivoting is by magnitude, with ties broken by the lower row index.
    pub fn det(&self) -> R<Value> {
        let Value::Matrix(m) = self else {
            return Err(EvalError::TypeMismatch {
                op: "det",
                lhs: self.type_name(),
                rhs: "a matrix",
            });
        };
        if m.rows != m.cols {
            return Err(EvalError::Singular("determinant needs a square matrix"));
        }
        let n = m.rows;
        if n == 0 {
            return Ok(Value::scalar(1.0));
        }

        // Work in base-SI magnitudes; the dimension of the determinant is the
        // product of the dimensions taken down the diagonal.
        let mut a: Vec<f64> = m.data.iter().map(|q| q.value).collect();
        // Every term of the Leibniz expansion picks one element from each row and
        // column, so for a dimensionally consistent matrix the determinant's
        // dimension is the product down the diagonal.
        let mut dim = Dimension::DIMENSIONLESS;
        for r in 0..n {
            dim = dim.mul(&m.get(r, r).dim);
        }

        let mut sign = 1.0_f64;
        for col in 0..n {
            let mut pivot = col;
            for r in (col + 1)..n {
                if math::abs(a[r * n + col]) > math::abs(a[pivot * n + col]) {
                    pivot = r;
                }
            }
            if a[pivot * n + col] == 0.0 {
                return Ok(Value::Scalar(Quantity::new(0.0, dim)));
            }
            if pivot != col {
                for k in 0..n {
                    a.swap(col * n + k, pivot * n + k);
                }
                sign = -sign;
            }
            for r in (col + 1)..n {
                let factor = a[r * n + col] / a[col * n + col];
                for k in col..n {
                    a[r * n + k] -= factor * a[col * n + k];
                }
            }
        }

        let mut product = sign;
        for i in 0..n {
            product *= a[i * n + i];
        }
        Ok(Value::Scalar(Quantity::new(product, dim)))
    }

    /// Matrix inverse by Gauss-Jordan elimination with partial pivoting.
    pub fn inv(&self) -> R<Value> {
        let Value::Matrix(m) = self else {
            return Err(EvalError::TypeMismatch {
                op: "inv",
                lhs: self.type_name(),
                rhs: "a matrix",
            });
        };
        if m.rows != m.cols {
            return Err(EvalError::Singular("inverse needs a square matrix"));
        }
        let n = m.rows;

        // Inverting a matrix with mixed dimensions is not generally meaningful,
        // so require a uniform one and invert the dimension wholesale.
        let dim = m.data.first().map_or(Dimension::DIMENSIONLESS, |q| q.dim);
        if m.data.iter().any(|q| q.dim != dim) {
            return Err(EvalError::Singular(
                "inverse needs every element to share one dimension",
            ));
        }

        let mut a: Vec<f64> = m.data.iter().map(|q| q.value).collect();
        let mut inv = vec![0.0; n * n];
        for i in 0..n {
            inv[i * n + i] = 1.0;
        }

        for col in 0..n {
            let mut pivot = col;
            for r in (col + 1)..n {
                if math::abs(a[r * n + col]) > math::abs(a[pivot * n + col]) {
                    pivot = r;
                }
            }
            if a[pivot * n + col] == 0.0 {
                return Err(EvalError::Singular("matrix is singular and has no inverse"));
            }
            if pivot != col {
                for k in 0..n {
                    a.swap(col * n + k, pivot * n + k);
                    inv.swap(col * n + k, pivot * n + k);
                }
            }
            let p = a[col * n + col];
            for k in 0..n {
                a[col * n + k] /= p;
                inv[col * n + k] /= p;
            }
            for r in 0..n {
                if r == col {
                    continue;
                }
                let factor = a[r * n + col];
                if factor == 0.0 {
                    continue;
                }
                for k in 0..n {
                    a[r * n + k] -= factor * a[col * n + k];
                    inv[r * n + k] -= factor * inv[col * n + k];
                }
            }
        }

        let inv_dim = dim.pow(Ratio::int(-1));
        let data = inv
            .into_iter()
            .map(|x| Quantity {
                value: x,
                dim: inv_dim,
                kind: Kind::Interval,
            })
            .collect();
        Ok(Value::Matrix(MatrixValue::new(n, n, data)))
    }
}

#[cfg(test)]
// Exact float comparison is the point here, not an oversight: this engine
// promises bit-reproducible results, so its tests assert bit equality.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn v(xs: &[f64]) -> Value {
        Value::Vector(VectorValue {
            elements: xs.iter().map(|x| Quantity::scalar(*x)).collect(),
        })
    }

    fn m(rows: usize, cols: usize, xs: &[f64]) -> Value {
        Value::Matrix(MatrixValue::new(
            rows,
            cols,
            xs.iter().map(|x| Quantity::scalar(*x)).collect(),
        ))
    }

    fn nums(value: &Value) -> Vec<f64> {
        value.elements().iter().map(|q| q.value).collect()
    }

    #[test]
    fn scalar_arithmetic() {
        let a = Value::scalar(6.0);
        let b = Value::scalar(4.0);
        assert_eq!(nums(&a.add(&b).unwrap()), vec![10.0]);
        assert_eq!(nums(&a.sub(&b).unwrap()), vec![2.0]);
        assert_eq!(nums(&a.mul(&b).unwrap()), vec![24.0]);
        assert_eq!(nums(&a.div(&b).unwrap()), vec![1.5]);
    }

    #[test]
    fn scalars_broadcast_over_collections() {
        let r = Value::scalar(2.0).mul(&v(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(nums(&r), vec![2.0, 4.0, 6.0]);
    }

    #[test]
    fn vectors_multiply_element_wise() {
        let r = v(&[1.0, 2.0, 3.0]).mul(&v(&[4.0, 5.0, 6.0])).unwrap();
        assert_eq!(nums(&r), vec![4.0, 10.0, 18.0]);
    }

    #[test]
    fn mismatched_vector_lengths_are_rejected() {
        assert!(matches!(
            v(&[1.0, 2.0]).add(&v(&[1.0])),
            Err(EvalError::ShapeMismatch { .. })
        ));
    }

    #[test]
    fn matrices_use_the_matrix_product() {
        // [[1,2],[3,4]] * [[5,6],[7,8]] = [[19,22],[43,50]]
        let a = m(2, 2, &[1.0, 2.0, 3.0, 4.0]);
        let b = m(2, 2, &[5.0, 6.0, 7.0, 8.0]);
        assert_eq!(nums(&a.mul(&b).unwrap()), vec![19.0, 22.0, 43.0, 50.0]);
    }

    #[test]
    fn matrix_times_vector_gives_a_vector() {
        let a = m(2, 2, &[1.0, 2.0, 3.0, 4.0]);
        let x = v(&[1.0, 1.0]);
        let r = a.mul(&x).unwrap();
        assert!(matches!(r, Value::Vector(_)));
        assert_eq!(nums(&r), vec![3.0, 7.0]);
    }

    #[test]
    fn nonconforming_matrix_product_is_rejected() {
        assert!(matches!(
            m(2, 3, &[0.0; 6]).mul(&m(2, 2, &[0.0; 4])),
            Err(EvalError::ShapeMismatch { .. })
        ));
    }

    #[test]
    fn transpose() {
        let a = m(2, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let t = a.transpose().unwrap();
        match &t {
            Value::Matrix(x) => {
                assert_eq!((x.rows, x.cols), (3, 2));
                assert_eq!(nums(&t), vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn determinant() {
        assert_eq!(
            nums(&m(2, 2, &[1.0, 2.0, 3.0, 4.0]).det().unwrap()),
            vec![-2.0]
        );
        assert_eq!(
            nums(&m(2, 2, &[1.0, 0.0, 0.0, 1.0]).det().unwrap()),
            vec![1.0]
        );
        // Singular.
        assert_eq!(
            nums(&m(2, 2, &[1.0, 2.0, 2.0, 4.0]).det().unwrap()),
            vec![0.0]
        );
    }

    #[test]
    fn inverse_times_original_is_the_identity() {
        let a = m(2, 2, &[4.0, 7.0, 2.0, 6.0]);
        let product = a.mul(&a.inv().unwrap()).unwrap();
        let got = nums(&product);
        for (i, x) in got.iter().enumerate() {
            let expected = if i % 3 == 0 { 1.0 } else { 0.0 };
            assert!((x - expected).abs() < 1e-12, "got {got:?}");
        }
    }

    #[test]
    fn singular_matrices_have_no_inverse() {
        assert!(matches!(
            m(2, 2, &[1.0, 2.0, 2.0, 4.0]).inv(),
            Err(EvalError::Singular(_))
        ));
    }

    #[test]
    fn matrix_powers_are_refused_rather_than_done_wrong() {
        assert!(matches!(
            m(2, 2, &[1.0, 2.0, 3.0, 4.0]).pow(&Value::scalar(2.0)),
            Err(EvalError::NotImplemented(_))
        ));
    }

    #[test]
    fn a_real_operand_is_promoted_beside_a_complex_one() {
        let c = Value::Complex(ComplexQuantity {
            re: 1.0,
            im: 2.0,
            dim: Dimension::DIMENSIONLESS,
        });
        let sum = c.add(&Value::scalar(1.0)).unwrap();
        assert_eq!(
            sum,
            Value::Complex(ComplexQuantity {
                re: 2.0,
                im: 2.0,
                dim: Dimension::DIMENSIONLESS,
            })
        );
        // And it stays complex: nothing demotes when the imaginary part is
        // zero, so a result's *type* never depends on its value.
        let real_again = c.sub(&c).unwrap();
        assert!(matches!(real_again, Value::Complex(_)), "{real_again:?}");
    }

    #[test]
    fn a_complex_element_in_a_collection_says_so() {
        // The unbuilt half of the value tower. Better named than dropped.
        let c = Value::Complex(ComplexQuantity {
            re: 1.0,
            im: 2.0,
            dim: Dimension::DIMENSIONLESS,
        });
        let v = Value::Vector(VectorValue {
            elements: vec![Quantity::scalar(1.0)],
        });
        assert!(matches!(
            c.add(&v),
            Err(EvalError::NotImplemented(
                "a complex element in a vector or matrix"
            ))
        ));
    }
}
