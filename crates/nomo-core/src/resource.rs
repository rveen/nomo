//! Images a worksheet carries, and where they live in its text.
//!
//! # The file is still its own source text
//!
//! A `.nomo` file has no container to put a resource fork in, so an image can
//! only live in it as base64. Written where the figure stands, a 116 KB blob in
//! the middle of a worksheet costs the text format the property it was chosen
//! for — that worksheets diff and review like code. So the body carries a
//! *reference* and the data goes in a trailer at the end:
//!
//! ```text
//! ' Measured response
//! ' image figure1 749x483
//!
//! ' --- resources ---
//! ' image figure1 png 116338
//! '   iVBORw0KGgoAAAANSUhEUgAABIkAAALrCAIAAAD…
//! '   AAAgAElEQVR4nOy9d3xUVfr4f2Yy6b1XSCM9IY…
//! ```
//!
//! The body then reads as it always did, and the blobs are one contiguous,
//! append-only region a `.gitattributes` rule can mark `-diff`.
//!
//! # Why the reference carries a size
//!
//! A figure in a worksheet is scanned evidence, and how large it was drawn is
//! part of what the author decided: a detail photographed at 1161 px wide was
//! placed at 749 px because that is the width at which it reads beside the
//! mathematics. The pixels alone cannot say that, so an import that keeps only
//! the pixels loses the layout — SMath's own figures are almost all scaled. The
//! size is therefore *placement*, and it goes on the reference in the body,
//! where the placement is, rather than in the trailer, which describes bytes.
//!
//! It is a natural size to scale down from and never a crop. A renderer that has
//! less room than the figure asks for shrinks it whole; there is no width at
//! which a reader is shown part of a diagram and not told so, and none at which
//! it is stretched either.
//!
//! # Why every line of it is a comment
//!
//! Because the version pragma already works this way. `' nomo 1` is an ordinary
//! comment that the document layer reads for meaning, chosen so that *nothing
//! downstream needs to know about it* — a build that has never heard of the
//! pragma still opens the file. The same reasoning applies here and matters
//! more: a worksheet carrying figures opens in every build that already exists,
//! and shows the trailer as the comments it is, rather than failing to parse.
//! That is the "old worksheets must always open" constraint of design note §7,
//! honoured in advance rather than retrofitted.
//!
//! It costs one thing, and it is worth naming: an image is not a statement, so
//! it cannot be produced by an expression, and nothing may compute one. That is
//! the right trade today — a figure in an engineering worksheet is scanned
//! evidence, not a result — and if it ever stops being right, the reference line
//! is what becomes a real statement.
//!
//! # No I/O, no decoding
//!
//! The engine may not read a file (`check-no-host-math.sh` enforces it), which
//! is also why external images beside the worksheet are not an option here. The
//! base64 is carried as text and handed to an HTML `data:` URI unchanged, so
//! rendering an image decodes nothing and cannot depend on the host.

use crate::ast::{Ast, Stmt};
use std::collections::{BTreeMap, BTreeSet};

/// The comment that begins the resource trailer.
///
/// Part of the format rather than decoration: everything from this line to the
/// end of the worksheet is data, and a renderer showing it as prose would print
/// several thousand lines of base64 at the reader.
pub const TRAILER: &str = "--- resources ---";

/// The indent that marks a continuation line inside a resource block.
///
/// Two spaces, because the parser has already taken the `'` and one space off
/// every comment: `'   AAAA` arrives here as `  AAAA`.
const CONTINUATION: &str = "  ";

/// One image, as the worksheet stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// The format the worksheet declared: `png`, `jpeg`.
    pub format: String,
    /// Base64, exactly as it appears in the trailer, with the line breaks taken
    /// out. Never decoded — see the module note.
    pub data: String,
}

impl Image {
    /// The size of the image itself, rather than of its transport.
    ///
    /// Four characters carry three bytes, and the padding says how many of the
    /// last three are real. Input that is not a whole number of quartets is not
    /// valid base64; this is a report rather than a decoder, so it rounds down
    /// instead of failing, and [`Image::is_well_formed`] is what refuses.
    pub fn bytes(&self) -> usize {
        let padding = self.data.bytes().rev().take_while(|&b| b == b'=').count();
        (self.data.len() / 4 * 3).saturating_sub(padding)
    }

    /// The media type for a `data:` URI, if this is a format we can name.
    ///
    /// Deliberately a short list. Naming a media type for a format nobody has
    /// seen is a guess, and a guess here produces a broken image rather than an
    /// honest report that the worksheet holds something we cannot show.
    pub fn media_type(&self) -> Option<&'static str> {
        match self.format.to_ascii_lowercase().as_str() {
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"),
            "bmp" => Some("image/bmp"),
            "webp" => Some("image/webp"),
            _ => None,
        }
    }

    /// Whether the payload is base64 and nothing else.
    ///
    /// Checked before the data reaches a `data:` URI. A `.nomo` file may have
    /// come from anywhere — the SMath importer reads third-party worksheets —
    /// and text that is not base64 has no business being pasted into an
    /// attribute in a document somebody is about to open.
    pub fn is_well_formed(&self) -> bool {
        !self.data.is_empty()
            && self.data.len().is_multiple_of(4)
            && self
                .data
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
    }
}

/// The images a worksheet carries, and which of its lines are their data.
#[derive(Debug, Clone, Default)]
pub struct Resources {
    images: BTreeMap<String, Image>,
    /// Statements that are part of the trailer, by index into the statement
    /// list. Parallel to the outcome list, which is how a renderer skips them.
    hidden: BTreeSet<usize>,
}

impl Resources {
    /// Read the trailer, if the worksheet has one.
    pub fn scan(ast: &Ast) -> Resources {
        let mut r = Resources::default();
        let start = ast.stmts.iter().position(|s| match s {
            Stmt::Comment { text, .. } => text.trim() == TRAILER,
            _ => false,
        });
        let Some(start) = start else {
            return r;
        };

        // Everything from the marker on is data, whatever it turns out to say.
        // A trailer that is malformed is still not prose, and printing it at the
        // reader is the one outcome worth ruling out entirely.
        r.hidden.extend(start..ast.stmts.len());

        let mut open: Option<(String, Image)> = None;
        for stmt in &ast.stmts[start + 1..] {
            let Stmt::Comment { text, .. } = stmt else {
                // Anything that is not a comment ends the trailer's data; it
                // cannot be part of a block, and the statement still evaluates.
                continue;
            };
            if let Some(rest) = text.strip_prefix(CONTINUATION) {
                if let Some((_, image)) = open.as_mut() {
                    image.data.push_str(rest.trim());
                }
                continue;
            }
            if let Some((name, image)) = header(text) {
                if let Some((previous, image)) = open.replace((name, image)) {
                    r.images.insert(previous, image);
                }
            }
        }
        if let Some((name, image)) = open {
            r.images.insert(name, image);
        }
        r
    }

    /// Whether this statement is trailer data rather than something to show.
    pub fn is_hidden(&self, index: usize) -> bool {
        self.hidden.contains(&index)
    }

    pub fn image(&self, name: &str) -> Option<&Image> {
        self.images.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Every image, by name, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Image)> {
        self.images.iter().map(|(k, v)| (k.as_str(), v))
    }
}

/// The size a figure is drawn at, in the pixels of the page it came from.
///
/// Placement rather than content: it says how large the figure stood in the
/// document, not what is in the file. A renderer treats it as a natural size to
/// scale *down* from — see [`reference`].
///
/// The width is what the figure is drawn at. The height is carried and written
/// with it, so the space is reserved before the image decodes, but the drawn
/// height follows the image's own proportions: SMath lets a picture region be
/// dragged out of shape, and a stretched diagram is a wrong diagram that nothing
/// on the page would admit to. Where the two agree — which is every figure
/// scaled proportionally, and so nearly all of them — this is the same picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

/// An `image` line in the body: the figure that goes here, and how big it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference<'a> {
    pub name: &'a str,
    /// `None` when the line does not say, which is every worksheet written
    /// before the size existed. A renderer then shows the image at its own
    /// size, which is what it did before.
    pub size: Option<Size>,
}

/// What an `image <name>` line in the body refers to, and at what size.
///
/// Two words name a figure; three name one and say how large it was drawn:
///
/// ```text
/// ' image figure1 749x483
/// ```
///
/// Never four: a header inside the trailer carries a format and a byte count as
/// well, and reading one as a body reference would show the same figure twice.
/// A third word that is not a size is not a reference either — the line stays
/// prose rather than becoming a figure whose size was quietly ignored.
///
/// # Why the size lives here and not in the trailer
///
/// The trailer describes the image; this describes where it stands. They are
/// different facts, and only this one can differ between two places the same
/// figure is used. It is also the half a person edits: changing how large a
/// figure appears should not mean touching the line that says what its bytes
/// are.
pub fn reference(comment: &str) -> Option<Reference<'_>> {
    let mut words = comment.split_whitespace();
    match (words.next(), words.next(), words.next(), words.next()) {
        (Some("image"), Some(name), None, None) => Some(Reference { name, size: None }),
        (Some("image"), Some(name), Some(size), None) => Some(Reference {
            name,
            size: Some(size_of(size)?),
        }),
        _ => None,
    }
}

/// `<width>x<height>`, both in pixels.
///
/// A zero in either is refused rather than carried: it would render as an
/// invisible figure, and a worksheet that silently shows nothing where a figure
/// stands is the failure the whole resource path is written to avoid.
fn size_of(word: &str) -> Option<Size> {
    let (w, h) = word.split_once('x')?;
    let (width, height) = (w.parse().ok()?, h.parse().ok()?);
    (width > 0 && height > 0).then_some(Size { width, height })
}

/// `image <name> <format> <bytes>` — the first line of a resource block.
///
/// The declared size is not kept. It is there so a person reading the source can
/// see how large a figure is without decoding it, and believing it over the data
/// would let a wrong number in the file describe an image that is really there.
fn header(comment: &str) -> Option<(String, Image)> {
    let mut words = comment.split_whitespace();
    match (words.next(), words.next(), words.next(), words.next()) {
        (Some("image"), Some(name), Some(format), Some(size)) if size.parse::<u64>().is_ok() => {
            Some((
                name.to_string(),
                Image {
                    format: format.to_string(),
                    data: String::new(),
                },
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;

    fn scan(source: &str) -> Resources {
        Resources::scan(&Document::parse(source).ast)
    }

    // "SGVsbG8h" decodes to `Hello!`, six bytes.
    const ONE: &str =
        "' image figure1\n\n' --- resources ---\n' image figure1 png 6\n'   SGVsbG8h\n";

    #[test]
    fn an_image_is_read_out_of_the_trailer() {
        let r = scan(ONE);
        let image = r.image("figure1").expect("figure1");
        assert_eq!(image.format, "png");
        assert_eq!(image.data, "SGVsbG8h");
        assert_eq!(image.bytes(), 6);
    }

    #[test]
    fn a_blob_split_over_many_lines_is_one_image() {
        // The whole reason the trailer wraps: a 116 KB single line is what the
        // body was spared, so it must not reappear as a constraint on the data.
        let r = scan("' --- resources ---\n' image f png 6\n'   SGVs\n'   bG8h\n");
        assert_eq!(r.image("f").unwrap().data, "SGVsbG8h");
    }

    #[test]
    fn the_trailer_is_not_prose() {
        // Rendered as comments, this worksheet prints its base64 at the reader.
        let r = scan(ONE);
        let doc = Document::parse(ONE);
        let marker = doc
            .ast
            .stmts
            .iter()
            .position(|s| matches!(s, Stmt::Comment { text, .. } if text.trim() == TRAILER))
            .unwrap();
        assert!(!r.is_hidden(marker - 1), "the body must still be shown");
        for i in marker..doc.ast.stmts.len() {
            assert!(r.is_hidden(i), "statement {i} should be trailer data");
        }
    }

    #[test]
    fn several_blocks_are_kept_apart() {
        let r = scan("' --- resources ---\n' image a png 6\n'   SGVs\n' image b png 2\n'   aGk=\n");
        assert_eq!(r.image("a").unwrap().data, "SGVs");
        assert_eq!(r.image("b").unwrap().data, "aGk=");
        assert_eq!(r.image("b").unwrap().bytes(), 2);
    }

    #[test]
    fn a_body_reference_is_two_words_or_three_and_a_block_header_is_four() {
        assert_eq!(reference("image figure1").map(|r| r.name), Some("figure1"));
        assert_eq!(reference("image figure1").and_then(|r| r.size), None);
        assert_eq!(reference("image figure1 png 6"), None);
        assert_eq!(reference("imagine that"), None);
        assert_eq!(reference("a note about the image"), None);
    }

    #[test]
    fn a_reference_may_say_how_large_the_figure_was_drawn() {
        let r = reference("image figure1 749x483").expect("a reference");
        assert_eq!(r.name, "figure1");
        assert_eq!(
            r.size,
            Some(Size {
                width: 749,
                height: 483
            })
        );
    }

    #[test]
    fn a_third_word_that_is_not_a_size_is_not_a_reference() {
        // The alternative is a figure drawn at a size nobody wrote, from a line
        // whose meaning we guessed at. Left un-matched, the line stays the
        // comment it is and the reader can see what it says.
        assert_eq!(reference("image figure1 large"), None);
        assert_eq!(reference("image figure1 749"), None);
        assert_eq!(reference("image figure1 749x"), None);
        assert_eq!(reference("image figure1 -1x8"), None);
    }

    #[test]
    fn a_zero_dimension_is_refused_rather_than_carried() {
        // It renders as an invisible figure, which is the one outcome the
        // resource path exists to prevent.
        assert_eq!(reference("image figure1 0x483"), None);
        assert_eq!(reference("image figure1 749x0"), None);
    }

    #[test]
    fn a_worksheet_without_a_trailer_hides_nothing() {
        let r = scan("' just prose\nx = 1\n");
        assert!(r.is_empty());
        assert!(!r.is_hidden(0));
    }

    #[test]
    fn data_that_is_not_base64_is_refused_rather_than_embedded() {
        // It reaches an HTML attribute in a document somebody opens, and a
        // `.nomo` file may have been written by the SMath importer out of a
        // third-party worksheet.
        let bad = Image {
            format: "png".into(),
            data: "\"><script>alert(1)</script>".into(),
        };
        assert!(!bad.is_well_formed());
        assert!(Image {
            format: "png".into(),
            data: "SGVsbG8h".into()
        }
        .is_well_formed());
    }

    #[test]
    fn a_format_we_cannot_name_is_not_guessed_at() {
        let odd = Image {
            format: "tiff".into(),
            data: "SGVsbG8h".into(),
        };
        assert_eq!(odd.media_type(), None);
    }
}
