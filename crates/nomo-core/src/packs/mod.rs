//! Packs: definitions a worksheet can bring in by name.
//!
//! ```nomo
//! use steel
//! sigma_allow = 0.6*Fy_A992
//! ```
//!
//! Twelve worksheets must not each redeclare steel. What they need is a curated
//! set of definitions with one place to correct them, and the interesting part
//! is not that idea but where the definitions come from.
//!
//! # Compiled in, not fetched and not read from disk
//!
//! Three routes were available and two of them are closed:
//!
//! * **Reading a file beside the worksheet** cannot work in the browser, which
//!   is the target this project builds for first. A browser opens a *file*, not
//!   a directory: the user picks one worksheet and the page never sees what sits
//!   next to it. An include that works on the command line and not in the editor
//!   would be a language feature half the users cannot use.
//! * **Fetching one over the network** would put a round trip inside a
//!   determinism claim and break the offline promise the service worker exists
//!   to keep. It would also mean a worksheet's answer depends on what a server
//!   said today.
//! * **Compiling them into the engine** costs a rebuild to change a constant and
//!   nothing else. The pack is in the artifact, so a worksheet using one gives
//!   the same answer on every machine and with the network off, which is the
//!   whole architecture in one sentence.
//!
//! The third is what this is. `nomo-core` still does no I/O: `include_str!` is
//! read by the compiler, not by the engine, and the guard in
//! `scripts/check-no-host-math.sh` is unaffected.
//!
//! # What a pack is
//!
//! An ordinary `.nomo` worksheet, in this directory, holding `global`
//! definitions. Global rather than positional so that `use` may sit anywhere in
//! a worksheet and the names are visible above it as well as below — the same
//! rule, and the same reason, as §6's globals.
//!
//! It is spliced into the syntax tree where the `use` statement stands, and the
//! statements it brings are hidden from the rendered output: a worksheet that
//! shows its work should show *its* work, not fourteen constants nobody typed.
//! They are ordinary statements in every other respect, so the dependency graph,
//! incremental recalculation and diagnostics need to know nothing about packs.
//!
//! # Naming
//!
//! A pack's names are its own and are not qualified: `use steel` brings
//! `E_steel`, not `steel.E`. Qualified names would need a resolution rule, a
//! lexer that admits `.` in an identifier, and a decision about what happens
//! when a worksheet shadows one — three new things to be wrong about. The packs
//! here suffix instead, which needs nothing and reads the way an engineer writes
//! anyway.

/// One pack: its name, and the worksheet it stands for.
pub struct Pack {
    pub name: &'static str,
    pub source: &'static str,
    /// One line, for `nomo packs` and for anything else that lists them.
    pub summary: &'static str,
}

/// Every pack this build carries, in name order.
pub const PACKS: &[Pack] = &[
    Pack {
        name: "constants",
        source: include_str!("constants.nomo"),
        summary: "physical constants in SI, the 2019 definitions among them",
    },
    Pack {
        name: "steel",
        source: include_str!("steel.nomo"),
        summary: "structural steel: elastic constants, density, and yield by grade",
    },
];

/// The pack called `name`, if there is one.
pub fn find(name: &str) -> Option<&'static Pack> {
    PACKS.iter().find(|p| p.name == name)
}

/// Every pack name, for an error message that suggests what was meant.
pub fn names() -> Vec<&'static str> {
    PACKS.iter().map(|p| p.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pack_is_a_worksheet_that_evaluates() {
        // A pack that does not evaluate would fail inside every worksheet that
        // used it, pointing at a `use` line rather than at itself. They are
        // checked here instead, where the message names the pack.
        for pack in PACKS {
            let sheet = crate::Sheet::new(pack.source);
            assert!(
                !sheet.has_errors(),
                "pack `{}` does not evaluate: {:?}",
                pack.name,
                sheet.diagnostics()
            );
        }
    }

    #[test]
    fn a_pack_binds_only_globals() {
        // Positional definitions would be invisible above the `use` line, which
        // would make where a pack is written change what it does.
        for pack in PACKS {
            for stmt in &crate::parse(pack.source).ast.stmts {
                assert!(
                    matches!(
                        stmt,
                        crate::ast::Stmt::GlobalDef { .. }
                            | crate::ast::Stmt::Comment { .. }
                            | crate::ast::Stmt::UnitDecl { .. }
                    ),
                    "pack `{}` holds a statement that is not a global: {stmt:?}",
                    pack.name
                );
            }
        }
    }

    #[test]
    fn the_list_is_sorted_and_named_once() {
        let listed = names();
        let mut tidy = listed.clone();
        tidy.sort_unstable();
        assert_eq!(tidy, listed, "PACKS should be in name order");
        tidy.dedup();
        assert_eq!(tidy.len(), listed.len(), "two packs share a name");
    }
}
