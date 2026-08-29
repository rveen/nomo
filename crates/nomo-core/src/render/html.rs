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
use crate::prose::{self, Block};
use crate::resource::{self, Reference, Resources};
use crate::value::Value;

const STYLE: &str = r#"
:root { color-scheme: light dark; }
body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  max-width: 46rem; margin: 2rem auto; padding: 0 1.5rem; line-height: 1.6;
}
h1 { font-size: 1.4rem; font-weight: 600; margin: 0 0 1.5rem; }
p.prose { margin: 1.2rem 0 0.4rem; }
/* Headings the worksheet wrote itself. The document's own title is the `h1`
   above, set once; these are sections inside it, so what they need is space
   before them rather than the title's margins. Sizes stay close together on
   purpose — an engineering worksheet is a few pages, not a manual, and a
   heading three times the size of the mathematics under it reads as a poster. */
h1.prose, h2.prose, h3.prose, h4.prose, h5.prose, h6.prose {
  margin: 1.8rem 0 0.5rem; line-height: 1.3; font-weight: 600;
}
h2.prose { font-size: 1.2rem; }
h3.prose { font-size: 1.05rem; }
h4.prose, h5.prose, h6.prose { font-size: 1rem; }
ul.prose, ol.prose { margin: 0.8rem 0; padding-left: 1.6rem; }
ul.prose li, ol.prose li { margin: 0.2rem 0; }
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
/* A verdict. Colour says it at a glance and the word says it on a monochrome
   printout, which is how an engineering worksheet is usually read once it has
   been signed. */
.verdict { font-weight: 700; margin-left: 0.6rem; }
/* The keyword stands where a name does, and there is no `=` after it to space
   the condition away from it. */
.check .name { margin-right: 0.5rem; }
.check.pass .verdict { color: #1a7f37; }
.check.fail .verdict { color: #b00020; }
.check.undecided .verdict { color: #8a6d00; }
.check.fail { background: #fff4f4; }
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
  /* A heading at the foot of a page is a heading in the wrong place. */
  h1.prose, h2.prose, h3.prose, h4.prose, h5.prose, h6.prose { break-after: avoid; }
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
///
/// `title` is what the caller knows the worksheet as — the file's name, for
/// `nomo html`. A worksheet that opens with a level-1 heading has said what it
/// is called itself, and that wins: it names the document, and the chrome's own
/// `<h1>` is dropped rather than printed above it. Two titles on one page, one
/// of them a file name, is what showing both would mean.
pub fn render(sheet: &Sheet, opts: &RenderOptions, title: &str) -> String {
    let body = body(sheet, opts);
    let (title, heading) = match opening_heading(sheet) {
        Some(own) => (own, String::new()),
        None => (title.to_string(), format!("<h1>{}</h1>\n", escape(title))),
    };
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n\
         {heading}{}</body>\n</html>\n",
        escape(&title),
        body
    )
}

/// The worksheet's own title: a level-1 heading opening its first run of prose.
///
/// Only the first run, and only its first block. A `# ` further down is a
/// section inside the document, not a second name for it.
fn opening_heading(sheet: &Sheet) -> Option<String> {
    let source = sheet.source();
    let mut run: Vec<&str> = Vec::new();
    let mut run_end: Option<u32> = None;

    for (i, outcome) in sheet.outcomes().iter().enumerate() {
        if sheet.resources().is_hidden(i) || sheet.is_version_pragma(i) || sheet.is_from_pack(i) {
            // The pragma stands above the title in most worksheets, so skipping
            // it is the point; anything hidden after the prose has begun ends it.
            if run.is_empty() {
                continue;
            }
            break;
        }
        match &outcome.kind {
            OutcomeKind::Comment(text)
                if resource::reference(text).is_none()
                    && adjacent(source, run_end, outcome.span.start) =>
            {
                run.push(text);
                run_end = Some(outcome.span.end);
            }
            _ => break,
        }
    }

    match prose::blocks(&run).first() {
        Some(Block::Heading { level: 1, text }) => Some(text.clone()),
        _ => None,
    }
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
    let mut r = Renderer::new(opts, &units, &source);

    let mut body = String::new();
    let eq = r#"<span class="eq">=</span>"#;

    // The prose run currently being collected, and where its last line ended.
    // Comments arrive one per line; a paragraph is a run of them, so they are
    // held until something that is not the next line of the same prose arrives.
    let mut run: Vec<&str> = Vec::new();
    let mut run_end: Option<u32> = None;

    for (i, outcome) in sheet.outcomes().iter().enumerate() {
        // The resource trailer is data. Rendered as prose it is several
        // thousand paragraphs of base64 ahead of the figures themselves. The
        // version pragma is metadata, and a paragraph beginning `nomo 1` is
        // what showing it would now produce.
        let hidden =
            sheet.resources().is_hidden(i) || sheet.is_version_pragma(i) || sheet.is_from_pack(i);

        // A run of prose ends at anything that is not its next line: a
        // statement, a figure, a blank source line, the trailer or the pragma.
        let continues = !hidden
            && match &outcome.kind {
                OutcomeKind::Comment(text) => {
                    resource::reference(text).is_none()
                        && adjacent(&source, run_end, outcome.span.start)
                }
                _ => false,
            };
        if !continues {
            body.push_str(&flush(&mut run));
            run_end = None;
        }
        if hidden {
            continue;
        }
        match &outcome.kind {
            OutcomeKind::Comment(text) => {
                if let Some(reference) = resource::reference(text) {
                    body.push_str(&figure(sheet.resources(), &reference));
                } else {
                    run.push(text);
                    run_end = Some(outcome.span.end);
                }
            }

            OutcomeKind::Assign { name, trace } => {
                if let Ok(Value::Plot(p)) = &trace.value {
                    body.push_str(&format!(
                        "<div class=\"step\"><span class=\"name\">{}</span>{eq}{}</div>\n",
                        escape(name),
                        escape(&r.symbolic(trace))
                    ));
                    body.push_str(&plot::svg(p, r.units, &r.numbers));
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

            OutcomeKind::Axis {
                vertical,
                described,
            } => {
                let which = if *vertical { "y" } else { "x" };
                body.push_str(&format!(
                    "<div class=\"step note\">axis {which} {}</div>\n",
                    escape(described)
                ));
            }

            OutcomeKind::Digits(figures) => {
                r.set_significant_figures(*figures);
                body.push_str(&format!(
                    "<div class=\"step note\">digits {figures}</div>\n"
                ));
            }

            OutcomeKind::Use(name) => {
                body.push_str(&format!(
                    "<div class=\"step note\">use {}</div>\n",
                    escape(name)
                ));
            }

            OutcomeKind::Check { trace, passed } => {
                let (word, class) = match passed {
                    Some(true) => ("pass", "pass"),
                    Some(false) => ("FAIL", "fail"),
                    None => ("not decided", "undecided"),
                };
                let mut line = format!(
                    "<div class=\"step check {class}\"><span class=\"name\">check</span>{}",
                    escape(&r.symbolic(trace))
                );
                if r.substitution_is_informative(trace) {
                    line.push_str(&format!(
                        "{eq}<span class=\"subst\">{}</span>",
                        escape(&r.substituted(trace))
                    ));
                }
                line.push_str(&format!("<span class=\"verdict\">{word}</span></div>\n"));
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
                    body.push_str(&plot::svg(p, r.units, &r.numbers));
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

    body.push_str(&flush(&mut run));
    body
}

/// Whether a comment starting at `start` is the next line of the run that ended
/// at `end`.
///
/// A comment's span runs from its `'` to the end of the line and stops short of
/// the newline, so two comments on consecutive lines have exactly one newline
/// between them. Two of them with a blank line between have two, and that blank
/// line is what separates one paragraph from the next — Markdown's own rule,
/// and what an empty `'` has always looked like on the page.
///
/// Byte arithmetic rather than line numbers: `Span::line_col` walks the source
/// from the top, and calling it once per outcome would make rendering quadratic
/// in the length of the worksheet.
fn adjacent(source: &str, end: Option<u32>, start: u32) -> bool {
    let Some(end) = end else {
        return true;
    };
    let gap = source.get(end as usize..start as usize).unwrap_or_default();
    gap.matches('\n').count() == 1
}

/// Render the collected run as prose and empty it.
fn flush(run: &mut Vec<&str>) -> String {
    if run.is_empty() {
        return String::new();
    }
    let html = prose_html(&prose::blocks(run));
    run.clear();
    html
}

/// Markdown blocks as markup.
///
/// Every string that arrives here is worksheet text on its way into a document
/// somebody is about to open, so all of it is escaped: the subset in
/// `crate::prose` has no raw HTML in it, deliberately, and this is the half of
/// that decision that has to hold.
fn prose_html(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Heading { level, text } => {
                out.push_str(&format!(
                    "<h{level} class=\"prose\">{}</h{level}>\n",
                    escape(text)
                ));
            }
            Block::Paragraph { text } => {
                out.push_str(&format!("<p class=\"prose\">{}</p>\n", escape(text)));
            }
            Block::List {
                ordered,
                start,
                items,
            } => {
                let tag = if *ordered { "ol" } else { "ul" };
                // A worksheet numbers steps with the mathematics between them,
                // so an item can be its own list. `start` is what keeps the
                // second step numbered 2 instead of renumbering the document.
                let from = match (*ordered, *start) {
                    (true, n) if n != 1 => format!(" start=\"{n}\""),
                    _ => String::new(),
                };
                out.push_str(&format!("<{tag} class=\"prose\"{from}>\n"));
                for item in items {
                    out.push_str(&format!("<li>{}</li>\n", escape(item)));
                }
                out.push_str(&format!("</{tag}>\n"));
            }
        }
    }
    out
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
    fn a_wrapped_paragraph_is_one_paragraph() {
        // What the block model is for: prose wrapped at 80 columns used to
        // render as a stack of one-line paragraphs.
        let html = body_of("' A worked design for a half-bridge LLC stage,\n' carried out the way the textbooks do it.\n");
        assert_eq!(
            html,
            "<p class=\"prose\">A worked design for a half-bridge LLC stage, carried out the way the textbooks do it.</p>\n"
        );
    }

    #[test]
    fn a_blank_line_starts_a_new_paragraph() {
        let html = body_of("' first\n\n' second\n");
        assert_eq!(
            html,
            "<p class=\"prose\">first</p>\n<p class=\"prose\">second</p>\n"
        );
    }

    #[test]
    fn an_empty_comment_starts_a_new_paragraph_too() {
        // `'` on its own is how a worksheet already writes a paragraph break,
        // and it is Markdown's blank line.
        let html = body_of("' first\n'\n' second\n");
        assert_eq!(
            html,
            "<p class=\"prose\">first</p>\n<p class=\"prose\">second</p>\n"
        );
    }

    #[test]
    fn a_statement_between_two_comments_ends_the_paragraph() {
        let html = body_of("' before\nx = 1\n' after\n");
        assert!(
            html.starts_with("<p class=\"prose\">before</p>\n"),
            "{html}"
        );
        assert!(html.ends_with("<p class=\"prose\">after</p>\n"), "{html}");
    }

    #[test]
    fn a_figure_between_two_comments_ends_the_paragraph() {
        // A figure is a block of its own, and prose either side of it is either
        // side of it.
        let html = body_of(
            "' above\n' image gauge\n' below\n\n' --- resources ---\n' image gauge png 6\n'   SGVsbG8h\n",
        );
        assert!(
            html.starts_with("<p class=\"prose\">above</p>\n<figure>"),
            "{html}"
        );
        assert!(html.ends_with("<p class=\"prose\">below</p>\n"), "{html}");
    }

    #[test]
    fn the_version_pragma_is_not_prose() {
        // It is metadata, and joined to the line under it it would open the
        // document with `nomo 1 Complex numbers`.
        let html = body_of("' nomo 1\n' Complex numbers\n");
        assert_eq!(html, "<p class=\"prose\">Complex numbers</p>\n");
    }

    #[test]
    fn headings_and_lists_are_rendered_as_such() {
        let html = body_of("' # Design\n' The method has three steps.\n\n' - one\n' - two\n");
        assert_eq!(
            html,
            "<h1 class=\"prose\">Design</h1>\n\
             <p class=\"prose\">The method has three steps.</p>\n\
             <ul class=\"prose\">\n<li>one</li>\n<li>two</li>\n</ul>\n"
        );
    }

    #[test]
    fn an_ordered_list_keeps_the_number_the_worksheet_wrote() {
        // A worksheet numbers its steps with the mathematics between them, so
        // the second step arrives as a list of its own.
        let html = body_of("' 2. Loop open.\n");
        assert_eq!(
            html,
            "<ol class=\"prose\" start=\"2\">\n<li>Loop open.</li>\n</ol>\n"
        );
        assert!(body_of("' 1. Loop closed.\n").contains("<ol class=\"prose\">"));
    }

    #[test]
    fn prose_is_escaped_and_the_subset_has_no_raw_html() {
        // A `.nomo` file may have been written by the importer out of a
        // third-party worksheet, and this output is assigned to `innerHTML` by
        // the browser front end.
        let html = body_of("' # <script>alert(1)</script>\n' - <img onerror=x>\n");
        assert!(!html.contains("<script>"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
    }

    #[test]
    fn a_worksheet_that_names_itself_is_not_titled_twice() {
        let sheet = Sheet::new("' nomo 1\n' # LLC converter\n' A worked design.\n");
        let doc = super::render(&sheet, &RenderOptions::default(), "llc");
        assert!(doc.contains("<title>LLC converter</title>"), "{doc}");
        assert!(!doc.contains("<h1>llc</h1>"), "{doc}");
        assert_eq!(doc.matches("<h1").count(), 1, "{doc}");
    }

    #[test]
    fn a_worksheet_with_no_heading_keeps_the_name_it_was_given() {
        let sheet = Sheet::new("' Cylinder volume\nr = 5 cm\n");
        let doc = super::render(&sheet, &RenderOptions::default(), "cylinder");
        assert!(doc.contains("<title>cylinder</title>"), "{doc}");
        assert!(doc.contains("<h1>cylinder</h1>"), "{doc}");
    }

    #[test]
    fn only_the_opening_block_names_the_document() {
        // A `# ` further down is a section inside the worksheet, not a second
        // name for it.
        let sheet = Sheet::new("' Cylinder volume\n\n' # Method\n");
        let doc = super::render(&sheet, &RenderOptions::default(), "cylinder");
        assert!(doc.contains("<h1>cylinder</h1>"), "{doc}");
    }

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
