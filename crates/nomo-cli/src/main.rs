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
        "html" => {
            // A flag rather than a worksheet statement: how a document is
            // typeset is a fact about the rendering asked for, not about the
            // engineering in it.
            let mathml = rest.iter().any(|a| a == "--mathml");
            let (font, text_font, files) = match font_option(rest) {
                Ok(pair) => pair,
                Err(message) => {
                    eprintln!("nomo: {message}");
                    return ExitCode::FAILURE;
                }
            };
            let (paged, files) = match paged_option(&files) {
                Ok(pair) => pair,
                Err(message) => {
                    eprintln!("nomo: {message}");
                    return ExitCode::FAILURE;
                }
            };
            MATHML.with(|m| m.set(mathml));
            FONT.with(|f| *f.borrow_mut() = font);
            TEXT_FONT.with(|f| *f.borrow_mut() = text_font);
            PAGED.with(|p| *p.borrow_mut() = paged);
            run_over(&files, render_html)
        }
        "test" => harness::run(rest),
        "bench" => bench::run(rest),
        "packs" => list_packs(),
        "units" => {
            println!("{}", nomo_core::api::units_json());
            ExitCode::SUCCESS
        }
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
         nomo html --mathml <file>    the same, with the mathematics typeset\n    \
         nomo html --font-url <url>   ...laid out with a font served beside it\n    \
         nomo html --embed-font <f>   ...with the font carried in the document\n    \
         nomo html --text-font-url <url> [--text-font-italic-url <url>]\n\
         \x20                                ...and the prose in a text face served beside it\n    \
         nomo html --paged <file>     laid out as pages, by Folio carried in it\n    \
         nomo html --folio-url <url>  ...by Folio served beside the document\n    \
         nomo html --embed-folio <f>  ...by the Folio script at <f>\n    \
         nomo ast    <file.nomo>...   print the syntax tree\n    \
         nomo test   [--write]        check every example against its snapshot\n    \
         nomo bench                   time the engine on worksheets of fixed shape\n    \
         nomo packs                   list the packs `use` can bring in\n    \
         nomo units                   the unit table as exact data, in JSON\n    \
         nomo version                 this build, and the formats it speaks\n\n\
         `test` runs from the repository root; --examples and --golden override\n\
         the directories it uses. So does --paged, which carries\n\
         web/vendor/folio/folio.polyfill.min.js: ./scripts/fetch-folio.sh builds it.\n\n\
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
/// Four numbers rather than one, because they answer different questions. The
/// build says which code this is. The worksheet format says which files it can
/// open — a worksheet declaring a later one still opens, with a warning. The
/// snapshot format says which golden files it can be compared against, and is
/// what the cross-target comparison agrees on before it compares anything. The
/// units format says how `nomo units` writes the table, for programs that read
/// it instead of running the engine.
fn version() {
    println!("nomo {}", env!("CARGO_PKG_VERSION"));
    println!("worksheet format {}", nomo_core::doc::CURRENT_VERSION);
    println!("snapshot format {}", nomo_core::golden::FORMAT);
    println!("units format {}", nomo_core::api::UNITS_FORMAT);
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

thread_local! {
    /// Whether `html` was asked for typeset output.
    ///
    /// A thread-local because `run_over` takes a plain function pointer, and
    /// threading an options struct through every command to carry one flag
    /// would be a worse trade than this is.
    static MATHML: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// The font `html` was asked to lay the mathematics out with. Same reason.
    static FONT: std::cell::RefCell<nomo_core::MathFont> =
        const { std::cell::RefCell::new(nomo_core::MathFont::Named) };

    /// The face `html` was asked to set the prose in. Same reason.
    static TEXT_FONT: std::cell::RefCell<nomo_core::TextFont> =
        const { std::cell::RefCell::new(nomo_core::TextFont::Named) };

    /// The engine `html --paged` lays the pages out with, read once for every
    /// worksheet rather than once per worksheet. `None` for the plain page.
    static PAGED: std::cell::RefCell<Option<PagedEngine>> =
        const { std::cell::RefCell::new(None) };
}

use nomo_core::render::paged::PagedEngine;

/// Where `--paged` finds Folio when nothing else is named: where
/// `scripts/fetch-folio.sh` puts it, relative to the repository root, the same
/// convention `nomo test` follows for `examples/`. A path baked into the
/// binary at build time would have been more convenient and would have put
/// the build machine's directory layout into every release.
const FOLIO_DEFAULT: &str = "web/vendor/folio/folio.polyfill.min.js";

/// Read `--paged`, `--folio-url` and `--embed-folio` out of the argument list.
///
/// The second and third say *which* Folio and imply the first, since naming an
/// engine is only meaningful for a document that is laid out by one. Two flags
/// for the same reason the font has two: linking a script and carrying one
/// are different decisions with different costs.
fn paged_option(args: &[String]) -> Result<(Option<PagedEngine>, Vec<String>), String> {
    enum Source {
        Plain,
        Default,
        Url(String),
        File(String),
    }
    let mut source = Source::Plain;
    let mut files = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--paged" => {
                if let Source::Plain = source {
                    source = Source::Default;
                }
            }
            "--folio-url" => {
                let url = rest
                    .next()
                    .ok_or("--folio-url needs a URL, as in `--folio-url folio.polyfill.min.js`")?;
                source = Source::Url(url.clone());
            }
            "--embed-folio" => {
                let path = rest
                    .next()
                    .ok_or("--embed-folio needs a path to Folio's folio.polyfill.min.js")?;
                source = Source::File(path.clone());
            }
            other => files.push(other.to_string()),
        }
    }
    let engine = match source {
        Source::Plain => None,
        Source::Url(url) => Some(PagedEngine::url(url)),
        Source::Default => Some(embed_folio(std::path::Path::new(FOLIO_DEFAULT), true)?),
        Source::File(path) => Some(embed_folio(std::path::Path::new(&path), false)?),
    };
    Ok((engine, files))
}

/// Read a Folio script, and the licence that has to travel with it.
///
/// The licence is `LICENSE` beside the script, which is where
/// `scripts/fetch-folio.sh` puts it. Unlike a font, a missing one is refused
/// rather than warned about: this program knows exactly what Folio's terms are
/// and that they ask for the notice to accompany copies, so there is nothing to
/// be unsure of.
fn embed_folio(path: &std::path::Path, default: bool) -> Result<PagedEngine, String> {
    let hint = if default {
        "\n  Run ./scripts/fetch-folio.sh from the repository root, or name a build with --embed-folio."
    } else {
        ""
    };
    let script = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read Folio at {}: {e}{hint}", path.display()))?;
    let licence = path.with_file_name("LICENSE");
    let notice = std::fs::read_to_string(&licence).map_err(|e| {
        format!(
            "cannot read Folio's licence at {}: {e}\n  A document that carries Folio redistributes it, and its MIT terms must travel with it.",
            licence.display()
        )
    })?;
    PagedEngine::embedded(script, notice).map_err(|e| format!("{}: {e}", path.display()))
}

/// Read the font flags out of the argument list: `--font-url` and
/// `--embed-font` for the mathematics, `--text-font-url` and
/// `--text-font-italic-url` for the prose.
///
/// The text face can be referenced and not carried. A missing text face gives
/// Georgia rather than wrong mathematics, so a standalone document names it and
/// fetches nothing unless told where it is served (`nomo_core::TextFont`).
///
/// Returns the two font policies and the arguments with the flags removed. Two flags
/// rather than one that guesses from its argument: referencing a font and
/// carrying one are different decisions with different costs, and a flag whose
/// meaning depends on whether a path happens to exist is a flag that does the
/// wrong thing quietly.
fn font_option(
    args: &[String],
) -> Result<(nomo_core::MathFont, nomo_core::TextFont, Vec<String>), String> {
    let mut font = nomo_core::MathFont::Named;
    let mut upright: Option<String> = None;
    let mut italic: Option<String> = None;
    let mut files = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--mathml" => {}
            "--font-url" => {
                let url = rest
                    .next()
                    .ok_or("--font-url needs a URL, as in `--font-url fonts/math.woff2`")?;
                font = nomo_core::MathFont::Url(url.clone());
            }
            "--embed-font" => {
                let path = rest
                    .next()
                    .ok_or("--embed-font needs a path to a .woff2 file")?;
                font = embed(std::path::Path::new(path))?;
            }
            "--text-font-url" => {
                let url = rest.next().ok_or(
                    "--text-font-url needs a URL, as in `--text-font-url fonts/text.woff2`",
                )?;
                upright = Some(url.clone());
            }
            "--text-font-italic-url" => {
                let url = rest
                    .next()
                    .ok_or("--text-font-italic-url needs a URL to the italic face")?;
                italic = Some(url.clone());
            }
            other => files.push(other.to_string()),
        }
    }
    // An italic on its own would declare a family whose upright is whatever
    // the browser falls back to, which is a mixed page that looks deliberate.
    let text_font = match (upright, italic) {
        (Some(upright), italic) => nomo_core::TextFont::Url { upright, italic },
        (None, None) => nomo_core::TextFont::Named,
        (None, Some(_)) => {
            return Err("--text-font-italic-url needs --text-font-url for the upright face".into())
        }
    };
    Ok((font, text_font, files))
}

/// Read a font file, and the licence that has to travel with it.
///
/// A document that embeds a font redistributes it, and most font licences —
/// the SIL Open Font License among them — require their terms to accompany the
/// bytes. So a licence file sitting beside the font is embedded with it. This
/// is not a guess about what the licence says: it is carrying whatever the font
/// came with, which is what the obligation actually is.
///
/// A font with no licence beside it is still embedded, with a warning rather
/// than a refusal. The file may be one the user has every right to embed, and
/// this program is not in a position to know.
fn embed(path: &std::path::Path) -> Result<nomo_core::MathFont, String> {
    let woff2 = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let notice = [
        "OFL.txt",
        "LICENSE",
        "LICENSE.txt",
        "LICENCE",
        "LICENCE.txt",
    ]
    .iter()
    .find_map(|name| std::fs::read_to_string(dir.join(name)).ok());
    let notice = match notice {
        Some(text) => text,
        None => {
            eprintln!(
                "nomo: warning: no licence file beside {}\n  A document that embeds a font redistributes it. If the font's licence\n  requires its terms to travel with it — the SIL Open Font License does —\n  put the licence file next to the font and render again.",
                path.display()
            );
            String::new()
        }
    };
    Ok(nomo_core::MathFont::Embedded { woff2, notice })
}

/// Render a worksheet to a standalone HTML file beside the source.
fn render_html(path: &str, source: &str) -> Verdict {
    let sheet = nomo_core::Sheet::new(source);
    let opts = nomo_core::RenderOptions {
        mathml: MATHML.with(std::cell::Cell::get),
        font: FONT.with(|f| f.borrow().clone()),
        text_font: TEXT_FONT.with(|f| f.borrow().clone()),
        ..Default::default()
    };
    let title = std::path::Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned());
    // The file's own name, extension included, for the foot of a paged
    // document: it is what a reader holding one loose sheet would search for.
    let file = std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned());
    let html = PAGED.with(|p| match p.borrow().as_ref() {
        Some(engine) => nomo_core::render::paged::render(&sheet, &opts, &title, &file, engine),
        None => nomo_core::render::html::render(&sheet, &opts, &title),
    });

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

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// A scratch directory of the test's own, named the way the harness
    /// tests name theirs, so two tests — or two runs — never share one, and
    /// removed when the test is done with it.
    struct Scratch(std::path::PathBuf);

    impl std::ops::Deref for Scratch {
        type Target = std::path::PathBuf;
        fn deref(&self) -> &std::path::PathBuf {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("nomo-cli-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    #[test]
    fn the_text_face_flags_are_read_and_removed() {
        let (_, text, files) = font_option(&args(&[
            "--text-font-url",
            "t.woff2",
            "a.nomo",
            "--text-font-italic-url",
            "ti.woff2",
        ]))
        .unwrap();
        assert_eq!(
            text,
            nomo_core::TextFont::Url {
                upright: "t.woff2".into(),
                italic: Some("ti.woff2".into())
            }
        );
        assert_eq!(files, args(&["a.nomo"]));

        let (_, text, _) = font_option(&args(&["a.nomo"])).unwrap();
        assert_eq!(text, nomo_core::TextFont::Named);
    }

    #[test]
    fn an_italic_without_its_upright_is_an_error() {
        let err = font_option(&args(&["--text-font-italic-url", "ti.woff2"])).unwrap_err();
        assert!(err.contains("--text-font-url"), "{err}");
        assert!(font_option(&args(&["--text-font-url"])).is_err());
    }

    #[test]
    fn without_paged_the_page_is_plain() {
        let (engine, files) = paged_option(&args(&["a.nomo", "b.nomo"])).unwrap();
        assert!(engine.is_none());
        assert_eq!(files, args(&["a.nomo", "b.nomo"]));
    }

    #[test]
    fn naming_an_engine_implies_paged() {
        let (engine, files) = paged_option(&args(&["--folio-url", "f.js", "a.nomo"])).unwrap();
        assert_eq!(engine, Some(PagedEngine::url("f.js")));
        assert_eq!(files, args(&["a.nomo"]));
        // --paged beside it does not put the default back.
        let (engine, _) = paged_option(&args(&["--folio-url", "f.js", "--paged"])).unwrap();
        assert_eq!(engine, Some(PagedEngine::url("f.js")));
    }

    #[test]
    fn a_flag_without_its_value_is_an_error() {
        assert!(paged_option(&args(&["--folio-url"])).is_err());
        assert!(paged_option(&args(&["--embed-folio"])).is_err());
    }

    #[test]
    fn an_embedded_folio_brings_its_licence() {
        let dir = scratch("embedded-folio");
        let script = dir.join("folio.polyfill.min.js");
        std::fs::write(&script, "/* folio */").unwrap();
        let path = script.to_string_lossy().into_owned();

        // No licence beside it: refused, and the message says why.
        let err = paged_option(&args(&["--embed-folio", &path])).unwrap_err();
        assert!(err.contains("licence"), "{err}");

        std::fs::write(dir.join("LICENSE"), "MIT License").unwrap();
        let (engine, _) = paged_option(&args(&["--embed-folio", &path])).unwrap();
        assert_eq!(
            engine,
            Some(PagedEngine::embedded("/* folio */", "MIT License").unwrap())
        );
    }

    #[test]
    fn a_script_that_could_end_its_element_is_not_embedded() {
        let dir = scratch("unsafe-folio");
        let script = dir.join("folio.js");
        std::fs::write(&script, "x = '</script>'").unwrap();
        std::fs::write(dir.join("LICENSE"), "MIT License").unwrap();
        let err = paged_option(&args(&["--embed-folio", &script.to_string_lossy()])).unwrap_err();
        assert!(err.contains("</script"), "{err}");
    }

    #[test]
    fn a_missing_engine_names_the_script_that_builds_one() {
        let err = paged_option(&args(&["--embed-folio", "/nonexistent/folio.js"])).unwrap_err();
        assert!(!err.contains("fetch-folio"), "{err}");
        // The default is relative to the working directory, so from a crate's
        // own directory it is never there, and the hint is what a user sees.
        if !std::path::Path::new(FOLIO_DEFAULT).exists() {
            let err = paged_option(&args(&["--paged"])).unwrap_err();
            assert!(err.contains("fetch-folio.sh"), "{err}");
        }
    }

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
