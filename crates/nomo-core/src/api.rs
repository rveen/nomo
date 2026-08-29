//! What a front end needs from the engine, in one call.
//!
//! The browser editor needs three things on every keystroke: syntax highlighting,
//! diagnostics with positions, and the rendered worksheet. All three come from
//! here, and all three are derived from the same parse and the same evaluation
//! the CLI uses.
//!
//! # Why this is not in TypeScript
//!
//! Invariant 1: one grammar, one syntax tree. It would be easy to write a
//! CodeMirror language mode that highlights `sin` as a function and `kg` as a
//! unit, and it would be wrong within a week — the moment the engine learns a
//! unit the highlighter does not, the editor and the results disagree about what
//! the worksheet says. CalcpadCE has exactly this split between `Calcpad.Core`
//! and `Calcpad.Highlighter`, and the design note calls it a permanent liability
//! (§10).
//!
//! So the front end gets *classified* tokens rather than a grammar. It cannot
//! disagree with the engine, because it is not deciding anything.
//!
//! # The classification is more than a lexer can do
//!
//! `m` is `Ident` to the lexer and could be metres or a variable; only evaluation
//! knows. [`classify`] resolves each identifier the way the evaluator does —
//! variable, then constant, then unit — using the state of the sheet after it
//! ran. That is highlighting no standalone grammar could produce.

use crate::ast::{Expr, Stmt};
use crate::diag::Severity;
use crate::doc::Sheet;
use crate::lex::{self, TokenKind};
use crate::render::{html, RenderOptions};
use crate::span::Span;

/// How a token should be coloured.
///
/// Deliberately about meaning, not spelling: `unit` and `variable` are the same
/// lexical shape and differ only in what the engine resolved them to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenClass {
    Number,
    Comment,
    Keyword,
    Operator,
    Bracket,
    Separator,
    /// A name bound by the worksheet.
    Variable,
    /// A name that resolved to a unit.
    Unit,
    /// A built-in or user-defined function, in call position.
    Function,
    /// `pi`, `e`, `tau`, `inf`.
    Constant,
    /// A string literal.
    Text,
    /// A name that resolves to nothing. Worth colouring: it is usually a typo,
    /// and the editor shows it before the diagnostic arrives.
    Unresolved,
}

impl TokenClass {
    pub fn name(self) -> &'static str {
        match self {
            TokenClass::Number => "number",
            TokenClass::Comment => "comment",
            TokenClass::Keyword => "keyword",
            TokenClass::Operator => "operator",
            TokenClass::Bracket => "bracket",
            TokenClass::Separator => "separator",
            TokenClass::Variable => "variable",
            TokenClass::Unit => "unit",
            TokenClass::Function => "function",
            TokenClass::Constant => "constant",
            TokenClass::Text => "text",
            TokenClass::Unresolved => "unresolved",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedToken {
    pub span: Span,
    pub class: TokenClass,
}

/// Classify every token in the sheet's source for highlighting.
pub fn classify(sheet: &Sheet) -> Vec<ClassifiedToken> {
    let source = sheet.source();
    let mut callees = Vec::new();
    let mut bound = Vec::new();
    for stmt in &sheet.ast().stmts {
        collect_names(stmt, &mut callees, &mut bound);
    }

    let mut out = Vec::new();
    for token in lex::lex(source).tokens {
        let class = match token.kind {
            TokenKind::Number => TokenClass::Number,
            TokenKind::Text => TokenClass::Text,
            TokenKind::Comment => TokenClass::Comment,
            TokenKind::KwUnit
            | TokenKind::KwFn
            | TokenKind::KwGlobal
            | TokenKind::KwCheck
            | TokenKind::KwIf
            | TokenKind::KwThen
            | TokenKind::KwElse
            | TokenKind::KwAnd
            | TokenKind::KwOr
            | TokenKind::KwNot => TokenClass::Keyword,
            TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Caret
            | TokenKind::Eq
            | TokenKind::Lt
            | TokenKind::Gt
            | TokenKind::Le
            | TokenKind::Ge
            | TokenKind::EqEq
            | TokenKind::Ne
            | TokenKind::Arrow => TokenClass::Operator,
            TokenKind::LParen | TokenKind::RParen | TokenKind::LBracket | TokenKind::RBracket => {
                TokenClass::Bracket
            }
            TokenKind::Comma => TokenClass::Separator,
            TokenKind::Newline | TokenKind::Eof => continue,
            TokenKind::Ident => classify_ident(token.span, source, sheet, &callees, &bound),
        };
        out.push(ClassifiedToken {
            span: token.span,
            class,
        });
    }
    out
}

/// Resolve one identifier the way the evaluator does.
///
/// The order matters and mirrors `Env::eval_ident`: a binding shadows a unit, so
/// a worksheet that writes `m = 4` gets `m` colored as a variable from then on,
/// which is exactly what it means.
fn classify_ident(
    span: Span,
    source: &str,
    sheet: &Sheet,
    callees: &[Span],
    bound: &[String],
) -> TokenClass {
    let text = span.text(source);

    // Call position is syntactic, so it is decided before anything else: `sin`
    // in `sin(x)` is a function even in a worksheet that also binds `sin`.
    if callees.contains(&span) {
        return TokenClass::Function;
    }
    if bound.iter().any(|n| n == text) {
        return TokenClass::Variable;
    }
    if crate::eval::is_constant(text) {
        return TokenClass::Constant;
    }
    if sheet.units().contains(text) {
        return TokenClass::Unit;
    }
    if crate::eval::BUILTINS.contains(&text) {
        return TokenClass::Function;
    }
    TokenClass::Unresolved
}

/// Gather call-site spans and every name the worksheet binds.
fn collect_names(stmt: &Stmt, callees: &mut Vec<Span>, bound: &mut Vec<String>) {
    match stmt {
        Stmt::Assign { name, value, .. } | Stmt::GlobalDef { name, value, .. } => {
            bound.push(name.text.clone());
            walk(value, callees);
        }
        Stmt::UnitDecl { name, value, .. } => {
            bound.push(name.text.clone());
            walk(value, callees);
        }
        Stmt::FnDef {
            name, params, body, ..
        } => {
            bound.push(name.text.clone());
            for p in params {
                bound.push(p.text.clone());
            }
            walk(body, callees);
        }
        Stmt::Query { expr, .. } | Stmt::Check { expr, .. } => walk(expr, callees),
        Stmt::Comment { .. } | Stmt::Error { .. } => {}
    }
}

fn walk(expr: &Expr, callees: &mut Vec<Span>) {
    match expr {
        Expr::Call { callee, args, .. } => {
            callees.push(callee.span);
            for a in args {
                walk(a, callees);
            }
        }
        Expr::Unary { operand, .. } => walk(operand, callees),
        Expr::Binary { lhs, rhs, .. } => {
            walk(lhs, callees);
            walk(rhs, callees);
        }
        Expr::Paren { inner, .. } => walk(inner, callees),
        Expr::If {
            cond,
            then,
            otherwise,
            ..
        } => {
            walk(cond, callees);
            walk(then, callees);
            walk(otherwise, callees);
        }
        Expr::Index { base, indices, .. } => {
            walk(base, callees);
            for i in indices {
                walk(i, callees);
            }
        }
        Expr::Vector { elements, .. } => {
            for e in elements {
                walk(e, callees);
            }
        }
        Expr::Matrix { rows, .. } => {
            for row in rows {
                for e in row {
                    walk(e, callees);
                }
            }
        }
        Expr::Convert { value, .. } => walk(value, callees),
        Expr::Number { .. } | Expr::Ident(_) | Expr::Text { .. } | Expr::Error { .. } => {}
    }
}

/// Byte offsets translated to UTF-16 code units, for a JavaScript host.
///
/// This exists because of a bug that only appears once a worksheet contains a
/// character outside the Basic Multilingual Plane's one-byte range — which for
/// this language is immediately, since `π`, `°`, `·` and `µ` are all ordinary
/// content. Rust indexes strings by UTF-8 byte and every [`Span`] is a byte
/// range. JavaScript indexes by UTF-16 code unit, and so does CodeMirror. An
/// em dash is three bytes and one code unit, so every highlight after the first
/// non-ASCII character lands two columns to the right and the editor colours the
/// wrong text.
///
/// The map is dense: one entry per byte of source, so a lookup is an index and
/// no assumption is made about diagnostics arriving in source order. A worksheet
/// is a few kilobytes and this is rebuilt per analysis, which is far cheaper
/// than the evaluation it accompanies.
struct Utf16Offsets(Vec<u32>);

impl Utf16Offsets {
    fn build(source: &str) -> Utf16Offsets {
        let mut map = vec![0u32; source.len() + 1];
        let mut utf16 = 0u32;
        for (byte, c) in source.char_indices() {
            map[byte] = utf16;
            utf16 += c.len_utf16() as u32;
            // Interior bytes of a multi-byte character share its start, so a
            // span that somehow points inside one degrades to its boundary
            // rather than to nonsense.
            for b in byte + 1..byte + c.len_utf8() {
                map[b] = map[byte];
            }
        }
        map[source.len()] = utf16;
        Utf16Offsets(map)
    }

    fn at(&self, byte: u32) -> u32 {
        let byte = byte as usize;
        self.0.get(byte).copied().unwrap_or_else(|| {
            // Out of range can only mean a span built against different text.
            self.0.last().copied().unwrap_or(0)
        })
    }
}

/// Everything the editor needs after an edit, as JSON.
///
/// JSON rather than a typed binding because the module must keep importing
/// nothing: `wasm-bindgen` would put generated glue between the engine and the
/// guarantee. Written by hand for the same reason `libm` is vendored — a
/// serialisation dependency in the engine's build would be one more thing
/// between the source and the artifact. It is forty lines and it is tested.
///
/// **All offsets in the payload are UTF-16 code units**, not bytes, because the
/// only consumer counts in UTF-16. See [`Utf16Offsets`].
pub fn analysis_json(sheet: &Sheet) -> String {
    let source = sheet.source();
    let offsets = Utf16Offsets::build(source);
    let mut out = String::from("{\"format\":1");

    out.push_str(",\"html\":");
    push_string(&mut out, &html::body(sheet, &RenderOptions::default()));

    out.push_str(",\"tokens\":[");
    for (i, token) in classify(sheet).iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"from\":{},\"to\":{},\"class\":\"{}\"}}",
            offsets.at(token.span.start),
            offsets.at(token.span.end),
            token.class.name()
        ));
    }

    out.push_str("],\"diagnostics\":[");
    for (i, d) in sheet.diagnostics().iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let (line, col) = d.span.line_col(source);
        let severity = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        // `to` is nudged past `from` for a zero-width span: an empty range at the
        // end of input is invisible in the editor's gutter, and a diagnostic
        // nobody can see is a diagnostic that does not exist. Widened after the
        // offset conversion so the extra unit is one the host can count.
        let from = offsets.at(d.span.start);
        let to = offsets.at(d.span.end).max(from + 1);
        out.push_str(&format!(
            "{{\"severity\":\"{severity}\",\"code\":\"{}\",\"from\":{from},\"to\":{to},\"line\":{line},\"col\":{col},\"message\":",
            d.code
        ));
        push_string(&mut out, &d.message);
        out.push('}');
    }

    out.push_str("],\"hasErrors\":");
    out.push_str(if sheet.has_errors() { "true" } else { "false" });

    // The verdicts, so the editor's status line can say them without reading
    // the rendered HTML back. A failed check is not an error and must not be
    // counted as one — the worksheet is right and the design is not — so it
    // travels in its own field.
    let checks = sheet.checks();
    out.push_str(&format!(
        ",\"checks\":{{\"total\":{},\"passed\":{},\"failed\":{},\"undecided\":{}}}",
        checks.total, checks.passed, checks.failed, checks.undecided
    ));
    out.push('}');
    out
}

/// Append a JSON string literal, escaped per RFC 8259.
fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Everything below 0x20 must be escaped; the rest may go through as
            // UTF-8, which keeps `π` and `·` readable in the payload.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(source: &str) -> Vec<(String, &'static str)> {
        let sheet = Sheet::new(source);
        classify(&sheet)
            .into_iter()
            .map(|t| (t.span.text(source).to_string(), t.class.name()))
            .collect()
    }

    fn class_of<'a>(pairs: &'a [(String, &'static str)], text: &str) -> Vec<&'a str> {
        pairs
            .iter()
            .filter(|(t, _)| t == text)
            .map(|(_, c)| *c)
            .collect()
    }

    #[test]
    fn a_unit_and_a_variable_are_told_apart() {
        // Both are `Ident` to the lexer. Only the engine knows which is which,
        // which is the entire argument for classifying here.
        let pairs = classes("r = 5 cm\n");
        assert_eq!(class_of(&pairs, "r"), ["variable"]);
        assert_eq!(class_of(&pairs, "cm"), ["unit"]);
    }

    #[test]
    fn a_binding_shadows_a_unit_in_the_highlighting_too() {
        // `m` is metres until the worksheet binds it. The editor has to agree
        // with the evaluator about that or it is lying about the document.
        let pairs = classes("m = 4\nx = m*2\n");
        assert_eq!(class_of(&pairs, "m"), ["variable", "variable"]);
    }

    #[test]
    fn constants_functions_and_keywords_are_distinct() {
        let pairs = classes("unit kip = 1000 lbf\nx = sin(pi)\n");
        assert_eq!(class_of(&pairs, "unit"), ["keyword"]);
        assert_eq!(class_of(&pairs, "sin"), ["function"]);
        assert_eq!(class_of(&pairs, "pi"), ["constant"]);
        assert_eq!(class_of(&pairs, "lbf"), ["unit"]);
        // The declared unit is a binding at its declaration and a unit after it.
        assert_eq!(class_of(&pairs, "kip"), ["variable"]);
    }

    #[test]
    fn an_unknown_name_is_marked_rather_than_guessed() {
        let pairs = classes("x = frobnicate + 1\n");
        assert_eq!(class_of(&pairs, "frobnicate"), ["unresolved"]);
    }

    #[test]
    fn call_position_wins_over_a_binding_of_the_same_name() {
        let pairs = classes("fn f(x) = x*2\ny = f(3)\n");
        // The definition's name is a binding; the call site is a function.
        assert_eq!(class_of(&pairs, "f"), ["variable", "function"]);
    }

    #[test]
    fn comments_and_numbers_survive_classification() {
        let pairs = classes("' prose\nx = 4\n");
        assert_eq!(class_of(&pairs, "' prose"), ["comment"]);
        assert_eq!(class_of(&pairs, "4"), ["number"]);
    }

    #[test]
    fn newlines_are_not_emitted() {
        // They carry no colour and would triple the payload.
        let sheet = Sheet::new("a = 1\nb = 2\nc = 3\n");
        assert!(classify(&sheet)
            .iter()
            .all(|t| !t.span.text(sheet.source()).contains('\n')));
    }

    #[test]
    fn tokens_are_in_source_order_and_do_not_overlap() {
        // CodeMirror requires decorations sorted by position; unsorted ranges
        // throw rather than degrade.
        let sheet = Sheet::new("unit kip = 1000 lbf\nM = 2 kip*3\nM -> kip\n");
        let tokens = classify(&sheet);
        for pair in tokens.windows(2) {
            assert!(
                pair[0].span.end <= pair[1].span.start,
                "{:?} then {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn offsets_are_utf16_not_bytes() {
        // The bug this guards against shifted every highlight in the editor two
        // columns right of the text it described, and it appeared the moment a
        // worksheet contained an em dash. Nothing in Rust notices: the byte
        // offsets are all perfectly correct, for a host that counts bytes.
        let source = "' — dash\nr = 5\n";
        let sheet = Sheet::new(source);
        let json = analysis_json(&sheet);

        // `r` is at byte 11 (the em dash is three bytes) but UTF-16 unit 9.
        assert_eq!(source.as_bytes()[11], b'r');
        assert!(
            json.contains("{\"from\":9,\"to\":10,\"class\":\"variable\"}"),
            "expected `r` at UTF-16 offset 9:\n{json}"
        );
    }

    #[test]
    fn a_worksheet_of_ascii_is_unaffected_by_the_conversion() {
        // The mapping must be the identity when there is nothing to convert,
        // or it would be trading one off-by-N for another.
        let source = "r = 5\n";
        let map = Utf16Offsets::build(source);
        for byte in 0..=source.len() as u32 {
            assert_eq!(map.at(byte), byte, "at {byte}");
        }
    }

    #[test]
    fn the_offset_map_handles_every_utf8_width() {
        // One, two, three and four byte characters. The last is a surrogate pair
        // in UTF-16 and so counts as two units, not one — the case that catches
        // a conversion written as "count the characters".
        let source = "a°—𝄞b";
        assert_eq!(source.len(), 11, "1 + 2 + 3 + 4 + 1 bytes");

        let map = Utf16Offsets::build(source);
        assert_eq!(map.at(0), 0, "a");
        assert_eq!(map.at(1), 1, "° starts");
        assert_eq!(map.at(3), 2, "— starts");
        assert_eq!(map.at(6), 3, "𝄞 starts");
        assert_eq!(map.at(10), 5, "b, after the surrogate pair");
        assert_eq!(
            map.at(source.len() as u32),
            source.encode_utf16().count() as u32,
            "the end of the map is the length in code units"
        );
    }

    #[test]
    fn an_offset_inside_a_character_falls_back_to_its_start() {
        // `π` is two bytes, so byte 1 is interior and byte 2 is the position
        // after it.
        let map = Utf16Offsets::build("π");
        assert_eq!(map.at(0), 0);
        assert_eq!(map.at(1), 0, "byte 1 is inside `π`");
        assert_eq!(map.at(2), 1, "byte 2 is the end of the string");
    }

    #[test]
    fn an_offset_past_the_end_does_not_panic() {
        // Can only happen if a span was built against different text, but the
        // editor must not lose its highlighting over it.
        let map = Utf16Offsets::build("abc");
        assert_eq!(map.at(99), 3);
    }

    #[test]
    fn a_diagnostic_after_a_multibyte_character_points_at_the_right_text() {
        let source = "' π\nx = 1 m + 1 s\n";
        let sheet = Sheet::new(source);
        let json = analysis_json(&sheet);
        let capture = json.split("{\"severity\"").nth(1).expect("a diagnostic");
        let from: u32 = field(capture, "\"from\":");
        // The comment is 3 characters plus a newline: 4 UTF-16 units. So the
        // second line starts at 4, and nothing may report an offset computed
        // from the 5 bytes it occupies.
        assert!(
            from >= 4 && from < source.encode_utf16().count() as u32,
            "offset {from} is not a UTF-16 position in this document"
        );
    }

    #[test]
    fn the_payload_is_valid_json_with_the_expected_shape() {
        let sheet = Sheet::new("r = 5 cm\nV = pi*r^2\n");
        let json = analysis_json(&sheet);
        assert!(json.starts_with("{\"format\":1,"));
        assert!(json.contains("\"tokens\":["));
        assert!(json.contains("\"diagnostics\":[]"));
        assert!(json.contains("\"hasErrors\":false"));
        assert!(json.ends_with('}'));
    }

    #[test]
    fn diagnostics_carry_a_range_the_editor_can_show() {
        let sheet = Sheet::new("x = 1 m + 1 s\n");
        let json = analysis_json(&sheet);
        assert!(json.contains("\"severity\":\"error\""), "{json}");
        assert!(json.contains("\"code\":\"SH201\""), "{json}");
        assert!(json.contains("\"hasErrors\":true"), "{json}");
    }

    #[test]
    fn a_zero_width_diagnostic_still_covers_a_character() {
        // A parse error at end of input has an empty span. Left as-is the editor
        // would render nothing at all.
        let sheet = Sheet::new("x = 1 +");
        let json = analysis_json(&sheet);
        for capture in json.split("{\"severity\"").skip(1) {
            let from: u32 = field(capture, "\"from\":");
            let to: u32 = field(capture, "\"to\":");
            assert!(to > from, "empty range in {capture}");
        }
    }

    fn field(text: &str, key: &str) -> u32 {
        let rest = &text[text.find(key).expect("field present") + key.len()..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        rest[..end].parse().expect("number")
    }

    #[test]
    fn quotes_and_control_characters_are_escaped() {
        // A worksheet is user input and reaches the payload verbatim. An
        // unescaped quote would turn a comment into a parse error in the host.
        let mut out = String::new();
        push_string(&mut out, "he said \"hi\"\\ and\ttabbed\n");
        assert_eq!(out, "\"he said \\\"hi\\\"\\\\ and\\ttabbed\\n\"");
    }

    #[test]
    fn unicode_passes_through_unescaped() {
        let mut out = String::new();
        push_string(&mut out, "π·m³");
        assert_eq!(out, "\"π·m³\"");
    }

    #[test]
    fn a_comment_containing_a_quote_round_trips_through_the_payload() {
        // Two escaping layers stack here and both have to be right. The user's
        // quote is escaped by the HTML renderer, and the quotes the renderer
        // itself writes around class attributes are escaped by the JSON writer.
        let sheet = Sheet::new("' the \"design\" note\nx = 1\n");
        let json = analysis_json(&sheet);

        assert!(
            json.contains("&quot;design&quot;"),
            "the prose quote should be an HTML entity by now:\n{json}"
        );
        assert!(
            json.contains("class=\\\"prose\\\""),
            "the renderer's own quotes must be JSON-escaped:\n{json}"
        );

        // Nothing may leave an unescaped `"` inside the html string. Walk it and
        // check the value terminates exactly where it should.
        let start = json.find("\"html\":\"").expect("html field") + "\"html\":\"".len();
        let mut chars = json[start..].char_indices();
        let closed = loop {
            match chars.next() {
                Some((_, '\\')) => {
                    chars.next();
                }
                Some((i, '"')) => break start + i,
                Some(_) => {}
                None => panic!("unterminated html string in {json}"),
            }
        };
        assert_eq!(
            &json[closed..closed + 2],
            "\",",
            "the html string ended somewhere unexpected:\n{json}"
        );
    }
}
