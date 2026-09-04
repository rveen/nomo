# Nomo

An engineering worksheet application: text syntax, first-class units, and results
that are **bit-reproducible across machines**.

The engine is Rust compiled to WebAssembly and runs entirely in the browser. Any
backend stores and shares documents; it never computes.

> **Status: the ten-phase plan is complete.** Worksheets parse, evaluate in
> dependency order with full dimensional analysis, recompute incrementally, and
> render as worked-through steps in text or self-contained HTML. A byte-exact
> golden-file suite guards all of it, and the whole corpus renders identically
> byte for byte natively and under WebAssembly — the claim the numeric model
> rests on, tested rather than argued. The browser editor does live
> recalculation, highlighting driven by the engine rather than by a grammar,
> opening and saving files, and it works offline. `docs/STATUS.md` records what
> is worth doing next. See
> [`docs/design-note.md`](docs/design-note.md) for the architecture and the
> reasoning behind each decision, and [`docs/language.md`](docs/language.md) for
> the language as it stands.

## Why

To replace SMath Studio with something open, Linux-native and web-based. The two
existing open alternatives were evaluated and rejected for specific reasons
recorded in the design note: CalcpadCE computes server-side and cannot be hosted
on its current branch, and EngineeringPaper.xyz carries a dependency graph only
its author can reproduce.

## Design commitments

These are the decisions that are expensive to reverse, so they are settled first.

- **Bit-reproducible arithmetic.** IEEE 754 binary64 with a `libm` compiled into
  the artifact rather than the host's. WebAssembly already pins everything else:
  no x87 excess precision, no implicit fused multiply-add, and general float
  arithmetic is absent from the specification's list of nondeterminism sources.
  Transcendentals were the one remaining hole, and vendoring closes it. The
  golden-file suite therefore compares **bit-exactly, with no tolerance**.
- **Evaluation returns a trace, not a value.** A worksheet's whole purpose is to
  show its work — `V = π·r²·h = π·(5 cm)²·(12 cm) = 0.942 dm³` — which is
  impossible to render from a bare result.
- **One grammar, one syntax tree.** Diagnostics, formatting and highlighting all
  consume the parser's output. Never a second implementation.
- **Units live on values**, as an exponent vector over base dimensions, so
  arithmetic enforces dimensional consistency rather than a separate pass that
  can disagree with it.
- **The worksheet is a dependency graph**, so recalculation is incremental and
  cycles are an error rather than a hang.

## Getting it

A tagged release publishes a command-line binary for linux-x86_64,
linux-aarch64 and macos-aarch64, the WebAssembly module, and `SHA256SUMS.txt`
covering all of them. The editor and the worked examples deploy to GitHub Pages
from the same workflow.

Each binary is built on a runner that owns its architecture — no
cross-compilation — and **published only after passing the golden suite on the
machine that built it**. The module is published only after being shown to agree
with a native build byte for byte. That is the point of listing its hash: the
determinism claim is then checkable by whoever downloads it rather than only by
whoever built it.

```bash
tar -xzf nomo-v0.2.0-linux-x86_64.tar.gz
./nomo version
./nomo check my-worksheet.nomo
```

Building from source is the section below, and needs nothing but a Rust
toolchain.

## Build

```bash
cargo test --workspace
cargo run -p nomo-cli -- render examples/beam.nomo
cargo run -p nomo-cli -- html   examples/beam.nomo   # standalone, offline
cargo run -p nomo-cli -- check  examples/cylinder.nomo  # evaluate, report only diagnostics

# Enforce the determinism invariants
./scripts/check-no-host-math.sh
```

## The golden-file suite

```bash
cargo run -p nomo-cli -- test           # compare; CI runs this
cargo run -p nomo-cli -- test --write   # regenerate, then read the diff
```

Every worksheet under `examples/` is rendered to a snapshot in `tests/golden/`
and compared **byte for byte**. There is no numeric tolerance, because the
engine is meant to produce the same bits on every machine: a last-digit
difference is a bug, not noise. Changing output is fine — regenerate and commit,
and the change to behaviour lands in the same diff as the change to code.

A snapshot holds the whole rendered trace, not just final values, so
substitution, unit choice and number formatting are pinned alongside the
arithmetic. It also records every result in base SI units at full round-trip
precision: the rendered columns show six significant figures and would hide
exactly the last-bit drift the suite exists to catch.

## Cross-target determinism

```bash
./scripts/compare-targets.sh     # needs node as a WebAssembly engine
./scripts/compare-arch.sh        # needs qemu-user and llvm-objdump
```

The first renders the corpus through the native build and through the
WebAssembly build and requires the two to be **byte-identical**, transcendentals
included. This is the verification the numeric model exists for.

The second asks the same question across instruction sets: it cross-builds to
aarch64 and renders the corpus under emulation, against the snapshots committed
from x86-64. All nine agree. Real ARM hardware is covered by the `arm64` job in
CI, which runs the suite natively rather than emulated.

Two gates run on the artifact itself rather than on the build that produced it:
it must declare **no imports at all** — so it cannot call the host for anything,
which is stronger than "no maths imports" — and it must enable no SIMD, read from
the `target_features` section LLVM writes into the module. Relaxed SIMD's fused
multiply-add is nondeterministic by specification, which is exactly what this
design forbids.

The aarch64 artifact is read the same way, for the same reason: it must contain
**no fused multiply-add**. aarch64 has FMA in its base instruction set, so `a*b +
c` could be contracted into one instruction that rounds once where the source
rounds twice. Building with contraction forced puts 209 of them in the binary and
the corpus stops matching, so this gate guards a measured difference rather than
a hypothetical one.

Node supplies the WebAssembly engine and nothing else. No package is installed;
everything under `scripts/` is dependency-free, because those scripts are part of
the evidence for the claim.

## The editor

```bash
./scripts/build-web.sh          # build, then check it in headless Chrome
cd web && node build.mjs --serve   # watch and serve on :8000
```

Static files and nothing else — HTML, CSS, one bundle, one `.wasm`. No backend,
no network traffic after load, and no worksheet leaves the tab.

**Syntax highlighting comes from the engine, not from a CodeMirror grammar.** The
engine classifies every token and the front end turns that into colour; it
decides nothing. A grammar in TypeScript would be a second description of the
language, and the first unit the engine learned and the grammar did not, the
editor would colour a worksheet differently from how it computes it — the split
CalcpadCE has and the design note calls a permanent liability. It also does what
a grammar cannot: `m` is a unit or a variable depending on whether *this*
worksheet bound it, which is knowable only after evaluation.

Editing one line recalculates that statement and its dependents, through the
dependency graph, and the status bar shows the count.

**Files, drafts and offline.** Open and save real `.nomo` files — writing back
to the file you opened where the browser allows it, downloading where it does
not, since Firefox and Safari have no File System Access API and the button says
which one you are getting. Saving stamps the `' nomo 1` version pragma, and the
engine decides how, because the version number belongs to the format rather than
to the front end. Whatever is in the editor is kept in IndexedDB, so closing the
tab does not lose an hour; the file on disk is still the document. A service
worker caches the five files that make up the application, so it runs with the
network off — not as a degraded mode, since nothing computes anywhere else.

Printing gives the worksheet without the editor. Both that and offline are
checked in CI, in a real browser, rather than assumed.

## Repository layout

| Path | What |
|---|---|
| `crates/nomo-core` | The engine. No I/O, no clock, no threads — compiles to `wasm32` unchanged. |
| `crates/nomo-cli` | Command-line front end. All filesystem access lives here. |
| `crates/nomo-wasm` | The WebAssembly boundary. A plain C ABI, so the artifact imports nothing. |
| `web/` | The browser editor. Static files; it decides nothing about the language. |
| `scripts/` | Determinism and browser gates. Dependency-free on purpose; they are the evidence. |
| `docs/design-note.md` | Architecture, decisions, and the evidence for them. |
| `docs/language.md` | The language specification. Grows with each phase. |
| `examples/` | Worksheets, and the corpus the golden-file suite runs on. |
| `corpora/` | The SMath corpora — fetched, not committed. See THIRD-PARTY.md. |
| `tests/golden/` | Expected output, one `.snap` per worksheet. Regenerated, never edited. |

## Licence

MIT — see [LICENSE](LICENSE).

Unless you state otherwise, any contribution you intentionally submit for
inclusion in this work is licensed as above, with no additional terms or
conditions.

Everything in this repository is the project's own work. The SMath worksheet
corpora the importer is measured against are third-party documents and are
**not** redistributed here — `./scripts/fetch-corpora.sh` obtains them and
verifies them against committed hashes. See [THIRD-PARTY.md](THIRD-PARTY.md) for
those, for the two reference codebases, and for the bundled build dependencies.
