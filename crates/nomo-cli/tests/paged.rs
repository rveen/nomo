//! `nomo html --paged`, end to end: the binary, a worksheet on disk, a Folio
//! script beside it, and the document it writes.
//!
//! The script is a stand-in rather than the real build. What is under test is
//! that the command finds it, carries it and its licence, and writes the paged
//! document; whether Folio paginates that document is `scripts/check-paged.mjs`'s
//! question, which needs a browser.

use std::path::PathBuf;
use std::process::{Command, Output};

/// A directory of the test's own, removed when the test is done with it.
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(name: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!("nomo-paged-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch");
    Scratch(dir)
}

fn nomo(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nomo"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run nomo")
}

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

const WORKSHEET: &str = "' # Spring rate\nk = 20 N/mm\nx = 3 mm\nF = k*x\n";

#[test]
fn the_default_engine_is_the_fetched_one() {
    // Laid out as the repository is, so the default path resolves from here.
    let dir = scratch("default");
    let vendor = dir.join("web/vendor/folio");
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(
        vendor.join("folio.polyfill.min.js"),
        "/* the folio stand-in */",
    )
    .unwrap();
    std::fs::write(vendor.join("LICENSE"), "MIT License, the stand-in's").unwrap();
    std::fs::write(dir.join("spring.nomo"), WORKSHEET).unwrap();

    let out = nomo(&dir, &["html", "--paged", "--mathml", "spring.nomo"]);
    assert!(out.status.success(), "{}", combined(&out));
    let doc = std::fs::read_to_string(dir.join("spring.html")).unwrap();

    assert!(doc.contains("/* the folio stand-in */"));
    assert!(doc.contains("<!--\nMIT License, the stand-in's\n-->"));
    assert!(doc.contains("content: \"Nomo\";"));
    assert!(doc.contains("@top-right { content: \"Spring rate\";"));
    // --mathml still reaches the worksheet's markup.
    assert!(doc.contains("<math"));
}

#[test]
fn a_linked_engine_is_not_read_at_all() {
    // Nothing at the default path, and none needed.
    let dir = scratch("linked");
    std::fs::write(dir.join("spring.nomo"), WORKSHEET).unwrap();

    let out = nomo(&dir, &["html", "--folio-url", "../folio.js", "spring.nomo"]);
    assert!(out.status.success(), "{}", combined(&out));
    let doc = std::fs::read_to_string(dir.join("spring.html")).unwrap();
    assert!(doc.contains("<script src=\"../folio.js\"></script>"));
}

#[test]
fn a_missing_engine_says_how_to_get_one() {
    let dir = scratch("missing");
    std::fs::write(dir.join("spring.nomo"), WORKSHEET).unwrap();

    let out = nomo(&dir, &["html", "--paged", "spring.nomo"]);
    assert!(!out.status.success());
    assert!(
        combined(&out).contains("fetch-folio.sh"),
        "{}",
        combined(&out)
    );
    // Refused before anything was written, not after a page without its engine.
    assert!(!dir.join("spring.html").exists());
}

#[test]
fn without_paged_the_page_is_the_plain_one() {
    let dir = scratch("plain");
    std::fs::write(dir.join("spring.nomo"), WORKSHEET).unwrap();

    let out = nomo(&dir, &["html", "spring.nomo"]);
    assert!(out.status.success(), "{}", combined(&out));
    let doc = std::fs::read_to_string(dir.join("spring.html")).unwrap();
    assert!(!doc.contains("@page"));
    assert!(!doc.contains("PagedConfig"));
}
