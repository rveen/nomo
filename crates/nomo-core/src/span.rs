//! Byte ranges into the source text.
//!
//! Every syntax node carries one, and evaluation carries them through into the
//! trace. Both diagnostics and substituted-form rendering depend on being able to
//! point back at the exact characters the user typed.

/// A half-open byte range `[start, end)` into the worksheet source.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// The span covering both operands. Used when a parent node's extent is the
    /// union of its children, which is nearly always.
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }

    pub fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Slice the originating source. Returns `""` if the span is out of bounds,
    /// which can only happen if a span was built against different text.
    pub fn text(self, source: &str) -> &str {
        source
            .get(self.start as usize..self.end as usize)
            .unwrap_or("")
    }

    /// One-based line and column of the span's start.
    ///
    /// Columns count characters, not bytes, so that `π`, `°` and `Δ` do not skew
    /// the position a reader is asked to look at.
    pub fn line_col(self, source: &str) -> (usize, usize) {
        let offset = self.start as usize;
        let mut line = 1;
        let mut col = 1;
        for (i, c) in source.char_indices() {
            if i >= offset {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }
}

impl core::fmt::Debug for Span {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_covers_both() {
        assert_eq!(Span::new(2, 5).to(Span::new(9, 12)), Span::new(2, 12));
        assert_eq!(Span::new(9, 12).to(Span::new(2, 5)), Span::new(2, 12));
    }

    #[test]
    fn text_slices_source() {
        assert_eq!(Span::new(4, 6).text("r = 5 cm"), "5 ");
        assert_eq!(Span::new(6, 8).text("r = 5 cm"), "cm");
    }

    #[test]
    fn out_of_bounds_is_empty_not_panic() {
        assert_eq!(Span::new(100, 200).text("short"), "");
    }

    #[test]
    fn line_col_is_one_based() {
        let src = "r = 5 cm\nh = 12 cm\n";
        assert_eq!(Span::new(0, 1).line_col(src), (1, 1));
        assert_eq!(Span::new(4, 5).line_col(src), (1, 5));
        assert_eq!(Span::new(9, 10).line_col(src), (2, 1));
    }

    #[test]
    fn line_col_counts_characters_not_bytes() {
        // `π` is two bytes, so the `=` sits at byte 3 but is the third
        // character. Counting bytes would report column 4.
        let src = "π = 3";
        assert_eq!(Span::new(3, 4).line_col(src), (1, 3));
    }
}
