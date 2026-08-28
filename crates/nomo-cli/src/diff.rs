//! A line-oriented unified diff, for showing why a golden file did not match.
//!
//! Hand-rolled rather than pulled from crates.io because it is sixty lines and
//! the dependency would otherwise sit in the build of a project whose whole
//! argument is that its outputs are reproducible from a small, auditable tree.

/// Lines of unchanged context shown either side of a change.
const CONTEXT: usize = 3;

/// Cap on the diff printed for one file. Beyond this the change is structural
/// and the reader needs the file, not the diff.
const MAX_LINES: usize = 200;

/// Cap on the LCS table. Golden files are small and the common head and tail are
/// trimmed before this is reached, so exceeding it means something unusual;
/// falling back beats allocating gigabytes.
const MAX_CELLS: usize = 4_000_000;

/// A unified diff of `expected` against `actual`.
///
/// Returns `None` when the two are identical line by line — which, if the bytes
/// differ, means they differ only in line endings or a trailing newline. The
/// caller is expected to say so rather than print nothing.
pub fn unified(expected: &str, actual: &str, label: &str) -> Option<String> {
    let a: Vec<&str> = expected.lines().collect();
    let b: Vec<&str> = actual.lines().collect();

    // Trim the matching head and tail first. A golden file usually differs in a
    // line or two out of many, so this leaves the LCS a small problem and keeps
    // the diff pointed at the change instead of restating the file.
    let mut head = 0;
    while head < a.len() && head < b.len() && a[head] == b[head] {
        head += 1;
    }
    let mut tail = 0;
    while tail < a.len() - head
        && tail < b.len() - head
        && a[a.len() - 1 - tail] == b[b.len() - 1 - tail]
    {
        tail += 1;
    }

    if head == a.len() && head == b.len() {
        return None;
    }

    let a_mid = &a[head..a.len() - tail];
    let b_mid = &b[head..b.len() - tail];

    let mut out = format!("--- {label} (expected)\n+++ {label} (rendered)\n");

    let lead = CONTEXT.min(head);
    out.push_str(&format!("@@ line {} @@\n", head - lead + 1));
    let mut emitted = 0;
    for line in &a[head - lead..head] {
        out.push_str(&format!(" {line}\n"));
        emitted += 1;
    }

    match ops(a_mid, b_mid) {
        Some(changes) => {
            for (sign, line) in changes {
                if emitted >= MAX_LINES {
                    out.push_str("... diff truncated ...\n");
                    return Some(out);
                }
                out.push_str(&format!("{sign}{line}\n"));
                emitted += 1;
            }
        }
        None => {
            out.push_str(&format!(
                "... {} expected lines replaced by {} rendered lines ...\n",
                a_mid.len(),
                b_mid.len()
            ));
        }
    }

    let trail = CONTEXT.min(tail);
    let from = a.len() - tail;
    for line in &a[from..from + trail] {
        out.push_str(&format!(" {line}\n"));
    }

    Some(out)
}

/// Longest-common-subsequence backtrace, as a run of ` `, `-` and `+` lines.
fn ops<'a>(a: &[&'a str], b: &[&'a str]) -> Option<Vec<(char, &'a str)>> {
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) > MAX_CELLS {
        return None;
    }

    let stride = m + 1;
    let mut lcs = vec![0u32; (n + 1) * stride];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * stride + j] = if a[i] == b[j] {
                lcs[(i + 1) * stride + j + 1] + 1
            } else {
                lcs[(i + 1) * stride + j].max(lcs[i * stride + j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push((' ', a[i]));
            i += 1;
            j += 1;
        } else if lcs[(i + 1) * stride + j] >= lcs[i * stride + j + 1] {
            out.push(('-', a[i]));
            i += 1;
        } else {
            out.push(('+', b[j]));
            j += 1;
        }
    }
    for line in &a[i..] {
        out.push(('-', line));
    }
    for line in &b[j..] {
        out.push(('+', line));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_has_no_diff() {
        assert!(unified("a\nb\nc\n", "a\nb\nc\n", "x").is_none());
    }

    #[test]
    fn a_changed_line_shows_both_sides() {
        let d = unified("a\nb\nc\n", "a\nB\nc\n", "x").unwrap();
        assert!(d.contains("-b\n"), "{d}");
        assert!(d.contains("+B\n"), "{d}");
        assert!(d.contains(" a\n") && d.contains(" c\n"), "{d}");
    }

    #[test]
    fn context_is_limited_and_located() {
        let expected = "1\n2\n3\n4\n5\n6\n7\n8\nX\n";
        let actual = "1\n2\n3\n4\n5\n6\n7\n8\nY\n";
        let d = unified(expected, actual, "x").unwrap();
        // Lines 1-5 are outside the three lines of context.
        assert!(!d.contains(" 5\n"), "{d}");
        assert!(d.contains(" 6\n"), "{d}");
        assert!(d.contains("@@ line 6 @@"), "{d}");
    }

    #[test]
    fn insertion_and_deletion_are_distinguished() {
        let d = unified("a\nc\n", "a\nb\nc\n", "x").unwrap();
        assert!(d.contains("+b\n"), "{d}");
        let removed = d
            .lines()
            .skip(2) // the `---` header line is not a removal
            .any(|l| l.starts_with('-'));
        assert!(!removed, "nothing was deleted:\n{d}");
    }

    #[test]
    fn a_trailing_newline_difference_reports_no_line_diff() {
        // `lines()` cannot see it; the caller has to say so instead of printing
        // an empty diff.
        assert!(unified("a\nb\n", "a\nb", "x").is_none());
    }

    #[test]
    fn empty_expected_is_all_additions() {
        let d = unified("", "a\nb\n", "x").unwrap();
        assert!(d.contains("+a\n") && d.contains("+b\n"), "{d}");
    }

    #[test]
    fn a_wholly_different_file_still_produces_a_diff() {
        let d = unified("a\nb\nc\n", "x\ny\nz\n", "f").unwrap();
        assert!(d.contains("-a\n") && d.contains("+x\n"), "{d}");
        assert!(d.starts_with("--- f (expected)\n+++ f (rendered)\n"), "{d}");
    }
}
