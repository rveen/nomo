//! `nomo bench` — how long the engine takes on worksheets of known shape.
//!
//! A **report, not a gate**. It exits zero however slow the news is, and CI runs
//! it the way it runs the importer's coverage report: to put the number in the
//! log on every push, so a change that costs ten times as much is visible in the
//! same place the change is. A wall-clock threshold on a shared runner would be
//! a flake generator, and a flaky gate is worse than none — it teaches people to
//! re-run rather than to look.
//!
//! The worksheets are generated here rather than read from `examples/`, for the
//! same reason the golden suite reads from there and not here: those exist to
//! pin behaviour and change whenever the language does, which would move these
//! numbers for reasons that have nothing to do with speed. The shapes below are
//! fixed, and each one is a question about the engine that the others do not
//! ask.
//!
//! Timing is the CLI's business, not the engine's: `nomo-core` has no clock —
//! `check-no-host-math.sh` enforces that — so everything measured here is
//! measured from outside.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// How many times each case runs. The best is reported.
///
/// Best rather than mean, because what varies between runs is interference —
/// another process, a migrated core, a cold cache — and interference only ever
/// adds. The fastest run is the one that says most about the engine.
const RUNS: usize = 5;

pub fn run(args: &[String]) -> std::process::ExitCode {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "nomo bench — time the engine on worksheets of fixed shape\n\n\
             Prints a report and always exits 0. The shapes are generated, not\n\
             read from examples/, so the numbers move only when the engine does."
        );
        return std::process::ExitCode::SUCCESS;
    }

    // Which build produced these, said out loud. The debug build is about three
    // times slower than release across every case here, and a number compared
    // against one from the other profile is worse than no number at all — CI
    // runs the debug build, because the job it runs in has already compiled it.
    let profile = if cfg!(debug_assertions) {
        "debug (about 3x slower than release; `cargo run --release` for the real thing)"
    } else {
        "release"
    };
    println!("nomo bench — best of {RUNS}, {profile}\n");
    println!(
        "{:<28} {:>9} {:>12} {:>14}",
        "case", "size", "time", "per unit"
    );
    println!("{}", "-".repeat(66));

    for case in cases() {
        let (time, unit) = (case.run)();
        println!(
            "{:<28} {:>9} {:>12} {:>14}",
            case.name,
            case.size,
            format_duration(time),
            unit
        );
    }

    println!(
        "\nA report, not a gate: this exits 0 whatever it printed. Compare a run\n\
         against the same machine's previous run, never against another machine's."
    );
    std::process::ExitCode::SUCCESS
}

struct Case {
    name: &'static str,
    size: String,
    run: Box<dyn Fn() -> (Duration, String)>,
}

fn cases() -> Vec<Case> {
    vec![
        // Throughput on an ordinary worksheet: many independent statements, each
        // carrying units. This is the shape of a long calculation sheet.
        whole_sheet("wide, 1000 statements", wide(1000), 1000, "line"),
        whole_sheet("wide, 5000 statements", wide(5000), 5000, "line"),
        // A chain, where every statement depends on the one above it. Same
        // number of statements, entirely different work for the dependency
        // graph: one path rather than a thousand roots.
        whole_sheet("chain, 3000 deep", chain(3000), 3000, "line"),
        // Numeric throughput with almost no parsing: three lines, a hundred
        // thousand elements, and a user function called once per element.
        whole_sheet("map over 100k elements", vectors(), 100_000, "element"),
        // Drawing. A plot samples a function at a fixed count and emits SVG, so
        // this is the renderer rather than the evaluator.
        whole_sheet("eight plots", plots(8), 8, "plot"),
        // The claim the editor makes, and the only case here that measures the
        // incremental path: editing one line of a long worksheet must not cost
        // what evaluating the worksheet costs.
        edit_one_line(),
    ]
}

/// Edit a single line of a long worksheet, as a keystroke does.
fn edit_one_line() -> Case {
    let source = wide(5000);
    let edited = source.replacen("= 1 m", "= 2 m", 1);
    let size = format!("{} kB", source.len().div_ceil(1000));
    Case {
        name: "edit one line of 5000",
        size,
        run: Box::new(move || {
            let mut sheet = nomo_core::Sheet::new(&source);
            // Alternate, so no run repeats the previous run's text and finds
            // nothing changed.
            let mut flip = false;
            let mut evaluated = 0;
            let time = best(|| {
                flip = !flip;
                let r = sheet.update(if flip { &edited } else { &source });
                evaluated = r.evaluated.len();
            });
            (time, format!("{evaluated} of 5000 evaluated"))
        }),
    }
}

/// Time `snapshot` over a generated worksheet — parse, evaluate, render, both
/// views. The same function the WebAssembly build exports, so this is the whole
/// pipeline and not a corner of it.
///
/// `count` and `unit` say what the worksheet is *made of*, which is not always
/// its number of lines: the vector case is three lines and a hundred thousand
/// elements, and reporting that per line would say nothing.
fn whole_sheet(name: &'static str, source: String, count: usize, unit: &'static str) -> Case {
    let size = format!("{} kB", source.len().div_ceil(1000));
    Case {
        name,
        size,
        run: Box::new(move || {
            let time = best(|| {
                let _ = nomo_core::golden::snapshot("bench", &source);
            });
            let each = time / count.max(1) as u32;
            (time, format!("{} / {unit}", format_duration(each)))
        }),
    }
}

fn best(mut body: impl FnMut()) -> Duration {
    // One untimed run first: the first pass touches pages nothing has touched,
    // and that cost belongs to the allocator rather than to the engine.
    body();
    let mut best = Duration::MAX;
    for _ in 0..RUNS {
        let start = Instant::now();
        body();
        best = best.min(start.elapsed());
    }
    best
}

/// `n` independent statements, each with a unit.
fn wide(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = writeln!(s, "v{i} = 1 m + {} mm", i % 97);
    }
    s
}

/// `n` statements, each reading the one above it.
fn chain(n: usize) -> String {
    let mut s = String::from("a0 = 1 m\n");
    for i in 1..n {
        let _ = writeln!(s, "a{i} = a{} + 1 mm", i - 1);
    }
    s
}

/// A hundred thousand elements through `map` and `sum`.
fn vectors() -> String {
    "fn f(x) = x^2 + 1\nv = map(f, range(1, 100000))\ns = sum(v)\n".into()
}

/// Eight curves drawn over a span.
fn plots(n: usize) -> String {
    let mut s = String::from("fn f(x) = sin(x)/(1 + x^2)\n");
    for i in 0..n {
        let _ = writeln!(s, "plot(f, 0, {})", i + 1);
    }
    s
}

fn format_duration(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 10_000 {
        format!("{ns} ns")
    } else if ns < 10_000_000 {
        format!("{:.1} µs", ns as f64 / 1e3)
    } else {
        format!("{:.1} ms", ns as f64 / 1e6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_generated_worksheet_evaluates() {
        // A benchmark measuring a worksheet full of errors would measure the
        // diagnostic path and say nothing about the engine. Small sizes: this
        // checks the shapes are right, not how fast they are.
        for (name, source) in [
            ("wide", wide(20)),
            ("chain", chain(20)),
            ("vectors", vectors()),
            ("plots", plots(2)),
        ] {
            let sheet = nomo_core::Sheet::new(&source);
            assert!(
                !sheet.has_errors(),
                "the {name} case does not evaluate: {:?}",
                sheet.diagnostics()
            );
        }
    }

    #[test]
    fn the_edit_case_really_edits() {
        // If the replacement ever stops matching, the incremental case would
        // report a recalculation of nothing and look wonderful.
        let source = wide(50);
        let edited = source.replacen("= 1 m", "= 2 m", 1);
        assert_ne!(source, edited, "the edit did not change the worksheet");
        let mut sheet = nomo_core::Sheet::new(&source);
        let r = sheet.update(&edited);
        assert_eq!(r.changed.len(), 1, "one line should have changed");
        assert!(!r.structural, "editing in place is not structural");
    }
}
