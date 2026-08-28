//! Import SMath worksheets, and check the result against the answers they carry.
//!
//! ```text
//! smath-import worksheet.sm                    # write the Nomo source to stdout
//! smath-import --check corpora/nomo-corpus/sm   # check a whole corpus
//! ```
//!
//! `--check` is the interesting mode: it imports every worksheet, evaluates it,
//! and compares each stored answer with what Nomo computed. See the `oracle`
//! module for what the comparison does and why it is allowed a tolerance when
//! the golden-file suite is not.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nomo_smath::emit::NoteKind;
use nomo_smath::oracle::{self, Verdict};

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--baseline <file>` turns the corpus into a regression gate: the
    // per-worksheet result is compared with a committed file and any difference
    // fails, exactly as `nomo test` does for rendered output. `--write`
    // regenerates it, so an intended change shows up in the diff beside the code
    // that caused it (design note §9).
    let mut baseline = None;
    if let Some(i) = args.iter().position(|a| a == "--baseline") {
        if i + 1 < args.len() {
            baseline = Some(PathBuf::from(args.remove(i + 1)));
        }
        args.remove(i);
    }
    let write = args.iter().any(|a| a == "--write");
    args.retain(|a| a != "--write");
    // `--lang xx` chooses which translation of a multilingual worksheet to keep.
    // Without it each region keeps its first, and the note says so.
    let mut language = None;
    if let Some(i) = args.iter().position(|a| a == "--lang") {
        if i + 1 < args.len() {
            language = Some(args.remove(i + 1));
        }
        args.remove(i);
    }
    let checking = args.iter().any(|a| a == "--check");
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();

    if paths.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!(
            "smath-import — turn SMath worksheets into Nomo\n\n\
             USAGE:\n    \
             smath-import <file.sm>              write Nomo source to stdout\n    \
             smath-import --check <file|dir>...  check every stored answer\n"
        );
        return ExitCode::FAILURE;
    }

    let mut files = Vec::new();
    for arg in &paths {
        let path = PathBuf::from(arg);
        if path.is_dir() {
            match collect(&path) {
                Ok(mut found) => files.append(&mut found),
                Err(e) => {
                    eprintln!("smath-import: {arg}: {e}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            files.push(path);
        }
    }
    files.sort();

    if !checking {
        if files.len() != 1 {
            eprintln!("smath-import: give one file, or --check for a whole corpus");
            return ExitCode::FAILURE;
        }
        return match read(&files[0]) {
            Ok(w) => {
                print!("{}", nomo_smath::emit_in(&w, language.as_deref()).source);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("smath-import: {}: {e}", files[0].display());
                ExitCode::FAILURE
            }
        };
    }

    check_all(&files, &args, baseline.as_deref(), write)
}

fn check_all(
    files: &[PathBuf],
    roots: &[String],
    baseline: Option<&Path>,
    write: bool,
) -> ExitCode {
    let mut totals: BTreeMap<String, usize> = BTreeMap::new();
    let mut notes: BTreeMap<NoteKind, usize> = BTreeMap::new();
    let mut unsupported: BTreeMap<String, usize> = BTreeMap::new();
    let mut disagreements: Vec<String> = Vec::new();
    let mut failures: BTreeMap<String, usize> = BTreeMap::new();
    // One line per worksheet, for the regression baseline. Built whether or not
    // a baseline was asked for, because it costs nothing and the alternative is
    // two code paths that can disagree.
    let mut per_worksheet: Vec<String> = Vec::new();
    let mut worksheets_clean = 0usize;
    let mut worksheets_with_answers = 0usize;
    let mut read_errors = 0usize;
    let mut disagreed_whole = 0usize;
    let mut disagreed_partial = 0usize;

    for path in files {
        // Relative to the searched root, not the bare file name: the mechanics
        // corpus ships the same worksheets under two edition directories.
        let name = label_for(path, roots);
        let w = match read(path) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("{name}: {e}");
                read_errors += 1;
                continue;
            }
        };
        let (emitted, report) = oracle::check(&w);
        // A worksheet with an untranslated construct can still evaluate every
        // line, because a variable the construct would have updated keeps the
        // value it had before. The answer is then wrong rather than missing, and
        // counting that alongside a disagreement in a *fully* translated
        // worksheet would blame the engine for a gap in the importer.
        let whole = !emitted
            .notes
            .iter()
            .any(|n| n.kind == NoteKind::Unsupported);

        for note in &emitted.notes {
            *notes.entry(note.kind).or_default() += 1;
            if note.kind == NoteKind::Unsupported {
                *unsupported.entry(shorten(&note.detail)).or_default() += 1;
            }
        }

        {
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for check in &report.checks {
                *counts.entry(label(&check.verdict)).or_default() += 1;
            }
            let unsupported_here = emitted
                .notes
                .iter()
                .filter(|n| n.kind == NoteKind::Unsupported)
                .count();
            // Counts alone are not enough. A deliberate break of `norm` — making
            // it return the sum of squares instead of its root — left every count
            // in this corpus untouched, because sixteen of the answers it feeds
            // are *angles*, and an angle does not change when the vector it came
            // from is scaled. A gate that only counts verdicts would have passed
            // a broken engine, so the computed values go in too.
            let digest = digest_of(&report);
            // And the values are not enough either, for the same reason one step
            // over: a worksheet's *text* can change without any answer moving.
            // The commit that made a definition free in `x` into a function of
            // `x` rewrote four lines in three wiki worksheets — turning broken
            // live definitions into `fn` — and every count and every value here
            // stayed exactly as it was. The emitted source is what the import
            // actually produces, so it is what the gate has to hold.
            let source = source_digest(&emitted.source);
            per_worksheet.push(format!(
                "{name}\tanswers={} agreed={} disagreed={} unevaluated={} unsupported={unsupported_here} values={digest} source={source}",
                report.checks.len(),
                counts.get(label(&Verdict::Agreed)).copied().unwrap_or(0),
                counts.get(label(&Verdict::Disagreed)).copied().unwrap_or(0),
                report.checks.len()
                    - counts.get(label(&Verdict::Agreed)).copied().unwrap_or(0)
                    - counts.get(label(&Verdict::Disagreed)).copied().unwrap_or(0),
            ));
        }

        if report.checks.is_empty() {
            continue;
        }
        worksheets_with_answers += 1;
        let mut clean = true;
        for check in &report.checks {
            *totals.entry(label(&check.verdict).into()).or_default() += 1;
            match check.verdict {
                Verdict::Disagreed => {
                    clean = false;
                    if whole {
                        disagreed_whole += 1;
                    } else {
                        disagreed_partial += 1;
                    }
                    if disagreements.len() < 40 {
                        disagreements.push(format!(
                            "    {name}:{} computed {:?}, SMath stored {:?} {}",
                            check.line,
                            check.computed.unwrap_or(f64::NAN),
                            check.expected.unwrap_or(f64::NAN),
                            check.detail
                        ));
                    }
                }
                Verdict::LineFailed => {
                    clean = false;
                    *failures.entry(shorten(&check.detail)).or_default() += 1;
                }
                Verdict::Agreed => {}
                _ => clean = false,
            }
        }
        if clean {
            worksheets_clean += 1;
        }
    }

    let agreed = *totals.get("agreed").unwrap_or(&0);
    let disagreed = *totals.get("disagreed").unwrap_or(&0);
    let comparable = agreed + disagreed;
    let total: usize = totals.values().sum();

    println!("SMath import — checked against the answers SMath stored");
    println!("======================================================\n");
    println!("{} worksheets, {read_errors} unreadable", files.len());
    println!("{total} stored answers in {worksheets_with_answers} worksheets\n");

    for (k, n) in &totals {
        println!("    {n:>5}  {k}");
    }
    if comparable > 0 {
        println!(
            "\n    {agreed} of {comparable} comparable answers agree ({:.1}%)",
            100.0 * agreed as f64 / comparable as f64
        );
        println!(
            "    {} of {} answers could be compared at all ({:.1}%)",
            comparable,
            total,
            100.0 * comparable as f64 / total as f64
        );
    }
    println!("    {worksheets_clean} of {worksheets_with_answers} worksheets check out completely");
    println!(
        "\n    of the {disagreed} disagreements, {disagreed_whole} are in a worksheet that\n    \
         translated completely and {disagreed_partial} in one that did not — where a dropped\n    \
         construct leaves a stale value that still evaluates"
    );

    if !notes.is_empty() {
        println!("\nImport notes");
        for (k, n) in &notes {
            println!("    {n:>5}  {k:?}");
        }
    }

    if !unsupported.is_empty() {
        println!("\nWhat could not be translated, by how often");
        let mut v: Vec<_> = unsupported.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (what, n) in v.iter().take(20) {
            println!("    {n:>5}  {what}");
        }
    }

    if !failures.is_empty() {
        println!("\nWhy a line with a stored answer failed to evaluate");
        let mut v: Vec<_> = failures.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (why, n) in v.iter().take(15) {
            println!("    {n:>5}  {why}");
        }
    }

    if !disagreements.is_empty() {
        println!("\nDisagreements — where Nomo and SMath both answered and differ");
        for d in &disagreements {
            println!("{d}");
        }
    }

    // The report always exits zero. Every number in it is a measurement of work
    // still to do, not a regression, and a tool that fails the build for having
    // measured something honestly would just stop being run.
    //
    // The **baseline** is the opposite, and that is the point: it says nothing
    // about how good the import is and everything about whether it changed.
    match baseline {
        None => ExitCode::SUCCESS,
        Some(path) => compare_baseline(path, &per_worksheet, write),
    }
}

/// Compare this run against a committed per-worksheet baseline.
///
/// The same discipline as the golden-file suite (§9): a change to the importer
/// or the engine that moves any worksheet's result fails until the baseline is
/// regenerated, so the behavioural change shows up in the diff beside the code
/// that caused it. The corpora live outside the repository, so this runs where
/// they are present rather than in CI.
fn compare_baseline(path: &Path, lines: &[String], write: bool) -> ExitCode {
    let mut sorted: Vec<&String> = lines.iter().collect();
    sorted.sort();
    let mut current = String::new();
    for l in sorted {
        current.push_str(l);
        current.push('\n');
    }

    if write {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        return match std::fs::write(path, &current) {
            Ok(()) => {
                println!(
                    "\nwrote baseline {} ({} worksheets)",
                    path.display(),
                    lines.len()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("smath-import: {}: {e}", path.display());
                ExitCode::FAILURE
            }
        };
    }

    let Ok(expected) = std::fs::read_to_string(path) else {
        eprintln!(
            "\nsmath-import: no baseline at {} — run again with --write to create it",
            path.display()
        );
        return ExitCode::FAILURE;
    };
    if expected == current {
        println!("\nok: {} worksheets match the baseline", lines.len());
        return ExitCode::SUCCESS;
    }

    // Only the worksheets that moved, because a corpus-wide diff of 60 lines
    // hides the two that matter.
    let before: BTreeMap<&str, &str> = expected.lines().filter_map(split_line).collect();
    let after: BTreeMap<&str, &str> = current.lines().filter_map(split_line).collect();
    println!("\nBASELINE CHANGED");
    for (name, now) in &after {
        match before.get(name) {
            Some(was) if was == now => {}
            Some(was) => println!("  ~ {name}\n      was {was}\n      now {now}"),
            None => println!("  + {name}  {now}"),
        }
    }
    for name in before.keys() {
        if !after.contains_key(name) {
            println!("  - {name}");
        }
    }
    println!("\nIf this was intended, re-run with --write and commit the baseline\nalongside the change.");
    ExitCode::FAILURE
}

fn split_line(l: &str) -> Option<(&str, &str)> {
    l.split_once('\t')
}

/// A digest of every value this worksheet computed, in line order.
///
/// FNV-1a over the bit patterns: short enough to sit on one line, and sensitive
/// to a change in any answer's last bit — which is the property the golden-file
/// suite relies on too, and the reason the engine is required to be
/// deterministic in the first place.
fn digest_of(report: &oracle::Report) -> String {
    let mut h = FNV_OFFSET;
    for c in &report.checks {
        h = fnv(&(c.line as u64).to_le_bytes(), h);
        // NaN has many bit patterns and comparing them would make the digest
        // depend on which one arrived; every NaN is the same answer here.
        h = match c.computed {
            Some(v) if v.is_nan() => fnv(b"nan", h),
            Some(v) => fnv(&v.to_bits().to_le_bytes(), h),
            None => fnv(b"-", h),
        };
    }
    format!("{h:016x}")
}

/// A digest of the `.nomo` source this worksheet imported to.
///
/// Every line of it, markers and base64 trailer included: the question this
/// answers is "did the import change", and an import that moved a figure or
/// reworded a marker changed. What it is *not* is a judgement — a source digest
/// says nothing about whether the new text is better, only that the diff has to
/// be looked at and the baseline regenerated deliberately.
fn source_digest(source: &str) -> String {
    format!("{:016x}", fnv(source.as_bytes(), FNV_OFFSET))
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a. Enough for a baseline: this detects change, it does not resist an
/// adversary, and nothing here is adversarial.
fn fnv(bytes: &[u8], mut h: u64) -> u64 {
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

fn label(v: &Verdict) -> &'static str {
    match v {
        Verdict::Agreed => "agreed",
        Verdict::Disagreed => "disagreed",
        Verdict::LineFailed => "line did not evaluate",
        Verdict::AnswerUnreadable => "stored answer unreadable",
        Verdict::ShapeDiffers => "answer and result are different shapes",
    }
}

/// Collapse a message down to its shape so that a hundred variations of "unknown
/// name `x`" group into one line rather than a hundred.
fn shorten(detail: &str) -> String {
    let mut out = String::new();
    let mut in_quotes = false;
    for c in detail.chars() {
        match c {
            '`' if !in_quotes => {
                in_quotes = true;
                out.push_str("`…");
            }
            '`' => {
                in_quotes = false;
                out.push('`');
            }
            _ if in_quotes => {}
            _ => out.push(c),
        }
    }
    out
}

fn read(path: &Path) -> Result<nomo_smath::Worksheet, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    nomo_smath::read(&bytes).map_err(|e| e.to_string())
}

/// A name for a worksheet that stays unique across the directories searched.
fn label_for(path: &Path, roots: &[String]) -> String {
    for root in roots {
        let root = Path::new(root);
        if root.is_dir() {
            if let Ok(rest) = path.strip_prefix(root) {
                return rest.to_string_lossy().into_owned();
            }
        }
    }
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn collect(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            out.append(&mut collect(&path)?);
        } else if path.extension().is_some_and(|e| e == "sm") {
            out.push(path);
        }
    }
    Ok(out)
}
