//! Tests for the golden-file suite itself.
//!
//! A regression net that silently passes is worse than none, so the failure
//! paths are tested as carefully as the success one: a changed snapshot, a
//! missing one, and one left behind by a deleted worksheet all have to fail
//! loudly. Each test builds a throwaway `examples/` and `tests/golden/` pair and
//! runs the real binary over it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

/// A scratch directory holding an `examples/` tree and a `golden/` tree.
struct Fixture {
    root: PathBuf,
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

impl Fixture {
    fn new() -> Fixture {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("nomo-harness-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("examples")).expect("create examples");
        std::fs::create_dir_all(root.join("golden")).expect("create golden");
        Fixture { root }
    }

    fn worksheet(&self, name: &str, source: &str) -> &Fixture {
        std::fs::write(self.root.join("examples").join(name), source).expect("write worksheet");
        self
    }

    fn golden(&self, name: &str) -> PathBuf {
        self.root.join("golden").join(name)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_nomo"))
            .arg("test")
            .args(args)
            .arg("--examples")
            .arg(self.root.join("examples"))
            .arg("--golden")
            .arg(self.root.join("golden"))
            .output()
            .expect("run nomo test")
    }

    fn verify(&self) -> Output {
        self.run(&[])
    }

    fn write(&self) -> Output {
        self.run(&["--write"])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

const CYLINDER: &str = "r = 5 cm\nh = 12 cm\nV = pi*r^2*h\nV -> dm^3\n";

#[test]
fn write_then_verify_passes() {
    let fx = Fixture::new();
    fx.worksheet("cylinder.nomo", CYLINDER);

    assert!(fx.write().status.success());
    assert!(fx.golden("cylinder.snap").exists());

    let verified = fx.verify();
    assert!(verified.status.success(), "{}", combined(&verified));
    assert!(combined(&verified).contains("1 worksheets match"));
}

#[test]
fn a_missing_snapshot_fails_rather_than_being_created() {
    // Verification must never write. A suite that quietly fills in the file it
    // was supposed to be checking against proves nothing.
    let fx = Fixture::new();
    fx.worksheet("cylinder.nomo", CYLINDER);

    let out = fx.verify();
    assert!(!out.status.success());
    assert!(
        combined(&out).contains("no snapshot at"),
        "{}",
        combined(&out)
    );
    assert!(!fx.golden("cylinder.snap").exists());
}

#[test]
fn a_changed_result_fails_and_shows_the_change() {
    // This is the whole point of the phase: a different number must not slip
    // through, and the diff must name it.
    let fx = Fixture::new();
    fx.worksheet("cylinder.nomo", CYLINDER);
    assert!(fx.write().status.success());

    fx.worksheet(
        "cylinder.nomo",
        "r = 5 cm\nh = 13 cm\nV = pi*r^2*h\nV -> dm^3\n",
    );

    let out = fx.verify();
    assert!(
        !out.status.success(),
        "a changed result must fail the suite"
    );
    let report = combined(&out);
    assert!(report.contains("0.942478"), "old value missing:\n{report}");
    assert!(report.contains("1.02102"), "new value missing:\n{report}");
    assert!(report.contains("1 differed"), "{report}");
}

#[test]
fn the_last_digit_is_not_forgiven() {
    // CalcpadCE has to compare decimals within a tolerance because it cannot
    // promise determinism. This engine can, so last-digit drift is a bug and
    // must fail. Forging it in the snapshot stands in for an engine that drifted.
    let fx = Fixture::new();
    fx.worksheet("cylinder.nomo", CYLINDER);
    assert!(fx.write().status.success());

    let path = fx.golden("cylinder.snap");
    let doctored = std::fs::read_to_string(&path)
        .unwrap()
        .replace("0.942478", "0.942479");
    std::fs::write(&path, doctored).unwrap();

    assert!(
        !fx.verify().status.success(),
        "a one-digit difference must fail; there is no tolerance by design"
    );
}

#[test]
fn write_reports_unchanged_without_touching_the_file() {
    let fx = Fixture::new();
    fx.worksheet("cylinder.nomo", CYLINDER);
    assert!(fx.write().status.success());

    let second = fx.write();
    assert!(second.status.success());
    assert!(combined(&second).contains("0 created, 0 updated, 1 unchanged"));
}

#[test]
fn write_updates_a_stale_snapshot() {
    let fx = Fixture::new();
    fx.worksheet("cylinder.nomo", CYLINDER);
    assert!(fx.write().status.success());

    fx.worksheet(
        "cylinder.nomo",
        "r = 5 cm\nh = 13 cm\nV = pi*r^2*h\nV -> dm^3\n",
    );
    let out = fx.write();
    assert!(out.status.success());
    assert!(combined(&out).contains("0 created, 1 updated"));
    assert!(fx.verify().status.success());
}

#[test]
fn a_snapshot_whose_worksheet_is_gone_is_reported() {
    // Otherwise a deleted example leaves its expected output behind for good:
    // nothing regenerates it and nothing compares it.
    let fx = Fixture::new();
    fx.worksheet("cylinder.nomo", CYLINDER);
    fx.worksheet("spare.nomo", "x = 1\n");
    assert!(fx.write().status.success());

    std::fs::remove_file(fx.root.join("examples").join("spare.nomo")).unwrap();

    let out = fx.verify();
    assert!(!out.status.success());
    assert!(
        combined(&out).contains("has no worksheet"),
        "{}",
        combined(&out)
    );
    // Reported, not deleted.
    assert!(fx.golden("spare.snap").exists());
}

#[test]
fn subdirectories_keep_their_shape() {
    let fx = Fixture::new();
    std::fs::create_dir_all(fx.root.join("examples/structural")).unwrap();
    fx.worksheet("structural/beam.nomo", "L = 30 ft\n");

    assert!(fx.write().status.success());
    assert!(
        fx.golden("structural/beam.snap").exists(),
        "nested worksheets must not collapse into one directory"
    );
    assert!(fx.verify().status.success());
}

#[test]
fn a_worksheet_with_errors_is_still_snapshotted() {
    // Diagnostics are output too, and a change to one is a change a user sees.
    let fx = Fixture::new();
    fx.worksheet("broken.nomo", "x = 1 +\n");

    assert!(fx.write().status.success());
    let snap = std::fs::read_to_string(fx.golden("broken.snap")).unwrap();
    assert!(snap.contains("error[SH004]"), "{snap}");
    assert!(fx.verify().status.success());
}

#[test]
fn an_empty_examples_directory_is_an_error() {
    // Silently passing with nothing to compare is how a suite stops protecting
    // anything without anyone noticing.
    let fx = Fixture::new();
    let out = fx.verify();
    assert!(!out.status.success());
    assert!(
        combined(&out).contains("no .nomo files"),
        "{}",
        combined(&out)
    );
}

#[test]
fn a_missing_examples_directory_says_where_to_run_from() {
    let out = Command::new(env!("CARGO_BIN_EXE_nomo"))
        .args(["test", "--examples", "does-not-exist"])
        .output()
        .expect("run nomo test");
    assert!(!out.status.success());
    assert!(
        combined(&out).contains("repository root"),
        "{}",
        combined(&out)
    );
}

#[test]
fn the_committed_suite_passes_from_the_repository_root() {
    // The one test that checks the real examples rather than a fixture. If the
    // snapshots in this repository are stale, this fails here as well as in CI.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root");

    let out = Command::new(env!("CARGO_BIN_EXE_nomo"))
        .arg("test")
        .current_dir(root)
        .output()
        .expect("run nomo test");

    assert!(out.status.success(), "{}", combined(&out));
}
