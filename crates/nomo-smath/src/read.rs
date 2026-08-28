//! Turning the bytes of a `.sm` file into regions.
//!
//! The file is XML, UTF-8 with a byte-order mark — 114 of 114 worksheets across
//! both corpora — carrying a `<?application?>` processing instruction with the
//! writing version, a `<settings>` block, and `<region>` elements each holding
//! exactly one payload. Line endings are *not* a constant (the wiki corpus is
//! CRLF throughout, the mechanics corpus LF), and nothing here depends on them.
//!
//! **Three container shapes, not two.** The math body changes at 0.88 (see
//! [`Era`]) and the container changes again at 1.x, independently:
//!
//! | | 0.82–0.85 | 0.88–0.98 | 1.3–1.5 |
//! |---|---|---|---|
//! | Root | `<regions>` | `<regions>` | `<worksheet>` wrapping `<regions>` |
//! | Namespace | none | none | `http://smath.info/schemas/worksheet/1.0` |
//! | Nesting | none | `<area>` contains what it collapses | `<area>` is a marker again; `<text>` may contain `<regions>` of inline math |
//!
//! Each of those breaks a reader written for the previous one *completely*
//! rather than partially: the namespace alone makes a pre-1.x reader find zero
//! regions in every 1.x file, and looking for nested regions in the wrong place
//! drops content without a word. So the era is never inferred from the version
//! string, only from structure, and both nesting sites are read.
//!
//! Regions are read **in file order** and never sorted. Across both corpora file
//! order, ascending `id` and ascending (top, left) coincide at page level
//! without exception, so SMath evidently normalises order on save. That is an
//! empirical regularity rather than a documented guarantee, so it is checked on
//! every read and reported through [`Worksheet::order_anomalies`] rather than
//! assumed.

use crate::expr::{self, Expr, Statement};

/// Which shape of the format a worksheet is written in. See the crate docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Era {
    /// 0.82–0.85. `<e>` sits directly under `<math>`, and a stored answer is the
    /// second operand of a binary `=`.
    Legacy,
    /// 0.88 and later. `<e>` sits under `<math><input>`, and a stored answer is
    /// a sibling `<result action="numeric">`.
    Modern,
}

#[derive(Debug)]
pub enum ReadError {
    NotUtf8,
    Xml(String),
    /// No `<regions>` element: not an SMath worksheet at all.
    NotAWorksheet,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::NotUtf8 => write!(f, "not valid UTF-8"),
            ReadError::Xml(e) => write!(f, "malformed XML: {e}"),
            ReadError::NotAWorksheet => write!(f, "no <regions> element"),
        }
    }
}

impl std::error::Error for ReadError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Operand,
    Operator,
    Function,
    Bracket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Unit,
    Str,
}

/// One `<e>` element.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    /// The `args` attribute: the arity, and the only thing separating unary from
    /// binary `-`. Never inferred from the glyph.
    pub args: Option<usize>,
    pub style: Option<Style>,
}

/// Document-level calculation settings.
///
/// `angle` and `precision` are not cosmetic. `angle` decides whether every trig
/// call in the document is in radians or degrees, and `precision` is the rounding
/// a stored result was displayed with — so both are needed before any answer in
/// the file can be compared with a recomputed one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub precision: Option<u32>,
    /// Whether `precision` counts significant digits rather than decimal places.
    /// Absent before 1.x.
    pub significant_digits: Option<bool>,
    /// Radians or degrees, and it decides every trig result in the document.
    /// **Absent in 1.x**, which puts the angle on `°`/`rad` unit operands
    /// instead, so its absence is not a malformed file.
    pub angle: Option<String>,
    pub fractions: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Math {
    pub statement: Statement,
    /// The answer SMath computed and stored, in the newer era.
    pub result: Option<Expr>,
    /// The unit the answer is displayed in. Without it a stored `96` against a
    /// computed `2.4384 m` looks like an engine bug rather than inches.
    pub contract: Option<Expr>,
    /// The note SMath attaches to the region, in every language it was written
    /// in. 221 of them in the mechanics corpus, and they are not decoration: the
    /// `description(x)` function *reads* this text, so a worksheet that labels a
    /// plot axis keeps the label here and nowhere else. Dropping it loses
    /// content that the math refers to.
    pub description: Vec<(Option<String>, String)>,
    /// Per-region display rounding, which overrides the document `precision`
    /// when present.
    pub decimal_places: Option<u32>,
    /// Whether this region's rounding counts *significant digits* rather than
    /// decimal places. 1.x carries it per region and it reinterprets the number
    /// in `decimal_places`, so a comparison that ignores it rounds to the wrong
    /// thing rather than merely to the wrong precision.
    pub significant_digits: Option<bool>,
    pub trailing_zeros: Option<bool>,
    /// `ignoreUnits="true"`: this region deliberately switches dimensional
    /// checking off, so a unit mismatch in it is the author's intent.
    pub ignore_units: bool,
    /// SMath's own error code for a region that already fails inside SMath. Such
    /// a region is not an import failure and must not be counted as one.
    pub error: Option<String>,
    /// The region's optimization setting: `0` none, `1` numeric, `2` symbolic.
    ///
    /// It is not a display preference. A region set to symbolic is evaluated by
    /// SMath's CAS, so a name nothing binds stays a **free symbol** there rather
    /// than raising the error a numeric region would — which is why a worksheet
    /// can hold a formula written for a reader instead of for the engine and
    /// still save clean, with no `error` attribute to mark it. 351 regions
    /// across 48 corpus files carry `optimize="2"`.
    pub optimize: Option<u8>,
    /// What kind of answer is stored. `Symbolic` results are kept as provenance
    /// for a human reading the migration; they are never assertions, because a
    /// numeric engine will not reproduce them.
    pub result_kind: Option<ResultKind>,
    pub evaluate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultKind {
    Numeric,
    Symbolic,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    /// Boxed because a math payload carries far more than any other — its
    /// statement, stored answer, contract, description and display flags — and
    /// an enum sized for its largest variant would make every text region as
    /// expensive as a math one.
    Math(Box<Math>),
    /// Prose, in every language it was written in.
    ///
    /// 902 text regions in the mechanics corpus carry the same paragraph in
    /// German, English and Russian. Which one an imported worksheet keeps is a
    /// policy question for whatever writes Nomo documents, not something the
    /// reader may decide by taking the first and saying nothing — so every
    /// variant is kept, prose and all, and the choice is made downstream.
    Text {
        /// `(language, prose)` in document order. The language is `None` when
        /// the region carries no `lang` attribute, which happens in 28 of the
        /// wiki corpus's regions.
        variants: Vec<(Option<String>, String)>,
    },
    /// A chart. `<plot>` is SMath's own; `<xyplot>` is the third-party X-Y Plot
    /// Region, which is a declarative chart specification — axes, grid, traces,
    /// fonts — wrapped around exactly one ordinary postfix `<input>`, so the same
    /// reducer reads both and only the tag distinguishes them in a report.
    Plot {
        expr: Expr,
        tag: String,
        /// The stored viewport, for a 2D `<plot>`. `None` for an `<xyplot>`,
        /// whose third-party region keeps its own axes and is not this model.
        /// See [`PlotView`].
        view: Option<PlotView>,
    },
    /// An embedded image: `format` is the encoding SMath declared — `png` in
    /// every corpus region — and `data` is its base64, verbatim.
    ///
    /// The reader used to keep the encoded length alone, on the grounds that a
    /// coverage report has no use for pixels. That was right for a report and
    /// wrong for an import: the worksheet that settled it is 77 lines of
    /// mathematics around 633 KB of PNG, so a migration that drops the figures
    /// has not migrated the document. Every caller reads one worksheet at a
    /// time, so holding the images costs one file's worth rather than a
    /// corpus's.
    ///
    /// Kept as base64 rather than decoded because the importer's only use for it
    /// is to write it out again, and a decode-then-encode round trip is a chance
    /// to change bytes for nothing in return.
    Picture {
        format: String,
        data: String,
        /// `(width, height)` in the page's pixels — the size SMath drew the
        /// figure at, which is almost never the size of the pixels it holds.
        /// The case that settled it holds a 1161x747 PNG placed at 749x483,
        /// so an import that keeps the data and drops this has kept the
        /// evidence and lost the author's decision about how it reads.
        ///
        /// `None` when the worksheet does not say, which is what a region with
        /// no box looks like in the oldest era.
        size: Option<(u32, u32)>,
    },
    /// A collapsible section. In the legacy era this is a bare marker in the
    /// flat region list; from 0.88 it is a container, and the regions it
    /// collapses are nested inside it.
    Area {
        title: String,
    },
    Unsupported {
        tag: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub id: Option<i64>,
    pub left: i64,
    pub top: i64,
    /// The region's box on the page. Kept because a `.sm` worksheet is a
    /// two-dimensional page and a `.nomo` one is a list of lines: knowing which
    /// regions shared a row is the only way to tell a reader that a label they
    /// see *after* a value was printed *beside* it.
    pub width: i64,
    pub height: i64,
    pub payload: Payload,
    /// Regions nested inside this one. Only a collapsible `<area>` has any, and
    /// only from version 0.88: 442 of the corpus's 3878 regions live one or two
    /// levels down, so a reader that takes only the top level silently drops
    /// eleven per cent of every worksheet that uses a collapsed section.
    pub children: Vec<Region>,
}

impl Region {
    /// This region and everything nested inside it, in reading order.
    fn walk<'a>(&'a self, out: &mut Vec<&'a Region>) {
        out.push(self);
        for c in &self.children {
            c.walk(out);
        }
    }
}

/// One `<assembly>` entry from a worksheet's `<dependencies>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Worksheet {
    /// The version from the `<?application?>` instruction, verbatim. Some are
    /// `0.85`, some are `0.96.4909.6802`.
    pub version: String,
    /// The plugins the file says it needs, from `<dependencies>`. Empty before
    /// 1.x, which had no manifest.
    ///
    /// This is the best thing the newer container adds. A migration report can
    /// say what a worksheet requires **before parsing a single token**, and it
    /// distinguishes a name nobody has heard of from one supplied by a plugin
    /// that is out of scope by decision — which are different answers.
    pub dependencies: Vec<Dependency>,
    pub era: Era,
    pub settings: Settings,
    pub regions: Vec<Region>,
    /// Regions from a top-level `<regions>` block that is not the content:
    /// SMath's page header and footer.
    ///
    /// Kept apart from `regions` rather than appended to them, because they are
    /// not the document. A header repeats on every printed page, and the ones
    /// that turned this up hold an author's name, a company logo, and a date
    /// like `04-04-2025` — which SMath stores as *arithmetic*, three operands
    /// and two subtractions. Read as worksheet content that is a query
    /// evaluating to -2025.
    ///
    /// Kept at all because the alternative was silence: the reader took the
    /// first `<regions>` block and no other, so this file's three header regions
    /// vanished with no marker and no count. One worksheet in 117 has a second
    /// block, which is exactly how a bug like that survives.
    pub furniture: Vec<Region>,
}

impl Worksheet {
    /// Regions whose position goes backwards relative to the one before them.
    ///
    /// Empty for every worksheet in both corpora. A non-empty result means this
    /// file breaks the ordering regularity the importer relies on, and its
    /// evaluation order should not be trusted to file order.
    ///
    /// Compared **at page level only**, which is the assumption that matters:
    /// item 3 of the design note's checklist relies on file order being page
    /// order so that evaluation may follow it.
    ///
    /// Nested regions are excluded deliberately. From 1.x a `<text>` may carry
    /// inline math in `<regions type="content">` blocks, one per formula, each
    /// positioned relative to the text rather than the page — inline math sits
    /// at `top="0"` — and a region's children are the concatenation of several
    /// such blocks. Every block is internally ordered, but the concatenation is
    /// not, so comparing across it reports an anomaly in a quarter of the
    /// mechanics corpus that says nothing about the file. Page order is a real
    /// property; inline order is file order and nothing else.
    pub fn order_anomalies(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut previous: Option<(i64, i64)> = None;
        for (i, r) in self.regions.iter().enumerate() {
            let here = (r.top, r.left);
            if previous.is_some_and(|before| here < before) {
                out.push(i);
            }
            previous = Some(here);
        }
        out
    }

    /// Every region in reading order, nested ones included.
    ///
    /// Depth-first and pre-order, so a collapsed section's own marker comes
    /// before its contents — which is the order they appear on the page and the
    /// order the document is evaluated in.
    pub fn flat(&self) -> Vec<&Region> {
        let mut out = Vec::new();
        for r in &self.regions {
            r.walk(&mut out);
        }
        out
    }

    /// Every page-header and footer region, in the same order.
    ///
    /// Separate from [`Worksheet::flat`] so that anything walking the document
    /// keeps getting the document. A caller that means "every region in the
    /// file" — a coverage report does — asks for both and says so.
    pub fn flat_furniture(&self) -> Vec<&Region> {
        let mut out = Vec::new();
        for r in &self.furniture {
            r.walk(&mut out);
        }
        out
    }

    /// Math regions that share a page row with prose to their left.
    ///
    /// A `.sm` worksheet is a page: the mechanics corpus writes a label at
    /// `left=0` and the value it describes at `left=135` on the same row. Read
    /// as a list of lines — which is what file order gives, and what §8.3
    /// established is correct — the label lands *after* the value, because it
    /// sits a few pixels lower. Nothing is lost and nothing is misordered, but a
    /// reader should be told that the two were side by side rather than assume
    /// a comment introduces the line under it.
    ///
    /// Counted rather than acted on. Only 716 of the mechanics corpus's 2744
    /// math regions have exactly one candidate label to their left, and 107 have
    /// several, so re-associating prose with values would be guesswork on a
    /// quarter of the document and wrong somewhere in it.
    pub fn side_by_side_rows(&self) -> usize {
        let flat = self.flat();
        let overlaps = |a: &Region, b: &Region| {
            a.top < b.top + b.height.max(1) && b.top < a.top + a.height.max(1)
        };
        flat.iter()
            .filter(|m| matches!(m.payload, Payload::Math(_)))
            .filter(|m| {
                flat.iter().any(|t| {
                    matches!(t.payload, Payload::Text { .. }) && t.left < m.left && overlaps(m, t)
                })
            })
            .count()
    }

    pub fn math(&self) -> impl Iterator<Item = &Math> {
        self.flat()
            .into_iter()
            .filter_map(|r| match &r.payload {
                Payload::Math(m) => Some(m.as_ref()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .into_iter()
    }
}

pub fn worksheet(bytes: &[u8]) -> Result<Worksheet, ReadError> {
    // SMath writes a byte-order mark. roxmltree will not accept one.
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| ReadError::NotUtf8)?;
    let doc = roxmltree::Document::parse(text).map_err(|e| ReadError::Xml(e.to_string()))?;

    let version = doc
        .descendants()
        .filter_map(|n| n.pi())
        .find(|pi| pi.target == "application")
        .and_then(|pi| pi.value)
        .and_then(attr_from_pi)
        .unwrap_or_default();

    // 1.x wraps everything in `<worksheet>`; before it the root *was*
    // `<regions>`. `<settings>` moved out to the wrapper at the same time, so
    // both are looked for from whichever element turns out to be the container.
    //
    // There may be more than one block, and taking the first was a silent bug:
    // a worksheet carrying `type="content"` followed by `type="header"` had its
    // three header regions — one text, one picture, one math — dropped without
    // a marker or a count. One file in 118 did it, which is how such a thing
    // survives being measured twice; the test below pins the shape rather than
    // the file, which is what keeps it once the file is gone.
    let root = doc.root_element();
    let blocks: Vec<roxmltree::Node> = match root.tag_name().name() {
        "regions" => vec![root],
        "worksheet" => root.children().filter(|n| is(n, "regions")).collect(),
        _ => return Err(ReadError::NotAWorksheet),
    };
    if blocks.is_empty() {
        return Err(ReadError::NotAWorksheet);
    }
    // `content` is the worksheet and anything else is furniture. Pre-1.x has a
    // single untyped block, so falling back to the first is what reads that era.
    let content = blocks
        .iter()
        .position(|b| b.attribute("type") == Some("content"))
        .unwrap_or(0);

    let settings = root
        .descendants()
        .find(|n| is(n, "settings"))
        .map(read_settings)
        .unwrap_or_default();

    let in_block = |b: &roxmltree::Node| -> Vec<Region> {
        b.children()
            .filter(|n| is(n, "region"))
            .map(read_region)
            .collect()
    };
    let regions = in_block(&blocks[content]);
    let furniture: Vec<Region> = blocks
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != content)
        .flat_map(|(_, b)| in_block(b))
        .collect();

    // Structural detection beats the version string. Version numbers in the
    // corpus range from `0.85` to `0.96.4909.6802`, and what actually matters is
    // whether math is wrapped in `<input>` — which is a fact about this file, not
    // about the release that wrote it.
    let era = if root.descendants().any(|n| is(&n, "input")) {
        Era::Modern
    } else {
        Era::Legacy
    };

    // Sits under the wrapper in 1.x and nowhere at all before it.
    let dependencies = root
        .descendants()
        .filter(|n| is(n, "assembly"))
        .map(|n| Dependency {
            name: n.attribute("name").unwrap_or("?").to_string(),
            version: n.attribute("version").unwrap_or("?").to_string(),
        })
        .collect();

    Ok(Worksheet {
        version,
        dependencies,
        era,
        settings,
        regions,
        furniture,
    })
}

/// Pull `version="..."` out of the pseudo-attributes of a processing instruction.
fn attr_from_pi(value: &str) -> Option<String> {
    let at = value.find("version=\"")? + "version=\"".len();
    let rest = &value[at..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn read_settings(node: roxmltree::Node) -> Settings {
    let mut s = Settings::default();
    for n in node.descendants() {
        let text = n.text().unwrap_or("").trim().to_string();
        if text.is_empty() {
            continue;
        }
        match n.tag_name().name() {
            "precision" => s.precision = text.parse().ok(),
            // 1.x only, and it changes what `precision` means.
            "significantDigitsMode" => s.significant_digits = Some(text == "true"),
            "angle" => s.angle = Some(text),
            "fractions" => s.fractions = Some(text),
            "title" => s.title = Some(text),
            "author" => s.author = Some(text),
            _ => {}
        }
    }
    s
}

fn read_region(node: roxmltree::Node) -> Region {
    // The payload is the first element that is not itself a region: from 0.88 an
    // `<area>` is followed by the regions it collapses, as siblings under it.
    let payload = node
        .children()
        .find(|n| n.is_element() && !is(n, "region"))
        .map(read_payload)
        .unwrap_or(Payload::Unsupported { tag: String::new() });

    Region {
        id: node.attribute("id").and_then(|v| v.parse().ok()),
        left: node
            .attribute("left")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        top: node
            .attribute("top")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        width: node
            .attribute("width")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        height: node
            .attribute("height")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        payload,
        children: nested(node),
    }
}

/// The regions nested inside this one, wherever this version hid them.
///
/// The corpus disagrees with itself twice over. In 0.82–0.85 nothing nests. From
/// 0.88 a collapsible `<area>` *contains* the regions it hides, as `<region>`
/// siblings under the region element. In 1.3–1.5 `<area>` reverts to a bare
/// marker — `<area single="true" collapsed="true"/>`, never holding a region —
/// and nesting moves to `<text>`, which may carry a `<regions type="content">`
/// block of math set inside the prose: 174 of the mechanics corpus's 308 nested
/// regions arrive that way and the remaining 134 sit one level deeper inside
/// those. Looking in only one of the two places drops content silently, which is
/// the failure item 23 exists to prevent, so both are collected.
fn nested(node: roxmltree::Node) -> Vec<Region> {
    let direct = node.children().filter(|n| is(n, "region"));
    // Only `<regions>` blocks belonging to *this* region. A nested region has
    // its own, and descending blindly would collect a grandchild twice: once
    // here and once as its own parent's child.
    let embedded = node
        .descendants()
        .filter(|n| is(n, "regions") && owner(*n) == Some(node))
        .flat_map(|n| n.children())
        .filter(|n| is(n, "region"));
    direct.chain(embedded).map(read_region).collect()
}

/// The innermost `<region>` an element belongs to.
fn owner<'a, 'i>(node: roxmltree::Node<'a, 'i>) -> Option<roxmltree::Node<'a, 'i>> {
    node.ancestors().skip(1).find(|n| is(n, "region"))
}

/// Tag comparison that ignores the XML namespace.
///
/// From 1.x every element sits in `http://smath.info/schemas/worksheet/1.0`, and
/// roxmltree's `has_tag_name` with a bare `&str` matches only elements with *no*
/// namespace — so the namespace alone makes a pre-1.x reader find zero regions in
/// every 1.x file. Comparing local names reads both eras through one path.
fn is(node: &roxmltree::Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

/// A 2D plot's stored viewport.
///
/// Only the two numbers that decide the horizontal domain are kept. What they
/// mean is not a guess: `PlotRegion.dll` initialises a 2D plot's frame to
/// `10·(width/height)/1.66` pixels per unit, and `Renderer::Scale` multiplies
/// that frame and the saved `scale_*` by the same factor — so a reloaded
/// worksheet's frame is `10·(w/h)/1.66·scale_y`, and the visible x runs from
/// `(-w/2 - transpose_x)` to `(+w/2 - transpose_x)` divided by it. The field
/// names are crossed in SMath's own source: the *horizontal* extent divides by
/// `limits_y`, which is what `scale_y` scales.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotView {
    pub scale_y: f64,
    pub transpose_x: i64,
}

/// The viewport of a 2D `<plot>`, if this region is one.
fn read_plot_view(node: roxmltree::Node) -> Option<PlotView> {
    if node.tag_name().name() != "plot" || node.attribute("type") != Some("2d") {
        return None;
    }
    Some(PlotView {
        scale_y: node.attribute("scale_y")?.parse().ok()?,
        transpose_x: node.attribute("transpose_x")?.parse().ok()?,
    })
}

fn read_payload(node: roxmltree::Node) -> Payload {
    match node.tag_name().name() {
        "math" => Payload::Math(Box::new(read_math(node))),
        "text" => {
            // The payload picker handed us the first `<text>`; its siblings are
            // the same prose in the other languages.
            let variants = node
                .parent()
                .into_iter()
                .flat_map(|p| p.children())
                .filter(|n| is(n, "text"))
                .map(|n| (n.attribute("lang").map(str::to_string), read_text(n)))
                .collect();
            Payload::Text { variants }
        }
        tag @ ("plot" | "xyplot") => Payload::Plot {
            expr: expr::reduce(&tokens_in(
                node.children().find(|n| is(n, "input")).unwrap_or(node),
            )),
            tag: tag.to_string(),
            view: read_plot_view(node),
        },
        // A Mathcad-toolbox block is a math region with a layout hint
        // (`seqop="sys"`), not a construct of its own.
        "mathcadblock" => Payload::Math(Box::new(read_math(node))),
        // `<picture>` carries base64 in `<raw format=…>`; 1.x's `<image>` wraps
        // the same idea in `<imagefile>`. Neither's pixels are of any use to a
        // coverage report, so only the encoded length is kept.
        "picture" | "image" => {
            let raw = node
                .descendants()
                .find(|n| is(n, "raw") || is(n, "imagefile"));
            Payload::Picture {
                size: picture_size(node, raw),
                format: raw
                    .and_then(|n| n.attribute("format"))
                    .or_else(|| raw.and_then(|n| n.attribute("type")))
                    .unwrap_or("?")
                    .to_string(),
                // XML is free to wrap the text of an element, and no corpus
                // region does; stripping is what makes that a fact about this
                // corpus rather than an assumption the length arithmetic in
                // `decoded_len` depends on.
                data: raw
                    .and_then(|n| n.text())
                    .map(|t| t.split_whitespace().collect())
                    .unwrap_or_default(),
            }
        }
        "area" => Payload::Area {
            title: node
                .children()
                .find(|n| is(n, "title"))
                .map(read_text)
                .unwrap_or_default(),
        },
        other => Payload::Unsupported {
            tag: other.to_string(),
        },
    }
}

/// The size SMath drew a picture at, in the page's pixels.
///
/// Two sources, and the more specific wins. 1.x's `<imagefile>` states the
/// picture's own box; `<picture>` states nothing, and the enclosing `<region>`
/// is then the only thing that says how large the figure stood. The two are not
/// quite the same measurement — the one `<imagefile>` in the mechanics corpus
/// is 117x100 inside a 127x108 region, so a region carries a few pixels of frame
/// around its content — and taking the region's box for a `<picture>` therefore
/// includes that frame. Subtracting a constant to correct for it would be
/// inventing a number no file states, for a difference of five pixels a side, so
/// the box is reported as the file gives it.
fn picture_size(node: roxmltree::Node, raw: Option<roxmltree::Node>) -> Option<(u32, u32)> {
    raw.and_then(box_of)
        .or_else(|| node.parent().and_then(box_of))
}

/// `width` and `height` off an element, when it declares both as real pixels.
///
/// A zero or a missing attribute is no answer rather than a zero-sized figure:
/// the oldest era's regions carry no box at all, and a figure drawn at nothing
/// is invisible where the worksheet meant to show one.
fn box_of(node: roxmltree::Node) -> Option<(u32, u32)> {
    let width: u32 = node.attribute("width")?.parse().ok()?;
    let height: u32 = node.attribute("height")?.parse().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

/// How many bytes `base64` decodes to, without decoding it.
///
/// Reported rather than the encoded length because the encoded length is an
/// artefact of the transport: a reader comparing two worksheets' figures, or a
/// person reading a coverage report, means the size of the image. Four
/// characters carry three bytes and the padding says how many of the last three
/// are real. Input that is not a whole number of quartets is not valid base64,
/// and the size is a report rather than a decoder, so it rounds down instead of
/// failing.
pub fn decoded_len(base64: &str) -> usize {
    let padding = base64.bytes().rev().take_while(|&b| b == b'=').count();
    (base64.len() / 4 * 3).saturating_sub(padding)
}

fn read_math(node: roxmltree::Node) -> Math {
    // In the newer era the expression is under `<input>`; in the older one it is
    // directly under `<math>`. Asking for `<input>` first and falling back to the
    // element itself covers both without branching on a version number.
    let input = node.children().find(|n| is(n, "input")).unwrap_or(node);

    let statement = expr::classify(expr::reduce(&direct_tokens(input)));

    let result = node
        .children()
        .find(|n| is(n, "result"))
        .map(|n| expr::reduce(&direct_tokens(n)));

    let contract = node
        .children()
        .find(|n| is(n, "contract"))
        .map(|n| expr::reduce(&direct_tokens(n)));

    let result_node = node.children().find(|n| is(n, "result"));

    let description = node
        .children()
        .filter(|n| is(n, "description"))
        .map(|n| (n.attribute("lang").map(str::to_string), read_text(n)))
        .collect();

    Math {
        statement,
        result,
        contract,
        description,
        decimal_places: node.attribute("decimalPlaces").and_then(|v| v.parse().ok()),
        significant_digits: node.attribute("significantDigitsMode").map(|v| v == "true"),
        trailing_zeros: node.attribute("trailingZeros").map(|v| v == "true"),
        ignore_units: node.attribute("ignoreUnits") == Some("true"),
        error: node.attribute("error").map(str::to_string),
        optimize: node.attribute("optimize").and_then(|v| v.parse().ok()),
        result_kind: result_node.map(|n| match n.attribute("action") {
            Some("numeric") => ResultKind::Numeric,
            Some("symbolic") => ResultKind::Symbolic,
            _ => ResultKind::Other,
        }),
        evaluate: node.attribute("evaluate") != Some("false"),
    }
}

fn read_text(node: roxmltree::Node) -> String {
    // Line structure inside a `<p>` is already lost in the source — the corpus
    // has words running together across what were plainly separate lines — so
    // there is no fidelity here to chase, only paragraphs to keep apart.
    let owner_region = owner(node);
    let mut out = String::new();
    for p in node
        .descendants()
        .filter(|n| is(n, "p") && owner(*n) == owner_region)
    {
        // Text nodes only. `Node::text()` answers for an element *and* for the
        // text node beneath it, so collecting over all descendants returns every
        // paragraph twice.
        let text: String = p
            .descendants()
            .filter(|n| n.is_text())
            .filter_map(|n| n.text())
            .collect();
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(text.trim());
    }
    out
}

/// `<e>` children of this element only.
///
/// Not descendants: a `<math>` in the newer era contains `<input>`, `<contract>`
/// and `<result>`, each with its own token stream, and folding them together
/// would splice an answer into the expression that produced it.
fn direct_tokens(node: roxmltree::Node) -> Vec<Token> {
    node.children().filter(|n| is(n, "e")).map(token).collect()
}

/// Every `<e>` beneath this element, for payloads that have no inner structure.
fn tokens_in(node: roxmltree::Node) -> Vec<Token> {
    node.descendants()
        .filter(|n| is(n, "e"))
        .map(token)
        .collect()
}

/// Undo SMath's `\XXXX\` character escapes in an operand or function name.
///
/// Names may carry them: a plot property path arrives as
/// `\007B\labels\007D\\0027\XLabel`, which is `{labels}'XLabel` — braces
/// and an apostrophe escaped by their code point. Three escapes appear across
/// both corpora (`{`, `}`, `'`, 392 times), and a name left escaped is a
/// different name, so this happens once here rather than at each place a name is
/// compared.
fn unescape(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_string();
    }
    let bytes: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        // `\` + four hex digits + `\`, and nothing else is an escape.
        let escape = bytes[i] == '\\'
            && i + 5 < bytes.len()
            && bytes[i + 5] == '\\'
            && bytes[i + 1..i + 5].iter().all(|c| c.is_ascii_hexdigit());
        if escape {
            let hex: String = bytes[i + 1..i + 5].iter().collect();
            if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                out.push(c);
                i += 6;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn token(node: roxmltree::Node) -> Token {
    Token {
        kind: match node.attribute("type") {
            Some("operator") => TokenKind::Operator,
            Some("function") => TokenKind::Function,
            Some("bracket") => TokenKind::Bracket,
            _ => TokenKind::Operand,
        },
        text: unescape(node.text().unwrap_or("")),
        args: node.attribute("args").and_then(|v| v.parse().ok()),
        style: match node.attribute("style") {
            Some("unit") => Some(Style::Unit),
            Some("string") => Some(Style::Str),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Assign;

    const LEGACY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<?application progid="SMath Studio" version="0.85"?>
<regions>
  <settings><calculation><precision>2</precision><angle>radians</angle></calculation></settings>
  <region id="0" left="72" top="0"><text lang="eng"><p>Hello</p></text></region>
  <region id="1" left="72" top="90"><math fraction-type="none" evaluate="true">
    <e type="operand">Ling</e><e type="operand">345.78</e><e type="operator" args="2">=</e>
  </math></region>
</regions>"#;

    /// 1.x: a `<worksheet>` root, everything in a namespace, `<area>` back to a
    /// bare marker, and inline math nested inside a `<text>`.
    const MODERN_1X: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<?application progid="SMath Solver" version="1.4.0.9654"?>
<worksheet xmlns="http://smath.info/schemas/worksheet/1.0">
  <settings ppi="144">
    <calculation><precision>3</precision><significantDigitsMode>true</significantDigitsMode></calculation>
  </settings>
  <regions type="content">
    <region left="0" top="0"><text lang="ger"><content><p>Balken</p></content>
      <regions type="content">
        <region left="2" top="0"><math significantDigitsMode="false" decimalPlaces="4">
          <input><e type="operand">q</e><e type="operand">3</e>
            <e type="operand" style="unit">kN</e><e type="operator" args="2">*</e>
            <e type="operator" args="2">:</e></input>
        </math></region>
      </regions>
    </text></region>
    <region left="0" top="36"><area single="true" collapsed="true" /></region>
    <region left="9" top="72"><math ignoreUnits="true" error="16">
      <input><e type="operand">q</e></input>
      <contract><e type="operand" style="unit">kN</e></contract>
      <result action="numeric"><e type="operand">3</e></result>
    </math></region>
    <region left="9" top="108"><math>
      <input><e type="operand">S</e></input>
      <result action="symbolic"><e type="operand">a</e><e type="operand">b</e>
        <e type="operator" args="2">+</e></result>
    </math></region>
    <region left="9" top="144"><xyplot width="300" name="XYPlot">
      <traces><trace linecolor="Blue" /></traces>
      <input><e type="operand">t</e><e type="function" args="1">x</e></input>
    </xyplot></region>
  </regions>
</worksheet>"##;

    const MODERN: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<?application progid="SMath Studio" version="0.98.6179.21440"?>
<regions>
  <settings><calculation><precision>4</precision></calculation></settings>
  <region id="17" left="216" top="1107"><math optimize="1" decimalPlaces="2">
    <input><e type="operand">X.max</e></input>
    <contract><e type="operand" style="unit">in</e></contract>
    <result action="numeric"><e type="operand">96</e></result>
  </math></region>
</regions>"#;

    #[test]
    fn a_byte_order_mark_is_not_a_parse_error() {
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend_from_slice(LEGACY.as_bytes());
        assert!(worksheet(&bytes).is_ok());
    }

    #[test]
    fn the_writing_version_comes_from_the_processing_instruction() {
        let w = worksheet(MODERN.as_bytes()).unwrap();
        assert_eq!(w.version, "0.98.6179.21440");
    }

    #[test]
    fn the_era_is_detected_from_structure_not_from_the_version() {
        assert_eq!(worksheet(LEGACY.as_bytes()).unwrap().era, Era::Legacy);
        assert_eq!(worksheet(MODERN.as_bytes()).unwrap().era, Era::Modern);
    }

    #[test]
    fn the_newer_era_keeps_its_answer_and_the_unit_it_is_shown_in() {
        // The whole point of reading `<contract>`: 96 is inches, and comparing it
        // against a computed 2.4384 m would look like an engine bug.
        let w = worksheet(MODERN.as_bytes()).unwrap();
        let m = w.math().next().unwrap();
        assert_eq!(m.result, Some(Expr::Number("96".into())));
        assert_eq!(m.contract, Some(Expr::Unit("in".into())));
        assert_eq!(m.decimal_places, Some(2));
    }

    #[test]
    fn a_result_is_never_spliced_into_the_expression_that_produced_it() {
        let w = worksheet(MODERN.as_bytes()).unwrap();
        let m = w.math().next().unwrap();
        assert_eq!(m.statement, Statement::Bare(Expr::Name("X.max".into())));
    }

    #[test]
    fn the_older_eras_equals_is_read_as_a_display_with_an_answer() {
        let w = worksheet(LEGACY.as_bytes()).unwrap();
        let m = w.math().next().unwrap();
        assert!(matches!(
            m.statement,
            Statement::Show {
                stored: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn settings_that_change_results_are_kept() {
        let w = worksheet(LEGACY.as_bytes()).unwrap();
        assert_eq!(w.settings.precision, Some(2));
        assert_eq!(w.settings.angle.as_deref(), Some("radians"));
    }

    #[test]
    fn regions_keep_file_order_and_their_geometry() {
        let w = worksheet(LEGACY.as_bytes()).unwrap();
        assert_eq!(w.regions.len(), 2);
        assert!(matches!(w.regions[0].payload, Payload::Text { .. }));
        assert_eq!(w.regions[1].top, 90);
        assert!(w.order_anomalies().is_empty());
    }

    #[test]
    fn a_region_that_goes_backwards_is_reported() {
        let backwards = LEGACY.replace(
            r#"id="1" left="72" top="90""#,
            r#"id="1" left="72" top="0""#,
        );
        let w = worksheet(backwards.as_bytes()).unwrap();
        // Same top, same left as region 0 is not backwards; make it clearly so.
        assert!(w.order_anomalies().is_empty());

        let backwards = LEGACY.replace(
            r#"id="0" left="72" top="0""#,
            r#"id="0" left="72" top="500""#,
        );
        let w = worksheet(backwards.as_bytes()).unwrap();
        assert_eq!(w.order_anomalies(), vec![1]);
    }

    #[test]
    fn a_global_definition_survives_the_read() {
        let src = LEGACY.replace(
            r#"<e type="operator" args="2">=</e>"#,
            r#"<e type="operator" args="2">≡</e>"#,
        );
        let w = worksheet(src.as_bytes()).unwrap();
        assert!(matches!(
            w.math().next().unwrap().statement,
            Statement::Define {
                kind: Assign::Global,
                ..
            }
        ));
    }

    const NESTED: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<?application progid="SMath Studio" version="0.98"?>
<regions>
  <region id="7" left="0" top="279"><area collapsed="true"><title lang="eng"><p>Licence</p></title></area>
    <region id="8" left="18" top="306"><text lang="ita"><p>ciao</p></text><text lang="eng"><p>hello</p></text></region>
    <region id="9" left="18" top="480"><math><input><e type="operand">a</e><e type="operand">1</e><e type="operator" args="2">:</e></input></math></region>
  </region>
</regions>"#;

    #[test]
    fn a_collapsed_section_does_not_hide_its_contents() {
        // 442 of the corpus's 3878 regions are nested one or two levels inside an
        // `<area>`. Reading only the top level loses them without a word.
        let w = worksheet(NESTED.as_bytes()).unwrap();
        assert_eq!(w.regions.len(), 1);
        assert_eq!(w.flat().len(), 3);
        assert_eq!(w.math().count(), 1);
    }

    #[test]
    fn a_collapsed_section_reads_before_what_it_contains() {
        let w = worksheet(NESTED.as_bytes()).unwrap();
        let flat = w.flat();
        assert!(matches!(flat[0].payload, Payload::Area { .. }));
        assert_eq!(flat[1].id, Some(8));
        assert_eq!(flat[2].id, Some(9));
        assert!(w.order_anomalies().is_empty());
    }

    #[test]
    fn both_languages_of_a_text_region_are_kept() {
        // Prose *and* language, for every variant: which one an import keeps is
        // a policy decision, and the reader must not pre-empt it by dropping
        // the text of all but the first.
        let w = worksheet(NESTED.as_bytes()).unwrap();
        match &w.flat()[1].payload {
            Payload::Text { variants } => {
                assert_eq!(
                    variants,
                    &[
                        (Some("ita".to_string()), "ciao".to_string()),
                        (Some("eng".to_string()), "hello".to_string()),
                    ]
                );
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn an_area_keeps_its_title() {
        let w = worksheet(NESTED.as_bytes()).unwrap();
        assert!(matches!(&w.regions[0].payload, Payload::Area { title } if title == "Licence"));
    }

    const WITH_HEADER: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<?application progid="SMath Solver" version="1.4.0.9654"?>
<worksheet xmlns="http://smath.info/schemas/worksheet/1.0">
  <regions type="content">
    <region left="0" top="0"><picture><raw format="png" encoding="base64">SGVsbG8h</raw></picture></region>
  </regions>
  <regions type="header">
    <region left="0" top="0"><text lang="spa"><content><p>Marc</p></content></text></region>
    <region left="0" top="9"><picture><raw format="png" encoding="base64">aGk=</raw></picture></region>
  </regions>
</worksheet>"##;

    #[test]
    fn a_picture_keeps_its_data() {
        // The reader used to keep the encoded length alone. An import that
        // reaches the emitter with a length cannot write the figure back.
        let w = worksheet(WITH_HEADER.as_bytes()).unwrap();
        match &w.flat()[0].payload {
            Payload::Picture { format, data, .. } => {
                assert_eq!(format, "png");
                assert_eq!(data, "SGVsbG8h");
            }
            other => panic!("expected a picture, got {other:?}"),
        }
    }

    #[test]
    fn a_second_regions_block_is_not_dropped() {
        // One worksheet in 118 carried a `type="header"` block after the
        // content, and taking the first `<regions>` lost its three regions —
        // one of them a picture — with no marker and no count.
        let w = worksheet(WITH_HEADER.as_bytes()).unwrap();
        assert_eq!(w.flat().len(), 1);
        assert_eq!(w.flat_furniture().len(), 2);
        assert!(matches!(
            w.flat_furniture()[1].payload,
            Payload::Picture { .. }
        ));
    }

    #[test]
    fn the_content_block_is_the_document_whichever_order_it_comes_in() {
        // Position is not the rule; `type="content"` is. A file that wrote its
        // header first would otherwise import the header as the worksheet.
        let swapped = WITH_HEADER
            .replace(r#"<regions type="content">"#, "<regions type=\"TMP\">")
            .replace(r#"<regions type="header">"#, r#"<regions type="content">"#)
            .replace("<regions type=\"TMP\">", r#"<regions type="header">"#);
        let w = worksheet(swapped.as_bytes()).unwrap();
        assert_eq!(w.flat().len(), 2);
        assert_eq!(w.flat_furniture().len(), 1);
    }

    #[test]
    fn a_pre_1x_worksheet_still_has_no_furniture() {
        // The older era's root *is* `<regions>`, and it carries no type. The
        // fallback to the first block is what keeps reading it.
        let w = worksheet(NESTED.as_bytes()).unwrap();
        assert_eq!(w.flat().len(), 3);
        assert!(w.furniture.is_empty());
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
    fn something_that_is_not_a_worksheet_is_refused() {
        assert!(matches!(
            worksheet(b"<html><body>no</body></html>"),
            Err(ReadError::NotAWorksheet)
        ));
    }
    #[test]
    fn reads_the_1x_container() {
        let w = worksheet(MODERN_1X.as_bytes()).unwrap();
        assert_eq!(w.version, "1.4.0.9654");
        // The math shape is still the 0.88 one; only the container changed.
        assert_eq!(w.era, Era::Modern);
        assert_eq!(w.settings.precision, Some(3));
        assert_eq!(w.settings.significant_digits, Some(true));
        // 1.x has no <angle>, and its absence is not a malformed file.
        assert_eq!(w.settings.angle, None);
        assert_eq!(w.regions.len(), 5);
    }

    #[test]
    fn finds_math_embedded_in_a_text_region() {
        let w = worksheet(MODERN_1X.as_bytes()).unwrap();
        // The formula lives in <text><regions>, which is where 1.x puts inline
        // math; a reader that looks only under <area> silently loses it.
        let inline = &w.regions[0].children;
        assert_eq!(inline.len(), 1);
        assert!(matches!(inline[0].payload, Payload::Math(_)));
        // ...and the text of the region it sits in is not swallowed by it.
        match &w.regions[0].payload {
            Payload::Text { variants } => assert_eq!(variants[0].1, "Balken"),
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn per_region_flags_that_decide_a_comparison() {
        let w = worksheet(MODERN_1X.as_bytes()).unwrap();
        let m = w.math().collect::<Vec<_>>();
        assert_eq!(m[0].significant_digits, Some(false));
        assert_eq!(m[0].decimal_places, Some(4));
        // A region SMath itself failed on, and one that opts out of units.
        assert!(m[1].ignore_units);
        assert_eq!(m[1].error.as_deref(), Some("16"));
        assert_eq!(m[1].result_kind, Some(ResultKind::Numeric));
        assert!(m[1].contract.is_some());
        // Symbolic results are read, but their kind keeps them out of the oracle.
        assert_eq!(m[2].result_kind, Some(ResultKind::Symbolic));
    }

    #[test]
    fn xyplot_is_a_plot_and_area_is_a_marker() {
        let w = worksheet(MODERN_1X.as_bytes()).unwrap();
        match &w.regions[4].payload {
            Payload::Plot { tag, .. } => assert_eq!(tag, "xyplot"),
            other => panic!("expected a plot, got {other:?}"),
        }
        // In 1.x an <area> holds nothing: the regions it collapses are siblings.
        assert!(matches!(w.regions[1].payload, Payload::Area { .. }));
        assert!(w.regions[1].children.is_empty());
    }

    #[test]
    fn page_order_holds_and_inline_order_is_not_checked() {
        let w = worksheet(MODERN_1X.as_bytes()).unwrap();
        assert!(w.order_anomalies().is_empty());
    }

    #[test]
    fn a_region_note_and_its_escaped_names_survive() {
        // `<description>` is not decoration: `description(x)` reads this text,
        // so an axis label lives here and nowhere else.
        let xml = r##"<?xml version="1.0" encoding="utf-8"?>
<?application progid="SMath Solver" version="1.4.0.9654"?>
<worksheet xmlns="http://smath.info/schemas/worksheet/1.0">
  <regions>
    <region left="0" top="0" width="90" height="25"><math>
      <description active="true" lang="eng"><content><p>Time in s</p></content></description>
      <description active="true" lang="ger"><content><p>Zeit in s</p></content></description>
      <input><e type="operand">\007B\labels\007D\\0027\XLabel</e>
        <e type="operand">1</e><e type="operator" args="2">:</e></input>
    </math></region>
  </regions>
</worksheet>"##;
        let w = worksheet(xml.as_bytes()).unwrap();
        let m: Vec<_> = w.math().collect();
        assert_eq!(
            m[0].description,
            vec![
                (Some("eng".into()), "Time in s".to_string()),
                (Some("ger".into()), "Zeit in s".to_string()),
            ]
        );
        // `\XXXX\` escapes are undone once, here: a name left escaped is a
        // different name from the one the worksheet meant.
        let Statement::Define { target, .. } = &m[0].statement else {
            panic!("expected a binding")
        };
        assert_eq!(*target, Expr::Name("{labels}'XLabel".into()));
    }

    #[test]
    fn prose_beside_a_value_is_counted_not_reordered() {
        // A label at left=0 and its value at left=135 on the same page row.
        let xml = r##"<?xml version="1.0" encoding="utf-8"?>
<?application progid="SMath Solver" version="1.4.0.9654"?>
<worksheet xmlns="http://smath.info/schemas/worksheet/1.0">
  <regions>
    <region left="135" top="171" width="90" height="25"><math><input>
      <e type="operand">c</e><e type="operand">1</e><e type="operator" args="2">:</e>
    </input></math></region>
    <region left="0" top="180" width="120" height="18"><text lang="ger">
      <content><p>Federsteifigkeit</p></content></text></region>
  </regions>
</worksheet>"##;
        let w = worksheet(xml.as_bytes()).unwrap();
        assert_eq!(w.side_by_side_rows(), 1);
    }
}
