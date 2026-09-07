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
//! # Reading and writing are one description
//!
//! [`attach`] writes what [`Resources::scan`] reads, and it lives here for that
//! reason. The marker's spelling, the 76-column wrap, the four words of a block
//! header and the two of a body reference are all *format*, and a second
//! description of a format is a second thing to keep in step: the SMath emitter
//! had its own copy of the wrap and its own byte count until this existed, and
//! either could have drifted from the reader that has to accept them.
//!
//! It also decides where a figure may go, which is not obvious. An image pasted
//! with the cursor somewhere in the trailer cannot be referred to from there —
//! everything below the marker is data — so the reference goes to the last line
//! of the body instead. That is the nearest place the figure can actually
//! appear, and the alternative is a worksheet that swallowed a paste.
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

/// How many base64 characters a line of a block carries.
///
/// 76 is the MIME line length, and with the comment marker and the indent the
/// line lands on 80 columns. The reader strips whitespace and does not care, so
/// this is a choice about how the file reads to a person.
const WRAP: usize = 76;

/// One image, as the worksheet stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// The format the worksheet declared: `png`, `jpeg`.
    pub format: String,
    /// Base64, exactly as it appears in the trailer, with the line breaks taken
    /// out. Never decoded — see the module note.
    pub data: String,
}

/// How many bytes `base64` decodes to, without decoding it.
///
/// The size of the image rather than of its transport, which is what a block
/// header states and what a person reading a coverage report means. Four
/// characters carry three bytes, and the padding says how many of the last three
/// are real. Input that is not a whole number of quartets is not valid base64;
/// this is a report rather than a decoder, so it rounds down instead of failing,
/// and [`Image::is_well_formed`] is what refuses.
pub fn decoded_len(base64: &str) -> usize {
    let padding = base64.bytes().rev().take_while(|&b| b == b'=').count();
    (base64.len() / 4 * 3).saturating_sub(padding)
}

impl Image {
    /// The size of the image itself, rather than of its transport.
    pub fn bytes(&self) -> usize {
        decoded_len(&self.data)
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
    /// Byte offset of the marker line, when the worksheet has a trailer.
    ///
    /// Not derivable from `images`: a worksheet can carry a marker and no blocks
    /// — one whose last figure was deleted — and writing a second marker under
    /// the first would be the reader's problem for ever after. It is also what
    /// says which offsets are body and which are data, which is how [`attach`]
    /// keeps a reference out of the region that would hide it.
    trailer: Option<usize>,
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
        r.trailer = Some(ast.stmts[start].span().start as usize);

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

    /// Where the trailer starts, in bytes, if the worksheet has one.
    pub fn trailer_at(&self) -> Option<usize> {
        self.trailer
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

/// One image, ready to be written into a worksheet.
///
/// The size is the one the *reference* carries — how large the figure is drawn —
/// and not the image's own. They are different facts and only the caller knows
/// the second, so nothing here infers one from the other.
#[derive(Debug, Clone, Copy)]
pub struct Attachment<'a> {
    /// `png`, `jpeg`: what a block header will state and what
    /// [`Image::media_type`] must be able to name.
    pub format: &'a str,
    /// The image, already base64. Nothing here decodes or re-encodes it.
    pub data: &'a str,
    pub size: Size,
}

/// Text to insert at a byte offset in the source it was computed against.
///
/// Byte offsets, like every [`Span`](crate::span::Span) in this crate. A host
/// that counts in UTF-16 translates them where it translates every other offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    pub at: usize,
    pub text: String,
}

/// What adding an image to a worksheet does to its text.
///
/// Two insertions rather than a new document, because a worksheet carrying
/// figures is measured in megabytes and rewriting all of it to add one line
/// would cost the editor its undo history and its scroll position for no reason.
///
/// **`reference` is applied before `trailer`.** The two can land on the same
/// offset — the end of a worksheet whose last line is where the cursor was — and
/// then the order is what puts the figure above its data rather than inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    /// What the worksheet now calls this figure: `figure1`, `figure2`, …
    pub name: String,
    /// The `' image <name> <width>x<height>` line, in the body.
    pub reference: Splice,
    /// The block, at the end of the file, with a trailer around it if the
    /// worksheet did not already have one.
    pub trailer: Splice,
}

/// Add an image to `source`: a reference near `cursor`, and its data at the end.
///
/// `existing` is `source`'s own resources — what names are taken, and whether
/// there is already a trailer. Passing another worksheet's would name the figure
/// after the wrong file's images and write a second marker.
///
/// The reference goes on a line of its own, because that is the only thing it
/// can be: it is a whole comment line, so inserting it where a cursor happens to
/// sit would split whatever line the cursor was in. It lands on the cursor's own
/// line when that line is blank, which is where a person putting the cursor on a
/// blank line meant it to go, and after that line otherwise.
pub fn attach(source: &str, existing: &Resources, cursor: usize, image: &Attachment) -> Attached {
    let name = free_name(existing);
    let at = reference_at(source, existing, cursor);

    let Size { width, height } = image.size;
    let mut reference = format!("' image {name} {width}x{height}\n");
    // Every other insertion point is the first byte of a line by construction.
    // This one need not be: a worksheet whose last line has no newline ends
    // mid-line, and appending there would weld the reference onto it.
    if at > 0 && !source[..at].ends_with('\n') {
        reference.insert(0, '\n');
    }

    let mut trailer = String::new();
    // The same unterminated last line, seen from the other splice — but only
    // when the reference is not already the thing that terminated it.
    if !source.is_empty() && !source.ends_with('\n') && at < source.len() {
        trailer.push('\n');
    }
    if existing.trailer_at().is_none() {
        // A blank line above the marker, as `examples/figures.nomo` has it: the
        // trailer is a different kind of thing from the body and reads as one.
        if !source.is_empty() {
            trailer.push('\n');
        }
        trailer.push_str(&format!("' {TRAILER}\n"));
        trailer.push_str("' Images the worksheet carries, base64, one block each.\n");
        trailer.push_str("' A block is `' image <name> <format> <bytes>` followed by the\n");
        trailer
            .push_str("' indented lines under it, up to the next block or the end of the file.\n");
    }
    trailer.push_str(&block(&name, image.format, image.data));

    Attached {
        name,
        reference: Splice {
            at,
            text: reference,
        },
        trailer: Splice {
            at: source.len(),
            text: trailer,
        },
    }
}

/// One resource block: the header, and the data wrapped under it.
///
/// Public because the SMath emitter writes trailers too, and a second
/// implementation of this is a second thing that can drift from the reader.
pub fn block(name: &str, format: &str, data: &str) -> String {
    let mut out = format!("' image {name} {format} {}\n", decoded_len(data));
    let mut rest = data;
    while !rest.is_empty() {
        // Base64 is ASCII and every piece is `WRAP` bytes in practice. Cutting
        // on a character boundary anyway, because a `.nomo` file may have been
        // written by the SMath importer out of a third-party worksheet, and the
        // failure mode of assuming otherwise is a panic rather than a report.
        let cut = rest.char_indices().nth(WRAP).map_or(rest.len(), |(i, _)| i);
        let (head, tail) = rest.split_at(cut);
        out.push_str("'   ");
        out.push_str(head);
        out.push('\n');
        rest = tail;
    }
    out
}

/// The first `figureN` the worksheet is not already using.
///
/// Numbered from one and never reusing a gap: a worksheet whose `figure1` was
/// deleted gets `figure1` back, which is what a person counting figures expects.
/// The names a person wrote by hand are what this walks around — it is why the
/// question is "what is taken" rather than "how many are there".
fn free_name(existing: &Resources) -> String {
    (1..)
        .map(|n| format!("figure{n}"))
        .find(|name| existing.image(name).is_none())
        .expect("an unbounded sequence has a first free name")
}

/// Where the reference line goes, in bytes.
fn reference_at(source: &str, existing: &Resources, cursor: usize) -> usize {
    let body = existing.trailer_at().unwrap_or(source.len());
    let cursor = cursor.min(source.len());
    if cursor >= body {
        // The cursor is in the trailer, where a reference would be data and the
        // figure would never appear. The last line of the body is the nearest
        // place it can, so the reference goes immediately above the marker.
        return body;
    }

    let start = source[..cursor].rfind('\n').map_or(0, |i| i + 1);
    let end = source[cursor..]
        .find('\n')
        .map_or(source.len(), |i| cursor + i);
    if source[start..end].trim().is_empty() {
        return start;
    }
    // After the cursor's line, which is `end + 1` unless the file ends there.
    (end + 1).min(body)
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

    // ---- writing --------------------------------------------------------

    /// Both splices applied, back to front so the first does not move the
    /// second. Equivalent to a host applying them in order against the offsets
    /// it was given, which is what CodeMirror does with one transaction.
    fn apply(source: &str, a: &Attached) -> String {
        let mut out = source.to_string();
        out.insert_str(a.trailer.at, &a.trailer.text);
        out.insert_str(a.reference.at, &a.reference.text);
        out
    }

    const DOT: Attachment = Attachment {
        format: "png",
        data: "SGVsbG8h",
        size: Size {
            width: 700,
            height: 394,
        },
    };

    fn attached(source: &str, cursor: usize) -> (String, Attached) {
        let a = attach(source, &scan(source), cursor, &DOT);
        (apply(source, &a), a)
    }

    #[test]
    fn what_attach_writes_is_what_scan_reads() {
        // The one test the whole module is for: the writer and the reader are
        // two halves of one format, and nothing else here would notice them
        // disagreeing.
        let (out, a) = attached("' A worksheet\nx = 1\n", 0);
        assert_eq!(a.name, "figure1");
        let r = scan(&out);
        let image = r.image("figure1").expect("figure1");
        assert_eq!(image.format, "png");
        assert_eq!(image.data, "SGVsbG8h");
        assert!(image.is_well_formed());
        assert!(out.contains("' image figure1 700x394\n"), "{out}");
    }

    #[test]
    fn the_reference_the_body_gets_is_one_the_body_can_read() {
        let (out, _) = attached("x = 1\n", 0);
        let line = out
            .lines()
            .find_map(|l| reference(l.trim_start_matches("' ")))
            .expect("a body reference");
        assert_eq!(line.name, "figure1");
        assert_eq!(
            line.size,
            Some(Size {
                width: 700,
                height: 394
            })
        );
    }

    #[test]
    fn a_name_already_taken_is_walked_around() {
        let (out, a) = attached(ONE, 0);
        assert_eq!(a.name, "figure2");
        let r = scan(&out);
        assert_eq!(r.image("figure1").unwrap().bytes(), 6);
        assert_eq!(r.image("figure2").unwrap().data, "SGVsbG8h");
    }

    #[test]
    fn a_worksheet_with_no_trailer_gets_one() {
        let (out, _) = attached("x = 1\n", 0);
        assert_eq!(out.matches(TRAILER).count(), 1, "{out}");
        assert!(scan(&out).trailer_at().is_some());
    }

    #[test]
    fn a_worksheet_that_already_has_one_does_not_get_a_second() {
        // Including the case `images` cannot answer: a marker whose last block
        // was deleted. A second marker under the first is data for ever after.
        let empty = "x = 1\n\n' --- resources ---\n";
        let a = attach(empty, &scan(empty), 0, &DOT);
        let out = apply(empty, &a);
        assert_eq!(out.matches(TRAILER).count(), 1, "{out}");
        assert_eq!(scan(&out).image("figure1").unwrap().data, "SGVsbG8h");
    }

    #[test]
    fn the_reference_lands_on_the_cursors_own_line_when_it_is_blank() {
        // Where a person who put the cursor on a blank line meant it to go.
        // The blank line survives below it, so the gap the author left between
        // two blocks is still a gap.
        let source = "' A worksheet\n\nx = 1\n";
        let (out, _) = attached(source, 14);
        assert_eq!(
            out.lines().take(4).collect::<Vec<_>>(),
            ["' A worksheet", "' image figure1 700x394", "", "x = 1"],
            "{out}"
        );
    }

    #[test]
    fn the_reference_lands_after_the_cursors_line_when_it_is_not() {
        // Mid-word in `x = 1`. The reference is a whole line, so the only other
        // option is splitting the statement the cursor was in.
        let (out, _) = attached("' A worksheet\nx = 1\ny = 2\n", 16);
        assert_eq!(
            out.lines().take(3).collect::<Vec<_>>(),
            ["' A worksheet", "x = 1", "' image figure1 700x394"],
            "{out}"
        );
    }

    #[test]
    fn a_cursor_in_the_trailer_puts_the_reference_in_the_body() {
        // Everything below the marker is data, so a reference written there
        // would be a figure that never appears — the paste would be swallowed.
        let source = "x = 1\n\n' --- resources ---\n' image figure1 png 6\n'   SGVsbG8h\n";
        let cursor = source.find("SGVs").unwrap();
        let a = attach(source, &scan(source), cursor, &DOT);
        let out = apply(source, &a);
        let marker = out.find(TRAILER).unwrap();
        let placed = out.find("' image figure2 700x394").expect("the reference");
        assert!(
            placed < marker,
            "the reference is inside the trailer:\n{out}"
        );
        assert_eq!(scan(&out).image("figure2").unwrap().data, "SGVsbG8h");
    }

    #[test]
    fn a_last_line_with_no_newline_is_terminated_rather_than_welded_to() {
        let (out, _) = attached("x = 1", 5);
        assert_eq!(
            out.lines().take(2).collect::<Vec<_>>(),
            ["x = 1", "' image figure1 700x394"],
            "{out}"
        );
        assert_eq!(scan(&out).image("figure1").unwrap().data, "SGVsbG8h");
    }

    #[test]
    fn an_empty_worksheet_is_a_worksheet() {
        let (out, _) = attached("", 0);
        assert!(out.starts_with("' image figure1 700x394\n"), "{out}");
        assert_eq!(scan(&out).image("figure1").unwrap().data, "SGVsbG8h");
    }

    #[test]
    fn a_cursor_past_the_end_is_the_end() {
        // The host's offset and the engine's source can disagree by a keystroke.
        let (out, _) = attached("x = 1\n", 9_000);
        assert_eq!(scan(&out).image("figure1").unwrap().data, "SGVsbG8h");
    }

    #[test]
    fn a_size_is_reported_in_bytes_not_in_base64() {
        // What a report means by the size of an image is the image, not its
        // transport. Four characters carry three bytes; padding says how many
        // of the last three are real.
        assert_eq!(decoded_len("SGVsbG8h"), 6);
        assert_eq!(decoded_len("aGk="), 2);
        assert_eq!(decoded_len("aQ=="), 1);
        assert_eq!(decoded_len(""), 0);
    }

    #[test]
    fn a_block_states_the_size_of_the_image_and_not_of_its_transport() {
        let written = block("f", "png", "SGVsbG8h");
        assert!(written.starts_with("' image f png 6\n"), "{written}");
    }

    #[test]
    fn a_blob_is_wrapped_and_comes_back_whole() {
        // 200 characters is three lines and a short one, which is where an
        // off-by-one in the wrap would show.
        let data = "A".repeat(200);
        let written = block("f", "png", &data);
        let lines: Vec<&str> = written.lines().skip(1).collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("'   ") && lines[0].len() == 80);
        assert_eq!(lines[2].len(), 4 + 200 - 2 * WRAP);
        assert_eq!(
            scan(&format!("' --- resources ---\n{written}"))
                .image("f")
                .unwrap()
                .data,
            data
        );
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
