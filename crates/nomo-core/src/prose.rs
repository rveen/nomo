//! The prose in a worksheet's comments, read as Markdown.
//!
//! # Why this is a block model and not a string
//!
//! Comments are statements (`lex.rs` keeps them as tokens rather than
//! discarding them, and `eval.rs` gives each one an outcome with a span)
//! precisely because a worksheet's prose is part of its output. What arrives
//! here is therefore an ordered run of lines with source positions behind them
//! — which is exactly the input a block parser wants, and why Markdown costs
//! this project a module rather than an architecture.
//!
//! Reading it here also fixes something that was wrong: the HTML renderer used
//! to emit one paragraph per *source line*, so a paragraph wrapped across five
//! lines rendered as five paragraphs. Joining a run of lines is the first thing
//! a block model does.
//!
//! Nothing about the language changes. This is a rendering concern: the parser,
//! the document layer, the dependency graph, the version pragma and the resource
//! trailer never see it, and a build that renders prose as flat lines still
//! renders every worksheet.
//!
//! # The subset is closed, and the exclusions are the interesting half
//!
//! Headings, paragraphs and lists. Deliberately absent, each for a reason that
//! design note §8.41 records with the measurement behind it:
//!
//! * **Setext headings and `---` thematic breaks.** `' --- resources ---` is the
//!   trailer sentinel, and a setext underline would promote any paragraph
//!   followed by a line of dashes into a heading. A worksheet must not acquire a
//!   section break by writing about a range of values. A bullet marker therefore
//!   requires a space after it, which is what leaves `---` as ordinary text.
//! * **Indented code blocks.** Of 6088 prose lines the SMath importer emits
//!   across both corpora, 227 are indented and 224 of those are wrapped
//!   continuations of the line above. In a worksheet leading whitespace is a
//!   wrap, not a program, so indentation plays no structural role at all: a line
//!   is classified by what it says after trimming. Nesting is not in the subset
//!   either, so an indented list marker is an item of the same flat list.
//! * **Raw HTML.** The renderer escapes everything it emits and the browser
//!   front end assigns the result to `innerHTML`. Passthrough would be an
//!   injection path, and there is no worksheet that needs one.
//! * **Inline emphasis.** Not here yet, and `_` should never be built: 106
//!   corpus prose lines carry two or more underscores and every sampled one is
//!   an identifier — `fn qq_at_1(k) = 448.83*xx[k]`, a line the importer
//!   commented out. Underscore emphasis would eat variable names in the kind of
//!   prose this project has most of.
//!
//! Inline formatting is **now built**, and still only the two §8.41 allowed:
//! `` ` `` for a code span and `**` for strong. The measurement that settles
//! them is the same kind §8.41 made against `_`. Across the 120580 prose lines
//! the importer emits from both corpora, `**` appears on **none**, so nothing
//! existing changes meaning. A backtick appears on **1595**, every one of them
//! inside the importer's own `[import] unsupported: …` marker, which already
//! quotes an identifier in backticks *because it is code* — so a code span is
//! what those lines have always meant and never got.
//!
//! Inline is read after the block join, not per source line, and one of this
//! project's own worksheets says why: `examples/conditions.nomo` wraps a code
//! span across two comment lines, leaving each with an odd number of backticks
//! and only the joined paragraph with an even one.
//!
//! This module returns plain text and spans, never markup: escaping belongs to
//! whichever renderer is consuming them.

/// One block of prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `# text` through `###### text`. `level` is 1 to 6.
    Heading { level: u8, text: String },
    /// A run of lines, joined into one paragraph.
    Paragraph { text: String },
    /// A flat list. `start` is the number the first item was written with, which
    /// is not always 1: a worksheet numbers steps with the mathematics between
    /// them, so each item can be its own single-item list and the numbering has
    /// to survive that.
    List {
        ordered: bool,
        start: u32,
        items: Vec<String>,
    },
}

/// Read a run of comment lines as prose.
///
/// The caller decides what a run is — consecutive comment outcomes on
/// consecutive source lines — and has already taken the `'` and one space off
/// each. An empty line separates blocks, which is Markdown's own rule and also
/// what an empty `'` has always looked like.
pub fn blocks(lines: &[&str]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut open = Open::None;

    for raw in lines {
        // Trailing whitespace carries no meaning: Markdown's two-space hard
        // break is not in the subset, and a worksheet cannot show one anyway.
        let line = raw.trim();
        if line.is_empty() {
            open.flush(&mut out);
            continue;
        }

        match classify(line, &open) {
            Line::Heading { level, text } => {
                open.flush(&mut out);
                out.push(Block::Heading {
                    level,
                    text: text.to_string(),
                });
            }

            Line::Item {
                ordered,
                number,
                text,
            } => {
                match &mut open {
                    // The same kind of marker continues the list it is in. A
                    // number that does not follow on is not an error and not a
                    // new list: only the first item's number is rendered, and
                    // the rest are the reader's to infer, as they are on paper.
                    Open::List {
                        ordered: open_ordered,
                        items,
                        ..
                    } if *open_ordered == ordered => {
                        items.push(text.to_string());
                    }
                    _ => {
                        open.flush(&mut out);
                        open = Open::List {
                            ordered,
                            start: number,
                            items: vec![text.to_string()],
                        };
                    }
                }
            }

            Line::Text(text) => match &mut open {
                Open::Paragraph(p) => {
                    p.push(' ');
                    p.push_str(text);
                }
                // A line under a list item that is not itself a marker is a
                // continuation of that item — Markdown calls it lazy
                // continuation, and it is how a wrapped bullet reads.
                Open::List { items, .. } => {
                    let item = items.last_mut().expect("a list is never empty");
                    item.push(' ');
                    item.push_str(text);
                }
                Open::None => open = Open::Paragraph(text.to_string()),
            },
        }
    }

    open.flush(&mut out);
    out
}

/// What a single line turned out to be.
enum Line<'a> {
    Heading {
        level: u8,
        text: &'a str,
    },
    Item {
        ordered: bool,
        number: u32,
        text: &'a str,
    },
    Text(&'a str),
}

/// The block currently being accumulated.
enum Open {
    None,
    Paragraph(String),
    List {
        ordered: bool,
        start: u32,
        items: Vec<String>,
    },
}

impl Open {
    fn flush(&mut self, out: &mut Vec<Block>) {
        match std::mem::replace(self, Open::None) {
            Open::None => {}
            Open::Paragraph(text) => out.push(Block::Paragraph { text }),
            Open::List {
                ordered,
                start,
                items,
            } => out.push(Block::List {
                ordered,
                start,
                items,
            }),
        }
    }
}

/// Decide what a non-empty, trimmed line is.
///
/// `open` matters for exactly one case, and it is a real hazard rather than
/// pedantry: an ordered marker may interrupt a paragraph only when it is
/// numbered 1. Otherwise a wrapped line beginning `1988. ` — a year at the head
/// of a continuation — would silently become a list.
fn classify<'a>(line: &'a str, open: &Open) -> Line<'a> {
    if let Some(rest) = escaped(line) {
        return Line::Text(rest);
    }
    if let Some((level, text)) = heading(line) {
        return Line::Heading { level, text };
    }
    if let Some(text) = bullet(line) {
        return Line::Item {
            ordered: false,
            number: 1,
            text,
        };
    }
    if let Some((number, text)) = ordered(line) {
        let interrupts_paragraph = matches!(open, Open::Paragraph(_));
        if !interrupts_paragraph || number == 1 {
            return Line::Item {
                ordered: true,
                number,
                text,
            };
        }
    }
    Line::Text(line)
}

/// `\# text` — a backslash before a marker, and nowhere else.
///
/// The narrowest rule that does the job: the backslash is removed only when it
/// is protecting something that would otherwise begin a block, so prose that
/// happens to start with a backslash keeps it.
fn escaped(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('\\')?;
    let is_marker = heading(rest).is_some() || bullet(rest).is_some() || ordered(rest).is_some();
    is_marker.then_some(rest)
}

/// `#` to `######`, a space, and something after it.
///
/// A lone `#` is prose. CommonMark reads it as an empty heading; in a worksheet
/// it is far likelier to be a stray character than a section with no name.
fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let text = line[hashes..].strip_prefix(' ')?.trim();
    if text.is_empty() {
        return None;
    }
    // A closing run of `#` is decoration, not content: `# Title #` names the
    // same section as `# Title`. It has to be preceded by a space to count,
    // which is what keeps `# Section ###;` and `# C#` intact — a heading whose
    // text really does end in a hash is not rare in this domain.
    let head = text.trim_end_matches('#');
    let text = if head.len() < text.len() && head.ends_with(' ') {
        head.trim_end()
    } else {
        text
    };
    (!text.is_empty()).then_some((hashes as u8, text))
}

/// `- text`, `* text`, `+ text`.
///
/// The space is required, and that is what keeps `---` ordinary text — which
/// matters, because the resource trailer is written with dashes.
fn bullet(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix('-')
        .or_else(|| line.strip_prefix('*'))
        .or_else(|| line.strip_prefix('+'))?;
    let text = rest.strip_prefix(' ')?.trim();
    (!text.is_empty()).then_some(text)
}

/// `1. text` or `1) text`, up to nine digits.
///
/// Nine because that is where CommonMark stops, and because it keeps the number
/// inside a `u32` without a fallible parse.
fn ordered(line: &str) -> Option<(u32, &str)> {
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits > 9 {
        return None;
    }
    let rest = &line[digits..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
    let text = rest.strip_prefix(' ')?.trim();
    if text.is_empty() {
        return None;
    }
    let number = line[..digits].parse().ok()?;
    Some((number, text))
}

/// A run of inline text, as much structure as the subset has.
///
/// Flat on purpose: a code span holds literal text and a strong span holds
/// plain text, and neither nests in the other. Nesting buys nothing a worksheet
/// has asked for and every level of it is another thing to be wrong about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    Text(String),
    /// `` `code` `` — held literally, so nothing inside it is read as a marker.
    Code(String),
    /// `**strong**`.
    Strong(String),
}

/// Read one block's text as inline spans.
///
/// # What is a marker and what is text
///
/// A backtick opens a code span and the next backtick closes it. An unmatched
/// backtick is text, which is what keeps a worksheet that writes one about
/// arithmetic from swallowing the rest of its paragraph.
///
/// `**` opens a strong span only when a non-space follows it, and closes one
/// only when a non-space precedes it. That is CommonMark's flanking rule
/// reduced to the part that matters here, and it is what makes `a ** b`
/// ordinary text: `**` with air on both sides is not emphasis in anybody's
/// Markdown.
///
/// `_` is not a marker and never will be — §8.41 counted 106 corpus prose lines
/// carrying two or more underscores and every sampled one was an identifier.
pub fn inline(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut i = 0;

    while i < bytes.len() {
        // A code span, if this backtick has a partner.
        if bytes[i] == b'`' {
            if let Some(close) = text[i + 1..].find('`') {
                let content = &text[i + 1..i + 1 + close];
                // Two adjacent backticks are not an empty code span. This subset
                // has no double-backtick delimiter, so `` is two characters a
                // worksheet wrote, and drawing an empty box for them would put
                // something on the page that nothing on the page asked for.
                if content.is_empty() {
                    plain.push('`');
                    i += 1;
                    continue;
                }
                flush_text(&mut spans, &mut plain);
                spans.push(Span::Code(content.to_string()));
                i += close + 2;
                continue;
            }
        }
        // A strong span, if this `**` opens one and something closes it.
        if bytes[i..].starts_with(b"**") {
            if let Some(content) = strong_at(text, i) {
                flush_text(&mut spans, &mut plain);
                spans.push(Span::Strong(content.to_string()));
                i += content.len() + 4;
                continue;
            }
        }
        // Anything else is text, one character at a time so that a marker which
        // turned out not to be one is kept exactly as written.
        let c = text[i..]
            .chars()
            .next()
            .expect("in bounds and on a boundary");
        plain.push(c);
        i += c.len_utf8();
    }
    flush_text(&mut spans, &mut plain);
    spans
}

/// The content of a strong span opening at `i`, if one opens and closes there.
fn strong_at(text: &str, i: usize) -> Option<&str> {
    let rest = &text[i + 2..];
    // Opening: a non-space has to follow, or `a ** b` would be emphasis.
    if !rest.chars().next().is_some_and(|c| !c.is_whitespace()) {
        return None;
    }
    let mut from = 0;
    while let Some(at) = rest[from..].find("**") {
        let end = from + at;
        // Closing: a non-space has to precede.
        if rest[..end]
            .chars()
            .next_back()
            .is_some_and(|c| !c.is_whitespace())
        {
            return Some(&rest[..end]);
        }
        // That `**` cannot close; look past it rather than giving up, so
        // `**a ** b**` closes at the last one.
        from = end + 2;
    }
    None
}

fn flush_text(spans: &mut Vec<Span>, plain: &mut String) {
    if !plain.is_empty() {
        spans.push(Span::Text(std::mem::take(plain)));
    }
}

/// A block's text with its markers removed and nothing else changed.
///
/// For the places that need words rather than formatting — a document's
/// `<title>`, which is an attribute and cannot carry markup.
pub fn plain(text: &str) -> String {
    inline(text)
        .into_iter()
        .map(|span| match span {
            Span::Text(t) | Span::Code(t) | Span::Strong(t) => t,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks_of(source: &str) -> Vec<Block> {
        let lines: Vec<&str> = source.lines().collect();
        blocks(&lines)
    }

    fn paragraph(text: &str) -> Block {
        Block::Paragraph {
            text: text.to_string(),
        }
    }

    fn heading(level: u8, text: &str) -> Block {
        Block::Heading {
            level,
            text: text.to_string(),
        }
    }

    fn list(ordered: bool, start: u32, items: &[&str]) -> Block {
        Block::List {
            ordered,
            start,
            items: items.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_wrapped_paragraph_is_one_paragraph() {
        // The whole reason this module exists: the renderer used to emit one
        // paragraph per source line, so prose wrapped at 80 columns came out as
        // a stack of one-line paragraphs.
        let b = blocks_of("A worked design for a half-bridge LLC stage,\ncarried out the way the textbooks do it.");
        assert_eq!(
            b,
            vec![paragraph(
                "A worked design for a half-bridge LLC stage, carried out the way the textbooks do it."
            )]
        );
    }

    #[test]
    fn a_blank_line_separates_paragraphs() {
        let b = blocks_of("first\n\nsecond");
        assert_eq!(b, vec![paragraph("first"), paragraph("second")]);
    }

    #[test]
    fn a_run_with_nothing_in_it_has_no_blocks() {
        assert_eq!(blocks_of(""), vec![]);
        assert_eq!(blocks_of("\n\n"), vec![]);
    }

    #[test]
    fn headings_take_one_to_six_hashes() {
        assert_eq!(blocks_of("# Design"), vec![heading(1, "Design")]);
        assert_eq!(blocks_of("###### Design"), vec![heading(6, "Design")]);
        // Seven is not a heading in any dialect, and reading it as one would be
        // inventing a level nothing can render.
        assert_eq!(
            blocks_of("####### Design"),
            vec![paragraph("####### Design")]
        );
    }

    #[test]
    fn a_hash_needs_a_space_and_a_name() {
        // `#3` is how a drawing calls out a part, and it is not a heading.
        assert_eq!(blocks_of("#3 bolts"), vec![paragraph("#3 bolts")]);
        assert_eq!(blocks_of("#"), vec![paragraph("#")]);
        assert_eq!(blocks_of("## "), vec![paragraph("##")]);
    }

    #[test]
    fn a_closing_run_of_hashes_is_decoration_but_only_after_a_space() {
        assert_eq!(blocks_of("# Design #"), vec![heading(1, "Design")]);
        assert_eq!(blocks_of("## Tank ###"), vec![heading(2, "Tank")]);
        // Preceded by anything else it is content. The corpus has a comment
        // banner ending `a ###;`, and a language called C# is not a heading
        // level short of its name.
        assert_eq!(blocks_of("# C#"), vec![heading(1, "C#")]);
        assert_eq!(blocks_of("# nodi a ###;"), vec![heading(1, "nodi a ###;")]);
    }

    #[test]
    fn a_heading_interrupts_a_paragraph() {
        let b = blocks_of("prose\n# Design");
        assert_eq!(b, vec![paragraph("prose"), heading(1, "Design")]);
    }

    #[test]
    fn a_bullet_takes_any_of_three_markers() {
        for marker in ["-", "*", "+"] {
            let b = blocks_of(&format!("{marker} perimetro\n{marker} area"));
            assert_eq!(b, vec![list(false, 1, &["perimetro", "area"])]);
        }
    }

    #[test]
    fn the_trailer_sentinel_is_not_a_list_and_dashes_are_not_a_rule() {
        // A bullet requires a space after its marker, and that is exactly what
        // keeps the resource trailer — and any prose that draws a line — out of
        // the block grammar. Thematic breaks and setext headings are out of the
        // subset for this reason; see the module comment.
        assert_eq!(
            blocks_of("--- resources ---"),
            vec![paragraph("--- resources ---")]
        );
        assert_eq!(blocks_of("Design\n---"), vec![paragraph("Design ---")]);
    }

    #[test]
    fn a_wrapped_bullet_continues_its_item() {
        let b = blocks_of("- If a root is between P and Q, then\n  one of the two is positive\n- and the other is not");
        assert_eq!(
            b,
            vec![list(
                false,
                1,
                &[
                    "If a root is between P and Q, then one of the two is positive",
                    "and the other is not"
                ]
            )]
        );
    }

    #[test]
    fn an_ordered_list_keeps_the_number_it_was_written_with() {
        // A worksheet numbers steps with the mathematics between them, so the
        // second step arrives here as its own run. Rendering it as `1.` would
        // renumber the document.
        assert_eq!(
            blocks_of("2. Loop open."),
            vec![list(true, 2, &["Loop open."])]
        );
        assert_eq!(blocks_of("1) First"), vec![list(true, 1, &["First"])]);
    }

    #[test]
    fn a_number_at_the_head_of_a_continuation_line_is_not_a_list() {
        // The hazard the interrupt rule exists for: a paragraph that wraps onto
        // a line beginning with a year would otherwise become a list.
        let b = blocks_of("Steigerwald's comparison of resonant topologies dates from\n1988. Every application note since repeats it.");
        assert_eq!(
            b,
            vec![paragraph(
                "Steigerwald's comparison of resonant topologies dates from 1988. Every application note since repeats it."
            )]
        );
    }

    #[test]
    fn a_list_starting_at_one_may_interrupt_a_paragraph() {
        let b = blocks_of("The method has three steps.\n1. Replace the square wave.");
        assert_eq!(
            b,
            vec![
                paragraph("The method has three steps."),
                list(true, 1, &["Replace the square wave."])
            ]
        );
    }

    #[test]
    fn a_bullet_interrupts_a_paragraph_whatever_it_says() {
        let b = blocks_of("The section computes:\n- perimetro");
        assert_eq!(
            b,
            vec![
                paragraph("The section computes:"),
                list(false, 1, &["perimetro"])
            ]
        );
    }

    #[test]
    fn changing_the_kind_of_marker_starts_a_new_list() {
        let b = blocks_of("- one\n1. two");
        assert_eq!(b, vec![list(false, 1, &["one"]), list(true, 1, &["two"])]);
    }

    #[test]
    fn indentation_plays_no_structural_role() {
        // 227 of the importer's 6088 prose lines are indented and 224 of them
        // are wrapped continuations, so leading whitespace is a wrap. Nesting is
        // not in the subset either, which is why an indented marker joins the
        // list it is under rather than starting one inside it.
        let b = blocks_of("- If P is a root, then\n    Pvalue is zero\n  - and so is the product");
        assert_eq!(
            b,
            vec![list(
                false,
                1,
                &[
                    "If P is a root, then Pvalue is zero",
                    "and so is the product"
                ]
            )]
        );
    }

    #[test]
    fn a_backslash_protects_a_marker_and_nothing_else() {
        assert_eq!(
            blocks_of("\\# not a heading"),
            vec![paragraph("# not a heading")]
        );
        assert_eq!(
            blocks_of("\\- not a bullet"),
            vec![paragraph("- not a bullet")]
        );
        // Prose that merely starts with a backslash keeps it: the escape is the
        // narrowest rule that does its job.
        assert_eq!(
            blocks_of("\\alpha is the ratio"),
            vec![paragraph("\\alpha is the ratio")]
        );
    }

    #[test]
    fn an_ordered_marker_stops_at_nine_digits() {
        assert_eq!(
            blocks_of("999999999. deep"),
            vec![list(true, 999999999, &["deep"])]
        );
        assert_eq!(
            blocks_of("1234567890. deep"),
            vec![paragraph("1234567890. deep")]
        );
    }

    #[test]
    fn an_item_needs_something_after_its_marker() {
        assert_eq!(blocks_of("-"), vec![paragraph("-")]);
        assert_eq!(blocks_of("- "), vec![paragraph("-")]);
        assert_eq!(blocks_of("1."), vec![paragraph("1.")]);
    }

    #[test]
    fn the_importers_marker_lines_are_inert() {
        // Every untranslatable construct becomes `' [import] unsupported: …`,
        // and a marker that turned into a heading or a list would be a marker
        // the reader misreads.
        let b = blocks_of("[import] unsupported: a `for` loop: whose body reads Time2Alt");
        assert_eq!(
            b,
            vec![paragraph(
                "[import] unsupported: a `for` loop: whose body reads Time2Alt"
            )]
        );
    }

    // ---- inline ------------------------------------------------------------

    use super::{inline, plain, Span};

    fn text(t: &str) -> Span {
        Span::Text(t.into())
    }

    #[test]
    fn the_two_markers_the_subset_has() {
        assert_eq!(
            inline("its **preload** and the `else` arm"),
            vec![
                text("its "),
                Span::Strong("preload".into()),
                text(" and the "),
                Span::Code("else".into()),
                text(" arm"),
            ]
        );
    }

    #[test]
    fn an_underscore_is_never_a_marker() {
        // §8.41 counted 106 corpus prose lines carrying two or more
        // underscores, and every sampled one was an identifier. Underscore
        // emphasis would eat variable names in the prose this project has most
        // of.
        assert_eq!(
            inline("sigma_allow against sigma_bolt"),
            vec![text("sigma_allow against sigma_bolt")]
        );
    }

    #[test]
    fn a_double_star_with_air_on_both_sides_is_text() {
        // CommonMark's flanking rule, reduced to the half that matters: `**`
        // opens only with a non-space after it and closes only with a non-space
        // before it. Without this, prose that writes about `**` acquires
        // emphasis from the middle of a sentence.
        assert_eq!(inline("a ** b"), vec![text("a ** b")]);
        assert_eq!(inline("a ** b ** c"), vec![text("a ** b ** c")]);
    }

    #[test]
    fn an_unmatched_marker_stays_as_written() {
        // A worksheet writing one backtick about arithmetic must not swallow
        // the rest of its paragraph.
        assert_eq!(inline("the ` character"), vec![text("the ` character")]);
        assert_eq!(inline("two **stars"), vec![text("two **stars")]);
    }

    #[test]
    fn two_adjacent_backticks_are_two_characters() {
        // The subset has no double-backtick delimiter, so `` is not an empty
        // code span — drawing one would put an empty box on the page.
        assert_eq!(inline("a `` b"), vec![text("a `` b")]);
    }

    #[test]
    fn a_code_span_holds_its_content_literally() {
        // Nothing inside a code span is a marker, which is what makes it usable
        // for writing about the markers.
        assert_eq!(
            inline("`**not strong**`"),
            vec![Span::Code("**not strong**".into())]
        );
    }

    #[test]
    fn a_strong_span_closes_at_the_last_pair_that_can_close_it() {
        // `**a ** b**` has a `**` in the middle that cannot close — a space
        // precedes it — so the span reaches the one that can.
        assert_eq!(inline("**a ** b**"), vec![Span::Strong("a ** b".into())]);
    }

    #[test]
    fn plain_takes_the_markers_off_and_changes_nothing_else() {
        // For a `<title>`, which is text and cannot carry markup.
        assert_eq!(plain("a **bold** `code` line"), "a bold code line");
    }

    #[test]
    fn a_span_that_wraps_across_source_lines_is_read_after_the_join() {
        // `examples/conditions.nomo` writes exactly this: each line carries an
        // odd number of backticks and only the joined paragraph an even one.
        let joined = blocks(&["the `else", "if` arm"]);
        let Block::Paragraph { text } = &joined[0] else {
            panic!("expected a paragraph: {joined:?}");
        };
        assert_eq!(
            inline(text),
            vec![
                super::Span::Text("the ".into()),
                Span::Code("else if".into()),
                super::Span::Text(" arm".into()),
            ]
        );
    }
}
