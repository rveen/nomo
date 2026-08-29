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
