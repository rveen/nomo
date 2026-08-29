//! End-to-end tests: source text in, evaluated results out.
//!
//! These exercise the whole pipeline — lex, parse, resolve, evaluate — because
//! that is the level at which a worksheet is either right or wrong.

use nomo_core::eval::{run_source, OutcomeKind};
use nomo_core::value::Value;
use nomo_core::{Quantity, UnitTable};

/// Evaluate a worksheet and return the value of its last query.
fn last_query(src: &str) -> Value {
    let (outcomes, diags) = run_source(src);
    let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "unexpected errors in {src:?}: {errors:#?}"
    );
    for o in outcomes.iter().rev() {
        if let OutcomeKind::Query(t) = &o.kind {
            return t.value.clone().expect("query failed");
        }
    }
    panic!("no query in {src:?}");
}

/// The last query's magnitude expressed in `unit`.
///
/// `unit` may be a simple name (`mm`, `°C`) or a compound expression (`dm^3`,
/// `m/s^2`). A simple name goes through the unit table, which is the only route
/// that can reach an offset scale; anything else is evaluated as an expression
/// and divided out, exactly as the engine's own `->` does.
fn last_in(src: &str, unit: &str) -> f64 {
    let q = match last_query(src) {
        Value::Scalar(q) => q,
        other => panic!("expected a scalar, got {other:?}"),
    };
    if let Ok(u) = UnitTable::new().resolve(unit) {
        return q.to_unit(&u).unwrap();
    }
    let Value::Scalar(u) = last_query(unit) else {
        panic!("`{unit}` did not evaluate to a single quantity");
    };
    assert_eq!(q.dim, u.dim, "`{unit}` has the wrong dimension for {src:?}");
    q.value / u.value
}

/// The last query's raw magnitude, for values that are dimensionless anyway.
/// The last statement's value, as the string it is.
fn last_text(src: &str) -> String {
    match last_query(src) {
        Value::Text(t) => t,
        other => panic!("expected a string, got {other:?}"),
    }
}

fn last_raw(src: &str) -> f64 {
    match last_query(src) {
        Value::Scalar(q) => {
            assert!(
                q.is_dimensionless(),
                "expected dimensionless, got {}",
                q.dim
            );
            q.value
        }
        other => panic!("expected a scalar, got {other:?}"),
    }
}

fn magnitudes(v: &Value) -> Vec<f64> {
    v.elements().iter().map(|q| q.value).collect()
}

/// The error messages a worksheet produces.
fn errors(src: &str) -> Vec<String> {
    run_source(src)
        .1
        .into_iter()
        .filter(|d| d.is_error())
        .map(|d| d.message)
        .collect()
}

fn assert_close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() < 1e-9,
        "{what}: got {got}, wanted {want}"
    );
}

// ---- the worked example from the design note ---------------------------

#[test]
fn cylinder_volume() {
    let src = "
r = 5 cm
h = 12 cm
V = pi*r^2*h
V -> dm^3
";
    assert_close(last_in(src, "dm^3"), 0.9424777960769379, "cylinder volume");
}

// ---- units flow through arithmetic -------------------------------------

#[test]
fn units_attach_by_juxtaposition() {
    assert_close(last_in("5 cm", "mm"), 50.0, "5 cm in mm");
    assert_close(last_in("2.5 kN", "N"), 2500.0, "2.5 kN in N");
}

#[test]
fn compound_units_parse_and_evaluate() {
    // The precedence case the parser exists to get right.
    assert_close(last_in("9.81 m/s^2", "m/s^2"), 9.81, "gravity");
    let src = "
m_ = 100 kg
a = 9.81 m/s^2
F = m_*a
F -> kN
";
    assert_close(last_in(src, "kN"), 0.981, "F = ma");
}

#[test]
fn mixed_unit_systems_combine() {
    // 1 in + 1 cm = 3.54 cm
    assert_close(last_in("1 in + 1 cm", "cm"), 3.54, "imperial plus metric");
}

#[test]
fn dimensional_mismatch_is_caught() {
    let e = errors("1 m + 1 s");
    assert_eq!(e.len(), 1);
    assert!(e[0].contains("cannot combine"), "{e:?}");
}

#[test]
fn a_stress_calculation() {
    let src = "
P = 50 kip
A = 12 in^2
sigma = P/A
sigma -> ksi
";
    assert_close(last_in(src, "ksi"), 50.0 / 12.0, "P/A");
}

// ---- affine temperature, end to end ------------------------------------

#[test]
fn affine_literals_evaluate() {
    assert_close(last_in("20 °C", "K"), 293.15, "20°C in K");
    assert_close(last_in("32 °F", "°C"), 0.0, "32°F in °C");
}

#[test]
fn affine_rules_hold_through_the_language() {
    assert_close(last_in("20 °C + 5 K", "°C"), 25.0, "point plus interval");
    assert_close(last_in("20 °C - 15 °C", "K"), 5.0, "point minus point");

    assert!(errors("20 °C + 5 °C")[0].contains("cannot add two temperatures"));
    assert!(errors("2 * (20 °C)")[0].contains("cannot scale"));
}

#[test]
fn a_bare_offset_scale_is_refused_with_advice() {
    let e = errors("x = °C");
    assert_eq!(e.len(), 1);
    assert!(e[0].contains("has no value on its own"), "{e:?}");
    assert!(e[0].contains("20 °C"), "message should show the fix: {e:?}");
}

// ---- vectors and matrices ----------------------------------------------

#[test]
fn vectors_carry_units() {
    let v = last_query("[5, 10, 15] Hz");
    assert_eq!(magnitudes(&v), vec![5.0, 10.0, 15.0]);
}

#[test]
fn the_shaker_calculation() {
    // Element-wise over parallel columns, which is what tabulated engineering
    // calculations look like.
    let src = "
f = [5, 10, 20] Hz
acc = [2.89, 3.04, 3.72] m/s^2
x = acc/(2*pi*f)^2
x
";
    let v = last_query(src);
    let got = magnitudes(&v);
    let want: Vec<f64> = [(2.89, 5.0), (3.04, 10.0), (3.72, 20.0)]
        .iter()
        .map(|(a, f)| a / (2.0 * std::f64::consts::PI * f).powi(2))
        .collect();
    for (g, w) in got.iter().zip(&want) {
        assert_close(*g, *w, "shaker stroke");
    }
}

#[test]
fn indexing_is_one_based() {
    assert_close(last_raw("[10, 20, 30][1]"), 10.0, "first element");
    assert_close(last_raw("[10, 20, 30][3]"), 30.0, "last element");
    assert!(errors("[10, 20, 30][0]")[0].contains("outside 1..=3"));
    assert!(errors("[10, 20, 30][4]")[0].contains("outside 1..=3"));
}

#[test]
fn matrix_indexing_takes_row_then_column() {
    assert_close(last_raw("[[1,2],[3,4]][2,1]"), 3.0, "row 2, column 1");
}

#[test]
fn matrix_algebra() {
    assert_close(last_raw("det([[1,2],[3,4]])"), -2.0, "determinant");
    let v = last_query("inv([[4,7],[2,6]]) * [[4,7],[2,6]]");
    let got = magnitudes(&v);
    for (i, x) in got.iter().enumerate() {
        let want = if i % 3 == 0 { 1.0 } else { 0.0 };
        assert!((x - want).abs() < 1e-12, "identity check: {got:?}");
    }
}

#[test]
fn a_stiffness_solve() {
    let src = "
K = [[4, -1], [-1, 4]] kN/mm
F = [10, 0] kN
d = inv(K)*F
d -> mm
";
    let v = last_query(src);
    let got = magnitudes(&v);
    // Solving [[4,-1],[-1,4]] d = [10,0] gives d = [8/3, 2/3] mm.
    assert_close(got[0] * 1000.0, 8.0 / 3.0, "d1 in mm");
    assert_close(got[1] * 1000.0, 2.0 / 3.0, "d2 in mm");
}

// ---- functions ----------------------------------------------------------

#[test]
fn user_functions_with_units() {
    let src = "
fn area(d) = pi*d^2/4
area(50 mm) -> cm^2
";
    assert_close(
        last_in(src, "cm^2"),
        std::f64::consts::PI * 25.0 / 4.0,
        "circle area",
    );
}

#[test]
fn wrong_arity_is_reported() {
    let e = errors("fn f(a, b) = a+b\nf(1)");
    assert!(e[0].contains("takes 2 arguments"), "{e:?}");
}

#[test]
fn builtins_require_dimensionless_arguments_where_they_must() {
    assert_close(last_raw("sin(pi/2)"), 1.0, "sin");
    // rad is dimensionless, so an angle in degrees works directly.
    assert_close(last_raw("sin(30 °)"), 0.5, "sin of 30 degrees");
    assert!(errors("sin(5 m)")[0].contains("dimensionless"));
}

#[test]
fn sqrt_halves_dimensions_end_to_end() {
    assert_close(last_in("sqrt(16 m^2)", "m"), 4.0, "sqrt of an area");
}

#[test]
fn aggregations() {
    assert_close(last_in("sum([1, 2, 3] m)", "m"), 6.0, "sum");
    assert_close(last_in("max([1, 5, 3] m)", "m"), 5.0, "max");
    assert_close(last_in("min([1, 5, 3] m)", "m"), 1.0, "min");
    assert_close(last_raw("length([1, 5, 3])"), 3.0, "length");
}

// ---- unit declarations --------------------------------------------------

#[test]
fn declared_units_work() {
    let src = "
unit tonf = 2000 lbf
5 tonf -> kN
";
    let table = UnitTable::new();
    let lbf = table.resolve("lbf").unwrap();
    let expected = 5.0 * 2000.0 * lbf.factor / 1000.0;
    assert_close(last_in(src, "kN"), expected, "declared unit");
}

// ---- diagnostics --------------------------------------------------------

#[test]
fn one_mistake_yields_one_diagnostic() {
    // The failure is deep inside the expression; it should be reported once.
    let e = errors("x = 1 m + (2 s * 3)");
    assert_eq!(e.len(), 1, "{e:?}");
}

#[test]
fn the_diagnostic_points_at_the_subexpression_that_failed() {
    let src = "y = 5 + nonexistent*2";
    let (_, diags) = run_source(src);
    let d = diags.iter().find(|d| d.is_error()).expect("an error");
    assert_eq!(d.span.text(src), "nonexistent");
}

#[test]
fn shadowing_a_multi_letter_unit_warns_but_still_works() {
    let (_, diags) = run_source("min = 5\nmin*2");
    let warnings: Vec<_> = diags.iter().filter(|d| !d.is_error()).collect();
    assert_eq!(warnings.len(), 1, "{diags:#?}");
    assert!(warnings[0].message.contains("is also a unit"));
    assert!(diags.iter().all(|d| !d.is_error()));
}

#[test]
fn ordinary_single_letter_variables_do_not_warn() {
    // `V`, `h`, `A`, `F`, `P`, `T` are all unit symbols and all completely
    // conventional variable names. Warning here would fire on nearly every real
    // worksheet and teach people to ignore warnings.
    let (_, diags) = run_source("V = 5 m^3\nh = 2 m\nA = 3 m^2\nF = 4 N\nT = 9 s");
    assert!(diags.is_empty(), "{diags:#?}");
}

#[test]
fn an_unparseable_line_does_not_stop_the_rest() {
    let (outcomes, _) = run_source("a = 1 +\nb = 2\nb");
    let has_b = outcomes
        .iter()
        .any(|o| matches!(&o.kind, OutcomeKind::Assign { name, .. } if name == "b"));
    assert!(has_b, "later lines should still evaluate");
}

// ---- determinism --------------------------------------------------------

#[test]
fn repeated_evaluation_is_bit_identical() {
    let src = "
r = 5 cm
V = pi*r^3*sin(0.7)/log2(9.3)
V
";
    let first = match last_query(src) {
        Value::Scalar(q) => q.value,
        other => panic!("{other:?}"),
    };
    for _ in 0..20 {
        let again = match last_query(src) {
            Value::Scalar(q) => q.value,
            other => panic!("{other:?}"),
        };
        assert_eq!(first.to_bits(), again.to_bits(), "evaluation drifted");
    }
}

#[test]
fn sums_reduce_left_to_right() {
    // These differ in the last bits under any other association, so the test
    // pins the order the language specifies rather than merely being close.
    let src = "sum([1e16, 1.0, -1e16, 1.0])";
    let got = match last_query(src) {
        Value::Scalar(q) => q.value,
        other => panic!("{other:?}"),
    };
    let expected = ((1e16f64 + 1.0) + -1e16) + 1.0;
    assert_eq!(got.to_bits(), expected.to_bits());
}

#[test]
fn a_quantity_survives_a_round_trip_through_its_own_unit() {
    let table = UnitTable::new();
    for (magnitude, unit) in [(1.0, "in"), (250.0, "MPa"), (12.0, "kip"), (20.0, "°C")] {
        let u = table.resolve(unit).unwrap();
        let q = Quantity::from_unit(magnitude, &u);
        assert_close(q.to_unit(&u).unwrap(), magnitude, unit);
    }
}

#[test]
fn the_cross_product_is_three_dimensional_and_carries_units() {
    // r × F: the moment an engineering worksheet actually asks for.
    let v = last_query("cross([1, 0, 0] m, [0, 2, 0] N)");
    assert_eq!(magnitudes(&v), vec![0.0, 0.0, 2.0], "r × F");
    // Parallel vectors cross to zero, in the dimension of the product.
    assert_eq!(
        magnitudes(&last_query("cross([1,0,0] m, [2,0,0] m)")),
        vec![0.0; 3]
    );
    // Right-handed, and it is the sign that makes a moment mean anything.
    assert_eq!(
        magnitudes(&last_query("cross([1,0,0], [0,1,0])")),
        vec![0.0, 0.0, 1.0]
    );
    assert_eq!(
        magnitudes(&last_query("cross([0,1,0], [1,0,0])")),
        vec![0.0, 0.0, -1.0]
    );
    // Two dimensions is a mistake, not a shorthand.
    assert!(errors("cross([1,0], [0,1])")[0].contains("cross"));
}

#[test]
fn shape_and_slicing() {
    assert_close(last_raw("rows([[1,2,3],[4,5,6]])"), 2.0, "rows");
    assert_close(last_raw("cols([[1,2,3],[4,5,6]])"), 3.0, "cols");
    // A vector answers as the column it is, which is what indexing assumes.
    assert_close(last_raw("rows([1,2,3])"), 3.0, "rows of a vector");
    assert_close(last_raw("cols([1,2,3])"), 1.0, "cols of a vector");
    assert_eq!(
        magnitudes(&last_query("row([[1,2,3],[4,5,6]], 2)")),
        vec![4.0, 5.0, 6.0]
    );
    assert_eq!(
        magnitudes(&last_query("col([[1,2,3],[4,5,6]], 3)")),
        vec![3.0, 6.0]
    );
    assert!(errors("col([[1,2],[3,4]], 3)")[0].contains("outside 1..=2"));
}

#[test]
fn joining_columns_and_rows() {
    // `augment` puts operands side by side: two columns become a 2×2.
    let a = last_query("augment([1, 2] m, [3, 4] m)");
    assert_eq!(magnitudes(&a), vec![1.0, 3.0, 2.0, 4.0], "row-major 2×2");
    // `stack` puts one above the other, so two columns become one longer one.
    assert_eq!(
        magnitudes(&last_query("stack([1,2] m, [3,4] m)")),
        vec![1.0, 2.0, 3.0, 4.0]
    );
    // A short column is a mistake, not a shape to be padded.
    assert!(errors("augment([1,2], [3,4,5])")[0].contains("augment"));
}

#[test]
fn sign_is_dimensionless_and_keeps_nan() {
    assert_eq!(
        magnitudes(&last_query("sign([-2 m, 0 m, 5 m])")),
        vec![-1.0, 0.0, 1.0]
    );
    // The sign of a length is a number, not a length.
    assert_close(last_in("sign(-3 m) * 2 m", "m"), -2.0, "dimensionless");
    assert!(last_raw("sign(0/0)").is_nan(), "NaN is not positive");
}

#[test]
fn the_euclidean_norm_keeps_the_dimension() {
    assert_close(last_raw("norm([3, 4])"), 5.0, "3-4-5");
    assert_close(
        last_in("norm([3 MPa, 4 MPa])", "MPa"),
        5.0,
        "a norm of stresses is a stress",
    );
    // Dividing by it is what worksheets actually do with it.
    let unit = last_query("[3 MPa, 4 MPa] / norm([3 MPa, 4 MPa])");
    let m = magnitudes(&unit);
    assert_close(m[0] * m[0] + m[1] * m[1], 1.0, "a unit vector");
}

#[test]
fn a_bracketed_root() {
    let src = "
fn f(x) = x^2 - 2
root(f, 1, 2)
";
    assert_close(last_raw(src), 2f64.sqrt(), "sqrt(2) by bisection");
    // The bracket carries its dimension into the answer.
    let src = "
fn g(x) = x - 3 m
root(g, 0 m, 10 m)
";
    assert_close(last_in(src, "m"), 3.0, "a root in metres");
    // No sign change means the bracket holds no odd number of roots. Refusing is
    // the point: a confident wrong root in a structural calculation is worse
    // than an error.
    let src = "
fn h(x) = x^2 + 1
root(h, 1, 2)
";
    assert!(errors(src)[0].contains("change sign"));
}

#[test]
fn the_identity_matrix() {
    let m = last_query("identity(3)");
    assert_eq!(
        magnitudes(&m),
        vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
    );
    // What the corpus uses it for: `det(S - λ·I)`, the characteristic equation
    // of a stress tensor. The ones are dimensionless so that they take their
    // dimension from `λ`, which is the only thing that makes the subtraction
    // work at all.
    let src = "
S = [[10, 4], [4, 6]] MPa
λ = 2 MPa
S - λ*identity(2)
";
    assert_eq!(
        magnitudes(&last_query(src)),
        vec![8e6, 4e6, 4e6, 4e6],
        "the diagonal shifted by λ",
    );
    // A 1×1 is the scalar it is, the same reading `mat` already gives.
    assert_close(last_raw("identity(1)"), 1.0, "one row");
}

#[test]
fn a_string_binds_and_compares() {
    // What the corpus does with strings: a verdict chosen by a condition, and a
    // grade compared against a key.
    let src = r#"
a = 3 m
a_max = 4 m
if a <= a_max then "singly" else "doubly"
"#;
    assert_eq!(last_text(src), "singly");
    assert_close(last_raw(r#""C24" == "C24""#), 1.0, "equal strings");
    assert_close(last_raw(r#""C24" == "C30""#), 0.0, "different strings");
    assert_close(last_raw(r#""C24" != "C30""#), 1.0, "not equal");
}

#[test]
fn a_string_has_no_arithmetic_and_no_order() {
    // Refused rather than given a meaning. `+` on two strings is the tempting
    // one, and concatenation is not something any corpus worksheet asks for.
    assert!(errors(r#""a" + "b""#)[0].contains("cannot apply"));
    assert!(errors(r#"2*"a""#)[0].contains("cannot apply"));
    assert!(errors(r#"sqrt("a")"#)[0].contains("arithmetic on a string"));
    // Ordering would have to pick a collation, and a worksheet has no way to
    // say which it meant.
    assert!(errors(r#""a" < "b""#)[0].contains("ordering two strings"));
}

#[test]
fn a_string_without_a_closing_quote_is_reported() {
    let (_, diagnostics) = nomo_core::eval::run_source("x = \"open\n");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("closing quote")),
        "{diagnostics:?}",
    );
}

#[test]
fn a_linear_system_solved_from_its_residual() {
    // Statics: three equations for three reactions, which is the shape most of
    // the SMath corpus's system solves have. No algebra — the coefficients come
    // out of evaluating the residual at zero and at one unit of each unknown.
    let src = "
mass = 10 kg
g = 9.81 m/s^2
acc = 2 m/s^2
fn balance(F, A, B) = [F + B - mass*acc, A - mass*g, B - F - A]
solve_linear(balance, [0 N, 0 N, 0 N])
";
    let got = magnitudes(&last_query(src));
    assert_close(got[0], -39.05, "F");
    assert_close(got[1], 98.1, "A");
    assert_close(got[2], 59.05, "B");
    // One unknown answers as the value it is.
    assert_close(
        last_in("fn f(x) = 2*x - 8 m\nsolve_linear(f, [0 m])", "m"),
        4.0,
        "one unknown",
    );
}

#[test]
fn a_mixed_dimension_system_solves() {
    // A moment equation beside a force equation makes the coefficients
    // dimensionally mixed by row, which `inv` refuses and is right to. The
    // dimensions come off and go back on instead.
    let src = "
L = 3 m
fn beam(RA, RB) = [RA + RB - 12 kN, RA*L - 4 kN*L/2]
solve_linear(beam, [0 kN, 0 kN])
";
    let got = magnitudes(&last_query(src));
    assert_close(got[0], 2000.0, "RA in newtons");
    assert_close(got[1], 10000.0, "RB in newtons");
}

#[test]
fn a_system_that_is_not_linear_is_caught_by_its_own_answer() {
    // Putting the answer back is a stronger check than probing for affinity: it
    // says *this is the solution* rather than *this looked affine here*, which
    // is what lets a caller who cannot verify linearity use this safely.
    let src = "fn curved(x, y) = [x^2 + y - 6, x - y - 1]\nsolve_linear(curved, [0, 0])";
    assert!(errors(src)[0].contains("not linear"), "{:?}", errors(src));
    let src = "fn f(x) = x^2 - 9 m^2\nsolve_linear(f, [0 m])";
    assert!(errors(src)[0].contains("not linear"), "{:?}", errors(src));
}

#[test]
fn equations_that_do_not_determine_their_unknowns_are_refused() {
    let src =
        "fn dependent(x, y) = [x + y - 3 m, 2*x + 2*y - 6 m]\nsolve_linear(dependent, [0 m, 0 m])";
    assert!(errors(src)[0].contains("singular"), "{:?}", errors(src));
    // And a residual whose count does not match the unknowns.
    let src = "fn wrong(x, y) = [x + y - 1 m]\nsolve_linear(wrong, [0 m, 0 m])";
    assert!(
        errors(src)[0].contains("one per unknown"),
        "{:?}",
        errors(src)
    );
}

#[test]
fn a_diagonal_matrix_from_a_vector() {
    assert_eq!(
        magnitudes(&last_query("diag([1, 2, 3])")),
        vec![1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0],
    );
    // The zeros carry the diagonal's dimension, which is what makes the result
    // a matrix that can be used: a dimensionless zero beside `3 kg` would make
    // the next addition report a mismatch.
    assert_close(
        last_in("det(diag([3, 4] kg))", "kg^2"),
        12.0,
        "a diagonal determinant is the product of its diagonal",
    );
    // One element is a 1×1, which is the scalar it is.
    assert_close(last_raw("diag(5)"), 5.0, "one element");
}

#[test]
fn a_diagonal_matrix_has_one_dimension() {
    // Refused here rather than one line further on, where the mismatch would be
    // reported against a zero the worksheet never wrote.
    assert!(errors("diag([3 m, 4 s])")[0].contains("one dimension"));
    // And it takes a vector, not a matrix: this makes a matrix out of a vector
    // and does not also read a diagonal back out of one.
    assert!(errors("diag([[1, 2], [3, 4]])")[0].contains("vector"));
}

#[test]
fn an_identity_matrix_needs_a_whole_positive_count() {
    assert!(errors("identity(0)")[0].contains("at least one row"));
    assert!(errors("identity(2.5)")[0].contains("whole number"));
    assert!(errors("identity(2 m)")[0].contains("dimensionless"));
    // The cap is on the elements rather than on `n`: a thousand rows is already
    // a million of them.
    assert!(errors("identity(2000)")[0].contains("million"));
}

// Exact equality on purpose, and the claim under test: a finite difference
// could not make any of these assertions, which is the whole argument for
// carrying the slope through the arithmetic instead.
#[allow(clippy::float_cmp)]
#[test]
fn a_derivative_is_exact() {
    // Polynomials, where the answer is known to the last bit rather than to a
    // tolerance: a finite difference could not do this and that is the reason
    // this is automatic differentiation and not one.
    assert_eq!(last_raw("fn f(x) = x^2\nderivative(f, 3)"), 6.0);
    assert_eq!(last_raw("fn f(x) = x^3 - 2*x\nderivative(f, 2)"), 10.0);
    assert_eq!(last_raw("fn f(x) = 1/x\nderivative(f, 4)"), -0.0625);
    // The chain rule through the worksheet's own functions, which is what
    // makes this usable at all: `Mg` in the SMath corpus is six functions deep.
    assert_eq!(
        last_raw("fn inner(x) = 2*x\nfn outer(x) = inner(x)^3\nderivative(outer, 1)"),
        24.0,
    );
    // A function that never reads its argument has slope zero. Answering is
    // better than refusing: a constant is a fine thing to differentiate.
    assert_eq!(last_raw("fn c(x) = 7\nderivative(c, 3)"), 0.0);
}

#[test]
fn a_derivative_carries_the_dimension_of_the_ratio() {
    // d(πr²)/dr at r = 2 m is 4π m: an area over a length, which falls out of
    // the arithmetic rather than being a rule about differentiation.
    assert_close(
        last_in("fn area(r) = π*r^2\nderivative(area, 2 m)", "m"),
        4.0 * std::f64::consts::PI,
        "an area differentiated by a length is a length",
    );
    // And the other way round: a length over a time is a speed.
    assert_close(
        last_in(
            "fn fall(t) = 1/2*9.81 m/s^2*t^2\nderivative(fall, 3 s)",
            "m/s",
        ),
        3.0 * 9.81,
        "a distance differentiated by a time is a speed",
    );
}

#[test]
fn the_elementary_functions_carry_their_slopes() {
    assert_close(
        last_raw("fn f(x) = sin(x)\nderivative(f, 0)"),
        1.0,
        "sin' = cos",
    );
    assert_close(
        last_raw("fn f(x) = exp(2*x)\nderivative(f, 0)"),
        2.0,
        "the chain rule through exp",
    );
    assert_close(
        last_raw("fn f(x) = ln(x)\nderivative(f, 4)"),
        0.25,
        "ln' = 1/x",
    );
    assert_close(
        last_raw("fn f(x) = sqrt(x)\nderivative(f, 9)"),
        1.0 / 6.0,
        "the square root halves the slope as well as the dimension",
    );
    // `x^x` needs the general power rule, both sides varying.
    assert_close(
        last_raw("fn f(x) = x^x\nderivative(f, 2)"),
        4.0 * (2f64.ln() + 1.0),
        "x^x",
    );
}

#[allow(clippy::float_cmp)]
#[test]
fn a_second_derivative() {
    // `derivative(f, x, 2)`. Exact, like the first: the chain rule is carried
    // twice rather than the first derivative being differenced again.
    assert_eq!(last_raw("fn f(x) = x^4\nderivative(f, 3, 2)"), 108.0);
    assert_eq!(last_raw("fn f(x) = x^3\nderivative(f, 2, 2)"), 12.0);
    // Through the worksheet's own functions, where the square in
    // `f(u)'' = f''(u)u'^2 + f'(u)u''` is what a first-order rule applied twice
    // would get wrong.
    assert_eq!(
        last_raw("fn u(x) = 2*x\nfn v(x) = u(x)^3\nderivative(v, 1, 2)"),
        48.0,
    );
    assert_eq!(last_raw("fn t(x) = ln(x)\nderivative(t, 4, 2)"), -0.0625);
    // Both sides of a power varying, which needs the general rule twice over.
    assert_close(
        last_raw("fn w(x) = x^x\nderivative(w, 2, 2)"),
        4.0 * ((2f64.ln() + 1.0).powi(2) + 0.5),
        "x^x",
    );
    // The inflection of a Gaussian is at one standard deviation, where the
    // second derivative is exactly zero. This is `normaldist.sm`'s question.
    assert_eq!(
        last_raw("fn n(x) = exp(0 - x^2/2)\nderivative(n, 1, 2)"),
        0.0
    );
}

#[test]
fn a_second_derivative_divides_the_dimension_twice() {
    // An acceleration out of a distance and a time, which is what the mechanics
    // corpus asks for.
    assert_close(
        last_in(
            "fn fall(t) = 1/2*9.81 m/s^2*t^2\nderivative(fall, 3 s, 2)",
            "m/s^2",
        ),
        9.81,
        "a distance differentiated twice by a time is an acceleration",
    );
}

#[test]
fn an_order_above_the_second_is_refused() {
    // A third would need a third component on the dual and a third column in
    // every rule. Both are ordinary work and neither is written, so the ceiling
    // is a number rather than a surprise.
    let src = "fn f(x) = x^4\nderivative(f, 2, 3)";
    assert!(
        errors(src)[0].contains("above the second"),
        "{:?}",
        errors(src)
    );
}

#[test]
fn a_function_with_no_rule_refuses_rather_than_answering_zero() {
    // The failure mode this whole design avoids: a missing rule reported as a
    // slope of zero would be believed, because zero is a plausible derivative.
    let src = "fn f(x) = floor(x)\nderivative(f, 2.5)";
    assert!(errors(src)[0].contains("derivative"), "{:?}", errors(src));
}

#[allow(clippy::float_cmp)]
#[test]
fn a_piecewise_definition_differentiates_on_the_branch_it_takes() {
    // A comparison inside a derivative asks about the value, so a clamp gives
    // the slope of the side it is on. Exact everywhere except at the switch,
    // where there is no derivative to be exact about.
    let src = "fn clamp(x) = if x > 10 then 10 else x\n";
    assert_eq!(last_raw(&format!("{src}derivative(clamp, 4)")), 1.0);
    assert_eq!(last_raw(&format!("{src}derivative(clamp, 40)")), 0.0);
}

#[test]
fn a_derivative_is_what_a_root_search_can_be_given() {
    // The shape a resonant-tank worksheet is written in, and why this exists:
    // the peak of a curve is where its derivative crosses zero, so the two
    // numerical methods compose. sin' = cos, whose zero in [0, 3] is π/2.
    let src = "
fn f(x) = sin(x)
fn slope(x) = derivative(f, x)
roots(slope, 0, 3)
";
    assert_close(
        last_raw(src),
        std::f64::consts::FRAC_PI_2,
        "the peak of a sine",
    );
}

#[test]
fn every_root_in_a_window() {
    // The shear force of `5.1.sm`, which is where the semantics came from: a
    // parabola whose two zeros are −1 ± √156/6.
    let src = "
fn v(x) = 5*2^2 - 6*x*(x + 2)
roots(v, -4, 4)
";
    let both = magnitudes(&last_query(src));
    assert_eq!(both.len(), 2, "two zeros in the window");
    // Increasing order, which is scan order.
    assert_close(both[0], -1.0 - 156f64.sqrt() / 6.0, "the lower zero");
    assert_close(both[1], -1.0 + 156f64.sqrt() / 6.0, "the upper zero");
    // A window holding one of them answers with the value rather than a vector
    // of one, which is what makes the common case read like arithmetic.
    let src = "
fn v(x) = 5*2^2 - 6*x*(x + 2)
roots(v, 0, 2)
";
    assert_close(last_raw(src), -1.0 + 156f64.sqrt() / 6.0, "one zero");
}

#[test]
fn a_window_carries_its_dimension_into_every_root() {
    let src = "
fn sag(x) = x - 3 m
roots(sag, 0 m, 10 m)
";
    assert_close(last_in(src, "m"), 3.0, "a root in metres");
    // A window and an answer are different dimensions, and the window's is the
    // one the roots are in.
    let src = "
fn drop(x) = x*x - 4 m^2
roots(drop, 0 m, 10 m)
";
    assert_close(
        last_in(src, "m"),
        2.0,
        "the root of an area equation is a length",
    );
}

#[test]
fn a_scan_that_finds_nothing_says_so() {
    // No sign change anywhere in 200 intervals. Answering with the nearest
    // sample would be inventing a root; the whole point of a scan is that it
    // reports what it saw.
    let src = "
fn h(x) = x^2 + 1
roots(h, -4, 4)
";
    assert!(
        errors(src)[0].contains("no sign change"),
        "{:?}",
        errors(src)
    );
    // A window of no width has no scan in it.
    let src = "
fn f(x) = x
roots(f, 2, 2)
";
    assert!(
        errors(src)[0].contains("two different ends"),
        "{:?}",
        errors(src)
    );
}

#[test]
fn a_root_exactly_on_a_sample_is_found_once() {
    // `x` is zero at the 100th of 200 samples across [−4, 4], and the two
    // intervals either side of it both change sign. Reporting it twice would
    // make a vector out of one answer.
    let src = "
fn f(x) = x
roots(f, -4, 4)
";
    assert_close(last_raw(src), 0.0, "the zero itself");
}

#[test]
fn a_scan_reads_a_gap_as_no_information() {
    // `1/x` changes sign across its pole, and a scan that treated the pole as a
    // crossing would report a root where the function has no value at all. The
    // sample at the pole is not finite, so no bracket is drawn across it — and
    // the two real roots on either side of it are still found.
    let src = "
fn f(x) = (x^2 - 1)/x
roots(f, -4, 4)
";
    let found = magnitudes(&last_query(src));
    assert_eq!(found.len(), 2, "{found:?}");
    assert_close(found[0], -1.0, "the lower root");
    assert_close(found[1], 1.0, "the upper root");
}

#[test]
fn a_definite_integral() {
    assert_close(
        last_raw("fn q(x) = x^2\nintegral(q, 0, 3)"),
        9.0,
        "∫x² over 0..3",
    );
    // Exact for a cubic, which is what Simpson's rule guarantees.
    assert_close(
        last_raw("fn c(x) = x^3\nintegral(c, 0, 2)"),
        4.0,
        "∫x³ over 0..2",
    );
    // Reversing the limits reverses the sign, as it must.
    assert_close(
        last_raw("fn q2(x) = x^2\nintegral(q2, 3, 0)"),
        -9.0,
        "reversed limits",
    );
    // The dimension falls out of f(x)·dx: a distributed load integrates to a
    // force with no rule about integration needed.
    let src = "
fn w(x) = 10 kN/m^2 * x
integral(w, 0 m, 3 m)
";
    assert_close(last_in(src, "kN"), 45.0, "a triangular load");
}

#[test]
fn several_curves_on_one_plot() {
    // Every argument but the last two names a curve, and they are drawn in the
    // order they were written — a legend reads down the same order.
    let src = "
fn a(x) = x^2
fn b(x) = 0 - x^2
plot(a, b, 0 - 2, 2)
";
    let Value::Plot(p) = last_query(src) else {
        panic!("expected a plot");
    };
    let names: Vec<&str> = p.series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["a", "b"]);
    // One span, sampled once: the curves are comparable because they were
    // asked the same questions.
    assert!(p
        .series
        .iter()
        .all(|s| s.points.len() == nomo_core::plot::SAMPLES));
    assert_eq!(p.y_range(), Some((-4.0, 4.0)));

    // One vertical axis, so one dimension across the curves as well as along
    // each: a length beside a plain number has no chart.
    let src = "
fn plain(x) = x
fn metres(x) = x m
plot(plain, metres, 0, 1)
";
    assert!(errors(src)[0].contains("cannot combine"));

    // And an argument before the span that is not a plain name is refused
    // rather than evaluated: there are no function values to evaluate it to.
    let src = "
fn a2(x) = x
plot(a2, 2, 0, 1)
";
    assert!(errors(src)[0].contains("must be a plain name"));
}

#[test]
fn a_plot_of_measured_points() {
    // An n×2 table: x in the first column, y in the second — the shape
    // `augment` builds, and the shape the SMath corpus plots. No span is
    // written, because the points brought their own x.
    let src = "
data = [[0 s, 1 m], [2 s, 5 m], [3 s, 4 m]]
plot(data)
";
    let Value::Plot(p) = last_query(src) else {
        panic!("expected a plot");
    };
    assert_eq!(p.extent, nomo_core::plot::Extent::Measured);
    assert_eq!(p.series[0].name, "data");
    assert_eq!(p.series[0].points, [(0.0, 1.0), (2.0, 5.0), (3.0, 4.0)]);
    // The extent of the data, which is the only span a table has. Not the order
    // it was written in: the third row goes back down, and the axis still ends
    // at the largest x.
    assert_close(p.from, 0.0, "the table's first x");
    assert_close(p.to, 3.0, "the table's last x");

    // Anything but a table of two columns is refused by name rather than
    // guessed at.
    assert!(errors("plot([1, 2, 3])")[0].contains("two columns wide"));
    assert!(errors("plot([[1, 2, 3], [4, 5, 6]])")[0].contains("two columns wide"));

    // One dimension per axis, across the tables as well as down a column.
    let src = "
a = [[0 s, 1 m], [1 s, 2 m]]
b = [[0 s, 1 kg], [1 s, 2 kg]]
plot(a, b)
";
    assert!(errors(src)[0].contains("cannot combine"));

    // A span turns the leading arguments back into function names, and a table
    // is not one. The error says which name it could not find rather than
    // silently reading the table as a curve.
    assert!(errors("d = [[0, 1]]\nplot(d, 0, 1)")[0].contains("not a known function"));
}

#[test]
fn a_recursion_that_never_ends_is_an_error_and_not_a_crash() {
    // Before the ceiling this aborted the process: no diagnostic, no worksheet,
    // and in the browser a tab that dies with nothing on the page. Three lines
    // anyone can type.
    let src = "
fn f(x) = f(x)
f(2)
";
    assert!(errors(src)[0].contains("never reaches an answer"));

    // Recursion itself is not the mistake. The conditional is lazy, so one that
    // reaches a base case answers, and the ceiling is well above the depth any
    // worksheet nests.
    assert_close(
        last_raw("fn fact(n) = if n <= 1 then 1 else n*fact(n - 1)\nfact(6)"),
        720.0,
        "6!",
    );
    assert_close(
        last_raw("fn count(k) = if k <= 0 then 0 else 1 + count(k - 1)\ncount(60)"),
        60.0,
        "sixty nested calls",
    );
}

// ---- the second batch of builtins ---------------------------------------

#[test]
fn remainder_hypotenuse_and_roots_carry_dimensions() {
    // `mod` keeps the sign of the dividend, which is what SMath's does — its
    // `Mod` is `rem` on two doubles — and refuses a zero divisor as it does.
    assert_close(last_in("mod(7 m, 2 m)", "m"), 1.0, "7 mod 2");
    assert_close(
        last_raw("mod(-7, 2)"),
        -1.0,
        "the sign follows the dividend",
    );
    assert!(errors("mod(7, 0)")[0].contains("by zero"));
    assert!(errors("mod(7 m, 2 s)")[0].contains("cannot combine"));

    assert_close(last_in("hypot(3 m, 4 m)", "m"), 5.0, "3-4-5");

    // The nth root divides the dimension, which is the reason the exponents are
    // rational rather than integer.
    assert_close(last_in("nthroot(8 m^3, 3)", "m"), 2.0, "the side of a cube");
    assert_close(
        last_raw("nthroot(-32, 5)"),
        -2.0,
        "an odd root of a negative",
    );
    assert!(errors("nthroot(-8, 2)")[0].contains("odd whole index"));
    assert!(errors("nthroot(8, 0)")[0].contains("nonzero index"));

    // A logarithm always states its base. Every `log` call in either corpus
    // does, so requiring it costs nothing real.
    assert_close(last_raw("log(8, 2)"), 3.0, "log base 2 of 8");
    assert!(errors("log(8)")[0].contains("argument"));
}

#[test]
fn collections_fold_and_order() {
    // `product` multiplies dimensions where `sum` requires them to agree.
    assert_close(last_in("product([2 m, 3 m, 4 m])", "m^3"), 24.0, "a volume");

    assert_close(last_in("mean([10, 20, 60] kN)", "kN"), 30.0, "the mean");
    // An even count averages the two middle readings; an odd one takes the
    // middle. Both need the collection sorted first, and neither disturbs it.
    assert_close(last_in("median([5, 1, 9, 3] mm)", "mm"), 4.0, "even median");
    assert_close(last_in("median([5, 1, 9] mm)", "mm"), 5.0, "odd median");

    // In millimetres, because 9 mm is 0.009000000000000001 m and comparing the
    // stored magnitudes exactly would be testing binary64 rather than `sort`.
    let sorted = magnitudes(&last_query("sort([5, 1, 9, 3] mm)"));
    for (got, want) in sorted.iter().zip([1.0, 3.0, 5.0, 9.0]) {
        assert_close(got * 1000.0, want, "sorted");
    }
    assert_eq!(
        magnitudes(&last_query("reverse([1, 2, 3])")),
        vec![3.0, 2.0, 1.0]
    );

    // Ordering across dimensions would mean comparing magnitudes in base units,
    // which is an answer with no meaning.
    assert!(errors("sort([1 m, 1 s])")[0].contains("one dimension"));
    assert!(errors("mean([1 m, 1 s])")[0].contains("one dimension"));
    assert!(errors("mean([20 °C, 30 °C])")[0].contains("offset"));
}

#[test]
fn a_matrix_can_be_traced_and_cut() {
    assert_close(last_raw("trace([[1, 2], [3, 4]])"), 5.0, "the diagonal");
    assert!(errors("trace([[1, 2, 3], [4, 5, 6]])")[0].contains("square"));

    // `submatrix(m, r1, r2, c1, c2)`, inclusive and counting from one — the
    // argument order and the inclusivity are SMath's, read from
    // `TMatrix::Submatrix(startRow, endRow, startCol, endCol)`.
    let m = "K = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]\n";
    assert_eq!(
        magnitudes(&last_query(&format!("{m}submatrix(K, 2, 3, 1, 2)"))),
        vec![4.0, 5.0, 7.0, 8.0]
    );
    // A single column comes back as a vector, which is what a column is here.
    assert_eq!(
        magnitudes(&last_query(&format!("{m}submatrix(K, 1, 3, 2, 2)"))),
        vec![2.0, 5.0, 8.0]
    );
    assert!(errors(&format!("{m}submatrix(K, 0, 2, 1, 2)"))[0].contains("outside"));
    assert!(errors(&format!("{m}submatrix(K, 1, 9, 1, 2)"))[0].contains("outside"));
    assert!(errors(&format!("{m}submatrix(K, 3, 1, 1, 2)"))[0].contains("at or after"));
}

#[test]
fn the_reciprocal_and_inverse_hyperbolic_functions() {
    // Reciprocals of the three that exist, so they cannot drift in the last
    // bits from `1/tan(x)` written out.
    assert_close(last_raw("cot(0.5)"), 1.0 / (0.5_f64).tan(), "cot");
    assert_close(last_raw("sec(0.5)"), 1.0 / (0.5_f64).cos(), "sec");
    assert_close(last_raw("csc(0.5)"), 1.0 / (0.5_f64).sin(), "csc");
    assert_close(last_raw("asinh(1)"), (1.0_f64).asinh(), "asinh");
    assert_close(last_raw("acosh(2)"), (2.0_f64).acosh(), "acosh");
    assert_close(last_raw("atanh(0.5)"), (0.5_f64).atanh(), "atanh");
    // Dimensionless in, as the rest of the trigonometry is.
    assert!(errors("cot(1 m)")[0].contains("dimensionless"));
}

// ---- interpolation ------------------------------------------------------

#[test]
fn linterp_reads_a_table() {
    // Midpoint of the second segment: 235 MPa at 100 K, 205 at 200 K.
    let table = "T = [20, 100, 200, 300] K\nFy = [250, 235, 205, 170] MPa\n";
    assert_close(
        last_in(&format!("{table}linterp(T, Fy, 150 K)"), "MPa"),
        220.0,
        "halfway down the second segment",
    );
    // The knots themselves, including both ends.
    for (at, want) in [("20 K", 250.0), ("200 K", 205.0), ("300 K", 170.0)] {
        assert_close(
            last_in(&format!("{table}linterp(T, Fy, {at})"), "MPa"),
            want,
            at,
        );
    }
}

#[test]
fn linterp_carries_dimensions_where_smath_drops_them() {
    // SMath's `linterp` takes `ToDouble()` of every entry and hands back a bare
    // number, which is why the one dimensioned use in either corpus divides the
    // units out by hand and multiplies them back. That form still works —
    assert_close(
        last_raw("x = [0, 1, 2]\ny = [0, 10, 20]\nlinterp(x, y, 1.5)"),
        15.0,
        "the dimensionless form SMath forces",
    );
    // — and so does the one it forced people to avoid.
    assert_close(
        last_in(
            "x = [0, 1, 2] m\ny = [0, 10, 20] kN\nlinterp(x, y, 1.5 m)",
            "kN",
        ),
        15.0,
        "a table that keeps its units",
    );
}

#[test]
fn linterp_refuses_what_it_cannot_answer() {
    let table = "T = [20, 100, 200] K\nFy = [250, 235, 205] MPa\n";
    // Outside the table. SMath extrapolates here, silently, off the slope of
    // the first or last segment; a material table asked for a temperature it
    // never covered is exactly where a confident wrong number does harm.
    for at in ["10 K", "500 K"] {
        let e = errors(&format!("{table}linterp(T, Fy, {at})"));
        assert!(
            e.iter().any(|m| m.contains("will not extrapolate")),
            "{at} should be refused, got {e:?}"
        );
    }
    // Not increasing. SMath sorts the pairs instead, which quietly repairs a
    // table whose columns were passed the wrong way round.
    assert!(errors("linterp([3, 2, 1], [1, 2, 3], 2)")[0].contains("strictly increasing"));
    assert!(errors("linterp([1, 1, 2], [1, 2, 3], 1)")[0].contains("strictly increasing"));
    // Shapes and dimensions.
    assert!(errors("linterp([1, 2, 3], [1, 2], 2)")[0].contains("linterp"));
    assert!(errors("linterp([1], [1], 1)")[0].contains("at least two points"));
    assert!(errors(&format!("{table}linterp(T, Fy, 5 m)"))[0].contains("cannot combine"));
    // An offset scale cannot take part in a weighted sum, the same rule a unit
    // declaration follows.
    assert!(errors("linterp([20 °C, 30 °C], [1, 2], 25 °C)")[0].contains("offset scale"));
}

// ---- checks -------------------------------------------------------------

/// The verdicts a worksheet reached.
fn checks(src: &str) -> nomo_core::doc::Checks {
    nomo_core::Sheet::new(src).checks()
}

#[test]
fn a_check_reports_a_verdict() {
    let c = checks("sigma = 142 MPa\nlimit = 160 MPa\ncheck sigma <= limit\n");
    assert_eq!((c.total, c.passed, c.failed), (1, 1, 0));

    let c = checks("d = 12 mm\ncheck d >= 16 mm\n");
    assert_eq!((c.total, c.passed, c.failed), (1, 0, 1));
}

#[test]
fn a_failed_check_is_not_an_error() {
    // The distinction the whole statement exists for. The arithmetic is right
    // and the design does not hold; a worksheet that reported that as an error
    // would put "this part is overstressed" in the same bucket as "this name is
    // not defined", and nothing downstream could tell them apart.
    let sheet = nomo_core::Sheet::new("d = 12 mm\ncheck d >= 16 mm\n");
    assert!(!sheet.has_errors(), "a failed check must not be an error");
    assert!(
        sheet.diagnostics().is_empty(),
        "a failed check must not produce a diagnostic: {:?}",
        sheet.diagnostics()
    );
    assert_eq!(sheet.checks().failed, 1);
}

#[test]
fn a_check_needs_a_condition() {
    // Anything that is not 1 or 0 is refused rather than read as true. A check
    // that passes because `5 m` is "truthy" hides exactly the mistake it exists
    // to catch, so this is strict on purpose.
    for src in [
        "sigma = 142 MPa\ncheck sigma\n",
        "check \"a string\"\n",
        "check 0.5\n",
        "check [1, 1]\n",
    ] {
        let e = errors(src);
        assert!(
            e.iter().any(|m| m.contains("needs a condition")),
            "{src:?} should be refused, got {e:?}"
        );
        assert_eq!(checks(src).undecided, 1, "{src:?}");
    }

    // And a condition that cannot be evaluated is undecided rather than failed:
    // there is a difference between a design that does not hold and one nobody
    // could work out.
    let c = checks("check 1 m <= 1 s\n");
    assert_eq!((c.failed, c.undecided), (0, 1));
}

#[test]
fn a_check_takes_part_in_the_dependency_graph() {
    // A check reads names, so editing what it reads has to re-evaluate it.
    // Without this it would keep reporting a verdict on the previous numbers.
    let mut sheet = nomo_core::Sheet::new("d = 12 mm\ncheck d >= 16 mm\n");
    assert_eq!(sheet.checks().failed, 1);
    let r = sheet.update("d = 20 mm\ncheck d >= 16 mm\n");
    assert!(
        r.evaluated.contains(&1),
        "the check should have been re-evaluated, got {:?}",
        r.evaluated
    );
    assert_eq!(sheet.checks().passed, 1);
}

#[test]
fn brackets_and_calls_multiply_and_are_bounded_together() {
    // Both ceilings were respected here and the stack ran out anyway: 120
    // brackets is under `MAX_NEST`, one call is under `MAX_DEPTH`, and 120
    // brackets *inside* 64 calls is some 7 700 nested evaluations. The shipped
    // WebAssembly build trapped on it, which in the browser took the editing
    // session with it. `MAX_EVAL_NEST` counts what actually consumes the stack.
    // On a thread with room, because a *debug* test thread gets 2 MiB and frames
    // several times larger than release — so it can overflow on the very
    // worksheet this checks is refused, and report a stack overflow instead of
    // the refusal. The limit is set from what the release WebAssembly build
    // carries; see `MAX_EVAL_NEST`.
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            let deep = format!(
                "fn f(x) = {}f(x){}\ny = f(1)\n",
                "(".repeat(120),
                ")".repeat(120)
            );
            assert!(!errors(&deep).is_empty(), "the product must be refused");
        })
        .expect("spawn")
        .join()
        .expect("refusing must not take the process down");

    // The other side of the same number: the limit has to leave room for what
    // the language says works. `MAX_DEPTH` recursion of an ordinary definition
    // costs several nested evaluations per call, and it still answers.
    assert_close(
        last_raw("fn fact(n) = if n <= 1 then 1 else n*fact(n - 1)\nfact(60)"),
        8.320987112741392e81,
        "60! at the call ceiling",
    );

    // And an expression that is deep but not recursive is untouched: this is a
    // bound on the product, not a second bracket limit.
    assert_close(
        last_raw(&format!("{}2 + 3{}", "(".repeat(120), ")".repeat(120))),
        5.0,
        "120 brackets and no calls",
    );
}

#[test]
fn a_vector_takes_its_column_index() {
    // `rows` and `cols` have always answered `n` and `1` for a vector, and
    // `augment`, `stack` and `reshape` all treat it as that column. Indexing
    // was the one corner that did not, so `v[i, 1]` was an error while
    // `rows(v)` said how many there were.
    assert_close(last_raw("v = [10, 20, 30]\nv[2, 1]"), 20.0, "v[2, 1]");
    assert_close(last_raw("v = [10, 20, 30]\nv[2]"), 20.0, "v[2]");
    // A column of one has no second column.
    assert!(errors("v = [10, 20, 30]\nv[2, 2]")[0].contains("outside 1..=1"));
    // And the element index is still bounds-checked on the way past.
    assert!(errors("v = [10, 20, 30]\nv[9, 1]")[0].contains("outside 1..=3"));
    // A matrix is unaffected: two indices are row and column as before, and one
    // still reads row-major.
    assert_close(last_raw("K = [[1, 2], [3, 4]]\nK[2, 1]"), 3.0, "K[2, 1]");
}
