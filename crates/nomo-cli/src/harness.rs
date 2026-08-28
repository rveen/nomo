//! The golden-file regression suite.
//!
//! Every worksheet under `examples/` is rendered to a snapshot and compared with
//! the expected file committed beside it in `tests/golden/`. CI runs the
//! comparison, so a change that alters rendered output cannot merge until the
//! expected files are regenerated — which puts the behavioural change in the
//! same diff as the code change that caused it. The idea is taken from
//! CalcpadCE's `compare_renderings.py`; see `docs/design-note.md` §9.
//!
//! Two deliberate differences from that script:
//!
//! * **Comparison is byte-exact.** CalcpadCE has to compare decimals within a
//!   tolerance because it cannot promise the same answer on two machines — its
//!   own comment blames AVX2. This engine can promise that, so a last-digit
//!   difference is not noise to be absorbed, it is the bug the suite is for. No
//!   tolerance, no reconciliation, no denylist for "large precision errors".
//! * **The whole trace is snapshotted**, not the final values, so substitution,
//!   unit choice and number formatting are pinned too.
//!
//! Paths are relative to the working directory, so this is run from the
//! repository root; `--examples` and `--golden` override that.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::diff;

const GUIDANCE: &str = "\
Rendered output has changed. If that was intended, run

    cargo run -p nomo-cli -- test --write

and commit the updated snapshots so the change to behaviour appears in the diff
beside the change to code. If it was not intended, this is a real regression:
the comparison is byte-exact because the engine is supposed to be deterministic.";

pub fn run(args: &[String]) -> ExitCode {
    let mut write = false;
    let mut examples = PathBuf::from("examples");
    let mut golden = PathBuf::from("tests/golden");

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--write" => write = true,
            "--examples" => match it.next() {
                Some(v) => examples = PathBuf::from(v),
                None => return fail("--examples needs a directory"),
            },
            "--golden" => match it.next() {
                Some(v) => golden = PathBuf::from(v),
                None => return fail("--golden needs a directory"),
            },
            other => return fail(&format!("unknown option `{other}`")),
        }
    }

    if !examples.is_dir() {
        return fail(&format!(
            "no worksheet directory at {} — run from the repository root, or pass --examples",
            examples.display()
        ));
    }

    let mut worksheets = Vec::new();
    if let Err(e) = collect(&examples, &mut worksheets) {
        return fail(&format!("cannot read {}: {e}", examples.display()));
    }
    // Sorted so the report reads the same everywhere; directory order is not
    // defined and differs between filesystems.
    worksheets.sort();

    if worksheets.is_empty() {
        return fail(&format!("no .nomo files under {}", examples.display()));
    }

    if write {
        write_all(&worksheets, &examples, &golden)
    } else {
        verify_all(&worksheets, &examples, &golden)
    }
}

fn verify_all(worksheets: &[PathBuf], examples: &Path, golden: &Path) -> ExitCode {
    let mut mismatched = Vec::new();
    let mut missing = Vec::new();
    let mut unreadable = Vec::new();

    for source_path in worksheets {
        let expected_path = golden_path(source_path, examples, golden);
        let rendered = match snapshot_of(source_path) {
            Ok(rendered) => rendered,
            Err(e) => {
                unreadable.push(format!("{}: {e}", source_path.display()));
                continue;
            }
        };

        match std::fs::read(&expected_path) {
            Err(_) => missing.push(expected_path),
            Ok(expected_bytes) => {
                if expected_bytes != rendered.as_bytes() {
                    mismatched.push((expected_path, expected_bytes, rendered));
                }
            }
        }
    }

    let orphans = find_orphans(worksheets, examples, golden);

    if mismatched.is_empty() && missing.is_empty() && unreadable.is_empty() && orphans.is_empty() {
        println!("ok: {} worksheets match their snapshots", worksheets.len());
        return ExitCode::SUCCESS;
    }

    for message in &unreadable {
        eprintln!("error: {message}");
    }
    for path in &missing {
        eprintln!(
            "error: no snapshot at {} — run with --write",
            path.display()
        );
    }
    for path in &orphans {
        eprintln!(
            "error: {} has no worksheet — delete it, or restore the example",
            path.display()
        );
    }

    for (path, expected_bytes, rendered) in &mismatched {
        let label = path.display().to_string();
        println!();
        match String::from_utf8(expected_bytes.clone()) {
            Ok(expected) => match diff::unified(&expected, rendered, &label) {
                Some(d) => print!("{d}"),
                None => println!(
                    "{label}: differs only in line endings or a trailing newline.\n\
                     Check that .gitattributes is keeping snapshots at LF."
                ),
            },
            Err(_) => println!("{label}: committed snapshot is not valid UTF-8"),
        }
    }

    if !mismatched.is_empty() {
        println!("\n{GUIDANCE}");
    }
    println!(
        "\n{} worksheet(s): {} matched, {} differed, {} missing, {} orphaned",
        worksheets.len(),
        worksheets.len() - mismatched.len() - missing.len() - unreadable.len(),
        mismatched.len(),
        missing.len(),
        orphans.len()
    );
    ExitCode::FAILURE
}

fn write_all(worksheets: &[PathBuf], examples: &Path, golden: &Path) -> ExitCode {
    let (mut created, mut updated, mut unchanged) = (0, 0, 0);
    let mut ok = true;

    for source_path in worksheets {
        let expected_path = golden_path(source_path, examples, golden);
        let rendered = match snapshot_of(source_path) {
            Ok(rendered) => rendered,
            Err(e) => {
                eprintln!("error: {}: {e}", source_path.display());
                ok = false;
                continue;
            }
        };

        let existing = std::fs::read(&expected_path).ok();
        if existing.as_deref() == Some(rendered.as_bytes()) {
            unchanged += 1;
            continue;
        }

        if let Some(parent) = expected_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("error: cannot create {}: {e}", parent.display());
                ok = false;
                continue;
            }
        }
        if let Err(e) = std::fs::write(&expected_path, &rendered) {
            eprintln!("error: cannot write {}: {e}", expected_path.display());
            ok = false;
            continue;
        }

        if existing.is_some() {
            updated += 1;
            println!("updated {}", expected_path.display());
        } else {
            created += 1;
            println!("created {}", expected_path.display());
        }
    }

    // Reported, never deleted: removing a committed file is not something a
    // regeneration command should do behind the author's back.
    for path in find_orphans(worksheets, examples, golden) {
        println!("orphaned {} — no matching worksheet", path.display());
        ok = false;
    }

    println!("{created} created, {updated} updated, {unchanged} unchanged");
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Read a worksheet and render its snapshot.
fn snapshot_of(path: &Path) -> Result<String, std::io::Error> {
    let source = std::fs::read_to_string(path)?;
    let name = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    Ok(nomo_core::snapshot(&name, &source))
}

/// Where a worksheet's snapshot belongs, mirroring the layout under `examples/`
/// so that subdirectories stay distinguishable.
fn golden_path(source: &Path, examples: &Path, golden: &Path) -> PathBuf {
    let relative = source.strip_prefix(examples).unwrap_or(source);
    golden
        .join(relative)
        .with_extension(nomo_core::golden::EXTENSION)
}

/// Committed snapshots with no worksheet left to produce them.
///
/// Without this, deleting an example leaves its expected output behind for good:
/// nothing regenerates it and nothing compares it, so it rots silently into a
/// record of behaviour that no longer exists.
fn find_orphans(worksheets: &[PathBuf], examples: &Path, golden: &Path) -> Vec<PathBuf> {
    let expected: Vec<PathBuf> = worksheets
        .iter()
        .map(|w| golden_path(w, examples, golden))
        .collect();

    let mut found = Vec::new();
    if collect_ext(golden, nomo_core::golden::EXTENSION, &mut found).is_err() {
        return Vec::new();
    }
    found.retain(|path| !expected.contains(path));
    found.sort();
    found
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    collect_ext(dir, "nomo", out)
}

fn collect_ext(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_ext(&path, ext, out)?;
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
    Ok(())
}

fn fail(message: &str) -> ExitCode {
    let _ = writeln!(std::io::stderr(), "nomo test: {message}");
    ExitCode::FAILURE
}
