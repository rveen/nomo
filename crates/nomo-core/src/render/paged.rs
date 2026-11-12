//! The paged document: the standalone HTML page, laid out as pages by Folio.
//!
//! # Who paginates
//!
//! Not this crate. What it writes is a CSS Paged Media document — `@page`
//! rules, margin boxes, break properties — and [Folio], a paged-media engine
//! carried in or beside the document, lays it out as pages in the browser. The
//! engine never paginates, so nothing here reads a font metric or measures a
//! box, and the determinism guard has nothing new to guard.
//!
//! That draws a line worth stating. **The numbers are bit-reproducible and the
//! page breaks are not**: where a page ends depends on the fonts the browser
//! sets the text in and on its own line breaking. A page break is presentation,
//! the same class of thing as the colour of a link, and the document this
//! module writes *is* byte-reproducible — it is the layout the browser makes of
//! it that is not. `--embed-font` is what narrows the gap in practice.
//!
//! # The page
//!
//! A4, with a head and a foot. The head carries the worksheet's title, at the
//! left, from the second page on; the foot carries the name of the file it was
//! rendered from at the left and `Page n of N` at the right, on every page — so
//! a sheet that comes loose from the rest still says where it belongs and where
//! to find its source. Nothing in the chrome varies between two renderings of
//! one worksheet — no date, no time — because a document whose bytes change
//! with the clock cannot be compared with itself.
//!
//! [Folio]: https://github.com/trukeio/folio

use super::{escape, html, MathFont, RenderOptions};
use crate::doc::Sheet;

/// The Folio engine a paged document is laid out by.
///
/// Built through [`PagedEngine::url`] or [`PagedEngine::embedded`] rather than
/// as an open enum, because an embedded script has to be checked before it is
/// trusted to sit inside a `<script>` element, and a constructor is the one
/// place that check cannot be skipped.
///
/// The script arrives already read: this crate does no I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagedEngine(Engine);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Engine {
    Url(String),
    Embedded { script: String, notice: String },
}

impl PagedEngine {
    /// Reference the script served beside the document.
    ///
    /// For a set of pages that share one copy. The document is no longer
    /// self-contained, which is why the CLI does not do this by default.
    pub fn url(url: impl Into<String>) -> Self {
        PagedEngine(Engine::Url(url.into()))
    }

    /// Carry the script in the document.
    ///
    /// Refused if the script could end the element it is placed in. A
    /// `<script>` element's content is raw text that ends at the first
    /// `</script`, in any case, and a `<!--` puts the parser into a state in
    /// which a later `<script` changes where it ends. Neither can be escaped
    /// from outside without parsing JavaScript, so a script containing either
    /// is not embedded at all. The pinned Folio build contains neither; this is
    /// what keeps a later one honest.
    ///
    /// `notice` is the licence the script is offered under, written into the
    /// document as a comment ahead of it. A document that carries the engine
    /// redistributes it, and Folio's MIT licence asks that its notice travel
    /// with copies — the same obligation, met the same way, as an embedded
    /// font's.
    pub fn embedded(
        script: impl Into<String>,
        notice: impl Into<String>,
    ) -> Result<Self, UnsafeScript> {
        let script = script.into();
        let lower = script.to_ascii_lowercase();
        if lower.contains("</script") {
            return Err(UnsafeScript("</script"));
        }
        if lower.contains("<!--") {
            return Err(UnsafeScript("<!--"));
        }
        Ok(PagedEngine(Engine::Embedded {
            script,
            notice: notice.into(),
        }))
    }
}

/// Why a script could not be embedded: the sequence it contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsafeScript(pub &'static str);

impl std::fmt::Display for UnsafeScript {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the script contains `{}`, which would end the <script> element it is embedded in",
            self.0
        )
    }
}

/// What the page looks like, appended after the standalone document's own
/// stylesheet so that it wins where the two disagree.
///
/// Folio measures with `@media print` applied and `@media screen` dropped, and
/// the pages it builds are then shown on screen — so most of this is written
/// for all media, not inside `@media print`, or the screen would show pages
/// laid out under one set of rules and styled by another.
const PAGE_STYLE: &str = r#"
/* ---- the paged document ------------------------------------------------ */
/* A sheet of paper is white. The standalone page follows the reader's scheme,
   and a dark page in the preview of a print is a preview of nothing. */
:root { color-scheme: light; }
body {
  max-width: none; margin: 0; padding: 0;
  background: #fff; color: #000; font-size: 10pt;
}
.error { color: #b00020; }
/* Paper has no scrollbar: a long line wraps rather than being clipped. */
.step { overflow-x: visible; white-space: pre-wrap; break-inside: avoid; }
p.prose { orphans: 2; widows: 2; }
figure { break-inside: avoid; }
h1, h1.prose, h2.prose, h3.prose, h4.prose, h5.prose, h6.prose { break-after: avoid; }

/* The margin boxes name the shipped text subset first. It resolves only when
   the document declares it (`--text-font-url`, which the editor passes), and
   otherwise falls through to the stack the body uses. */
@page {
  size: A4;
  margin: 20mm 18mm 20mm;
  /* The title and the file name are per document, so their `content` is
     written after this sheet; how they are set is the same for every one.
     Each box is its own rule because a margin rule is an at-rule, and at-rules
     take no selector list. A long title or file name is cut with an ellipsis
     rather than running into the box beside it. */
  @top-left {
    font: 400 8pt/1 "STIX Two Text Subset", "STIX Two Text", Georgia, "Times New Roman", serif;
    color: #555;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  @bottom-left {
    font: 400 8pt/1 "STIX Two Text Subset", "STIX Two Text", Georgia, "Times New Roman", serif;
    color: #555;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  @bottom-right {
    content: "Page " counter(page) " of " counter(pages);
    font: 400 8pt/1 "STIX Two Text Subset", "STIX Two Text", Georgia, "Times New Roman", serif;
    color: #555;
  }
}

/* Around the pages on screen: a ground for the sheets to sit on. */
@media screen {
  html { background: #e9ebef; }
  body { background: transparent; }
  .pagedjs_page { background: #fff; box-shadow: 0 1px 6px #0003; margin: 1.5rem auto; }
}

/* Printed before Folio has finished, the document would come out as one long
   unpaginated page that looks finished and is not. While pagination is under
   way the root carries `data-nomo-paging`, set by the script below and removed
   by Folio's `after` hook, and a print taken then is blank rather than wrong.
   The attribute is set by script, so a reader with scripts off prints the
   plain document, which is the right fallback.

   Folio builds each page from a clone of the source's `html` and `body`,
   attributes and all, taken while pagination is under way — so every page
   carries an `html` with `data-nomo-paging` on it, and a rule keyed on the
   attribute matches inside every finished page. The first version of this
   printed every page blank but for its margin boxes; found by printing one.
   Scoping the selector to the real root does not help, because Folio rewrites
   `:root` to match each page's clone as well. So the hook clears the attribute
   from the clones too, which no selector could. */
@media print {
  html[data-nomo-paging] body { visibility: hidden; }
}
"#;

/// Render an evaluated worksheet as a document Folio lays out as pages.
///
/// The worksheet's markup is [`html::body`], unchanged, so the golden suite
/// covers this output's content exactly as it covers the standalone page's;
/// what is added is the page and the engine.
///
/// `file` is the name of the file the worksheet was read from, extension and
/// all — `beam.nomo` — and goes in the foot of every page. Empty leaves that
/// corner blank: a worksheet that never had a file has no name to print, and
/// inventing one would put a name on paper that finds nothing.
pub fn render(
    sheet: &Sheet,
    opts: &RenderOptions,
    title: &str,
    file: &str,
    engine: &PagedEngine,
) -> String {
    // The title runs in the head of every page after the first. Written into the stylesheet as a string rather
    // than read off the heading with `string-set`: a worksheet's later level-1
    // headings are sections, and `string-set` would make the running head
    // follow them.
    let mut running = format!(
        "@page {{ @top-left {{ content: {}; }} }}\n",
        css_string(&html::title_of(sheet, title))
    );
    if !file.is_empty() {
        running.push_str(&format!(
            "@page {{ @bottom-left {{ content: {}; }} }}\n",
            css_string(file)
        ));
    }
    // The first page carries the title in full, so the running head would only
    // repeat it. Written last, after the rule that sets the title, so the first
    // page loses it whether `:first` is weighed by specificity or by order.
    // The foot stays on every page: "Page 1 of 3" is how a reader knows there
    // is a page 3, and the file name is how they find the source.
    running.push_str("@page :first { @top-left { content: none; } }\n");
    let style = format!("{PAGE_STYLE}{running}");
    let head = format!("{}{}", config(&opts.font), script(engine));
    html::document(sheet, opts, title, &style, &head)
}

/// The settings Folio's drop-in reads, and the marker that pagination is done.
///
/// `mathFont` asks Folio to prove the family has a usable OpenType MATH table
/// before it measures anything, and to refuse if not. Only named when the
/// document carries or references the font this project ships: with
/// [`MathFont::Named`] there is no family the document can promise, and a
/// check that fails on most machines is not a check.
///
/// `after` replaces `data-nomo-paging` — on the root and on every page's clone
/// of it, see [`PAGE_STYLE`] — with `data-nomo-pages`, the page count,
/// which is what a test waits for and what the print rule above releases on;
/// `data-nomo-overflowed` counts the pages whose content did not fit, which
/// Folio reports rather than hides and a test should not have to dig for.
fn config(font: &MathFont) -> String {
    let settings = match font {
        MathFont::Named => "",
        MathFont::Url(_) | MathFont::Embedded { .. } => {
            "settings: { mathFont: \"STIX Two Math Subset\" }, "
        }
    };
    format!(
        "<script>\n\
         document.documentElement.setAttribute(\"data-nomo-paging\", \"\");\n\
         window.PagedConfig = {{ {settings}after: function (flow) {{\n  \
         var root = document.documentElement;\n  \
         document.querySelectorAll(\"[data-nomo-paging]\").forEach(function (e) {{ e.removeAttribute(\"data-nomo-paging\"); }});\n  \
         root.setAttribute(\"data-nomo-pages\", String(flow ? flow.total : 0));\n  \
         root.setAttribute(\"data-nomo-overflowed\", String(flow ? flow.overflowed.length : 0));\n\
         }} }};\n\
         </script>\n"
    )
}

/// The engine itself, after the configuration it reads on load.
fn script(engine: &PagedEngine) -> String {
    match &engine.0 {
        Engine::Url(url) => format!("<script src=\"{}\"></script>\n", escape(url)),
        Engine::Embedded { script, notice } if notice.is_empty() => {
            format!("<script>\n{script}\n</script>\n")
        }
        Engine::Embedded { script, notice } => format!(
            "<!--\n{}\n-->\n<script>\n{script}\n</script>\n",
            html::escape_comment(notice)
        ),
    }
}

/// Text as a CSS string literal, safe inside a `<style>` element.
///
/// The quote and the backslash are escaped as CSS escapes them; a line break
/// becomes `\A`, which is what a newline is inside a CSS string; and `<`
/// becomes `\3C`, so a title containing `</style>` cannot close the stylesheet
/// it is written into. Every escape ends in a space, which CSS consumes, so the
/// character after one can never be mistaken for another hex digit.
fn css_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\3C "),
            c if c.is_control() => out.push_str(&format!("\\{:X} ", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Sheet;

    fn paged(source: &str, title: &str, engine: &PagedEngine) -> String {
        render(
            &Sheet::new(source),
            &RenderOptions::default(),
            title,
            &format!("{title}.nomo"),
            engine,
        )
    }

    fn embedded() -> PagedEngine {
        PagedEngine::embedded("/* folio */", "").unwrap()
    }

    #[test]
    fn the_title_heads_the_page_and_the_file_and_page_number_foot_it() {
        let doc = paged("x = 1\n", "beam", &embedded());
        assert!(doc.contains("@page { @top-left { content: \"beam\"; } }"));
        assert!(doc.contains("@page { @bottom-left { content: \"beam.nomo\"; } }"));
        assert!(doc.contains("\"Page \" counter(page) \" of \" counter(pages)"));
        assert!(doc.contains("size: A4;"));
        // Nothing at the head's right any more, and no placeholder anywhere.
        assert!(!doc.contains("@top-right"));
        assert!(!doc.contains("\"Nomo\""));
        // The first page loses the title, after the rule that sets it.
        let title = doc.find("@top-left { content: \"beam\"").unwrap();
        let first = doc
            .find("@page :first { @top-left { content: none; } }")
            .unwrap();
        assert!(title < first);
    }

    #[test]
    fn a_worksheet_without_a_file_has_no_name_in_its_foot() {
        let opts = RenderOptions::default();
        let doc = render(&Sheet::new("x = 1\n"), &opts, "untitled", "", &embedded());
        assert!(!doc.contains("@bottom-left { content:"));
        // A file name is text like a title, escaped the same way.
        let doc = render(
            &Sheet::new("x = 1\n"),
            &opts,
            "x",
            "a</style>.nomo",
            &embedded(),
        );
        assert!(doc.contains("@bottom-left { content: \"a\\3C /style>.nomo\"; }"));
    }

    #[test]
    fn the_worksheet_markup_is_the_standalone_pages() {
        // What lets the golden suite stand for this output too.
        let sheet = Sheet::new("' # Beam\nL = 2 m\nF = 3 kN\nM = F*L\n");
        let opts = RenderOptions::default();
        let doc = render(&sheet, &opts, "beam", "beam.nomo", &embedded());
        assert!(doc.contains(&html::body(&sheet, &opts)));
    }

    #[test]
    fn the_running_title_is_the_worksheets_own() {
        // An opening heading names the document, as it does the standalone one.
        let doc = paged("' # Shaft sizing\nd = 20 mm\n", "shaft", &embedded());
        assert!(doc.contains("@top-left { content: \"Shaft sizing\";"));
        let doc = paged("d = 20 mm\n", "shaft", &embedded());
        assert!(doc.contains("@top-left { content: \"shaft\";"));
    }

    #[test]
    fn a_title_cannot_close_the_stylesheet() {
        assert_eq!(css_string("a</style>b"), "\"a\\3C /style>b\"");
        assert_eq!(css_string("say \"hi\"\\"), "\"say \\\"hi\\\"\\\\\"");
        assert_eq!(css_string("a\nb"), "\"a\\A b\"");
        let doc = paged("x = 1\n", "x</style><script>", &embedded());
        assert_eq!(doc.matches("</style>").count(), 1);
    }

    #[test]
    fn a_script_that_could_end_its_element_is_refused() {
        assert_eq!(
            PagedEngine::embedded("s = '</script>'", ""),
            Err(UnsafeScript("</script"))
        );
        assert_eq!(
            PagedEngine::embedded("s = '</SCRIPT '", ""),
            Err(UnsafeScript("</script"))
        );
        assert_eq!(
            PagedEngine::embedded("a <!-- b", ""),
            Err(UnsafeScript("<!--"))
        );
        assert!(PagedEngine::embedded("if (a < b) c--", "").is_ok());
    }

    #[test]
    fn the_engine_follows_its_configuration() {
        // The drop-in reads `window.PagedConfig` when it loads, so the
        // configuration has to be in place before the engine's script runs.
        let doc = paged("x = 1\n", "x", &PagedEngine::url("folio.js?a&b"));
        let config = doc.find("window.PagedConfig").unwrap();
        let engine = doc
            .find("<script src=\"folio.js?a&amp;b\"></script>")
            .unwrap();
        assert!(config < engine);
        let doc = paged("x = 1\n", "x", &embedded());
        assert!(doc.find("window.PagedConfig").unwrap() < doc.find("/* folio */").unwrap());
    }

    #[test]
    fn the_math_font_is_checked_only_when_it_is_carried() {
        let named = paged("x = 1\n", "x", &embedded());
        assert!(!named.contains("mathFont"));
        let opts = RenderOptions {
            font: MathFont::Url("math.woff2".into()),
            ..RenderOptions::default()
        };
        let carried = render(&Sheet::new("x = 1\n"), &opts, "x", "x.nomo", &embedded());
        assert!(carried.contains("mathFont: \"STIX Two Math Subset\""));
    }

    #[test]
    fn an_embedded_engine_carries_its_licence() {
        let engine = PagedEngine::embedded("/* folio */", "MIT -- with a --> in it").unwrap();
        let doc = paged("x = 1\n", "x", &engine);
        let notice = doc.find("<!--\nMIT - - with a - -> in it\n-->").unwrap();
        assert!(notice < doc.find("/* folio */").unwrap());
        // A linked engine is not redistributed by the document, so it carries
        // nothing.
        assert!(!paged("x = 1\n", "x", &PagedEngine::url("folio.js")).contains("<!--"));
    }

    #[test]
    fn the_pages_can_be_set_in_the_shipped_text_face() {
        // What the editor's frame needed: it is a document of its own, so the
        // face the editor declares never reached its pages.
        let opts = RenderOptions {
            text_font: crate::render::TextFont::Url {
                upright: "fonts/text.woff2".into(),
                italic: Some("fonts/text-italic.woff2".into()),
            },
            ..RenderOptions::default()
        };
        let doc = render(&Sheet::new("x = 1\n"), &opts, "x", "x.nomo", &embedded());
        assert!(doc.contains("src: url(\"fonts/text.woff2\")"));
        assert!(doc.contains("src: url(\"fonts/text-italic.woff2\")"));
        assert!(doc.contains("body { font-family: \"STIX Two Text Subset\""));
    }

    #[test]
    fn two_renderings_are_the_same_bytes() {
        let source = "' # Spring\nk = 20 N/mm\nx = 3 mm\nF = k*x\n";
        assert_eq!(
            paged(source, "spring", &embedded()),
            paged(source, "spring", &embedded())
        );
    }
}
