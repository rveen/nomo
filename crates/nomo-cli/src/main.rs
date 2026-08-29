//! Nomo command-line front end.
//!
//! All filesystem access lives here; `nomo-core` stays free of I/O so that it
//! compiles to WebAssembly unchanged.
//!
//! Commands grow with the phases. Today: `check`, which evaluates a worksheet
//! and reports its diagnostics; `render` and `html`, which produce output;
//! `ast`, which dumps the syntax tree; `test`, the golden-file regression
//! suite; and `bench`, which times the engine and reports rather than gates.

mod bench;
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
        "bench" => bench::run(rest),
        "packs" => list_packs(),
        "version" | "--version" | "-V" => {
            version();
            ExitCode::SUCCESS
        }
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
         nomo test   [--write]        check every example against its snapshot\n    \
         nomo bench                   time the engine on worksheets of fixed shape\n    \
         nomo packs                   list the packs `use` can bring in\n    \
         nomo version                 this build, and the formats it speaks\n\n\
         `test` runs from the repository root; --examples and --golden override\n\
         the directories it uses.\n\n\
         EXIT: 0 all well; 1 the worksheet does not evaluate; 2 it evaluates and\n\
         a `check` statement failed — the arithmetic is right and the design is\n\
         not, which is a different thing and gets a different code.\n"
    );
}

/// How a worksheet came out, worst first when several are given.
///
/// Three outcomes rather than two, because a failed check is not a broken
/// worksheet. `check sigma <= sigma_allow` failing means the arithmetic is
/// right and the design does not hold; reporting that as an error would put it
/// in the same bucket as an undefined name, and a script could not tell the
/// difference between "this sheet is wrong" and "this part is overstressed".
/// Ordering matters: a worksheet that does not evaluate outranks one whose
/// checks failed, because it has not established anything to fail.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum Verdict {
    Ok,
    ChecksFailed,
    Errors,
}

impl Verdict {
    fn of(sheet: &nomo_core::Sheet) -> Verdict {
        if sheet.has_errors() {
            Verdict::Errors
        } else if sheet.checks().failed > 0 {
            Verdict::ChecksFailed
        } else {
            Verdict::Ok
        }
    }

    fn exit(self) -> ExitCode {
        match self {
            Verdict::Ok => ExitCode::SUCCESS,
            // 1 is "this worksheet does not evaluate"; 2 is "it evaluates and
            // says no". CI can act on the second without treating it as a bug.
            Verdict::Errors => ExitCode::from(1),
            Verdict::ChecksFailed => ExitCode::from(2),
        }
    }
}

/// Report how many verdicts a worksheet reached, when it states any.
fn report_checks(path: &str, sheet: &nomo_core::Sheet) {
    let c = sheet.checks();
    if c.total == 0 {
        return;
    }
    let mut line = format!("{path}: {} check{}", c.total, plural(c.total));
    if c.failed > 0 {
        line.push_str(&format!(", {} FAILED", c.failed));
    }
    if c.undecided > 0 {
        line.push_str(&format!(", {} not decided", c.undecided));
    }
    if c.failed == 0 && c.undecided == 0 {
        line.push_str(", all passed");
    }
    println!("{line}");
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Which build this is, and which formats it speaks.
///
/// Three numbers rather than one, because they answer different questions. The
/// build says which code this is. The worksheet format says which files it can
/// open — a worksheet declaring a later one still opens, with a warning. The
/// snapshot format says which golden files it can be compared against, and is
/// what the cross-target comparison agrees on before it compares anything.
fn version() {
    println!("nomo {}", env!("CARGO_PKG_VERSION"));
    println!("worksheet format {}", nomo_core::doc::CURRENT_VERSION);
    println!("snapshot format {}", nomo_core::golden::FORMAT);
}

/// What `use` can bring in, and what each one holds.
///
/// The packs are compiled into the engine, so this list is a fact about the
/// build rather than about the machine it runs on — which is the same reason
/// they are compiled in at all.
fn list_packs() -> ExitCode {
    for pack in nomo_core::packs::PACKS {
        println!("{:<12} {}", pack.name, pack.summary);
        for stmt in nomo_core::parse(pack.source).ast.stmts {
            if let nomo_core::ast::Stmt::GlobalDef { name, .. } = stmt {
                println!("             {}", name.text);
            }
        }
    }
    ExitCode::SUCCESS
}

fn run_over(paths: &[String], f: fn(&str, &str) -> Verdict) -> ExitCode {
    if paths.is_empty() {
        eprintln!("nomo: expected at least one file");
        return ExitCode::FAILURE;
    }
    let mut worst = Verdict::Ok;
    for path in paths {
        match std::fs::read_to_string(path) {
            Ok(source) => worst = worst.max(f(path, &source)),
            Err(e) => {
                eprintln!("nomo: cannot read {path}: {e}");
                worst = Verdict::Errors;
            }
        }
    }
    worst.exit()
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
fn check_one(path: &str, source: &str) -> Verdict {
    let sheet = nomo_core::Sheet::new(source);
    report(path, source, sheet.diagnostics());
    if !sheet.has_errors() {
        // The author's statements, not the pack's: a worksheet that says `use
        // steel` did not thereby write fourteen more lines, and counting them
        // would make the number mean nothing.
        let written = (0..sheet.ast().stmts.len())
            .filter(|i| !sheet.is_from_pack(*i))
            .count();
        println!("{path}: ok ({written} statements)");
    }
    report_checks(path, &sheet);
    Verdict::of(&sheet)
}

/// Render a worksheet: the substituted form and results.
fn render_text(path: &str, source: &str) -> Verdict {
    let sheet = nomo_core::Sheet::new(source);
    let opts = nomo_core::RenderOptions::default();
    print!("{}", nomo_core::render::text::render(&sheet, &opts));
    report(path, source, sheet.diagnostics());
    Verdict::of(&sheet)
}

/// Render a worksheet to a standalone HTML file beside the source.
fn render_html(path: &str, source: &str) -> Verdict {
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
            return Verdict::Errors;
        }
    }
    report(path, source, sheet.diagnostics());
    Verdict::of(&sheet)
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

fn dump_ast(path: &str, source: &str) -> Verdict {
    let parsed = nomo_core::parse(source);
    println!("{path}:\n{:#?}", parsed.ast);
    report(path, source, &parsed.diagnostics);
    if parsed.has_errors() {
        Verdict::Errors
    } else {
        Verdict::Ok
    }
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
                check_one("t.nomo", wrong) == Verdict::Errors,
                "check passed a worksheet that does not evaluate: {wrong:?}"
            );
        }
        assert!(check_one("t.nomo", "r = 5 cm\nV = pi*r^2\n") == Verdict::Ok);
    }

    #[test]
    fn a_failed_check_is_its_own_verdict() {
        // Three outcomes, three exit codes. A script that runs a worksheet has
        // to be able to tell "this sheet is broken" from "this design does not
        // hold", and the exit code is the only thing it reads.
        let ok = "sigma = 10 ksi\ncheck sigma <= 20 ksi\n";
        let failing = "sigma = 30 ksi\ncheck sigma <= 20 ksi\n";
        let broken = "sigma = nosuchname\ncheck 1 <= 2\n";

        assert!(check_one("t.nomo", ok) == Verdict::Ok);
        assert!(check_one("t.nomo", failing) == Verdict::ChecksFailed);
        assert!(check_one("t.nomo", broken) == Verdict::Errors);

        // A worksheet that does not evaluate outranks one whose checks failed:
        // it has not established anything to fail.
        assert!(Verdict::Errors > Verdict::ChecksFailed);
        assert!(Verdict::ChecksFailed > Verdict::Ok);
    }

    #[test]
    fn a_warning_is_not_a_failure() {
        // `min` is a unit and a conventional variable name; shadowing it warns
        // and the worksheet is still fine.
        assert!(check_one("t.nomo", "min = 5\nmin\n") == Verdict::Ok);
    }
}
