# Changelog

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
