//! Nomo command-line front end.
//!
//! All filesystem access lives here; `nomo-core` stays free of I/O so that it
//! compiles to WebAssembly unchanged.
//!
//! Commands grow with the phases. Today: `check`, which evaluates a worksheet
//! and reports its diagnostics; `render` and `html`, which produce output;
//! `ast`, which dumps the syntax tree; and `test`, the golden-file regression
//! suite.

mod diff;
mod harness;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match args.split_first() {
        Some((c, r)) => (c.as_str(), r),
        None => {
            usage();
            return ExitCode::FAILURE;
        }
    };

    match cmd {
        "check" => run_over(rest, check_one),
        "ast" => run_over(rest, dump_ast),
        "render" => run_over(rest, render_text),
        "html" => run_over(rest, render_html),
        "test" => harness::run(rest),
        "--help" | "-h" | "help" => {
            usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("nomo: unknown command `{other}`\n");
            usage();
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!(
        "nomo — engineering worksheets\n\n\
         USAGE:\n    \
         nomo check  <file.nomo>...   evaluate and report diagnostics\n    \
         nomo render <file.nomo>...   evaluate and print worked results\n    \
         nomo html   <file.nomo>...   write a standalone HTML file\n    \
         nomo ast    <file.nomo>...   print the syntax tree\n    \
         nomo test   [--write]        check every example against its snapshot\n\n\
         `test` runs from the repository root; --examples and --golden override\n\
         the directories it uses.\n"
    );
}

fn run_over(paths: &[String], f: fn(&str, &str) -> bool) -> ExitCode {
    if paths.is_empty() {
        eprintln!("nomo: expected at least one file");
        return ExitCode::FAILURE;
    }
    let mut ok = true;
    for path in paths {
        match std::fs::read_to_string(path) {
            Ok(source) => ok &= f(path, &source),
            Err(e) => {
                eprintln!("nomo: cannot read {path}: {e}");
                ok = false;
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Evaluate a worksheet and report everything wrong with it.
///
/// Evaluating, not just parsing. Most of what is wrong with a worksheet is not
/// a syntax error — an undefined name, a dimension that does not combine, a
/// cycle between two definitions — and none of that exists until the sheet has
/// been evaluated. A `check` that only parsed reported `ok` on
/// `examples/diagnostics.nomo`, which is a page of deliberate mistakes.
///
/// The `ok` line waits on errors rather than on silence, so a worksheet that
/// only draws warnings still says it passed, above them.
fn check_one(path: &str, source: &str) -> bool {
    let sheet = nomo_core::Sheet::new(source);
    report(path, source, sheet.diagnostics());
    if !sheet.has_errors() {
        println!("{path}: ok ({} statements)", sheet.ast().stmts.len());
    }
    !sheet.has_errors()
}

/// Render a worksheet: the substituted form and results.
fn render_text(path: &str, source: &str) -> bool {
    let sheet = nomo_core::Sheet::new(source);
    let opts = nomo_core::RenderOptions::default();
    print!("{}", nomo_core::render::text::render(&sheet, &opts));
    report(path, source, sheet.diagnostics());
    !sheet.has_errors()
}

/// Render a worksheet to a standalone HTML file beside the source.
fn render_html(path: &str, source: &str) -> bool {
    let sheet = nomo_core::Sheet::new(source);
    let opts = nomo_core::RenderOptions::default();
    let title = std::path::Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned());
    let html = nomo_core::render::html::render(&sheet, &opts, &title);

    let out = std::path::Path::new(path).with_extension("html");
    match std::fs::write(&out, html) {
        Ok(()) => println!("{}", out.display()),
        Err(e) => {
            eprintln!("nomo: cannot write {}: {e}", out.display());
            return false;
        }
    }
    report(path, source, sheet.diagnostics());
    !sheet.has_errors()
}

fn report(path: &str, source: &str, diagnostics: &[nomo_core::Diagnostic]) {
    for d in diagnostics {
        let (line, col) = d.span.line_col(source);
        let severity = match d.severity {
            nomo_core::Severity::Error => "error",
            nomo_core::Severity::Warning => "warning",
        };
        eprintln!("{path}:{line}:{col}: {severity}[{}]: {}", d.code, d.message);
    }
}

fn dump_ast(path: &str, source: &str) -> bool {
    let parsed = nomo_core::parse(source);
    println!("{path}:\n{:#?}", parsed.ast);
    report(path, source, &parsed.diagnostics);
    !parsed.has_errors()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_evaluates_rather_than_only_parsing() {
        // The bug this exists for: every one of these parses, and `check`
        // called them all ok.
        for wrong in [
            "q = nosuchname + 1\n",
            "q = 1 m + 1 s\n",
            "a = b + 1\nb = a + 1\n",
        ] {
            assert!(
                !check_one("t.nomo", wrong),
                "check passed a worksheet that does not evaluate: {wrong:?}"
            );
        }
        assert!(check_one("t.nomo", "r = 5 cm\nV = pi*r^2\n"));
    }

    #[test]
    fn a_warning_is_not_a_failure() {
        // `min` is a unit and a conventional variable name; shadowing it warns
        // and the worksheet is still fine.
        assert!(check_one("t.nomo", "min = 5\nmin\n"));
    }
}
