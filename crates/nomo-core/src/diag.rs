//! Diagnostics.
//!
//! Codes are stable identifiers so that documentation and tests can refer to a
//! specific error without depending on its wording. `SH0xx` is lexing and
//! parsing; later phases claim their own ranges (units `SH1xx`, evaluation
//! `SH2xx`, document/graph `SH3xx`).

use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn error(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            span,
        }
    }

    pub fn warning(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span,
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Lexing and parsing diagnostic codes.
pub mod codes {
    /// A character that cannot begin any token.
    pub const UNEXPECTED_CHAR: &str = "SH001";
    /// A numeric literal that does not parse, e.g. `1.2.3` or `1e`.
    pub const MALFORMED_NUMBER: &str = "SH002";
    /// An opening delimiter with no match.
    pub const UNCLOSED_DELIMITER: &str = "SH003";
    /// A token appeared where an expression was required.
    pub const EXPECTED_EXPRESSION: &str = "SH004";
    /// A specific token was required and something else was found.
    pub const EXPECTED_TOKEN: &str = "SH005";
    /// The left side of `=` is not something that can be assigned to.
    pub const INVALID_ASSIGN_TARGET: &str = "SH006";
    /// Matrix rows of differing length.
    pub const RAGGED_MATRIX: &str = "SH007";
    /// Trailing input the parser could not attach to anything.
    pub const TRAILING_INPUT: &str = "SH008";
    /// A string literal with no closing quote before the end of the line.
    pub const UNTERMINATED_TEXT: &str = "SH009";
}
