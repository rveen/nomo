# Changelog

## 0.2.0 — what a plot is about

A worksheet could draw a curve; it could not say what the curve was *of*. This
release adds the two words a reader looks for first, and fixes what the work
uncovered.

### The language

- **`axis x "Frequency"`** names what an axis measures, and **`label "Gain",
  "Phase"`** names the curves in a legend. The unit stays at the end of the
  axis rather than merging into the label: `Frequency` and `Hz` are questions a
  reader asks at different moments, and merging them would mean rewriting the
  label whenever the worksheet changed units. After `axis y`, the comma decides
  — two expressions are limits, one is a label — which is syntactic rather than
  by the kind of the expression, so a label can be a name holding a string.
- A plot that names no axis is drawn exactly as it was: the label takes a row of
  its own, so only two of 29 snapshots moved, and both because their worksheets
  now use the feature.

This was the most-wanted plot feature by corpus ranking — 88 `description` calls
across the SMath worksheets, every one of them an axis label or a trace name.
Design note §8.46. The importer still refuses `XYPlot'Labels'XLabel` and
`Traces#n'Name`; the language can express them now, and what stands in the way
is three measurements against the corpora rather than a missing construct.

### Fixed

- **Two leaks older than the feature that found them**, both because
  `Sheet::update` never reset the environment even when it re-evaluated every
  statement: a deleted definition went on applying, so removing `x = 5` left
  `y = x` computing 5 from a name written nowhere in the document; and a deleted
  `axis x log` went on drawing a logarithmic chart. A full pass now starts from
  nothing, as opening the file does. Because a setting binds no name, the graph
  has no edge from an `axis` or `label` line to the plots it governs, so
  changing one is a full pass too — otherwise renaming an axis would leave the
  old word on the chart until something unrelated redrew it.
- **The site deploys from `main`, not from a tag.** The `github-pages`
  environment allows deployments from branches only, so `v0.1.0`'s Pages job
  failed in one second with no step run. Deploying the page from the default
  branch and the artifacts from a tag is also the better arrangement on its own
  terms: fixing a typo on a page should not require cutting a release.


## 0.1.0 — the first release

The first build anybody outside this repository can have: a command-line tool
for linux-x86_64, linux-aarch64 and macos-aarch64, the WebAssembly module with
its hash, and the editor and worked examples deployed as a static site. Every
binary is built on a runner that owns its architecture and published only after
passing the golden suite on the machine that built it.

### The language

- **`check sigma <= sigma_allow`** — a worksheet states its own verdicts. A
  failed check is not an error: the arithmetic is right and the design is not,
  so it carries no diagnostic and gets its own exit code (2, against 1 for a
  worksheet that does not evaluate).
- **`use steel`** — packs of curated definitions, compiled into the engine
  rather than read from disk or fetched, because a browser opens a file and not
  a directory and a fetch would put the network inside a determinism claim.
- **`digits 4`** — significant figures from a line downwards. Presentation only;
  the full-precision values the cross-target comparison reads are untouched.
- **`axis x log`, `axis y 0, 100`** — a logarithmic axis and a drawn window. The
  log axis changes the *sampling* as well as the drawing: 257 samples spaced
  linearly across four decades put four of them in the first one.
- **A conversion in an assignment is remembered.** `sigma = M/S -> ksi` makes
  every later use read in ksi, compound targets included, so a worksheet's
  verdict lines read `10 ksi ≤ 30 ksi` rather than in pascals.
- **New functions**: `linterp` for reading a table, `rk4` for an initial value
  problem, `eigenvalues`/`eigenvectors` for a symmetric matrix, and seventeen
  more a worksheet expects to have — `mod`, `hypot`, `nthroot`, `log(x, b)`,
  `cot`/`sec`/`csc`, the inverse hyperbolics, `product`, `mean`, `median`,
  `sort`, `reverse`, `trace`, `submatrix`.
- **Complex vectors**, so a branch of impedances is one value.

### The editor

- Completion offering a name with what it holds and a unit with its dimension,
  hover explaining a name, and F12 to where it was defined — all from the
  engine's own symbol table, with no second parser in the front end.
- A **Typeset** toggle: the mathematics as MathML, with fractions, superscripts
  and radicals. `nomo html --mathml` does the same for a standalone document.
- The editor now **replaces a failed engine** rather than dying with it.

### Worked examples

Six mechanical how-tos written for the language rather than to exercise it — a
bolted joint, a shaft in combined bending and torsion, a column across the
buckling transition, bearing life, a compression spring, a pressure vessel worked
thin-wall and thick-wall side by side — plus a Bode plot and a cooling transient.
Each says what it leaves out, names its least certain number, and stops its plots
where their equations stop. `build-gallery.sh` renders all of them into a
browsable set of self-contained pages.

### Fixed

- **Two crashes reachable from ordinary typing.** A matrix literal with one
  comma missing built a matrix claiming a shape its data did not have and
  indexed out of bounds; and bracket depth times call depth, each within its own
  ceiling, ran the stack out — the two limits multiplied and nothing bounded the
  product. Both were found by a new randomized test within seconds of its
  existing, in an engine with 581 passing tests.
- **A worksheet could kill the editor permanently.** A trapped WebAssembly
  instance stays broken, so it was not the deep edit that failed but every edit
  after it, while the page went on looking like it was working.
- **The `corpus` CI job**, red since it was written, was failing on a trap in
  our own fetch script rather than on access to the corpora.

### Added for the sake of the record

- `nomo bench` — six fixed shapes timed and printed in CI as a report, not a
  gate.
- A randomized robustness test: fixed seed, no dependency, asserting the
  properties that must hold whatever the input is.
- `docs/roadmap.md`, and design note §8.42–§8.45 recording four investigations,
  two of which ended in a refusal rather than a feature.

## Unreleased

The repository was re-founded on a clean licensing basis. Everything in it is
now the project's own work under the MIT licence; nothing third-party is
committed. The git history before this point was squashed rather than rewritten
commit by commit, because two of the worksheets it carried were customer
documents that had been present since the second commit. What that history
contained is summarised below so the reasoning is not lost — the design note and
`docs/STATUS.md` remain the detailed record, and both were written as the work
happened.

### Naming

- The project was renamed from **Sheaf** to **Nomo**, after the *nomogram* — the
  printed calculating chart engineers read answers off before there were
  computers. Two reasons, neither cosmetic. `sheaf-core` on crates.io had been
  taken by an unrelated numerical-solver crate, so the engine could not have been
  published under its own name; and "sheaf" is close to unpronounceable for a
  Spanish speaker, since Spanish has no initial /ʃ/ and the `ea` digraph gives no
  clue. "Nomo" reads identically in both languages.
- The four crates are `nomo-core`, `nomo-cli`, `nomo-wasm` and `nomo-smath`, the
  CLI binary is `nomo`, and the file extension is `.nomo`. The repository is
  `nomo-math`, which disambiguates the search without putting "math" on a product
  that deliberately has no computer algebra system.
- The rename is nominal and was verified to be. The golden suite passed
  **without regeneration**, so no rendered layout depended on the old name's
  length. Every corpus baseline moved its `source=` digest and nothing else: all
  114 worksheets kept identical verdict counts and identical `values=` digests,
  the change being the one-line `' nomo 1` banner the importer emits.

### Licensing

- The SMath corpora are **fetched, not committed**. `scripts/fetch-corpora.sh`
  downloads both and verifies every file against the hashes in
  `scripts/corpora/`; `corpora/` is gitignored. The provenance is committed and
  the documents are not, which is what lets an MIT-licensed repository be
  measured against worksheets it may not redistribute. Both fetch paths are
  verified against live upstream.
- Two industrial worksheets — a resonant-converter design and an
  interlock-monitor design from an on-board-charger project — were **removed**,
  along with the two examples, two golden snapshots and one corpus baseline
  derived from them. They were customer documents. Passages that cite what they
  measured now say "the converter worksheet" and "the interlock worksheet".
- `examples/llc.nomo` and `examples/interlock.nomo` are **clean-room
  replacements**, written from public engineering practice, carrying the same
  engine features through the golden suite and the WebAssembly comparison. Their
  diagrams are drawn by `scripts/make-example-figures.py` from the Python
  standard library alone.
- `THIRD-PARTY.md` and `NOTICE` added; `README.md` states the licence and
  the contribution terms.

### What the squashed history built

**The engine.** Lexer, parser, rational dimensions over the seven SI base
dimensions, units with prefixes and affine temperature scales, evaluation to a
displayable trace, and a document graph evaluated in dependency order. IEEE 754
binary64 throughout with a pure-Rust math library compiled in, so results are
bit-reproducible across machines and targets.

- Complex numbers for scalars, `Re`/`Im`/`conj`/`abs`/`arg`.
- Vectors and matrices: indexing, `det`/`inv`/`transpose`, element-wise versus
  matrix `*`, `identity(n)`, `diag(v)`, `augment`/`stack`/`row`/`col`.
- Strings as verdicts and keys, with no arithmetic and no order.
- Repetition without mutation: `range`, `map`, `iterate`.
- Numerics: `root`, `roots` over a window, `integral`, `solve_linear`, and
  `derivative` — exact, by forward-mode automatic differentiation, with no step
  to tune and no CAS.
- Plots: one curve, several curves over one span, and a table of measured
  points, drawn as engine-generated SVG.
- Figures carried inside the worksheet as a base64 trailer, at the size they
  were drawn at.
- Two fixed ceilings on recursion — 64 nested calls, 100 000 calls per
  statement — set by the tightest target so a worksheet cannot answer natively
  and kill a browser tab.

**The SMath importer** (`crates/nomo-smath`). Reads `.sm` worksheets, reduces
their postfix token streams, emits `.nomo`, and checks itself against the
answers SMath stored a decade ago. Nothing is translated on a guess and nothing
is dropped in silence: an unsupported construct becomes a visible marker and a
counted note. `for` loops that fill a vector become `map`; `diff` at a point
becomes `derivative`; a definition free in `x` that a plot draws is read as a
function of `x`; `sys(…)` on a definition is read as a list of curves.

- The plot span was settled by disassembling SMath's own `PlotRegion.dll` after
  two attempts to infer it from worksheets disagreed by four orders of
  magnitude. Reading the implementation outranks inference from files.
- The oracle grew to read tables and complex answers, taking agreement across
  both corpora from 285 of 319 comparable answers to 587 of 620.

**The gates.** A byte-exact golden suite over `examples/`; a corpus baseline
recording verdict counts plus digests of both the computed values and the
emitted source; a determinism guard forbidding host math and I/O in the engine;
native-versus-WebAssembly and x86-64-versus-aarch64 comparisons, both byte-exact;
and artifact checks asserting no imports and no SIMD in the WebAssembly module.

**Refused, deliberately.** A computer algebra system; SMath's `range` with a
step, `solve`'s range semantics, the `ltle`/`ltlt`/`lele` boundary convention and
the `—` operator, each because the corpus cannot settle what they mean.
