# CLAUDE.md

Guidance for Claude Code (claude.ai/code) working in this repository.

## What this is

**Nomo** — a web-based engineering worksheet application meant to replace SMath
Studio. Text syntax (not visual math entry), units first-class with real
dimensional analysis, engine in Rust compiled to WASM running entirely
client-side, and any backend restricted to document storage — never computation.
Numeric model is IEEE 754 binary64 with a math library compiled into the module
so results are bit-reproducible across machines. **No CAS.** `.nomo` is the file
extension.

`docs/design-note.md` is the architecture and the evidence behind every decision;
read it first for any question about why something is the way it is. §12
separates what was verified from what is still assumed. `docs/STATUS.md` is the
handoff snapshot — where the work stands and how to verify it. `docs/language.md`
is the language reference, and the worksheets under `examples/` are drawn from it.
`docs/smath.md` is the SMath import how-to: how to run it, what translates, and
what is refused and why. `docs/roadmap.md` is what to build next and in what
order; STATUS's "What is worth doing next" holds the evidence behind that
ranking.

## Layout

| Directory | What it is |
|---|---|
| `crates/nomo-core/` | The engine: lexer, parser, dimensions and units, values, evaluation to a trace, document graph, renderers. No I/O, no host math — see the determinism guard below. |
| `crates/nomo-cli/` | `nomo render`, `html`, `check`, `test`. |
| `crates/nomo-wasm/` | The C-ABI wrapper the browser build loads. |
| `crates/nomo-smath/` | The SMath `.sm` importer: reader, expression reduction, emitter, coverage report, and the stored-answer oracle. |
| `web/` | The browser editor (CodeMirror 6), and `font.mjs`, which subsets the math font `dist/` ships. |
| `tests/golden/` | Byte-exact snapshots of every worksheet under `examples/`. |
| `tests/corpus/` | Per-worksheet baselines for the SMath corpora — the importer's regression gate. |
| `corpora/` | The SMath worksheets the importer is measured against. Third-party, no redistribution terms, **gitignored** — `./scripts/fetch-corpora.sh` obtains them and verifies them against the hashes in `scripts/corpora/`. See THIRD-PARTY.md. |
| `reference/` | Two third-party codebases to learn from. **Gitignored and never committed**; one of them is a gigabyte. See `docs/STATUS.md`. |

## Commands

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
./scripts/check-no-host-math.sh          # determinism guard
cargo run -p nomo-cli -- test           # golden-file suite; --write to accept
./scripts/compare-targets.sh             # native vs WebAssembly, byte-exact
./scripts/compare-arch.sh                # x86-64 vs aarch64 (needs qemu-user)
./scripts/fetch-corpora.sh               # obtain the SMath corpora; --verify to check only
./scripts/fetch-font.sh                  # obtain the math font; --verify to check only
./scripts/check-corpus.sh                # SMath import regression gate; --write to accept
./scripts/build-web.sh                   # front end
```

`docs/STATUS.md` carries the full list, including the importer's coverage and
check binaries.

## Working rules, learned rather than assumed

- **Determinism is the product.** `nomo-core` and `nomo-wasm` must not call
  host math or do I/O; `check-no-host-math.sh` enforces it. Reduction order is
  part of the language, not an implementation detail: nodes are computed as
  `a + i*step` rather than by repeated addition, and an iteration applies one
  step at a time because reassociating would show in the last bits.
- **Every limit is a fixed number, and the tightest target sets it.** Sample
  counts, panel counts, the million-element range cap, `MAX_NEST`, `MAX_DEPTH`,
  `MAX_EVAL_NEST` and `MAX_CALLS` are all counts rather than tolerances, so the
  answer cannot depend on the machine. They also have to be chosen *together*:
  bracket depth and call depth each had a ceiling and the product of the two
  still ran the stack out, which is what `MAX_EVAL_NEST` bounds. `MAX_DEPTH` is 64 because a recursion 200 calls deep *answered*
  natively and *trapped* in WebAssembly — raising one of these is a cross-target
  decision, never a tuning knob.
- **Two gates, and they are different.** The golden suite compares Nomo's own
  output **bit-exactly** — any difference there is a bug. The corpus oracle
  compares against decimal strings another program wrote a decade ago, so a
  tolerance derived from the stored literal is legitimate *there and nowhere
  else*. Never loosen the golden suite.
- **Never implement an SMath construct on a guess.** The design note refuses
  several on this ground and says why each time: `range` with a step, `solve`'s
  range semantics, the `ltle`/`ltlt`/`lele` boundary convention, and the `—`
  operator. A construct that is right most of the time is worse than one that
  reports it cannot tell. Where the corpus *can* settle a meaning, use it — that
  is how `†` (cross product) and `norme` (Euclidean norm) were resolved.
- **SMath itself is on this machine, and it settles what the corpus cannot.**
  SMath Studio 1.5 is at `/opt/smath` as an AppImage (`--appimage-extract`
  unpacks it), and `monodis`/`ikdasm` disassemble its plugins. That is how the
  plot span was settled — design note §8.21 — after two attempts to infer it
  from worksheets disagreed by four orders of magnitude; it also confirmed
  `sys(…)`'s shape operands and turned up axis-limit attributes no worksheet
  uses. Reading the implementation is evidence, not a guess, and it outranks
  inference from files whenever both are available. Note what does *not* work
  here: the GUI aborts on this distribution's pango, and batch export and the
  `-t` self-test are licence-gated.
- **Never silently drop anything on import.** An unsupported construct becomes a
  visible marker in the output and a counted note, so a coverage report can rank
  the work and a human can see the gap.
- **A worksheet is a set of definitions, not a script.** Nothing mutates, so
  there is no loop statement and no indexed assignment; `map`, `iterate` and
  `range` are what real worksheet loops were doing. That last clause is now
  measured rather than asserted: of 105 `for` loops across the corpora, 39
  are an element-wise fill that `map` says exactly, and the importer translates
  them. What is left is recurrences and accumulators, which are folds.
- **Nothing third-party gets committed.** This repository is MIT licensed and
  everything in it is the project's own work. The corpora are fetched and hash-
  verified, never vendored; `reference/` stays gitignored. Two industrial
  worksheets that development used were customer documents and have been removed
  along with everything derived from them — comments that cite what they
  measured say "the converter worksheet" or "the interlock worksheet" and mean
  files that are no longer here. `examples/llc.nomo` and
  `examples/interlock.nomo` are their clean-room replacements. Before adding a
  worksheet, a figure or a fixture, check that it is ours to add.
- Comments explain *why*, and the repo's existing density is the guide. One
  commit per phase, with the reasoning for anything non-obvious in the message.

## The reference codebases

`reference/CalcpadCE/` and `reference/EngineeringPaper.xyz-main/` are two
unrelated third-party projects solving the same problem with different
architectures. They exist **to be learned from, not extended** — design note §10
records what to take from each and what to avoid. Never mix their code,
conventions or tooling into Nomo or into each other. They carry their own nested
`CLAUDE.md` files and skills, which apply only inside their own subtrees.
