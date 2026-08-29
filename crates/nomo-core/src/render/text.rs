//! Plain-text rendering.
//!
//! This is the format the golden-file suite diffs, so it is deliberately stable
//! and free of anything positional: no column alignment that shifts when an
//! unrelated line grows longer, and no trailing whitespace.

use super::{RenderOptions, Renderer};
use crate::doc::Sheet;
use crate::eval::OutcomeKind;
use crate::resource::{self, Reference, Resources};

/// Render an evaluated worksheet as plain text.
pub fn render(sheet: &Sheet, opts: &RenderOptions) -> String {
    let source = sheet.source().to_string();
    let units = sheet.units().clone();
    let r = Renderer::new(opts, &units, &source);
    let mut out = String::new();

    for (i, outcome) in sheet.outcomes().iter().enumerate() {
        // The resource trailer is data, not prose. Printing it would put
        // several thousand lines of base64 in front of the reader, and into
        // every golden snapshot. The version pragma is metadata for the same
        // reason: it says which format the file is in, not anything about the
        // engineering, and the two renderers agree on what counts as prose.
        if sheet.resources().is_hidden(i) || sheet.is_version_pragma(i) || sheet.is_from_pack(i) {
            continue;
        }
        match &outcome.kind {
            OutcomeKind::Comment(text) => {
                if let Some(reference) = resource::reference(text) {
                    out.push_str(&image_line(sheet.resources(), &reference));
                    out.push('\n');
                } else if text.is_empty() {
                    out.push('\n');
                } else {
                    out.push_str(text);
                    out.push('\n');
                }
            }

            OutcomeKind::Assign { name, trace } => {
                let mut line = format!("{name} = {}", r.symbolic(trace));
                if r.substitution_is_informative(trace) {
                    line.push_str(&format!(" = {}", r.substituted(trace)));
                }
                if !r.is_literal_quantity(trace) {
                    let result = r.result(trace);
                    if result != r.symbolic(trace) {
                        line.push_str(&format!(" = {result}"));
                    }
                }
                out.push_str(&line);
                out.push('\n');
            }

            OutcomeKind::Query(trace) => {
                let mut line = r.symbolic(trace).to_string();
                if r.substitution_is_informative(trace) {
                    line.push_str(&format!(" = {}", r.substituted(trace)));
                }
                line.push_str(&format!(" = {}", r.result(trace)));
                out.push_str(&line);
                out.push('\n');
            }

            OutcomeKind::Use(name) => {
                out.push_str(&format!("use {name}\n"));
            }

            OutcomeKind::Check { trace, passed } => {
                // The verdict stands where a result would, because for a check
                // it *is* the result: the 1 or 0 the comparison produced says
                // nothing a reader wants, and "pass" says the whole of it.
                let mut line = format!("check {}", r.symbolic(trace));
                if r.substitution_is_informative(trace) {
                    line.push_str(&format!(" = {}", r.substituted(trace)));
                }
                line.push_str(match passed {
                    Some(true) => " — pass",
                    Some(false) => " — FAIL",
                    None => " — [not decided]",
                });
                out.push_str(&line);
                out.push('\n');
            }

            OutcomeKind::UnitDecl { name, trace } => {
                // The defining expression, not its base-SI magnitude: `1000
                // lbf/ft` says what the unit is, `14593.9 kg·s⁻²` does not.
                out.push_str(&format!("unit {name} = {}\n", r.symbolic(trace)));
            }

            OutcomeKind::FnDef(name) => {
                out.push_str(&format!("fn {name} defined\n"));
            }

            OutcomeKind::NotEvaluated => {
                out.push_str("[not evaluated]\n");
            }

            OutcomeKind::Malformed => {
                out.push_str("[unparsed]\n");
            }
        }
    }

    out
}

/// How an image is written in the text rendering.
///
/// A description rather than the picture, because this is the format the
/// golden-file suite diffs: it has to be stable, and it has to say enough that
/// a changed or missing figure shows up as a changed line. Nothing here is
/// positional, for the same reason the rest of this renderer is not.
fn image_line(resources: &Resources, reference: &Reference<'_>) -> String {
    let name = reference.name;
    // The size the figure is drawn at is part of the worksheet, so it belongs in
    // the format the golden suite diffs: an import that started scaling figures
    // differently would otherwise change every rendered page and no snapshot.
    let size = match reference.size {
        Some(s) => format!(", {}x{}", s.width, s.height),
        None => String::new(),
    };
    match resources.image(name) {
        // Reported rather than skipped: a reference whose data is not in the
        // file is exactly the kind of loss a migration has to make visible.
        None => format!("[image {name}: missing]"),
        Some(image) if !image.is_well_formed() => format!("[image {name}: unreadable]"),
        Some(image) => format!(
            "[image {name}: {}, {} bytes{size}]",
            image.format,
            image.bytes()
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::doc::Sheet;
    use crate::render::RenderOptions;

    fn text_of(source: &str) -> String {
        super::render(&Sheet::new(source), &RenderOptions::default())
    }

    #[test]
    fn an_image_is_described_rather_than_drawn() {
        // This is what the golden suite diffs, so it has to say enough that a
        // changed or missing figure shows up as a changed line.
        let t =
            text_of("' image gauge\n\n' --- resources ---\n' image gauge png 6\n'   SGVsbG8h\n");
        assert_eq!(t, "[image gauge: png, 6 bytes]\n");
    }

    #[test]
    fn the_size_a_figure_is_drawn_at_is_part_of_the_line() {
        let t = text_of(
            "' image gauge 749x483\n\n' --- resources ---\n' image gauge png 6\n'   SGVsbG8h\n",
        );
        assert_eq!(t, "[image gauge: png, 6 bytes, 749x483]\n");
    }

    #[test]
    fn a_reference_with_no_data_is_reported() {
        assert_eq!(text_of("' image nowhere\n"), "[image nowhere: missing]\n");
    }

    #[test]
    fn the_trailer_is_not_printed_at_the_reader() {
        let t = text_of("x = 1\n' --- resources ---\n' image g png 6\n'   SGVsbG8h\n");
        assert_eq!(t, "x = 1\n");
    }
}
