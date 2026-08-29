//! Tokeniser.
//!
//! Newlines are significant — a worksheet is a sequence of line-oriented
//! statements — so they are emitted as tokens rather than skipped as whitespace.
//!
//! Identifiers admit the characters engineering worksheets actually use: Greek
//! letters, `°`, `%` and `_`. The SMath corpus surveyed in the design note
//! contains `π`, `φ`, `Φ`, `Ω`, `°`, `χ` and names like `Ling_rms_N`, so treating
//! identifiers as ASCII-only would reject real input on day one.

use crate::diag::{codes, Diagnostic};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// A numeric literal. The text is retained verbatim; conversion to `f64`
    /// happens in the parser so that malformed literals produce one diagnostic
    /// rather than two.
    Number,
    /// An identifier. Units and variables share this token and this namespace —
    /// `m` is lexically just a name, and only evaluation decides it means metres.
    Ident,
    /// `"a verdict in words"`. The span covers the quotes; the parser takes the
    /// text between them.
    Text,
    /// `unit`
    KwUnit,
    /// `fn`
    KwFn,
    /// `global`
    KwGlobal,
    /// `check` — a statement that states a limit and reports a verdict.
    KwCheck,
    /// `use` — bring in a pack of definitions.
    KwUse,
    /// `digits` — how many significant figures results are shown to.
    KwDigits,
    /// `axis` — how the plots below are drawn.
    KwAxis,
    /// `label` — names for the curves of the plot below.
    KwLabel,
    /// `if`, `then`, `else` — a conditional expression.
    KwIf,
    KwThen,
    KwElse,
    /// `and`, `or`, `not`. Words rather than symbols: `!` is factorial to every
    /// engineer who has met SMath, and `&`/`|` read as bitwise to everyone who
    /// has met C.
    KwAnd,
    KwOr,
    KwNot,
    /// `' prose to end of line`. Retained rather than discarded: comments are a
    /// worksheet's documentation and must reach the renderer.
    Comment,
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Eq,
    /// `->`, unit conversion.
    Arrow,
    /// Comparisons. `==` rather than `=`, which already binds a name.
    Lt,
    Gt,
    Le,
    Ge,
    EqEq,
    Ne,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    /// One or more newlines, and therefore a statement boundary.
    Newline,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn text<'s>(&self, source: &'s str) -> &'s str {
        self.span.text(source)
    }
}

/// True if `c` may appear as the first character of an identifier.
fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '°' || c == '%'
}

/// True if `c` may appear in an identifier after the first character.
fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

pub struct Lexed {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn lex(source: &str) -> Lexed {
    let mut tokens = Vec::new();
    let mut diagnostics = Vec::new();
    let bytes = source.as_bytes();
    let mut chars = source.char_indices().peekable();

    while let Some(&(start, c)) = chars.peek() {
        let start_u32 = start as u32;

        // Horizontal whitespace only; newlines are tokens.
        if c == ' ' || c == '\t' || c == '\r' {
            chars.next();
            continue;
        }

        // A string literal: `"` to the next `"` on the same line. No escapes,
        // deliberately — a worksheet's strings are labels and verdicts, and the
        // one thing an escape would buy is a quote inside a quote, which no
        // corpus string wants. An unterminated one ends at the newline and is
        // reported there rather than swallowing the rest of the document.
        if c == '"' {
            chars.next();
            let mut end = start + 1;
            let mut closed = false;
            while let Some(&(i, c)) = chars.peek() {
                if c == '\n' {
                    break;
                }
                chars.next();
                end = i + c.len_utf8();
                if c == '"' {
                    closed = true;
                    break;
                }
            }
            let span = Span::new(start_u32, end as u32);
            if closed {
                tokens.push(Token {
                    kind: TokenKind::Text,
                    span,
                });
            } else {
                diagnostics.push(Diagnostic::error(
                    codes::UNTERMINATED_TEXT,
                    span,
                    String::from("a string has no closing quote on this line"),
                ));
            }
            continue;
        }

        // Comment: `'` to end of line. Emitted as a token, not discarded — in a
        // worksheet these are the prose, and the renderer has to lay them out.
        // The newline itself is left to be tokenised separately.
        if c == '\'' {
            let mut end = start + 1;
            chars.next();
            while let Some(&(i, c)) = chars.peek() {
                if c == '\n' {
                    break;
                }
                end = i + c.len_utf8();
                chars.next();
            }
            tokens.push(Token {
                kind: TokenKind::Comment,
                span: Span::new(start_u32, end as u32),
            });
            continue;
        }

        if c == '\n' {
            // Collapse a run of blank lines into a single boundary.
            let mut end = start;
            while let Some(&(i, c)) = chars.peek() {
                if c == '\n' || c == ' ' || c == '\t' || c == '\r' {
                    end = i + c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(Token {
                kind: TokenKind::Newline,
                span: Span::new(start_u32, end as u32),
            });
            continue;
        }

        // Numbers, including the leading-dot form (`.019`) that appears in real
        // worksheets. A `.` not followed by a digit is not a number.
        let starts_number = c.is_ascii_digit()
            || (c == '.' && bytes.get(start + 1).is_some_and(|b| b.is_ascii_digit()));
        if starts_number {
            let mut end = start;
            let mut seen_dot = false;
            let mut seen_exp = false;
            let mut malformed = false;

            while let Some(&(i, c)) = chars.peek() {
                match c {
                    '0'..='9' => {
                        end = i + 1;
                        chars.next();
                    }
                    '.' if !seen_dot && !seen_exp => {
                        seen_dot = true;
                        end = i + 1;
                        chars.next();
                    }
                    '.' => {
                        // A second dot, e.g. `1.2.3`.
                        malformed = true;
                        end = i + 1;
                        chars.next();
                    }
                    'e' | 'E' if !seen_exp => {
                        // Only an exponent if a digit or sign follows; otherwise
                        // `2e` is the number 2 juxtaposed with the name `e`.
                        let next = bytes.get(i + 1).copied();
                        let after_sign = bytes.get(i + 2).copied();
                        let is_exp = match next {
                            Some(b'0'..=b'9') => true,
                            Some(b'+') | Some(b'-') => {
                                matches!(after_sign, Some(b'0'..=b'9'))
                            }
                            _ => false,
                        };
                        if !is_exp {
                            break;
                        }
                        seen_exp = true;
                        end = i + 1;
                        chars.next();
                        if matches!(chars.peek(), Some(&(_, '+')) | Some(&(_, '-'))) {
                            if let Some((j, _)) = chars.next() {
                                end = j + 1;
                            }
                        }
                    }
                    _ => break,
                }
            }

            let span = Span::new(start_u32, end as u32);
            if malformed {
                diagnostics.push(Diagnostic::error(
                    codes::MALFORMED_NUMBER,
                    span,
                    format!("`{}` is not a valid number", span.text(source)),
                ));
            }
            tokens.push(Token {
                kind: TokenKind::Number,
                span,
            });
            continue;
        }

        if is_ident_start(c) {
            let mut end = start + c.len_utf8();
            chars.next();
            while let Some(&(i, c)) = chars.peek() {
                if is_ident_continue(c) {
                    end = i + c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let span = Span::new(start_u32, end as u32);
            let kind = match span.text(source) {
                "unit" => TokenKind::KwUnit,
                "fn" => TokenKind::KwFn,
                "global" => TokenKind::KwGlobal,
                "check" => TokenKind::KwCheck,
                "use" => TokenKind::KwUse,
                "digits" => TokenKind::KwDigits,
                "axis" => TokenKind::KwAxis,
                "label" => TokenKind::KwLabel,
                "if" => TokenKind::KwIf,
                "then" => TokenKind::KwThen,
                "else" => TokenKind::KwElse,
                "and" => TokenKind::KwAnd,
                "or" => TokenKind::KwOr,
                "not" => TokenKind::KwNot,
                _ => TokenKind::Ident,
            };
            tokens.push(Token { kind, span });
            continue;
        }

        // Punctuation and operators.
        chars.next();
        let mut end = start + c.len_utf8();
        let kind = match c {
            '+' => TokenKind::Plus,
            '-' => {
                if matches!(chars.peek(), Some(&(_, '>'))) {
                    chars.next();
                    end += 1;
                    TokenKind::Arrow
                } else {
                    TokenKind::Minus
                }
            }
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '^' => TokenKind::Caret,
            '=' => {
                if matches!(chars.peek(), Some(&(_, '='))) {
                    chars.next();
                    end += 1;
                    TokenKind::EqEq
                } else {
                    TokenKind::Eq
                }
            }
            // The two-character forms come first so that `<=` never lexes as
            // `<` followed by `=`, which would parse as a comparison against an
            // assignment and report something baffling.
            '<' => {
                if matches!(chars.peek(), Some(&(_, '='))) {
                    chars.next();
                    end += 1;
                    TokenKind::Le
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                if matches!(chars.peek(), Some(&(_, '='))) {
                    chars.next();
                    end += 1;
                    TokenKind::Ge
                } else {
                    TokenKind::Gt
                }
            }
            '!' => {
                if matches!(chars.peek(), Some(&(_, '='))) {
                    chars.next();
                    end += 1;
                    TokenKind::Ne
                } else {
                    diagnostics.push(Diagnostic::error(
                        codes::UNEXPECTED_CHAR,
                        Span::new(start_u32, end as u32),
                        String::from(
                            "`!` on its own is not an operator; write `!=` for `not equal`",
                        ),
                    ));
                    continue;
                }
            }
            // The typeset spellings an engineer actually writes. `π` and `°` are
            // already ordinary characters here, so refusing `≤` would be an odd
            // place to draw the line.
            '≤' => TokenKind::Le,
            '≥' => TokenKind::Ge,
            '≠' => TokenKind::Ne,
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            _ => {
                diagnostics.push(Diagnostic::error(
                    codes::UNEXPECTED_CHAR,
                    Span::new(start_u32, end as u32),
                    format!("unexpected character `{c}`"),
                ));
                continue;
            }
        };
        tokens.push(Token {
            kind,
            span: Span::new(start_u32, end as u32),
        });
    }

    let n = source.len() as u32;
    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span::new(n, n),
    });

    Lexed {
        tokens,
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        lex(src).tokens.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn assignment_with_unit() {
        use TokenKind::*;
        assert_eq!(kinds("r = 5 cm"), vec![Ident, Eq, Number, Ident, Eof]);
    }

    #[test]
    fn comments_are_kept_as_tokens() {
        use TokenKind::*;
        assert_eq!(
            kinds("' a note\nx = 1"),
            vec![Comment, Newline, Ident, Eq, Number, Eof]
        );
    }

    #[test]
    fn comment_text_excludes_the_newline() {
        let src = "' Shaker specifications\nk = 4";
        let toks = lex(src).tokens;
        assert_eq!(toks[0].text(src), "' Shaker specifications");
    }

    #[test]
    fn arrow_is_one_token() {
        use TokenKind::*;
        assert_eq!(
            kinds("V -> dm^3"),
            vec![Ident, Arrow, Ident, Caret, Number, Eof]
        );
    }

    #[test]
    fn minus_is_not_arrow() {
        use TokenKind::*;
        assert_eq!(kinds("a - b"), vec![Ident, Minus, Ident, Eof]);
    }

    #[test]
    fn number_forms() {
        let src = "1 9.81 2.5e3 1e-6 .019";
        let toks = lex(src).tokens;
        let nums: Vec<&str> = toks
            .iter()
            .filter(|t| t.kind == TokenKind::Number)
            .map(|t| t.text(src))
            .collect();
        assert_eq!(nums, vec!["1", "9.81", "2.5e3", "1e-6", ".019"]);
    }

    #[test]
    fn e_without_digits_is_juxtaposed_identifier() {
        use TokenKind::*;
        // `2e` is 2 times the name `e`, not a malformed exponent.
        assert_eq!(kinds("2e"), vec![Number, Ident, Eof]);
        assert!(lex("2e").diagnostics.is_empty());
    }

    #[test]
    fn malformed_number_reports_once() {
        let d = lex("1.2.3").diagnostics;
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].code, codes::MALFORMED_NUMBER);
    }

    #[test]
    fn greek_and_degree_are_identifiers() {
        use TokenKind::*;
        assert_eq!(kinds("π"), vec![Ident, Eof]);
        assert_eq!(kinds("°C"), vec![Ident, Eof]);
        assert_eq!(kinds("Ling_rms_N"), vec![Ident, Eof]);
    }

    #[test]
    fn keywords() {
        use TokenKind::*;
        assert_eq!(
            kinds("unit kip = 1000 lbf"),
            vec![KwUnit, Ident, Eq, Number, Ident, Eof]
        );
        assert_eq!(
            kinds("fn f(x) = x"),
            vec![KwFn, Ident, LParen, Ident, RParen, Eq, Ident, Eof]
        );
    }

    #[test]
    fn blank_lines_collapse_to_one_boundary() {
        use TokenKind::*;
        assert_eq!(kinds("a\n\n\nb"), vec![Ident, Newline, Ident, Eof]);
    }

    #[test]
    fn unexpected_char_reports_and_continues() {
        let l = lex("a @ b");
        assert_eq!(l.diagnostics.len(), 1);
        assert_eq!(l.diagnostics[0].code, codes::UNEXPECTED_CHAR);
        // Lexing continues past the bad character.
        assert_eq!(
            l.tokens
                .iter()
                .filter(|t| t.kind == TokenKind::Ident)
                .count(),
            2
        );
    }

    #[test]
    fn spans_are_byte_accurate_with_multibyte_input() {
        let src = "π = 3";
        let toks = lex(src).tokens;
        assert_eq!(toks[0].text(src), "π");
        assert_eq!(toks[2].text(src), "3");
    }
}
