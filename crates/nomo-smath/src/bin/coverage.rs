//! Read every `.sm` worksheet given and report what the importer cannot handle.
//!
//! ```text
//! smath-coverage corpora/nomo-corpus/sm
//! ```
//!
//! Directories are walked recursively: the wiki corpus is one flat directory,
//! the mechanics corpus is a repository with a directory per edition.
//! Everything is read in sorted order so that two runs over the same input
//! produce the same report.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nomo_smath::coverage::Coverage;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!(
            "smath-coverage — what an SMath corpus contains that the importer cannot yet read\n\n\
             USAGE:\n    smath-coverage <file.sm | directory>...\n"
        );
        return ExitCode::FAILURE;
    }

    let mut files = Vec::new();
    for arg in &args {
        let path = PathBuf::from(arg);
        if path.is_dir() {
            match collect(&path) {
                Ok(mut found) => files.append(&mut found),
                Err(e) => {
                    eprintln!("smath-coverage: {arg}: {e}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            files.push(path);
        }
    }
    files.sort();

    if files.is_empty() {
        eprintln!("smath-coverage: no .sm files found");
        return ExitCode::FAILURE;
    }

    let mut coverage = Coverage::default();
    for path in &files {
        // The path relative to the root it was found under, not the bare
        // file name: the mechanics corpus ships the same 29 worksheets in two
        // edition directories, so bare names collide and any per-file tally
        // silently halves.
        let name = label_for(path, &args);
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                coverage.failures.push((name, e.to_string()));
                continue;
            }
        };
        match nomo_smath::read(&bytes) {
            Ok(w) => coverage.add(&name, &w),
            Err(e) => coverage.failures.push((name, e.to_string())),
        }
    }

    print!("{coverage}");

    // A file that could not be read at all is a failure of the tool; a construct
    // it read but cannot translate is the report doing its job. Only the first
    // is worth a non-zero exit.
    if coverage.failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
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
