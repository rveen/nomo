//! HTML rendering.
//!
//! # No external typesetting
//!
//! Mathematics is set with Unicode characters — `·`, `π`, superscript digits —
//! and CSS, not by shipping a typesetting library. That keeps the output a
//! single self-contained file that renders offline, prints correctly, and has no
//! dependency to keep current. If the browser front end later needs finer
//! typesetting than Unicode gives, it can layer that on; the golden-file suite
//! diffs the text form regardless.
//!
//! Printing is a first-class concern rather than an afterthought: worksheets get
//! signed and filed, and retrofitting print styles is much harder than keeping
//! them.

use super::{plot, RenderOptions, Renderer};
use crate::doc::Sheet;
use crate::eval::OutcomeKind;
use crate::resource::{self, Reference, Resources};
use crate::value::Value;

const STYLE: &str = r#"
:root { color-scheme: light dark; }
body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  max-width: 46rem; margin: 2rem auto; padding: 0 1.5rem; line-height: 1.6;
}
h1 { font-size: 1.4rem; font-weight: 600; margin: 0 0 1.5rem; }
.prose { margin: 1.2rem 0 0.4rem; }
.step {
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
  font-size: 0.95rem; margin: 0.15rem 0; padding: 0.15rem 0;
  overflow-x: auto;
}
.name { font-weight: 600; }
.eq { opacity: 0.45; padding: 0 0.35rem; }
.subst { opacity: 0.7; }
.result { font-weight: 600; }
.error { color: #b00020; font-weight: 600; }
.note { opacity: 0.6; font-style: italic; }
/* A figure is evidence in an engineering worksheet, so it is sized to be read
   rather than to fit a column: the size its reference asks for, never wider
   than the page, and never split across a page break when printed. With the
   `width`/`height` attributes on the image, these two properties are what make
   a page narrower than the figure shrink it whole rather than crop it. */
figure { margin: 1.2rem 0; }
/* A plot is drawn by the engine as markup, so it is styled here rather than
   carrying colours of its own — the same picture has to read on a white page
   and a dark one. */
figure.plot svg { width: 100%; max-width: 42rem; height: auto; display: block; }
.plot-grid { stroke: currentColor; stroke-width: 0.5; opacity: 0.15; }
.plot-axis { stroke: currentColor; stroke-width: 1; opacity: 0.5; }
.plot-curve { fill: none; stroke: #0072b2; stroke-width: 1.8; stroke-linejoin: round; }
/* Six curve colours, the Okabe-Ito colourblind-safe palette without its yellow
   and its black: a curve cannot take the page's colour the way the structure
   does, because curves have to differ from each other as well as from the
   ground. `render/plot.rs` cycles through them and gives a legend swatch the
   same class as the curve it names. */
.plot-curve-1 { stroke: #0072b2; }
.plot-curve-2 { stroke: #d55e00; }
.plot-curve-3 { stroke: #009e73; }
.plot-curve-4 { stroke: #cc79a7; }
.plot-curve-5 { stroke: #e69f00; }
.plot-curve-6 { stroke: #56b4e9; }
/* A measured point is drawn as an open ring in its series' own colour,
   which is why it takes the curve's class rather than one of its own. */
.plot-mark { stroke-width: 1.3; }
.plot-label { fill: currentColor; opacity: 0.6; font-size: 11px; }
.plot-unit { fill: currentColor; opacity: 0.75; font-size: 11px; font-style: italic; }
.plot-x { text-anchor: middle; }
.plot-start { text-anchor: start; }
.plot-end { text-anchor: end; }
.plot-y { text-anchor: end; }
.plot-y-title { text-anchor: start; }
.plot-legend { text-anchor: start; opacity: 0.75; }
figure img { max-width: 100%; height: auto; display: block; }
figcaption { font-size: 0.8rem; opacity: 0.55; margin-top: 0.3rem; }
@media (prefers-color-scheme: dark) { .error { color: #ff6b6b; } }
@media print {
  body { max-width: none; margin: 0; font-size: 10pt; }
  .step { overflow-x: visible; white-space: pre-wrap; }
  figure { break-inside: avoid; }
}
"#;

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Render an evaluated worksheet as a standalone HTML document.
pub fn render(sheet: &Sheet, opts: &RenderOptions, title: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n\
         <h1>{}</h1>\n{}</body>\n</html>\n",
        escape(title),
        escape(title),
        body(sheet, opts)
    )
}

/// The worksheet's markup on its own, without the document chrome.
///
/// Split out because this is the part that reflects what was calculated. The
/// golden-file suite snapshots it rather than the whole document, so a change to
/// the stylesheet does not churn every expected file, and a change to a
/// worksheet's output cannot hide in the noise if it did.
pub fn body(sheet: &Sheet, opts: &RenderOptions) -> String {
    let source = sheet.source().to_string();
    let units = sheet.units().clone();
    let r = Renderer::new(opts, &units, &source);

    let mut body = String::new();
    let eq = r#"<span class="eq">=</span>"#;

    for (i, outcome) in sheet.outcomes().iter().enumerate() {
        // The resource trailer is data. Rendered as prose it is several
        // thousand paragraphs of base64 ahead of the figures themselves.
        if sheet.resources().is_hidden(i) {
            continue;
        }
        match &outcome.kind {
            OutcomeKind::Comment(text) => {
                if let Some(reference) = resource::reference(text) {
                    body.push_str(&figure(sheet.resources(), &reference));
                } else if !text.is_empty() {
                    body.push_str(&format!("<p class=\"prose\">{}</p>\n", escape(text)));
                }
            }

            OutcomeKind::Assign { name, trace } => {
                if let Ok(Value::Plot(p)) = &trace.value {
                    body.push_str(&format!(
                        "<div class=\"step\"><span class=\"name\">{}</span>{eq}{}</div>\n",
                        escape(name),
                        escape(&r.symbolic(trace))
                    ));
                    body.push_str(&plot::svg(p, r.units, &r.opts.numbers));
                    continue;
                }
                let mut line = format!(
                    "<div class=\"step\"><span class=\"name\">{}</span>{eq}{}",
                    escape(name),
                    escape(&r.symbolic(trace))
                );
                if r.substitution_is_informative(trace) {
                    line.push_str(&format!(
                        "{eq}<span class=\"subst\">{}</span>",
                        escape(&r.substituted(trace))
                    ));
                }
                if !r.is_literal_quantity(trace) {
                    let result = r.result(trace);
                    if result != r.symbolic(trace) {
                        line.push_str(&format!("{eq}{}", result_span(trace, &result)));
                    }
                }
                line.push_str("</div>\n");
                body.push_str(&line);
            }

            OutcomeKind::Query(trace) => {
                // A plot's result is the picture. Shown under the line that
                // asked for it rather than inside it, because a chart is not an
                // inline value and squeezing it into the result column would
                // make every other row that tall.
                if let Ok(Value::Plot(p)) = &trace.value {
                    body.push_str(&format!(
                        "<div class=\"step\">{}</div>\n",
                        escape(&r.symbolic(trace))
                    ));
                    body.push_str(&plot::svg(p, r.units, &r.opts.numbers));
                    continue;
                }
                let mut line = format!("<div class=\"step\">{}", escape(&r.symbolic(trace)));
                if r.substitution_is_informative(trace) {
                    line.push_str(&format!(
                        "{eq}<span class=\"subst\">{}</span>",
                        escape(&r.substituted(trace))
                    ));
                }
                let result = r.result(trace);
                line.push_str(&format!("{eq}{}</div>\n", result_span(trace, &result)));
                body.push_str(&line);
            }

            OutcomeKind::UnitDecl { name, trace } => {
                body.push_str(&format!(
                    "<div class=\"step note\">unit {} = {}</div>\n",
                    escape(name),
                    escape(&r.symbolic(trace))
                ));
            }

            OutcomeKind::FnDef(name) => {
                body.push_str(&format!(
                    "<div class=\"step note\">fn {} defined</div>\n",
                    escape(name)
                ));
            }

            OutcomeKind::NotEvaluated => {
                body.push_str("<div class=\"step error\">not evaluated</div>\n");
            }
            OutcomeKind::Malformed => {
                body.push_str("<div class=\"step error\">unparsed</div>\n");
            }
        }
    }

    body
}

fn result_span(trace: &crate::trace::Trace, result: &str) -> String {
    let class = if trace.value.is_err() {
        "error"
    } else {
        "result"
    };
    format!("<span class=\"{class}\">{}</span>", escape(result))
}

/// One image, inline.
///
/// # Why a `data:` URI and not a file beside the document
///
/// The same reason the worksheet embeds it: `nomo html` promises a single
/// self-contained file that renders offline and prints correctly, and a folder
/// of loose PNGs is not that. The base64 already in the worksheet is exactly
/// what a `data:` URI wants, so this copies text and decodes nothing — which is
/// also what keeps the engine free of anything the host would have to do.
///
/// # Why it can refuse
///
/// A `.nomo` file may have been written by the SMath importer out of a
/// third-party worksheet, so the payload is untrusted text on its way into an
/// attribute in a document somebody is about to open. It goes in only if it is
/// base64 and nothing else, and only under a media type we can name rather than
/// guess. Anything else becomes a visible note, because a worksheet quietly
/// missing a figure is the failure worth ruling out.
///
/// # Why the size is two attributes and not a style
///
/// `width` and `height` are the natural size of the box, and the stylesheet
/// above already says `max-width: 100%; height: auto`. Those three together are
/// the one arrangement that gives the figure the size the worksheet asked for,
/// shrinks it whole when the page is narrower than that, and never crops: a
/// figure is evidence, and a reader shown two-thirds of a diagram with no sign
/// of it is worse off than one shown a small diagram. The attributes also give
/// the browser the aspect ratio before the `data:` URI is decoded, so a
/// worksheet of figures does not reflow as it loads.
///
/// `height: auto` is what makes the width the operative half: the drawn height
/// follows the image's own proportions rather than the attribute, so a reference
/// dragged out of shape in SMath renders undistorted. `scripts/check-figures.mjs`
/// asserts all of this in a browser, which is the only thing that can.
///
/// A worksheet whose reference says no size — every one written before the size
/// existed — renders as it did then, at the image's own size.
fn figure(resources: &Resources, reference: &Reference<'_>) -> String {
    let name = reference.name;
    let note = |what: &str| format!("<p class=\"note\">[image {}: {what}]</p>\n", escape(name));
    let Some(image) = resources.image(name) else {
        return note("missing");
    };
    if !image.is_well_formed() {
        return note("unreadable");
    }
    let Some(media) = image.media_type() else {
        return note(&format!(
            "{} is not a format this build can show",
            image.format
        ));
    };
    let size = match reference.size {
        Some(s) => format!(" width=\"{}\" height=\"{}\"", s.width, s.height),
        None => String::new(),
    };
    format!(
        "<figure><img src=\"data:{media};base64,{}\"{size} alt=\"{}\"></figure>\n",
        image.data,
        escape(name)
    )
}

#[cfg(test)]
mod tests {
    use crate::doc::Sheet;
    use crate::render::RenderOptions;

    fn body_of(source: &str) -> String {
        super::body(&Sheet::new(source), &RenderOptions::default())
    }

    const GAUGE: &str = "' image gauge\n\n' --- resources ---\n' image gauge png 6\n'   SGVsbG8h\n";

    #[test]
    fn an_image_is_embedded_where_it_was_referenced() {
        let html = body_of(GAUGE);
        assert!(
            html.contains(
                r#"<figure><img src="data:image/png;base64,SGVsbG8h" alt="gauge"></figure>"#
            ),
            "{html}"
        );
    }

    #[test]
    fn a_figure_is_drawn_at_the_size_the_reference_gives() {
        // Two attributes and no style: with `max-width: 100%; height: auto` in
        // the stylesheet, this is the size the figure asks for, shrunk whole
        // when the page is narrower, and never cropped.
        let html = body_of(
            "' image gauge 749x483\n\n' --- resources ---\n' image gauge png 6\n'   SGVsbG8h\n",
        );
        assert!(
            html.contains(
                r#"<img src="data:image/png;base64,SGVsbG8h" width="749" height="483" alt="gauge">"#
            ),
            "{html}"
        );
    }

    #[test]
    fn a_reference_without_a_size_renders_as_it_always_did() {
        // Every worksheet written before the size existed is this one.
        let html = body_of(GAUGE);
        assert!(!html.contains("width="), "{html}");
    }

    #[test]
    fn the_trailer_does_not_reach_the_output() {
        // Otherwise a worksheet renders several thousand paragraphs of base64
        // ahead of the figures it was carrying.
        let html = body_of(GAUGE);
        assert!(
            !html.contains("<p class=\"prose\">image gauge png 6"),
            "{html}"
        );
        assert!(!html.contains("--- resources ---"), "{html}");
        assert_eq!(html.matches("SGVsbG8h").count(), 1, "{html}");
    }

    #[test]
    fn a_reference_with_no_data_is_reported() {
        let html = body_of("' image nowhere\n");
        assert!(
            html.contains(r#"<p class="note">[image nowhere: missing]</p>"#),
            "{html}"
        );
    }

    #[test]
    fn a_payload_that_is_not_base64_never_reaches_the_attribute() {
        // A `.nomo` file may have been written out of a third-party SMath
        // worksheet, so the payload is untrusted on its way into a document
        // somebody opens.
        let html = body_of(
            "' image x\n' --- resources ---\n' image x png 4\n'   \"><script>alert(1)</script>\n",
        );
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("[image x: unreadable]"), "{html}");
    }

    #[test]
    fn a_format_that_cannot_be_named_is_reported_rather_than_guessed() {
        let html = body_of("' image x\n' --- resources ---\n' image x tiff 6\n'   SGVsbG8h\n");
        assert!(!html.contains("data:"), "{html}");
        assert!(
            html.contains("is not a format this build can show"),
            "{html}"
        );
    }

    #[test]
    fn prose_that_merely_mentions_an_image_is_still_prose() {
        let html = body_of("' see the image below\n");
        assert!(
            html.contains(r#"<p class="prose">see the image below</p>"#),
            "{html}"
        );
    }
}
