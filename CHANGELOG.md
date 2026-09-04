# Changelog

## Unreleased

### Changed

- **A name that spells a Greek letter is typeset as one.** `sigma_allow` draws
  as σ_allow, `lambda` as λ, `Delta_p` as Δ_p — in `nomo html --mathml` and
  behind the editor's **Typeset** toggle. Presentation only: the language
  already accepted `σ` typed directly and still treats the two spellings as two
  names. The letter is the one *Unicode* gives that name rather than the one
  TeX gives it, so `phi` and a typed `φ` are the same letter; the `var` names
  take the symbol forms. A name maps only where the Greek glyph differs from
  the Latin one it would otherwise be set as, which is why there is no
  `omicron` and no `Alpha` — and which reproduces TeX's uppercase set exactly.
  Units are never read this way: `psi` stays pounds per square inch.
- **The italic and the upright now follow ISO 80000-2**, and the Greek table is
  what does it. Nothing asks for either: MathML Core italicises a
  one-character `<mi>` and leaves a longer one upright, which is already the
  rule that a quantity symbol is a single italic letter and a descriptive
  subscript is an upright word. As the word `sigma` a stem was five characters
  and set upright; as σ it is one.
- **The name column is typeset with the rest of the line.** It was plain text,
  which would have left `sigma_allow` on the left of a line whose formula read
  σ_allow. `unit … = …` and `fn … defined` stay as prose notes.

### Added

- **The math font is shipped rather than named.** Typeset output is laid out
  from a font's OpenType MATH table, and until now both stylesheets named fonts
  that have one and hoped the reader's machine did too. `web/dist/` now carries
  a 162 kB subset of STIX Two Math, precached by the service worker so it works
  offline, with the named stack kept behind it as the fallback. The font is
  fetched and hash-verified by `./scripts/fetch-font.sh` and **not committed** —
  the same rule as the corpora — and subset by `web/font.mjs` using the harfbuzz
  subsetter, which is an npm package rather than a third toolchain.
- **`nomo html --font-url <url>`** references a font served beside the document,
  and **`nomo html --embed-font <file>`** carries one inside it. The default is
  unchanged: no `@font-face`, nothing fetched, one self-contained file.
  `--embed-font` also embeds the licence file sitting beside the font, because a
  document that carries a font redistributes it.
- **A text face for the prose.** A worksheet is a document, and prose set in the
  machine's UI sans beside mathematics set in a book face looks like two things
  stapled together. The editor now ships STIX Two Text — the face STIX Two Math
  was drawn against — as its two variable faces, 173 kB carrying the whole
  400–700 weight axis. A standalone `nomo html` *names* it rather than carrying
  it: a missing text face gives Georgia, while a missing math font gives wrong
  mathematics, and only the second is worth the weight in every exported file.
- **A conditional is drawn as a brace over its cases**, which is how mathematics
  draws one. `else if` flattens into rows rather than nesting, and an arm that
  did not run is shown as written rather than pretending it was computed. It was
  the last construct still running out as a sentence.
- **The gallery is typeset.** All 28 worked examples show their mathematics as
  mathematics, sharing one math font file rather than carrying a copy each.
  Typesetting stays off by default everywhere else.
- **The result column is typeset with the rest of the line.** It was the last
  plain text on a typeset line, and it showed — a result reading `8.427e-5`
  beside a substituted value on the same line reading 8.427 × 10⁻⁵. An error, a
  vector, a string and a complex number keep their text.
- **A typeset line is set whole.** The result, the `=` between the columns and
  the words `check` and `pass` sit outside the `<math>` elements and were staying
  in monospace while the formula beside them was a book face. Typeset lines are
  now marked `step typeset` and set in the math face throughout. An untypeset
  line keeps its monospace, which is right for linear text whose columns line up.

### Fixed

- **`pi` no longer typesets as the word "pi".** The MathML renderer had no
  symbol table of its own and did not use the one the linear renderer already
  had, so the typeset column drew `pi` beside a text column showing π. Both now
  read from one table. Constants are marked upright, which is how ISO 80000-2
  sets π, e and ∞ and what distinguishes them from a variable of the same name.
- **An arm of a conditional that did not run is now resolved.** The evaluator
  sketches such an arm without evaluating it, and classified every name in it as
  a variable. The text column hid that — it writes a name the same either way —
  but it was wrong there too: `column.nomo` read `(2·pi)` in an untaken arm
  beside `π²` in the taken one on the same line. It now reads `(2·π)`, and
  typeset output stops setting a metre in italic. One golden snapshot moved.
- **A complex value keeps the brackets it needs**, so `(3 + 4i)²` is not
  `3 + 4i²`. Bracketing is asked of the linear renderer, which has always had to
  answer it, rather than judged a second time.
- **A conversion no longer hides the expression it converts.** `A = pi/4*d^2 ->
  mm^2` has `Convert` at the top of its trace, and the typeset renderer fell
  back for it — dropping the *whole* expression to running text. Since a
  worksheet writes a conversion more often than not, that was most of what
  typeset output ever fell back on: 135 of the 342 whole-expression fallbacks
  across `examples/`. The linear renderer has always walked straight through a
  conversion, because `-> mm^2` belongs to the result column.
- **The substituted column is mathematics, not a caption.** It was `<mtext>` of
  a formatted string, which cost three things at once: `<mtext>` is *space-like*
  in MathML, so an operator beside it got no spacing at all and a comparison
  read `160≥105.5`; a unit's `²` was a literal character where the symbolic
  column beside it drew a real superscript; and `8.427e-5` is not how a typeset
  document writes 8.427 × 10⁻⁵. All three are fixed. A vector, a matrix, a
  string and a complex number are not "a number and a unit" and stay as text.
- **Every operator states its own spacing.** Measured in Chrome against the
  MathML operator dictionary — 5/18 em for a relation, 4/18 for a sum, 3/18 for
  a product, which are TeX's values. Nothing changes where the operands are
  ordinary markup; where one has to stay running text, the spacing no longer
  depends on what it is made of.
- **`<msup>` and `<msub>` are given the two children they take.** `(a+b)^2` has
  been emitting a superscript with *six* children since typeset output was
  built, so the bracket drew inside the exponent. A browser lays the extra
  children out flat rather than failing, which is why it lasted. A substituted
  quantity under a power is now bracketed as well: `(50 mm)²`, not `50 mm²`,
  which is a different quantity.
- **A unit stands off its number.** ISO 80000-1 requires a space between a
  numerical value and its unit symbol, and `ImplicitMul` emitted U+2062
  INVISIBLE TIMES, which is exactly zero wide — so `d = 50 mm` typeset as
  `50mm` beside a substituted column reading `(50 mm)`, one line disagreeing
  with itself. Typeset quantities now carry a thin space. Ordinary algebra does
  not: `2x` stays tight, because what tells them apart is whether the right
  operand is a unit. `90°` stays tight too — the standard exempts the
  plane-angle symbols, where `20 °C` is not exempt.

MathJax was measured and declined: 850 kB of script plus 1.8 MB of fonts fetched
at run time, which ends `nomo html`'s self-containment and makes typesetting
asynchronous where printing is synchronous — to buy cross-browser consistency
that MathML Core already provides for the repertoire this renderer emits. Design
note §8.47 through §8.52.

## 0.2.1 — a font that can set the mathematics

Typeset output was asking a font with no MATH table to lay out a fraction. No
language changes; the three commits since 0.2.0 are one visible fix and two
pieces of repository hygiene.

### Fixed

- **The typeset columns name a math font.** MathML Core reads the fraction bar
  thickness, the axis height, the script shifts and the stretchy bracket
  recipes from the font's OpenType MATH table. Nothing named a font for `math`,
  so a worked line inherited the monospace stack `.step` sets — no MATH table at
  all, and the worst case for a fraction, because the browser then guesses every
  one of those constants from ordinary text metrics. Both stylesheets now name
  Latin Modern Math, STIX Two Math, TeX Gyre Pagella Math, Cambria Math and then
  the `math` generic, so a platform can offer whatever it has. Named and never
  fetched: the artifact stays one self-contained file that needs no script and
  no font from the network, which is the same rule the SVG plots follow. Which
  entry wins is the machine's business, and it was measured rather than assumed
  — Chrome sets the same string to the same width in the stack and in the font
  the stack resolved to, against a different width for the monospace it
  replaced. No snapshot moves; the golden files carry no CSS.
- **`repository` names the repository this actually is.** It pointed at
  `trukeio/nomo-math`, which is neither the remote nor a repository that exists
  — a name researched for availability in design note §2 had been written down
  as a name that was taken. Nothing reads it today, but it becomes the
  "Repository" link at the first `cargo publish`.

### The build

- **The five actions still on Node 20 move to their current majors.** The runner
  had begun force-upgrading them to Node 24, and a forced runtime upgrade is the
  runner deciding what the workflow runs on — the thing the pinned Rust
  toolchain exists to prevent one layer down. Two of the jumps carry breaking
  changes that were checked rather than assumed: `upload-pages-artifact` v4 stops
  including hidden files, and `web/dist` has no dotfile in it; `download-artifact`
  v8 makes a digest mismatch an error, which is the direction this repository
  would have chosen anyway.


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
