//! Rendering tests: source text in, worked-through output out.
//!
//! These pin the shape of what a reader sees, which is the thing the golden-file
//! suite will diff. They are written as whole-worksheet comparisons rather than
//! unit tests of the walker, because the output is only right or wrong as a whole.

use nomo_core::render::{text, RenderOptions};
use nomo_core::Sheet;

fn render(src: &str) -> String {
    text::render(&Sheet::new(src), &RenderOptions::default())
}

fn lines(src: &str) -> Vec<String> {
    render(src).lines().map(str::to_string).collect()
}

// ---- the design note's worked example -----------------------------------

#[test]
fn the_cylinder_shows_its_work() {
    let out = lines(
        "\
r = 5 cm
h = 12 cm
V = pi*r^2*h
V -> dm^3",
    );
    assert_eq!(
        out,
        vec![
            "r = 5 cm",
            "h = 12 cm",
            "V = π·r²·h = π·(5 cm)²·(12 cm) = 0.000942478 m³",
            "V = 0.942478 dm³",
        ]
    );
}

#[test]
fn one_line_carries_all_three_columns() {
    // Symbolic, substituted, result — the whole point of the trace.
    let out = lines("r = 5 cm\nh = 12 cm\nV = pi*r^2*h -> dm^3");
    assert_eq!(out[2], "V = π·r²·h = π·(5 cm)²·(12 cm) = 0.942478 dm³");
}

// ---- substitution -------------------------------------------------------

#[test]
fn substitution_uses_the_unit_the_binding_was_written_in() {
    // Not `0.05 m`: the reader wants the number they typed.
    let out = lines("r = 5 cm\nd = 2*r");
    assert_eq!(out[1], "d = 2·r = 2·(5 cm) = 0.1 m");
}

#[test]
fn constants_are_not_expanded() {
    // Substituting 3.14159 for π lengthens the line and says nothing.
    let out = lines("r = 2 m\na = pi*r^2");
    assert!(out[1].contains("π·(2 m)²"), "{out:?}");
}

#[test]
fn substituting_a_bare_name_is_suppressed_as_noise() {
    let out = lines("x = 5 m\nx -> mm");
    assert_eq!(out[1], "x = 5000 mm");
}

#[test]
fn a_literal_quantity_is_not_restated() {
    // `g = 9.81 m/s²` is already the answer; `= 9.81 m·s⁻²` adds a column and
    // no information.
    assert_eq!(lines("g = 9.81 m/s^2"), vec!["g = 9.81 m/s²"]);
    assert_eq!(lines("f = [5, 10] Hz"), vec!["f = [5, 10] Hz"]);
}

// ---- parenthesisation ---------------------------------------------------

#[test]
fn substituted_values_are_bracketed_where_precedence_demands() {
    // `5 cm` is a product, so squaring it needs brackets.
    let out = lines("r = 5 cm\na = r^2");
    assert!(out[1].contains("(5 cm)²"), "{out:?}");
    assert!(!out[1].contains("5 cm²"), "{out:?}");
}

#[test]
fn redundant_brackets_are_not_reproduced() {
    // The user's own parentheses are dropped and re-added by precedence, so
    // they never double up.
    let out = lines("x = ((2)) + ((3))");
    assert_eq!(out[0], "x = 2 + 3 = 5");
}

#[test]
fn brackets_appear_where_they_change_meaning() {
    let out = lines("x = (2 + 3)*4");
    assert_eq!(out[0], "x = (2 + 3)·4 = 20");
}

#[test]
fn power_is_right_associative_in_output_too() {
    let out = lines("x = 2^3^2");
    // Mixed notation would read ambiguously, so the whole chain uses carets.
    assert_eq!(out[0], "x = 2^3^2 = 512");
}

// ---- notation -----------------------------------------------------------

#[test]
fn juxtaposition_stays_juxtaposition() {
    // `5 cm`, never `5·cm`. Explicit `*` does print as `·`.
    assert_eq!(lines("x = 5 cm"), vec!["x = 5 cm"]);
    assert_eq!(lines("y = 2*3"), vec!["y = 2·3 = 6"]);
}

#[test]
fn unit_exponents_are_superscripts() {
    let out = lines("a = 2 m\nb = a*a");
    assert!(out[1].ends_with("m²"), "{out:?}");
}

#[test]
fn negative_unit_exponents_are_superscripts() {
    let out = lines("v = 3 m\nt = 2 s\na = v/t/t");
    assert!(out[2].ends_with("m·s⁻²"), "{out:?}");
}

// ---- units in results ---------------------------------------------------

#[test]
fn results_prefer_a_named_si_unit_over_base_dimensions() {
    let out = lines("m_ = 100 kg\na = 9.81 m/s^2\nF = m_*a");
    assert!(out[2].ends_with(" N"), "{out:?}");
}

#[test]
fn an_explicit_conversion_wins() {
    let out = lines("m_ = 100 kg\na = 9.81 m/s^2\nF = m_*a -> kN");
    assert!(out[2].ends_with("0.981 kN"), "{out:?}");
}

#[test]
fn temperatures_render_on_their_own_scale() {
    assert_eq!(lines("t = 20 °C\nt -> °F")[1], "t = 68°F");
}

#[test]
fn vectors_render_element_by_element() {
    let out = lines("f = [5, 10] Hz\ng = f*2");
    assert!(out[1].contains("[10 Hz, 20 Hz]"), "{out:?}");
}

// ---- prose and declarations ---------------------------------------------

#[test]
fn comments_become_prose() {
    let out = lines("' Shaker specifications\nk = 4");
    assert_eq!(out[0], "Shaker specifications");
}

#[test]
fn a_unit_declaration_shows_what_it_declares() {
    // `1000 lbf/ft` says what a klf is; `14593.9 kg·s⁻²` does not.
    let out = lines("unit klf = 1000 lbf/ft");
    assert_eq!(out[0], "unit klf = 1000 lbf/ft");
}

// ---- errors -------------------------------------------------------------

#[test]
fn a_failed_line_reports_in_place_and_the_rest_survives() {
    let out = lines("a = 1 m + 1 s\nb = 2");
    assert!(out[0].contains("cannot combine"), "{out:?}");
    assert_eq!(out[1], "b = 2");
}

// ---- determinism --------------------------------------------------------

#[test]
fn rendering_is_deterministic() {
    let src = "r = 5 cm\nV = pi*r^3*sin(0.7)\nV -> mm^3";
    let first = render(src);
    for _ in 0..20 {
        assert_eq!(render(src), first);
    }
}

#[test]
fn output_has_no_trailing_whitespace() {
    // Golden files are diffed; trailing whitespace is noise that editors strip
    // and comparisons then fail on.
    let out = render("' A note\nr = 5 cm\nV = pi*r^2 -> cm^2\nbad = 1 m + 1 s");
    for line in out.lines() {
        assert_eq!(line, line.trim_end(), "trailing whitespace in {line:?}");
    }
}

// ---- HTML ---------------------------------------------------------------

#[test]
fn html_is_self_contained_and_escaped() {
    let sheet = Sheet::new("' a < b & c\nx = 5 cm");
    let html = sheet_html(&sheet);
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("a &lt; b &amp; c"), "prose must be escaped");
    // No external resources: it has to work offline and print correctly.
    assert!(!html.contains("http://"), "no external references");
    assert!(!html.contains("https://"), "no external references");
    assert!(!html.contains("<script"), "no scripts");
    assert!(
        html.contains("@media print"),
        "print styles are not optional"
    );
}

fn sheet_html(sheet: &Sheet) -> String {
    nomo_core::render::html::render(sheet, &RenderOptions::default(), "test")
}

#[test]
fn digits_changes_what_is_shown_from_that_line_down() {
    let text = render("x = 2/3 m\ndigits 3\ny = 2/3 m\ndigits 8\nz = 2/3 m\n");
    assert!(text.contains("0.666667 m"), "the default is six: {text}");
    assert!(text.contains("0.667 m"), "three after `digits 3`: {text}");
    assert!(
        text.contains("0.66666667 m"),
        "eight after `digits 8`: {text}"
    );
}

#[test]
fn digits_is_presentation_and_nothing_else() {
    // The values section of a snapshot is full precision by design — it is what
    // the cross-target comparison compares — so a display directive must not
    // reach it.
    let snap = nomo_core::golden::snapshot("t", "digits 3\nx = 2/3 m\n");
    let values = snap
        .split("=== values ===")
        .nth(1)
        .expect("a values section");
    assert!(
        values.contains("0.6666666666666666"),
        "digits reached the values section: {values}"
    );
}

#[test]
fn a_compound_conversion_target_substitutes_as_written() {
    // `-> mm^2` and `-> MN/m` are not names in the unit table, so they could not
    // become a hint and every later use of the name read in base units. The
    // most ordinary engineering units in the language were the ones that did
    // not propagate.
    let text = render("A = 3 m*4 m -> mm^2\nq = A*2\n");
    assert!(
        text.contains("12000000 mm²·2") || text.contains("1.2e7 mm²·2"),
        "the substitution should read in mm²: {text}"
    );
}

#[test]
fn a_hint_that_does_not_fit_its_value_is_not_used() {
    // `M = 500 N*m` offers `m` as the unit it was written in — the right
    // operand of the multiplication — and that is a length where the value is a
    // moment. Showing `500 m` for it would be worse than showing base units,
    // and for a while it did.
    let text = render("M = 500 N*m\nZ = 2 m^3\nsigma = M/Z\n");
    assert!(
        !text.contains("500 m/"),
        "a moment was shown as a length: {text}"
    );
    assert!(
        text.contains("500 J/"),
        "expected base units instead: {text}"
    );
}

/// The HTML body with the mathematics typeset.
fn typeset(src: &str) -> String {
    let sheet = nomo_core::Sheet::new(src);
    let opts = nomo_core::RenderOptions {
        mathml: true,
        ..Default::default()
    };
    nomo_core::render::html::body(&sheet, &opts)
}

#[test]
fn division_becomes_a_fraction_and_a_power_a_superscript() {
    let out = typeset("w = 2 kN/m\nL = 6 m\nM = w*L^2/8\n");
    assert!(out.contains("<mfrac>"), "no fraction: {out}");
    assert!(out.contains("<msup>"), "no superscript: {out}");
    // The unit is upright and the name is not: that distinction is most of what
    // makes typeset mathematics readable.
    assert!(out.contains("<mi mathvariant=\"normal\">kN</mi>"), "{out}");
    assert!(out.contains("<mi>w</mi>"), "{out}");
}

#[test]
fn a_fraction_bar_replaces_the_brackets_it_makes_unnecessary() {
    // `M/(2 MPa)` is a fraction with `2 MPa` under the bar. Drawing the
    // brackets as well would be saying the same thing twice, and it is the
    // difference between typeset output and linear text with a bar in it.
    let out = typeset("M = 1 J\nr = sqrt(M/(2 MPa))\n");
    assert!(out.contains("<msqrt>"), "no radical: {out}");
    let fraction = out
        .split("<mfrac>")
        .nth(1)
        .and_then(|s| s.split("</mfrac>").next())
        .expect("a fraction");
    assert!(
        !fraction.contains("<mo>(</mo>"),
        "the fraction kept brackets it does not need: {fraction}"
    );
}

#[test]
fn a_subscripted_name_is_drawn_as_one() {
    let out = typeset("sigma_allow = 1 MPa\nx = sigma_allow*2\n");
    assert!(
        out.contains("<msub><mi>σ</mi><mi>allow</mi></msub>"),
        "the underscore should become a subscript: {out}"
    );
}

#[test]
fn the_name_column_is_typeset_with_the_rest_of_the_line() {
    // A line reading `sigma_allow` on the left beside a formula reading σ_allow
    // on the right, both naming the same quantity, is worse than a page that
    // typesets neither.
    let out = typeset("sigma_allow = 1 MPa\nx = sigma_allow*2\n");
    assert!(
        out.contains("<span class=\"name\"><math display=\"inline\"><mrow><msub><mi>σ</mi><mi>allow</mi></msub>"),
        "the name column was left as text: {out}"
    );
    let sheet = nomo_core::Sheet::new("sigma_allow = 1 MPa\n");
    let plain = nomo_core::render::html::body(&sheet, &RenderOptions::default());
    assert!(
        plain.contains("<span class=\"name\">sigma_allow</span>"),
        "the name column should be plain text without the flag: {plain}"
    );
}

#[test]
fn a_name_that_spells_a_greek_letter_is_set_as_one() {
    // What an engineer types on an ordinary keyboard for σ, λ and Δ. Setting
    // them as the words is the difference between output that has been typeset
    // and output that only looks as though it has.
    let out = typeset("lambda = 2\nDelta_p = 3 Pa\ntheta = 1\nx = lambda*theta\n");
    assert!(out.contains("<mi>λ</mi>"), "lambda: {out}");
    assert!(out.contains("<mi>θ</mi>"), "theta: {out}");
    assert!(
        out.contains("<msub><mi>Δ</mi><mi>p</mi></msub>"),
        "Delta_p: {out}"
    );
    assert!(!out.contains("lambda"), "the word survived: {out}");
}

#[test]
fn the_greek_table_stops_where_the_glyph_stops_being_latin() {
    // Ο is o and Β is B to look at, so mapping them would change the codepoint
    // without changing the glyph and would take the name from a worksheet using
    // it as an ordinary variable. That rule is why TeX has no `\omicron`
    // either.
    let out = typeset("omicron = 1\nBeta = 2\nx = omicron*Beta\n");
    assert!(out.contains("<mi>omicron</mi>"), "omicron mapped: {out}");
    assert!(out.contains("<mi>Beta</mi>"), "Beta mapped: {out}");
}

#[test]
fn a_greek_name_and_the_letter_itself_draw_the_same() {
    // The point of taking Unicode's name for the character rather than TeX's:
    // `phi` and `φ` are two spellings of one letter, and a reader who typed one
    // must not see the other. TeX's `\phi` is the symbol form ϕ, which would
    // have made these two disagree.
    let spelled = typeset("phi = 1\nx = phi*2\n");
    let typed = typeset("φ = 1\nx = φ*2\n");
    assert!(spelled.contains("<mi>φ</mi>"), "spelled: {spelled}");
    assert!(typed.contains("<mi>φ</mi>"), "typed: {typed}");
}

#[test]
fn a_unit_is_never_read_as_a_greek_name() {
    // `psi` is pounds per square inch, not ψ. Units resolve in their own branch
    // and render upright; the table must not reach them.
    let out = typeset("p = 30 psi\nq = p*2\n");
    assert!(
        out.contains("<mi mathvariant=\"normal\">psi</mi>"),
        "a unit was read as a Greek name: {out}"
    );
    assert!(!out.contains("ψ"), "a unit was read as a Greek name: {out}");
}

#[test]
fn a_constant_is_upright_and_says_what_the_text_column_says() {
    // The typeset column used to draw the *word* `pi` beside a text column
    // showing π, because it had no table of its own and did not use the
    // renderer's. Upright because ISO 80000-2 sets a mathematical constant in
    // roman, which is also what tells it from a variable of the same name.
    let out = typeset("r = 5 cm\nh = 12 cm\nV = pi*r^2*h\n");
    assert!(
        out.contains("<mi mathvariant=\"normal\">π</mi>"),
        "pi should be an upright π: {out}"
    );
    assert!(!out.contains("<mi>pi</mi>"), "the word survived: {out}");
}

#[test]
fn what_has_no_typeset_form_falls_back_rather_than_leaving_a_hole() {
    // A conditional has no standard typeset form here, so the linear text is
    // carried through as text — a sentence in the middle of a formula beats a
    // gap in the middle of a worksheet.
    let out = typeset("x = if 1 > 0 then 2 m else 3 m\n");
    assert!(out.contains("<mtext>"), "no fallback: {out}");
    assert!(
        out.contains("if"),
        "the fallback lost the expression: {out}"
    );
}

#[test]
fn a_unit_is_separated_from_its_number_and_algebra_is_not() {
    // ISO 80000-1 §7.1.3: a space always separates the unit from the number.
    // `ImplicitMul` emits U+2062, which says "multiply" and is exactly zero
    // wide, so `50 mm` typeset as `50mm` — while the substituted column beside
    // it, which goes through `<mtext>`, kept the space. One line disagreeing
    // with itself.
    let spaced = "<mo lspace=\"0\" rspace=\"0.167em\">&#8290;</mo>";

    for (source, what) in [
        ("d = 50 mm\nx = d*2\n", "a plain unit"),
        ("w = 2.5 kN/m\nx = w*2\n", "a unit under a fraction bar"),
        ("A = 2 m^2\nx = A*2\n", "a unit raised to a power"),
        ("M = 5 N*m\nx = M*2\n", "a compound unit"),
        ("t = 20 °C\nx = t\n", "an affine literal"),
        (
            "p = 50 %\nx = p*2\n",
            "percent, which ISO spaces like any other",
        ),
    ] {
        let out = typeset(source);
        assert!(out.contains(spaced), "{what} lost its space: {out}");
    }

    // The same juxtaposition is ordinary algebra, and `2x` is correctly tight.
    // What tells them apart is the right operand: a unit, or a name.
    for (source, what) in [
        ("x = 3\ny = 2 x\n", "a coefficient on a variable"),
        ("x = 3\nz = 2 (x + 1)\n", "a coefficient on a bracket"),
    ] {
        let out = typeset(source);
        assert!(!out.contains(spaced), "{what} should stay tight: {out}");
    }
}

#[test]
fn the_plane_angle_degree_is_the_exception_the_standard_names() {
    // The same clause exempts the plane-angle symbols: `90°` takes no space
    // where `20 °C` does. Matched by symbol, because that is what the exception
    // is about.
    let out = typeset("a = 90 °\nb = a*2\n");
    assert!(
        out.contains("<mn>90</mn><mo>&#8290;</mo><mi mathvariant=\"normal\">°</mi>"),
        "a plane angle should be tight: {out}"
    );
}

#[test]
fn a_typeset_line_is_set_whole_rather_than_half() {
    // The result, the `=` between the columns and the words `check` and `pass`
    // sit outside the `<math>` elements, so without this they keep `.step`'s
    // monospace while the formula beside them is a book face. A line set half
    // in one design and half in another is the loudest thing on the page.
    let out = typeset("M = 1 J\nx = M*2\n");
    assert!(
        out.contains("<div class=\"step typeset\">"),
        "a typeset line should say so: {out}"
    );
    let sheet = nomo_core::Sheet::new("M = 1 J\n");
    let plain = nomo_core::render::html::body(&sheet, &RenderOptions::default());
    assert!(
        plain.contains("<div class=\"step\">") && !plain.contains("typeset"),
        "an untypeset line is linear text and stays monospace: {plain}"
    );
}

#[test]
fn typesetting_is_off_unless_asked_for() {
    let plain = render("M = 2 kN*3 m\n");
    assert!(
        !plain.contains("<math"),
        "MathML leaked into the default: {plain}"
    );
}
