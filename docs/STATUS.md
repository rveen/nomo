# Status

Snapshot for picking the work back up. Last updated at the **v0.3.0** release.
The nine numbered phases were complete before any of the language work below;
the most recent commits are the importer's seventh phase, figure sizing,
complex numbers, plots, several curves on one plot, a plot of a table of
measured points, the importer's side of it, a definition that is really a
function of `x`, prose as Markdown (§8.41), the v0.1.0 release, labelled axes,
the v0.2.0 release, the v0.2.1 release, Greek letters in the typeset columns
(§8.47), the math font the output is set in (§8.48), the text face around it
(§8.49), the space a unit stands off its number by (§8.50), the substituted
column becoming mathematics (§8.51), the conditional drawn as cases with the
gallery turned on (§8.52), and the v0.3.0 release.

**A four-step plan for typographic quality is under way (2026-09-04).** It was
costed against the alternative of shipping MathJax, which was measured and
declined: `mml-chtml.js` is 850 KB with a 1.8 MB font set fetched at runtime,
which ends `nomo html`'s self-containment and makes typesetting asynchronous
where printing is synchronous, and it buys cross-browser consistency that
MathML Core in all three engines already provides for the repertoire this
renderer emits — no big operators, no limits, no line breaking. Revisit if any
of those three arrive. The steps, in order:

1. **Greek names and the italic/upright discipline** — **done**, §8.47. No
   dependency at all.
2. **Self-host one math font** — **done**, §8.48. STIX Two Math from
   `stipub/stixfonts` at tag `v2.13b171`, fetched and hash-verified by
   `scripts/fetch-font.sh`, subset from 552 KB to **162 KB** by `web/font.mjs`,
   shipped as `stix-two-math-subset.woff2` with `OFL.txt` beside it. Came in
   below the 238 KB estimate because `math-auto` produces only the *italic*
   alphabets, so the bold, script, fraktur, double-struck, sans and monospace
   ranges of U+1D400–1D7FF are unreachable and were dropped. hb-subset via the
   `subset-font` npm package rather than fontTools, so the build gains no third
   toolchain. `nomo html` gained `--font-url` and `--embed-font`.
3. **A text face to match the mathematics** — **done**, §8.49. STIX Two Text as
   the two *variable* faces rather than three statics: **173 KB** for the pair
   against 197 KB, and it carries the whole 400–700 axis so the verdict line's
   700 works. Shipped to the editor; a standalone `nomo html` names the face and
   carries nothing, because a missing text face gives Georgia while a missing
   *math* font gives wrong mathematics. Looking at the result also found that a
   typeset line was set half in the math face and half in `.step`'s monospace;
   it is now marked `step typeset` and set whole.
4. **No MathJax.**

**Where the last session stopped, and what it left.** Everything below is
committed and pushed, and every gate named here is green. Three things are
pending, in the order they are worth doing:

1. **Nothing from the typography plan.** It is finished, and the last thing it
   turned up — inline Markdown — is built (§8.53). The next items are the two
   below.
2. **The importer's plot configuration.** `XYPlot'Labels'XLabel` and
   `Traces#n'Name` are still refused, and the reason they were refused is gone:
   the language can say them now. Before translating, three things need
   measuring against the corpora — whether a configuration block sits before or
   after the plot it configures, whether trace indices are dense from zero, and
   how the block's region name relates to the `plot(…)` call the emitter
   writes. Design note §8.46 and roadmap step 21 hold the detail.
3. **Multiple open documents in the editor** — deferred by the owner, and
   listed under "What this build does not do" with the reason.

`./scripts/compare-arch.sh` did not run in that session: `qemu-user` is not
installed on this machine. CI's `arm64` job covers it.

## Where things stand

| Phase | State | What exists |
|---|---|---|
| 0 Scaffolding | **done** | Cargo workspace, pinned toolchain 1.97.1, MIT licence, CI |
| 1 Lexer, parser, AST | **done** | Pratt parser, spans on every node, recoverable diagnostics |
| 2 Dimensions and units | **done** | Rational exponents, SI + imperial, affine temperature rules |
| 3 Values and trace | **done** | Scalars/vectors/matrices of `Quantity`, evaluation returns a `Trace` |
| 4 Document and graph | **done** | Dependency DAG, globals, cycle detection, incremental recalculation |
| 5 Renderer | **done** | Three-column text and self-contained HTML, significant figures |
| 6 Golden-file harness | **done** | `nomo test`, byte-exact, 9 snapshots, CI gate |
| 7 WASM + cross-target determinism | **done** | `nomo-wasm`, C ABI, corpus byte-identical native vs WASM |
| 8 Browser editor | **done** | CodeMirror 6, engine-driven highlighting, live recalc, print |
| 9 Local persistence, offline | **done** | Open/save with a cross-browser fallback, draft in IndexedDB, service worker |
| — SMath importer | **seven phases in** | `nomo-smath`: reads **both** corpora — 54 wiki worksheets (0.82–0.98) and 60 mechanics worksheets (1.3–1.5) — emits `.nomo`, and checks itself against 1179 stored answers. Agreement: 312/344 wiki, 283/283 mechanics. Design note §8.13–§8.39 |
| — A second batch of builtins | **done** | `mod` `hypot` `nthroot` `log(x, b)` `cot` `sec` `csc` `asinh` `acosh` `atanh` `product` `mean` `median` `sort` `reverse` `trace` `submatrix`. Conventions read from SMath where it has one — `mod`'s sign, `submatrix`'s inclusive one-based bounds. `stdev` and `rank` are deliberately absent; `docs/language.md` says why. Took the wiki corpus from 304/337 to **312/344** |
| — Releasing | **run four times** | A tag builds a binary per architecture with no cross-compilation, each published only after passing the golden suite on the machine that built it; the wasm module goes out with its hash after agreeing with native byte for byte; the editor and gallery deploy to Pages from `main`. `v0.1.0` proved the shell on real runners and found the tag-versus-branch fault in the `pages` job; `v0.2.0` is the first tag cut with that split already in place, and `v0.2.1` is a patch carrying the math-font fix; `v0.3.0` is the first tag whose `pages` job publishes a *typeset* gallery, and the first whose build fetches something from the network — the fonts, hash-verified |
| — The gallery, and migration shown | **done** | `build-gallery.sh` renders every example into a browsable set of pages, typeset since §8.52 and sharing one math font file, and `docs/smath.md` finally *shows* an import: an SMath worksheet written here — ours to publish, unlike the corpora — beside what Nomo makes of it and what it computes. That fixture is also the only importer test that runs without the corpora |
| — How-to worksheets | **six of six** | `bolt`, `shaft`, `column`, `bearing`, `spring`, `vessel` — a bolted joint, a shaft in combined bending and torsion, a column across the buckling transition, bearing life, a compression spring against six constraints, and a pressure vessel worked thin-wall and thick-wall side by side. The first worksheets written *for* the language rather than to exercise it; their acceptance is an engineer agreeing with the method rather than a green gate. Each says what it leaves out |
| — The editor assists | **done** | Completion offering a name with what it holds and a unit with its dimension, hover saying what a name is, F12 to its definition, and a Typeset toggle that puts §18's MathML where a reader actually looks. All from the engine's own symbol table — no second parser in the front end. **Multiple open documents is the one piece not built**; the boundary already supports it and the reason is below |
| — Typeset output | **first phase done** | `nomo html --mathml` renders the symbolic and substituted columns as MathML: division becomes a fraction, a power a superscript, `sqrt` a radical, and a bracket the fraction bar makes unnecessary is dropped. Off by default. `check-mathml.mjs` asks Chrome where the numerator ended up, because a browser without MathML draws it *beside* the denominator rather than failing. `math` names a math-font stack in both stylesheets instead of inheriting `.step`'s monospace: MathML layout reads the fraction bar, the axis and the script shifts from an OpenType MATH table, and the fonts are named, never fetched. **Fifth phase (§8.52):** a conditional is
a brace over cases, `else if` flattening into rows; an arm that did not run is
resolved by the evaluator rather than left as bare names, which moved one golden
snapshot and made the text column consistent with itself; bracketing is asked of
the linear renderer rather than judged twice, so a complex value keeps its
brackets; and the result column is typeset. **The gallery is typeset**, sharing
one font file across all 28 pages. **Fourth phase (§8.51):** a conversion is
transparent rather than a fallback, which is most of what typeset output ever
fell back on; the substituted column is set as a number and a unit rather than
as running text, so a unit exponent is a real superscript and `8.427e-5` is
8.427 × 10⁻⁵; and every operator states its own spacing, because `<mtext>` is
space-like in MathML and an operator beside it was getting none. Two `<msup>`
arity bugs fell out, one of them present since step 18. **Third phase (§8.48):** the font is
*shipped* rather than named — a 162 kB subset of STIX Two Math, fetched and
hash-verified rather than committed, precached by the service worker, with the
named stack behind it as the fallback. `nomo html` gained `--font-url` and
`--embed-font`; the default still writes no `@font-face` and stays one
self-contained file. **Second phase:** a name that spells a Greek letter is set as one — `sigma_allow` draws as σ_allow — which is also what gets the italic right, since MathML Core italicises a one-character `<mi>` and leaves a word upright, and that is ISO 80000-2's rule for a symbol and its descriptive subscript. The letter is Unicode's for that name rather than TeX's, so `phi` and a typed `φ` agree; a name maps only where the Greek glyph differs from the Latin one, which reproduces TeX's uppercase set and its omicron gap. Constants are upright and now come from the renderer's own table, so `pi` no longer typesets as the word `pi` beside a text column showing π. The name column is typeset with the rest of the line. Design note §8.47 |
| — Complex vectors | **done** | A vector literal with a complex element is a complex vector: elementwise arithmetic, `sum`, `abs`/`arg`/`Re`/`Im`/`conj`, indexing. Everything else refuses by name — the alternative was an aggregate seeing an empty collection and answering, which is how `sort` gave back `[]`. A complex *matrix* is still not built. Nine exhaustive matches, exactly as §8.40 predicted for a second value tower |
| — Eigenvalues | **done** | `eigenvalues(m)` and `eigenvectors(m)` for a symmetric matrix, by cyclic Jacobi at a fixed twelve sweeps. Exactly symmetric or refused, with the remedy named — a nearly-symmetric matrix is the heuristic zero-test §8.40 refuses. `examples/shaft.nomo` now computes its principal stresses and checks them against the Tresca stress it already had, which is an independent check of the solver inside a real calculation |
| — Initial value problems | **first phase done** | `rk4(f, y0, a, b, steps)` integrates `y' = f(x, y)` at a fixed step and answers with a table `plot` can draw and `linterp` can read. The method is named and the step count stated because both change the answer. First-order and scalar only. `examples/transient.nomo` checks it against a case whose answer is known: 6 µK at fifty steps, 0.14 K at five |
| — Axes | **done** | `axis x log`, `axis y 0, 100`, `linear`, `auto`. A logarithmic horizontal axis changes the *sampling* too — a decade sweep spaced linearly puts four of 257 samples in its first decade — which is why it lives on the plot value rather than in the renderer. SMath has axis limits and no log scale at all, read from `PlotRegion.dll`, so the limits follow a precedent and the scale is ours. `examples/bode.nomo` |
| — Labelled axes and named curves | **done** | `axis x "Frequency"` says what an axis measures and `label "Gain", "Phase"` names the curves; the unit stays at the end of the axis, because `Frequency` and `Hz` are different questions. The most-wanted plot feature by corpus ranking — 88 `description` calls, all of them `XLabel` or a trace name (§8.44) — and now §8.46. It also turned up two leaks older than itself: a full pass kept the old environment, so a deleted definition and a deleted `axis x log` both went on applying |
| — Display precision | **done** | `digits n` sets significant figures from a line downwards — presentation only, so the full-precision values the cross-target comparison uses are untouched. All six how-tos wanted it, and so does the corpus: 1279 SMath regions carry an explicit precision that Nomo could not express |
| — Packs | **done** | `use steel` brings in a curated set of definitions, compiled into the engine rather than read from disk or fetched — a browser opens a file, not a directory, and a fetch would put the network inside a determinism claim. Two packs so far, `constants` and `steel`; `nomo packs` lists them. `examples/packs.nomo` |
| — Tables | **first phase done** | `linterp(xs, ys, x)` reads a value out of a table, with units, refusing to extrapolate. Settled by disassembling SMath's own implementation (§8.42), which extrapolates, sorts and drops units — all three decided the other way here, on purpose. `cinterp`, `ainterp` and the lookup family are not built, and §8.42 says why. `examples/tables.nomo` |
| — Checks | **done** | `check sigma <= sigma_allow` states a limit and reports a verdict. A failed check is not an error — the arithmetic is right and the design is not — so it carries no diagnostic and gets its own exit code: `nomo check` answers 0, 1 for a worksheet that does not evaluate, 2 for one whose check failed. `examples/checks.nomo` |
| — Conditions | **done** | Comparisons, `and`/`or`/`not`, and a lazy `if … then … else` expression |
| — Repetition | **done** | `range`, `map`, `iterate` — loops without mutation, so the DAG is untouched |
| — Plots | **third phase done** | `plot(f, a, b)`, sampled at a fixed count and drawn as engine-generated SVG; `plot(f, g, …, a, b)` for several curves, with a legend; `plot(m)` for a table of measured points, which needs no span. The importer draws both kinds now: a table, and a function of `x` over the span its viewport implies (design note §8.21) — including one a definition named rather than wrote out, which is how SMath says "this curve" (§8.22) |
| — Derivatives | **done** | `derivative(f, x)` and `derivative(f, x, 2)`, the slope and the curvature at a point, by forward-mode automatic differentiation: exact, no step size, and the dimension of `f/xⁿ` falls out of the arithmetic. Not symbolic and never will be — SMath's `diff` is a CAS operation and this is the other thing a worksheet means by a derivative (§8.27, §8.33) |
| — Root finding | **done** | `root(f, a, b)` bisects a bracket; `roots(f, a, b)` scans a window — 200 intervals, every sign change bisected — and answers with one root or a vector of them. The second exists because SMath's `solve` is a search rather than a bracket, which was settled by reading `SpecialFunctions.dll` (design note §8.24) |
| — Strings | **first phase done** | A literal, bindable, choosable by `if`, comparable with `==`. No arithmetic, no order, none inside a collection — which is what the 41 corpus markers for them needed and no more (§8.32) |
| — Complex numbers | **first phase done** | `i`, arithmetic with units, `Re`/`Im`/`conj`/`arg`/`abs`. Transcendentals of a complex argument and complex collections are not built; see `docs/language.md` |
| — Prose as Markdown | **done** | A comment's text is Markdown in a closed subset: headings, paragraphs, lists. `crates/nomo-core/src/prose.rs` reads a run of comment lines into blocks and the HTML renderer lays them out; the language, the graph and the file format are untouched. Inline is `` ` `` and `**`, and nothing else: `_` would eat identifiers and a single `*` is the multiplication operator — 61 corpus prose lines pair one and every pair encloses arithmetic. Design note §8.41 and §8.53, `examples/prose.nomo` |

680 tests and 29 golden snapshots. `git log` is one commit per phase, and each
commit message records the reasoning behind anything non-obvious in it.

### Starting a new session here

Everything below is verifiable from a clean checkout, and nothing is in flight —
the working tree was clean at the last commit. Read `docs/design-note.md` for why
anything is the way it is, `docs/language.md` for what the language currently
does, `docs/smath.md` for how to import an SMath worksheet and what does and
does not survive it, and then:

1. **Run the gates first** (next section). They take a couple of minutes and
   they are the fastest way to learn what this repository considers true. The
   corpora are third-party and are not in git — run `./scripts/fetch-corpora.sh`
   once and the `check-corpus.sh` lines work from a clean checkout; set
   `CORPUS_ROOT` if the worksheets are somewhere else. The fonts are the same
   arrangement — `./scripts/fetch-font.sh`, which `build-web.sh` runs for you.
   See THIRD-PARTY.md.
2. **The next piece of work is named** under "What is worth doing next". The
   item that stood there longest — what span an SMath `<plot>` was drawn over —
   is answered: it was read out of `PlotRegion.dll` and checked against six
   worksheets (design note §8.21), and the step behind it — a definition that is
   really a function of `x` (§8.22) — is built, so two of the converter
   worksheet's three charts draw. The third needs `sys(…)` as an expression, and
   that is named there.
3. **The worked example was a converter worksheet** and it was finished: 34 of
   34 stored answers agreed, all three charts drew, `nomo check` exited zero,
   and the 7 markers left were the ones this document lists as not work. It went
   from 1 answer checked and 22 markers over eight commits. That worksheet was a
   customer document and has been removed (THIRD-PARTY.md); what it proved
   stands, and `examples/llc.nomo` is the clean-room worksheet that now carries
   the same engine features through the golden suite.

## Verifying the current state

```bash
cd /files/work/nomo
cargo test --workspace                                  # 680 tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
./scripts/check-no-host-math.sh                         # determinism guard
cargo build -p nomo-core --target wasm32-unknown-unknown

cargo run -p nomo-cli -- render examples/beam.nomo
cargo run -p nomo-cli -- html   examples/beam.nomo
cargo run -p nomo-cli -- test                          # golden-file suite
cargo run -p nomo-cli -- packs                         # what `use` can bring in
cargo run -p nomo-cli -- version                       # build, and the formats it speaks
cargo run --release -p nomo-cli -- bench               # timings; a report, exits 0
./scripts/compare-targets.sh                            # native vs WebAssembly
./scripts/compare-arch.sh                               # x86-64 vs aarch64 (needs qemu-user)
./scripts/build-gallery.sh                              # the worked examples as a
                                                        # browsable set of pages
./scripts/build-web.sh                                  # front end; also runs the nine
                                                        # browser checks, including
                                                        # check-figures.mjs,
                                                        # check-plots.mjs and
                                                        # check-recovery.mjs, which assert
                                                        # what only a browser can see

./scripts/fetch-font.sh                                 # the fonts, likewise; run by
                                                        # build-web.sh
./scripts/fetch-corpora.sh                              # the corpora are third-party and
                                                        # are not committed; this brings
                                                        # them down and hash-verifies them.
                                                        # Everything below needs it first.
./scripts/fetch-corpora.sh --verify                     # check what is already there

cargo run -p nomo-smath --bin smath-coverage -- \
    corpora/nomo-corpus/sm                        # what the .sm reader cannot yet read
cargo run -p nomo-smath --bin smath-import -- --check \
    corpora/nomo-corpus/sm                        # Nomo's answers against SMath's own
cargo run -p nomo-smath --bin smath-import -- x.sm     # write one worksheet as .nomo,
                                                        # figures included, at the size SMath
                                                        # drew them (docs/language.md,
                                                        # "Figures"); nomo html embeds them
cargo run -p nomo-smath --bin smath-import -- --lang eng x.sm   # pick a language

./scripts/check-corpus.sh                               # corpus regression gate
./scripts/check-corpus.sh --write                       # accept an intended change
```

`check-corpus.sh` is to the importer what `nomo test` is to the renderer: a
committed per-worksheet baseline in `tests/corpus/`, compared exactly, so any
change that moves a result fails until the baseline is regenerated alongside it.
Each line carries three things, and each was added because the ones before it
let something through:

- **Verdict counts.** How many answers agreed, disagreed, did not evaluate, and
  how many markers the worksheet carries.
- **A digest of the computed values.** Counts alone let a broken `norm` through,
  because sixteen of the answers that depend on it are angles, and an angle does
  not change when its vector is scaled.
- **A digest of the emitted `.nomo` source.** Values are not enough either, one
  step over: the commit that made a definition free in `x` into a function of
  `x` rewrote four lines in three wiki worksheets — turning broken live
  definitions into `fn` — and every count and every value stayed exactly as it
  was. That change was found by diffing emissions by hand, which is not a gate.
  A source digest says nothing about whether the new text is better; it says the
  diff has to be looked at and the baseline regenerated deliberately.

Two sets: the wiki and mechanics corpora. There used to be a third —
`tests/corpus/standalone.txt`, gating the loose worksheets at the top of
`corpora/` — and it went when those worksheets did, because two of the four were
customer documents and the other two were byte-identical copies of files already
in the wiki set.

The corpora are fetched rather than committed. CI runs `fetch-corpora.sh` and
then this as its `corpus` job; set `CORPUS_ROOT` if they are elsewhere.

Note that `nomo check examples/bearing.nomo` exits **2** on purpose: its
bearing does not reach the life the duty asks for, the worksheet says so and
goes on to compute the rating that would, and 2 is the code for "this evaluates
and a check failed" as distinct from 1 for "this does not evaluate". It is the
one worksheet that exercises that path end to end.

Note that `nomo check examples/diagnostics.nomo` exits non-zero on purpose:
that worksheet is a page of deliberate mistakes whose error messages the suite
pins. `check` evaluates — it did not until the commit that says so, and reported
`ok` on that page of mistakes, because nothing wrong with them is a syntax
An imported converter worksheet used to be the second such case, for its `diff`
chain; it exited zero by §8.28.

All of the above were green at the last commit, run locally. That is not the
same as CI being green: the `corpus` job cannot fetch the worksheets on a
runner, and this list assumes they are already on the machine.

## Releasing

`.github/workflows/release.yml` runs on a tag (`v*`) and on a manual dispatch.
Four jobs: a binary per architecture, the WebAssembly module, the Pages
deployment, and the release itself.

Three things about it are deliberate. **Nothing is cross-compiled** — each
binary is built on a runner that owns its architecture, which is the same
argument the `arm64` CI job makes and the reason the golden suite runs *inside*
the release job: an artifact that has not reproduced the committed snapshots on
the machine that built it is not published. **The module is published with its
hash**, after `compare-targets.sh` has shown it agrees with a native build, so
the determinism claim can be checked by a reader. And **releasing uses `gh`**
rather than a third-party action: it is on every runner and it is GitHub's own,
so publishing costs this repository no dependency the engine would not be
allowed.

**What the first tag settled.** `v0.1.0` published all five artifacts and
**macOS agreed** — its golden suite passed, which is a third platform's word on
the central claim, after x86-64 and aarch64. The Pages source had to be set to
"GitHub Actions" by hand first; that is done.

**And what it caught.** The `pages` job failed in one second with no step run,
because the `github-pages` environment allows deployments from **branches only**
and a tag is not one. The site is therefore deployed from `main` and the release
artifacts from a tag — which is the better arrangement anyway: the page should
track the default branch, and fixing a typo on it should not require cutting a
release. `binaries` and `wasm` stay tag-only, or every push would build three
binaries for nothing.

**What has been checked here, and what has not.** The workflow's shell — the
packaging, the checksums, the collection of artifacts into one `SHA256SUMS.txt`,
the release notes — was extracted from the YAML and run against stand-in
artifacts, and does what it says. The runner behaviour, the Pages deployment and
the `gh release create` call cannot be exercised from a development machine;
`v0.1.0` exercised them for the first time, `v0.2.0` is the second, `v0.2.1` the
third and `v0.3.0` the fourth, and the only way to check any of them is to read
the run.

**What the second tag settled.** `v0.2.0` carries labelled axes and named
curves, the two evaluator leaks that work uncovered, and the `pages` fix — so it
is the first tag cut with the branch-versus-tag split already in place, and it
confirmed the split: the push to `main` ran `pages` alone and skipped the other
three, the tag ran the other three and skipped `pages`, and both went green.
That is the fault `v0.1.0` found, fixed and now demonstrated rather than
argued. All five artifacts published — three tarballs, the module, and one
`SHA256SUMS.txt` covering them — and **macOS agreed again**, which is the second
tag's word on the central claim.

The whole tag run took under two minutes: 26–50 s per binary and 29 s for the
module, so the cost of releasing is not a reason to release less often.

Before it was cut, every gate that can run on a development machine was run
here: 650 tests, 29 snapshots, native and WebAssembly byte-identical, 114 corpus
worksheets unmoved, nine browser checks. `compare-arch.sh` was not among them —
`qemu-user` is not installed here — and CI's `arm64` job is what covers it.

**What the third tag settled.** `v0.2.1` is a patch: the typeset columns name a
math font, `repository` names the right repository, and the five workflow
actions still on Node 20 move to their current majors. It is the first tag whose
content changes *nothing in any answer* — a font is layout — so the golden suite
was there to show exactly that, and 29 snapshots and 114 corpus worksheets
stayed put.

What it actually settles is the action bump, which until it ran was the one
change this repository could not check. Only `pages` runs on a push to `main`,
so `upload-artifact@v7`, `download-artifact@v8` and the `release` job's use of
them had never executed anywhere; a tag was the only thing that would run them.
It ran them, and the annotations are gone: the `v0.2.0` tag run carried five
Node 20 deprecation warnings — four for `upload-artifact@v4`, one for
`download-artifact@v4` — and the `v0.2.1` run carries none, read from the
check-run annotations of both. `download-artifact@v8`'s stricter digest check
passed rather than rejecting anything, and `upload-pages-artifact@v5`'s dropping
of hidden files left the site unaffected exactly as `web/dist` predicted: the
deployed `style.css` was fetched afterwards and carries the font stack.

The split held a second time — the tag ran `binaries`, `wasm` and `release` and
skipped `pages`, the push to `main` ran `pages` and skipped the other three —
and **macOS agreed a third time**, which is the third tag's word on the central
claim. Timings are steady enough to plan around: 25 s, 32 s and 49 s for the
three binaries, 27 s for the module, 12 s to collect and publish, and 1 m 42 s
for the site.

**What the published hash does and does not claim.** `SHA256SUMS.txt` lets a
reader check that the module they downloaded is the one this workflow built. It
is **not** a claim that a build of the same source on another machine produces
the same bytes — the artifact carries absolute paths and a toolchain fingerprint
and it does not. The claim this project makes is about *answers*, and
`compare-targets.sh` is what checks it: it ran inside the `wasm` job before the
module was uploaded, so what went out had been shown to agree with a native
build byte for byte on the results themselves.

**What the fourth tag settled.** `v0.3.0` is the typography release, and two
things about the machinery could only be checked by cutting it.

**The build now fetches from the network, and it worked on a runner.** Every
previous build needed nothing but the checkout; `scripts/fetch-font.sh` reaches
raw.githubusercontent.com for the STIX faces and verifies them against
`scripts/fonts/upstream.sha256`. That path had only ever run on the machine that
wrote it. It ran on GitHub's, and the proof is stronger than a green tick: the
three font files the deployed site serves are **162116, 82300 and 90256 bytes**,
which are byte-for-byte the sizes built here. The runner fetched the same
upstream files and hb-subset — pinned in `package-lock.json` — produced the same
bytes from them, so the subset is reproducible in the way §8.48 claimed rather
than merely believed.

**The gallery is typeset on the live site.** `examples/conditions.html` carries
five cases blocks and twenty `<math>` elements, `column.html` one and sixteen,
and every page references `../fonts/stix-two-math-subset.woff2` rather than
carrying a copy — one 162 kB file for 28 pages. That is §8.52's arrangement,
observed on the deployment rather than in `web/dist`.

The branch-versus-tag split held for the fourth time: the push to `main` ran
`pages` alone and skipped the other three; the tag ran the other three, skipped
`pages`, and published five artifacts in 2 m 46 s. **macOS agreed again**, which
is this tag's word on the central claim. No deprecation annotations, so the
action majors `v0.2.1` moved to are still current.

Before it was cut, every gate that can run on a development machine was run
here: 672 tests, 29 snapshots with one deliberately updated, native and
WebAssembly byte-identical, and all ten browser checks from an empty
`web/vendor` and `web/dist`. `compare-arch.sh` was not among them — `qemu-user`
is not installed here — and CI's `arm64` job is what covers it.

## Timings

`nomo bench` generates worksheets of fixed shape and times the whole pipeline —
parse, evaluate, render both views — through the same `snapshot` function the
WebAssembly build exports. It is a **report, not a gate**: it exits zero however
slow the news is, and CI runs it on every push the way it runs the coverage
report. A wall-clock threshold on a shared runner would be a flake generator,
and a flaky gate teaches people to re-run rather than to look.

Release build, this machine, 2026-08-29:

| case | time | per unit |
|---|---|---|
| wide, 1000 statements | 14.3 ms | 14.3 µs / line |
| wide, 5000 statements | 76.6 ms | 15.3 µs / line |
| chain, 3000 deep | 40.9 ms | 13.6 µs / line |
| map over 100k elements | 792 ms | 7.9 µs / element |
| eight plots | 13.1 ms | 1.6 ms / plot |
| edit one line of 5000 | 5.0 ms | 1 of 5000 evaluated |

The debug build is about three times slower across every case, and the report
names the profile it ran under so the two are not compared by accident.

Two of these are worth reading rather than just recording:

- **A call costs about 8 µs**, which is the environment copy `MAX_CALLS`
  already documents: a call clones the variables, functions, hints and unit
  table it runs in. A hundred thousand of them is 0.8 s, which is exactly the
  "second or so" that budget was chosen to bound. It is the obvious thing to
  make faster — the maps are read-only in the child scope and could be shared
  rather than copied — and nothing yet needs it.
- **Editing one line of a 5 000-line worksheet costs 5.0 ms** against 77 ms for
  the whole sheet, so the incremental path is worth about fifteen times. It is
  not fifteen *thousand*: `Sheet::update` re-parses the document, rebuilds the
  dependency graph and rescans resources on every edit, and only the
  *evaluation* is incremental. At the editor's 60 ms debounce that is invisible;
  it is the number that decides whether it stays invisible on a worksheet ten
  times longer.

## The golden-file suite

`nomo test` renders every worksheet under `examples/` and compares it byte for
byte with the snapshot committed in `tests/golden/`. `--write` regenerates.
`--examples` and `--golden` override the directories; otherwise it runs from the
repository root. CI runs the comparison, so a change that alters output cannot
merge until the snapshots are regenerated, which puts the behavioural change in
the same diff as its cause.

The snapshot is built by `nomo_core::golden::snapshot`, which is a pure function
of `(name, source)`. That placement is deliberate and matters for phase 7: the
WASM build calls the same function, so comparing native against browser output
compares the numerics rather than the CLI.

Four sections per snapshot: the rendered text, the HTML body, every result in
base SI at full precision, and the diagnostics.

### The corpus

Phase 6 took the corpus from four worksheets to eight and phase 7 added a ninth.
Each was chosen by asking what the suite could not see:

- `functions.nomo` — every transcendental the language has. **Nothing else under
  `examples/` called one.** The design's central claim is that transcendentals
  come from a vendored libm rather than the host's, so without this file phase 7
  would compare native against WASM and agree without having tested it.
- `temperature.nomo` — the affine point/interval rules, which were settled
  deliberately and were entirely unguarded.
- `matrix.nomo` — matrices, indexing, `det`/`inv`/`transpose`, element-wise
  versus matrix `*`. Its last line, `K*inv(K)`, records
  `[[1, -1.11022e-16], [8.88178e-16, 1]]`: accumulated float residue, and
  therefore a good canary for any target that reassociates.
- `diagnostics.nomo` — one deliberate mistake per line. Error messages are
  output too. This worksheet is *expected* to fail `nomo check`.
- `nonfinite.nomo` (phase 7) — every route to a NaN, both infinities, and signed
  zero, so the cross-target comparison covers the one value WebAssembly does not
  fully pin.
- `complex.nomo` — complex arithmetic with and without units, every function
  that takes a value apart, and the operations that are refused. Its last block
  is a series RLC impedance, which is what the corpus actually uses complex
  numbers for.
- `plots.nomo` — a plot's samples reach the snapshot at full precision, so the
  cross-target comparison covers the drawing as well as the numbers. It includes
  a curve with a gap in it and a flat one, which are the two cases where the
  drawing has to decide something.
- `llc.nomo` — a resonant-converter design carried out by the first-harmonic
  method. Several curves on one plot, a curve named by a definition, `roots` over
  a dimensioned span, `derivative`, a complex impedance taken apart by
  `Re`/`Im`/`abs`/`arg`, `min` over a mapped vector, and two figures — so its
  charts and its images go through the native-versus-WebAssembly comparison. It
  replaces an imported customer worksheet that carried the same features; that
  one was also the importer's own output verbatim, which this is not, and the
  loss is noted under "Third-party material".
- `interlock.nomo` — a sense chain and the four states it has to tell apart.
  The same names redefined once per scenario, which is what positional binding
  is for; a table of computed points plotted against each other; three figures.
  It replaces a second customer worksheet.

### The values section, and why the phase took longer than planned

Step 5 of the plan — perturb the evaluator and confirm the suite screams —
**failed on the first attempt, and the failure was real.** Changing `π` by one
unit in the last place left every rendered line identical, because results
display six significant figures. The suite passed. A snapshot of rendered output
alone cannot see last-bit drift, which is precisely the drift this project claims
to have eliminated and therefore precisely what the suite exists to catch.

So a snapshot also records each result's base-SI magnitude at full round-trip
precision (Rust's `{:?}` for `f64`: the shortest decimal that reads back as the
same bits — exact, and still legible in a diff). With that section the same
perturbation fails loudly, naming the file and both values. Two tests pin this:
`golden::tests::a_difference_below_the_displayed_precision_is_still_visible` and
`the_last_digit_is_not_forgiven` in the CLI's harness tests.

This mattered directly in phase 7: without the values section the
native-versus-WASM comparison would have been just as blind, and would have
"proved" the numeric thesis without testing it.

### Deliberate differences from CalcpadCE's `compare_renderings.py`

That script is the model (design note §9), and the departures are all in one
direction:

- **No tolerance, no reconciliation, no denylist.** CalcpadCE compares decimals
  within a tolerance and keeps a denylist of examples with "large precision
  errors", because it cannot promise the same answer on two machines. This engine
  can, so every one of those mechanisms would hide the bug being hunted.
- **The whole trace**, not final values.
- **Orphan detection.** A snapshot whose worksheet has been deleted is reported
  as an error rather than left to rot into a record of behaviour that no longer
  exists. Reported, never auto-deleted.

### Things the suite depends on

- `.gitattributes` pins `*.snap` and `*.nomo` to LF. Byte-exact comparison and
  Git's line-ending rewriting cannot coexist. If a snapshot ever fails with
  "differs only in line endings", that file is the reason.
- The text renderer avoids trailing whitespace and column alignment, both of
  which would make diffs noisy. Tests pin both.

## Cross-target determinism

`./scripts/compare-targets.sh` is the verification the numeric model exists for.
Four gates, each localising a different failure:

1. Build `nomo-wasm` for `wasm32-unknown-unknown`.
2. `scripts/check-wasm.mjs` — the artifact imports nothing and enables no SIMD.
3. `nomo test` — the native build matches the committed snapshots.
4. `scripts/compare-targets.mjs` — the WASM build matches those same snapshots.

Native == snapshots and WASM == snapshots together give native == WASM byte for
byte. **All 9 worksheets currently agree**, transcendentals and float residue
included.

Node is the WebAssembly engine and nothing more. No package is installed, and
everything under `scripts/` is dependency-free, because these scripts are part of
the evidence for the determinism claim and should be readable in one sitting.

### The second architecture

`./scripts/compare-arch.sh` is the same question asked across instruction sets
rather than across compilation targets. Three gates:

1. Build `nomo-cli` for `aarch64-unknown-linux-musl`.
2. No fused multiply-add in the artifact — see the gates section below, where
   this one is the interesting entry.
3. `nomo test` under `qemu-aarch64` — the aarch64 build matches the snapshots
   committed from x86-64.

**All 9 worksheets agree.** musl rather than gnu so that rust-lld links a static
binary and no cross-compiler or glibc sysroot has to be installed; the libc is
irrelevant to the answer, because `check-no-host-math.sh` already forbids the
engine from calling one for arithmetic.

The gates were both confirmed to fail when they should. Perturbing `π` by one
unit in the last place makes gate 3 report `cylinder.nomo` and both values, the
same negative control phase 6 used; forcing contraction makes gate 2 count 209
`fmadd` instructions and gate 3 fail on `matrix.nomo`.

### What this does and does not prove

Worth being precise, because the project's whole premise is at stake and this
would be the worst place for an overclaim.

**Proved.** Two independent compilations of the same Rust source — one to x86-64
machine code, one to WebAssembly bytecode then JIT-compiled by V8 — produce
identical bits across the corpus. That is a real test of the vendored-libm
decision: had the engine called a dynamically linked platform libm, the WASM
build could not have done the same thing and the two would diverge. The
zero-imports gate independently confirms the WASM side cannot reach the host at
all.

**Also proved, since `scripts/compare-arch.sh` was added.** Agreement across CPU
architectures. The engine cross-builds to `aarch64-unknown-linux-musl` and
renders the corpus under `qemu-aarch64`, byte for byte against the snapshots
committed from x86-64. All 9 worksheets agree. That is a genuinely different
instruction stream — different instruction selection, different register
allocation, different code paths through the vendored libm — not a re-run of the
same machine code.

**Still not proved.** Agreement on real ARM silicon. qemu is user-mode emulation
and computes floating point in software. For the operations this engine actually
performs that should be indistinguishable from hardware, because IEEE 754
specifies `+ - * /` and `sqrt` exactly and everything else is built from them in
Rust that qemu never sees — but "should be" is an argument again, and the point
of this section is not to make those.

The `arm64` job in CI closes it on real hardware, and it is written and
committed. The repository now has a remote — `git@github.com:rveen/nomo.git`
— so the job can run; whether it has, and what it said, is a question for the
Actions tab rather than for this file, which cannot see it.

### What the gates actually establish

- **Zero imports** is the strong one, and it is stronger than the plan asked for.
  The plan wanted "no math imports"; the artifact declares *no imports at all*,
  so it cannot call the host for anything — not `Math.sin`, not a platform libm.
  Checked with `WebAssembly.Module.imports()`, which is exact rather than a scan.
- **No SIMD** is read from the artifact's own `target_features` custom section,
  which LLVM emits listing the features the module was compiled against. Reading
  the artifact beats inferring from build flags that could be overridden. The
  module currently declares `+bulk-memory +bulk-memory-opt
  +call-indirect-overlong +multivalue +mutable-globals +nontrapping-fptoint
  +reference-types +sign-ext`, and no `simd128` or `relaxed-simd`. This is what
  closes the `relaxed_madd` hole: that instruction rounds once on hardware with
  FMA and twice without, which is exactly the drift this design forbids.
- **Wasm 3.0's deterministic execution profile** was *not* adopted. It remains
  unverified for engine support (design note §12), and the two gates above pin
  the same properties from the artifact side today. Worth revisiting when engines
  ship it; nothing depends on it.
- **No fused multiply-add** is the aarch64 counterpart, and it is the one gate
  here that was confirmed to guard a *measured* difference rather than a
  hypothetical one. aarch64 has FMA in its base instruction set, so LLVM could
  contract `a*b + c` into a single `fmadd` where baseline x86-64 cannot.
  `compare-arch.sh` disassembles the artifact and requires zero of them.

  Rust does not enable contraction, so the count is zero — but building the same
  source with `-C llvm-args=--fp-contract=fast` puts **209** of them in the
  binary, and the corpus then fails: `matrix.nomo`'s last line, `K·inv(K)`,
  renders `[[1, 0], ...]` instead of `[[1, -1.11022e-16], ...]`. The float
  residue this worksheet was added to preserve is exactly what a single rounding
  cleans away. The canary worked as designed, on the first target that could
  trip it.

### NaN

NaN payload bits, and the sign of a NaN computed from non-NaN operands, are the
only float nondeterminism WebAssembly admits. Normalisation happens where a NaN
could be observed — `golden::number` — rather than at the module boundary, since
the snapshot is the only place a value is written out today.

Rust's `Debug` for `f64` already prints `NaN` for every payload and both signs,
so the normalisation is currently a no-op. It is written out explicitly anyway:
the guarantee is load-bearing for the cross-target comparison, and resting it on
an undocumented formatting detail of the standard library is the kind of thing no
test would notice changing. `golden::tests::every_nan_is_written_the_same_way`
pins it against four different NaN bit patterns.

The infinities and signed zero are *not* normalised. They are exactly specified
by IEEE 754, so collapsing them would hide real differences;
`examples/nonfinite.nomo` pins every route the language offers to each.

## The browser editor

`./scripts/build-web.sh` builds the engine, bundles the front end into
`web/dist/`, and then checks the result in headless Chrome. `cd web && node
build.mjs --serve` watches and serves on :8000.

Static files and nothing else: the page is HTML, CSS, one bundle and one `.wasm`.
No backend, no network traffic after load, and no worksheet leaves the tab.

### Highlighting comes from the engine, not from a grammar

There is no CodeMirror language mode. `nomo_core::api::classify` walks the
lexer's tokens and the parser's AST and labels each one — `unit`, `variable`,
`function`, `constant`, `keyword`, `unresolved` — and the front end turns that
list into decorations. `web/src/highlight.js` decides nothing.

This is invariant 1 applied where it is easiest to break. A CodeMirror mode
listing the units would be simple to write and wrong within a week: the first
unit the engine learned and the mode did not, the editor would colour a worksheet
differently from how it computes it. That split is exactly what CalcpadCE has
between `Calcpad.Core` and `Calcpad.Highlighter`, and design note §10 calls it a
permanent liability.

It also does what a grammar cannot. `m` is coloured as a unit or as a variable
depending on whether *this worksheet* bound it, which is knowable only after
evaluation. `classify_ident` resolves names in the evaluator's own order —
variable, then constant, then unit — so the two cannot disagree.

`eval::BUILTINS` is the one place the function names are listed for a caller that
needs to recognise one without calling it. It is a second list next to
`call_builtin`'s dispatch, so `builtins_match_the_dispatch` fails if they drift.

### The boundary carries JSON, and `wasm-bindgen` stayed refused

The decision left open at the end of phase 7. The module gained three exports —
`nomo_document_new`, `nomo_document_update`, `nomo_document_free` — over the
existing convention, and `analysis_json` hand-writes the payload. **The artifact
still imports nothing**, which `check-wasm.mjs` asserts on every build.

`nomo_document_*` holds a `Sheet` between calls, which is what reaches the
phase-4 dirty-subgraph path: editing one line re-evaluates that statement and its
dependents, and the count is reported in the payload and shown in the status bar.
Editing `a` in `a = 1 / b = a*2 / c = 99` recalculates two statements, not three.

`crates/nomo-wasm/boundary.mjs` is the host half of the calling convention, kept
beside the Rust that defines the other half because the two are one contract.
Node's scripts and the browser both use it, so there is one implementation rather
than two that can drift.

### The bug that only a browser could find

**Every highlight in the editor was two columns to the right of the text it
described**, and every test was green. Rust indexes strings by UTF-8 byte and
every `Span` is a byte range; JavaScript and CodeMirror index by UTF-16 code
unit. The starting worksheet contains an em dash — three bytes, one code unit —
so everything after it slid.

`api::Utf16Offsets` converts, and the payload now documents that **all offsets
are UTF-16 code units**. Five unit tests cover it, including the four-byte
character that is a surrogate pair and therefore two units rather than one.

The lesson for phase 9 and after: this class of bug is invisible to `cargo test`
by construction, because the byte offsets were all perfectly correct — for a host
that counts bytes. `scripts/check-browser.mjs` exists because of it, and asserts
that the span wrapping `cm` contains exactly `cm`. Reverting the conversion makes
it fail, which was checked.

### What the browser checks cover

- `check-session.mjs` — the editing path in Node against the real module: edits,
  incremental counts, recovery from a worksheet that briefly stops parsing, and
  UTF-16 offsets. Everything a keystroke touches except CodeMirror itself.
- `check-browser.mjs` — loads the built page in headless Chrome with
  `--dump-dom`, then asserts the worksheet evaluated to the same value the CLI
  gives and that highlighting landed on the right characters.
- `check-print.mjs` — drives Chrome over the DevTools protocol, emulates print
  media and asks the page what is visible. Header, footer and editor must be
  gone; the worksheet must remain, must not clip and must wrap. Both checks were
  confirmed to fail when the thing they check is broken.
- `check-assist.mjs` — types into the editor and reads what appears: the
  completion list and what it says beside each name, the hover tooltip, where
  F12 leaves the cursor, and whether the Typeset toggle produces a fraction
  whose numerator sits above its denominator. It drives everything through the
  DOM rather than through CodeMirror's own API, because the editor does not
  publish its view object and a test hook in the application to let a check
  reach it would be a worse trade than measuring what the page shows. F12 is
  dispatched as a page event: a browser keeps that key for its own developer
  tools, and what is under test is the editor's binding.
- `check-mathml.mjs` — renders a worksheet with `--mathml`, then asks the page
  where the numerator of a fraction ended up. It is the one check that could not
  be replaced by an assertion on the markup: a browser that does not implement
  MathML draws `<mfrac>` as a run of characters rather than failing, so the
  worksheet would read `w · L 2 8` and every markup assertion would still pass.
  Confirmed to fail against output rendered without the flag.
- `check-recovery.mjs` — sabotages the engine so that one `update` throws, as a
  trap would, and asserts the editor replaces the instance and carries on with
  the buffer intact. It exists because the failure it guards against was
  permanent and silent, and because nothing a user can type can produce it any
  more: the parser's nesting limit closed the one route to it, which left the
  recovery with no natural way to be exercised. Confirmed to fail with the
  recovery removed.

`scripts/chrome.mjs` is a ~150-line CDP client over Node's built-in WebSocket. It
is not a browser automation framework and should not become one; if a check needs
more than it offers, that is the moment to reach for a real one.

### Two build systems, and why that is not the thing that was refused

`cargo` builds the engine and `esbuild` bundles the interface. The WebAssembly
artifact whose reproducibility this project rests on is produced by cargo alone,
with no JavaScript tool anywhere in that path. That is the difference between
esbuild, which is fine, and `wasm-bindgen`, which was refused: a bundler for the
user interface is downstream of the guarantee and cannot affect it. `web/` has
its own `package.json`; `web/node_modules` and `web/dist` are not committed.

## Files, drafts and offline

### Two ways to open a file, because browsers differ

The File System Access API yields a *handle*, so Save writes back to the file
that was opened. Chrome and Edge have it; Firefox and Safari do not, and
supporting all of them is a hard requirement inherited from
EngineeringPaper.xyz (design note §11 item 6).

So there are two paths, and the difference is shown rather than hidden. With a
handle there is a **Save**; without one the button says **Download**, because a
Save that quietly drops a second copy in `~/Downloads` is worse than a button
that says what it does. `web/src/storage.js` has both.

`check-files.mjs` exercises both, and produces each deliberately rather than
inheriting whatever the running browser supports — one injected script reads the
query string and either removes the API or stubs the pickers. Worth knowing:
**Chrome does expose the API on `127.0.0.1`**, because that is a secure context,
so an earlier version of this check silently tested the same branch twice.

### Saving stamps the version pragma, and the engine decides how

`doc::stamp_version` adds `' nomo 1` to a worksheet that has none, and the front
end asks for it through `nomo_for_saving` rather than composing the line. The
version number and the pragma's spelling are facts about the format; a
JavaScript function writing `' nomo 1` would still be writing `1` long after
`CURRENT_VERSION` said otherwise.

A worksheet that already declares a version keeps it — **including one from the
future**. Relabelling a version 99 file as version 1 would turn "I cannot fully
read this" into silent corruption the next time a build tried to migrate it.

The stamped text goes back into the editor after a save, so the buffer and the
file never differ by a line nobody can see.

### The draft is a safety net, not storage

Whatever is in the editor is written to IndexedDB on a debounce and restored on
load. The file on disk is the document; the draft only exists so that closing a
tab does not lose an hour.

Every storage failure is swallowed. Private browsing, a denied quota and a full
disk all make IndexedDB throw, and none of them is a reason to stop someone
editing. `loadDraft` also has a **timeout**, because IndexedDB can block
indefinitely rather than fail — a blocked upgrade, a database open in another
tab — and startup waits for it. A worksheet application showing nothing at all
because it is still asking whether there was a draft has failed at the only job
that matters.

### Offline

`web/src/sw.js` caches five files and there is nothing else to cache. Offline is
not a degraded mode here: no computation happens anywhere but this tab, so a
worksheet opened on a train gives the same answers as one opened at a desk.

`check-offline.mjs` types into the editor, reloads, cuts the network with
Chrome's network emulation, reloads again, and requires the worksheet to still
evaluate. It also **counts requests to its own server** and fails if any arrive
while offline: a page can look like it works offline because everything is still
in the HTTP cache, which is not offline support but luck with a short shelf life.
The test server sends `no-store` for the same reason. Disabling the service
worker registration makes this check fail, which was verified.

### A check that broke for an instructive reason

`check-browser.mjs` used `--dump-dom --virtual-time-budget`, which was simpler
than driving the protocol. It broke the moment startup touched IndexedDB: virtual
time advances timers, a database request is not a timer, and the page never
finished starting — so every assertion in the file failed at once, in a way that
looked like the front end had collapsed.

It is now on the same CDP path as the other three, waiting for
`document.body.dataset.ready` instead of for a clock. **Waiting for a specific
signal beats waiting for a duration**, and the suite now has one mechanism rather
than two.

## The SMath reader

`crates/nomo-smath` is the first stage of the importer specified in design note
§8. It **reads**; it does not evaluate and it does not yet write Nomo documents.
That split is deliberate and follows §8.8: run a reader over every worksheet
available, report what it cannot handle, and let the counts decide the order of
the work, because deciding what `el` or `if` should become is wasted effort if
the corpus turns out to hinge on something else.

`smath-coverage` over the 54-worksheet corpus, today:

```
54 worksheets read      35 legacy (pre-0.88), 19 modern (0.88+)
3878 regions            2162 math, 1443 text, 106 area, 99 picture, 49 plot, 19 unsupported
2162 math statements    1196 positional, 149 global (≡), 74 stated equation (≡, binds
                        nothing), 247 display-with-answer, 496 bare
553 stored answers      across 51 of 54 files — 247 legacy, 306 modern, 197 naming their unit
2809 function calls     2135 built in, 650 defined by their own worksheet, 24 unknown
27 units on the input side
```

**Every file reads, and 99.1% of function calls resolve.** The 24 that do not are
eight names in five files, and each was checked by hand rather than assumed: two
are chained `≡` definitions whose target is an expression rather than a call,
one is an undefined `Max`, one is defined by the unidentified `—` operator, and
the rest are of a piece. None is a false positive from a thin registry, which is
the property that makes the report worth trusting.

### Why the report exists before the emitter

Because writing the reader corrected the design note six times, and each
correction would have been an expensive rewrite if it had been discovered after
the semantics were built on top of it. They are recorded in design note §8.9. The
one that matters most: **the format has two eras, splitting at version 0.88**, and
an importer written to the shape §8 originally described finds *no math at all*
in 19 of the 54 files while reporting a clean import.

The second most important is quieter. From 0.88 a collapsible `<area>` **contains**
the regions it hides rather than marking them, and 442 of 3878 regions are nested
one or two levels down. The first version of the reader took only the top level,
and its counts looked plausible — about eleven per cent low across every category
at once, which is exactly the kind of wrongness that survives review. It was
caught by comparing against the corpus README's independently measured totals,
which is the argument for having such a number written down somewhere.

### What the reader does not decide

- **Nothing is translated on a guess.** `|`, `—` and `†` are narrowed in §8.9 but
  not implemented; they are reported. So is every unknown function name.
- **Nothing is silently dropped** (§8.7 item 23). A construct the reader cannot
  handle becomes an `Unsupported` marker that keeps whatever *did* reduce beneath
  it, so a malformed region still reports the four functions it calls.
- **Which language to keep** in the 97 bilingual text regions is left open; the
  reader keeps every variant and the report counts them.

### The one dependency

`roxmltree`. The engine has exactly one dependency and that restraint is worth
keeping, but a migration tool that corrupts a worksheet through a hand-rolled XML
scanner is the worst outcome available here, and these files carry entity
references and multilingual text. It is pure Rust, has no unsafe, and its
read-only tree matches how regions are read: in file order, once. It is confined
to this crate; `nomo-core` remains dependency-free apart from `libm`.

## The oracle: checking Nomo against SMath's own answers

The importer now emits `.nomo` source and the harness runs it. Over the wiki
corpus:

```
507 stored answers in 51 worksheets

      304  agreed
        2  answer and result are different shapes
       33  disagreed
      162  line did not evaluate
        6  stored answer unreadable

    304 of 337 comparable answers agree (90.2%)
    337 of 507 answers could be compared at all (66.5%)
    11 of 51 worksheets check out completely
```

and over the mechanics corpus:

```
672 stored answers in 58 worksheets

      283  agreed
       10  answer and result are different shapes
      373  line did not evaluate
        6  stored answer unreadable

    283 of 283 comparable answers agree (100.0%)
    283 of 672 answers could be compared at all (42.1%)
    5 of 58 worksheets check out completely
```

The mechanics corpus leans on the CAS — 30 of its 60 worksheets declare the
Maxima plugin — which is why fewer than half its answers can be compared at all,
and why none of the ones that can disagrees.

**The number that matters is not 90.2%.** It is this: of the 33 disagreements,
**none is in a worksheet that translated completely.** Every one is in a
worksheet where some construct was dropped, and the mechanism is worth
understanding because it is the failure mode this project exists to avoid.

### A dropped construct does not produce a missing answer. It produces a wrong one.

Four of the distribution worksheets converge a value in a `while` loop, which the
importer does not translate. The loop region becomes a marker comment — visible,
counted, honest. But the *variables* the loop would have updated keep the values
they were initialised with, and the lines that display them still evaluate
perfectly well. `chisquareddist.sm` reports its iteration count as 1 where SMath
stored 5, and its answer as 50 where SMath stored 15.9872. Nothing errors.

So the report splits disagreements by whether the worksheet translated in full,
and the split is currently 0 / 33. Averaging them would have hidden both the good
news and the bad.

### The tolerance, and the one place tolerance is allowed

A stored answer is what SMath *displayed*, so the comparison gets half a unit in
the last displayed place — the only tolerance anywhere in this project, and it
exists because the other side of the comparison is a decimal string written by a
different program, not because this engine is uncertain. The golden-file suite
still compares bit-exactly and must keep doing so.

Getting the tolerance right took two goes, and the first was wrong in an
instructive way. SMath writes a large answer in scientific form — `1.8491*10^5` —
and its `precision` setting counts decimals **of the mantissa**. Reading that
setting as decimals of the *value* made the tolerance 100000 times too tight, and
the corpus dutifully reported five correct answers as disagreements.

The fix was to stop consulting the setting. The stored literal states its own
precision: `1.8491` means ±0.00005 of a mantissa, and scaling by
`expected / 1.8491` carries that through the exponent and the units at once. That
is both simpler and more truthful — it reads the precision off what SMath
actually wrote rather than off a document setting that may not describe this
region.

### What the 535 non-evaluating lines are

Coverage, not correctness. Why a line with a stored answer failed to evaluate,
162 in the wiki corpus and 373 in the mechanics one:

| Why the line did not evaluate | wiki | mechanics |
|---|---:|---:|
| `…` has no value: the statement that defines it failed | 94 | 157 |
| `…` is not defined | 56 | 79 |
| `…` is not a known function | 4 | 64 |
| a subexpression could not be evaluated | 6 | 34 |
| a dimension clash | — | 33 |
| a derivative of a derivative | — | 4 |
| wrong number of arguments | 2 | 2 |
| **total** | **162** | **373** |

Most of those are second-order: a name has no value *because* the statement that
defines it was the one refused. What ranks the remaining work is the other list
the report prints — what could not be translated — headed in both corpora by free
symbols (159 and 180 regions), then a definition whose body calls a function with
no Nomo equivalent (79 and 132), then a displayed expression that does (8 and
124), then assignment into an index (33 and 64).

`if`, `while`, `for` and `line` dominated when this was first written, and the
language has since grown all of them: `if` is an expression, and a `for` that
fills a vector is `map` (see "Loops" above). What is left of `for` is recurrences
and accumulators, and what is left of `while` is tolerance loops — 13 regions,
and a different problem.

### Free symbols: the last silent failure the emitter had

A worksheet can use a name it never defines. SMath allows it where the region is
set to symbolic optimization — `optimize="2"`, 352 regions across 49 files of the
three corpus sets — because its CAS keeps the name as a symbol instead of
failing, and the file then saves with no `error` attribute to find. Nomo has no
free symbols, so the importer was emitting those lines as source and the
worksheet opened in the editor with undefined names and no marker to explain
them.

The interlock worksheet is the case that surfaced it: `Vout : …*V2 - …*V1`, where `V1`
and `V2` appear once each in the whole file and are bound nowhere. It is the
generic form of the amplifier's transfer function, written for a reader, and the
same `Vout` is reassigned four lines later with the values substituted. Nothing
is wrong with the worksheet.

A statement using a name the document never binds is now a marker plus the
translated line as a comment. Across both corpora that is **268 regions** — unit
labels typed as bare math (`ft`, `s`, `lb`), the symbolic-solve unknowns
(`A.x`, `S.1`) that go to `Solve`/`Assign`, a free `i` for the imaginary unit,
and the interlock worksheet. No worksheet lost an agreed answer and none gained a
disagreement; at that commit `unsupported` rose 562 → 732 (wiki) and
1126 → 1224 (mechanics), which is 268 gaps that were previously invisible. Those
four totals are the reading *then* and do not compare with what the report prints
now: a region carrying two markers has since become one note rather than two, and
the current totals are 582 and 827. Design note §8.20.

Not followed: the cascade. A commented-out definition leaves its dependants
undefined and those lines still emit, because deleting a whole dependency chain
would hide an error whose cause is marked three lines above it.

## A bug in the engine, found by the importer — fixed

**A failed binding used to fall through to a unit, so the worksheet reported a
confident wrong number instead of an error.**

```nomo
PF = undefined_thing     ' error: `undefined_thing` is not defined
PF                       ' 1e15 F        ← before
PF                       ' error: `PF` has no value…   ← now
```

`PF` is not a unit anyone would write; it is a *prefixed* one, peta-farad, and
the two-letter space of every SI prefix against every unit symbol is large enough
that ordinary variable names fall into it. `Zs` is zetta-seconds. Both appear in
the corpus — `ElecEngExample.sm` reported a power factor as 1e15 F and
`aircraft_performance.sm` a service ceiling as 1e21 s — and the oracle found both,
not any test here.

The engine already knew better. It emits `SH202`, "`PF` is also a unit; this
binding hides it for the rest of the worksheet", on the very line whose failure
then handed the name back. **The fix is to keep that promise when the binding
fails**: `Env` tracks names whose defining statement produced no value, and a use
of one reports `DefinitionFailed` rather than resolving onward. A name that
*nothing* binds is still a unit — the fall-through is right, and only wrong when
something tried to take the name.

Three details that are easy to get wrong and are pinned by tests:

- **A failed rebinding takes the earlier value with it.** `x = 1 m` … `x = <error>`
  … `x` reports the failure rather than 1 m, because a use takes the nearest
  definition above it and that is the one that failed.
- **A binding that recovers is available again**, so incremental recalculation
  while someone is typing does not leave a name poisoned.
- **A function parameter outranks a failed binding of the same name.** A body
  sees the definition site's bindings, so a failed one reaches it; a parameter is
  a real binding and wins.

Constants are covered by the same rule as units: `pi = <error>` followed by `pi`
is an error, not 3.14159.

### What it changed in the corpus

Nothing computes differently. Five answers that were fabricated stopped being
answers: disagreements fell from 32 to 27 and non-evaluating lines rose from 161
to 166, with the agreed count unchanged at 187. The agreement rate moved from
85.4% to 87.4% purely because five wrong numbers left the denominator.

That is the whole value of the fix, and it is worth stating plainly: it makes no
worksheet more correct, and it stops five of them lying.

## Conditions

Comparisons, `and`/`or`/`not`, and `if … then … else`. This reopens the v1 scope
decision, which had deliberately excluded conditionals — done at the user's
request, and worth recording as a decision rather than drift.

The corpus is what argued for it. `if` is 131 uses on the input side and the
comparison and logical operators are another 208, so this was the single largest
block of untranslatable material by a wide margin.

### The three decisions inside it

**A comparison answers the dimensionless 1 or 0; there is no boolean type.**
Adding one would touch every arm of the value tower to buy an error message,
while SMath — whose worksheets this language has to be able to receive — already
computes with comparisons as numbers. What is enforced instead is the part that
catches real mistakes: a condition must be *dimensionless*, so `if x then …` with
`x` in metres is an error rather than a coin toss.

**`if` is an expression, not a statement.** It composes with arithmetic and lets
a function body be piecewise without any new statement form, which is what
`fn stress(f, a) = if a > 0 m^2 then f/a else 0 Pa` needs. Both arms are
required: an `if` with no `else` would have to mean something when the condition
is false, and in a language where every expression has a value there is no honest
answer.

**Only the arm that is taken is evaluated**, and `and`/`or` short-circuit. This
is not an optimisation. It is what lets a guard guard — `if n > 0 then bay[n]
else 0 m` must never index at zero — and it means the untaken arm raises no
diagnostic about work nobody asked for.

### What laziness costs the trace

A worksheet shows its work, so the arm that did not run still has to be *shown*
even though it has no values. `eval::sketch` mirrors the unevaluated arm into a
trace whose nodes carry `EvalError::NotTaken`, and `Trace::children` filters
those out so error search never descends into work that did not happen. One rule
covers the short-circuited operand of `and`/`or` too, since it is marked the same
way.

The renderer then shows the taken arm substituted and the untaken arm as written:

```
margin = if too_long then span - allowed else allowed - span
       = if 1 then 6 m - 4 m else allowed - span
       = 2 m
```

Numeric literals keep their value inside a sketch, because a literal needs no
evaluating to be known and without them an unrendered arm would print as a row of
question marks.

### The dependency graph over-approximates, deliberately

Both arms are dependencies even though one will not run. Which arm wins depends
on values and the graph is built before any value exists, so `collect_names`
descends into both. That can only add an edge, never lose one, and a lost edge is
a stale result on screen.

### Effect on the corpus

Agreed answers went from 187 to 215 and comparable ones from 214 to 242 — the
share of stored answers the harness can check at all rose from 54% to 61%. Those
are the wiki corpus at that commit; it reads 304 of 337 today. The disagreement
split is still 0 in worksheets that translated completely.

## Repetition, and the loop that was not written

Loops were asked for alongside conditionals. What went in is `range`, `map` and
`iterate`, and the shape of that answer is the interesting part.

**SMath's loops mutate.** `for(k, range(1, length(yy)), line(yy[k] ← hPS(xx[k]),
zz[k] ← hPP(xx[k])))` is a real corpus region, and it assigns into vectors by
index inside a statement block. Design note §6 says a worksheet is a set of
definitions with dependencies rather than a script, and adding mutation at
worksheet level would contradict that directly — the dependency graph would stop
describing the document.

So the corpus was read for what the loops are *doing* rather than what they are.
Nearly all of them build a vector element by element from another vector over a
range, which is `map`; the rest accumulate, which is `map` and then `sum`; and a
few converge, which is `iterate`. None of the three mutates anything, none needs
a statement form, and the DAG is untouched.

- **`range(a, b[, step])`** — the loop counter, and on its own the commonest
  `for` in the corpus. Dimensioned, so tabulating a physical quantity needs no
  trick. Elements are `a + i*step`, not repeated addition: ten additions of `0.1`
  reach `0.9999999999999999` where ten times `0.1` is exactly `1`, and a test
  pins that.
- **`map(f, v)`** — what `for(k, …, y[k] ← f(x[k]))` means.
- **`iterate(f, x, n)`** — a convergence loop with a fixed count rather than a
  tolerance test. It terminates, takes the same number of steps everywhere, and
  cannot spin.

`map` and `iterate` take the **name** of a function. That is as far towards
higher-order as the language goes: no lambdas, no closures, no function values,
and `Expr::Call`'s callee is still always a name, which the whole evaluator
assumes. A function name gets its own trace leaf, `TraceNode::FnRef`, so the
rendered line still reads `map(step, xs)`; it is marked not-evaluated, since a
function is not a value and nothing should try to print one for it.

Ranges and repetition counts are capped at a million. Not a tuning knob: a
browser tab has no way out of a hang.

### The importer could not yet translate a `for`, deliberately

**Superseded** — the element-wise fill translates now; see "39 are an
element-wise fill" below. Kept because the reason it was deferred is the guard
that case still applies.

The language had the constructs; the importer did not yet recognise SMath's loop
shapes and rewrite them, so the corpus numbers did not move. That was a separate
piece of work and it was left undone rather than done badly.

The reason is worth recording. Rewriting `for(v, range, target[v] ← body)` into
`target = map(fn, range)` is only correct if the loop fills the whole of
`target`, and the corpus contains cases where it does not — `Finite differences.sm`
runs `for(i, range(0, n), x[i+1] ← …)`, whose indices are offset by one from its
range. A rewrite that assumed otherwise would produce a worksheet that evaluates
cleanly and answers wrongly, which is the exact failure this project is built to
prevent, and it would be invisible: the oracle counts a stale-but-plausible
answer as a disagreement only when a stored answer happens to sit on that line.

So `for`, `while` and `line` were reported as unsupported. What would make a
`for` translatable is a check that the loop's range and the assigned index agree,
which is real analysis rather than a pattern match — and that check is what the
later commit wrote. `while` is still refused, and for a different reason.

## Three silent gaps between the emitter and the oracle — closed

Asking whether the converter worksheet could be imported completely turned up three
faults that had nothing to do with what that worksheet was being asked about, and
all three were **silent**: the import reported no marker, the counts looked
healthy, and the worksheet was wrong or unchecked anyway.

**A call to a function the worksheet defines was never written out.** The
emitter resolved a callee against the built-in registry and refused anything
else, so `f(x) : x^2` emitted as `fn f(x) = x^2` and the `f(3)` two lines below
it became a marker. 650 call sites across the two corpora, and the failure had
been invisible because it is reported under the same wording as a genuinely
unknown SMath function. A worksheet's own function is now looked for first —
the order SMath resolves in — and the call is emitted whether or not the
definition survived translation, so a broken definition cascades to
`… is not defined` exactly as a broken *variable* definition already did,
instead of being reported twice under two different names.

**A definition's stored answer was thrown away.** `<result action="numeric">`
became an assertion only under a display region. The newer era keeps the answer
beside the definition, so of the converter worksheet's 34 stored answers, 28
reached nothing.
A definition asserts the value of what it binds exactly as a bare display of that
name would, and the oracle already handled the `Assign` outcome — the answers
were simply never offered to it.

**A unit that appeared only in a stored answer had no Nomo spelling.** The
name table was built from the input side, and a worksheet routinely states its
inputs in one unit and gets its answers in another: the converter worksheet is
written in volts and amps and SMath answers in ohms, farads and henries. Those assertions could
not be written down, so they were quietly not made. The same bug the plot
payloads had, one payload further along, fixed the same way.

Together these took both corpora from 285 of 319 comparable answers to **442 of
472**, and the converter worksheet from one checked answer to thirty, all agreeing. That pair
is the reading at that commit — the same two corpora are 587 of 620 today.

### What the new assertions immediately found

Six of them disagreed, in three mechanics worksheets, and every one was a real
defect the corpus had been carrying unseen: **a variable named after a unit the
same worksheet uses.** SMath tells `m := 1 kg` from `d := 1 N*(m/s)^-1` by an
attribute on the operand; Nomo has one namespace, and a binding hides a unit of
the same name for the rest of the worksheet. So every length below became a mass.
`Auflage 1/10.1_ZI.sm` computed `sqrt(2*g*r*sin(90°))` as `4.42945 kg/s` where
SMath says `4.429 m/s` — **the right number with a nonsense dimension, and
nothing in the output saying so.** Only the stored answer's *dimension* catches
that, which is why it stayed hidden while the answers went unchecked.

The variable now moves aside — `m` becomes `m_` — and the unit keeps the
spelling, because the unit's is fixed by the language and the variable's is not.
It is reported as a `Renamed` note rather than done quietly, since the reading
Nomo would otherwise have taken is the plausible one. The rename fires only on a
real collision: a worksheet with a mass `m` and no length in it keeps the name
its author wrote. Mechanics agreement is now **183 of 183**.

The two disagreements that remain new are in `NewtonRaphsonApplications.sm`,
where a `while` loop is dropped and `xG` still holds the initial guess — the
stale-value mechanism described above, now visible on two more lines because
`f(xG)` can finally be written.

## Complex numbers, and the two decisions inside them

Design note item 29 reserved the shape and named the requirement: `Re`, `Im`,
`conj`, `arg` and an imaginary-unit operand `i`, **with units attached** —
`(1 + 2i)·Ω`, results stored in `VA`. That is now built for scalars.

`i` is a constant, like `pi` and `e`, and needs no number syntax of its own:
juxtaposition is already multiplication, so `4i` is `4*i` and `4i Ω` is
`4*i*Ω` — the reading that already makes `2e` mean `2*e`. A binding wins over
it, so a worksheet using `i` for a current keeps it by saying so.

**One dimension, not two.** Both parts are components of a single measurement,
so they share a dimension and every operation is a rule already written for
`Quantity`, applied once. `1 m + 2i s` is an error rather than a value with two
dimensions in it, and the renderer writes the unit once, outside the brackets.

**Division is Smith's method, written out rather than taken from a library.**
The textbook conjugate formula computes `c² + d²`, which overflows to infinity
and then returns NaN for operands whose quotient is an ordinary number — `1e200`
squared is not an `f64`. Smith's divides through by the larger part first, uses
only the four operations IEEE 754 specifies exactly, and is therefore
bit-reproducible. Which formula is used decides the last bits, so it is part of
the language and not an implementation detail. `compare-targets.sh` confirms the
new snapshot is byte-identical native and in WebAssembly.

Two decisions worth keeping in view:

**A complex value never becomes real again on its own.** `(1 + 2i) - 2i` stays
complex and displays as `1 + 0i`. Demoting when the imaginary part happens to be
zero would make a result's *type* depend on its value, and depend on it through
a floating-point comparison — the imaginary part of a real computation is hardly
ever exactly zero, so the rule would fire on some worksheets and not on others
differing in the last bit. `Re(z)` is how a reader asks for a real.

**Whole exponents only.** `z^2` is repeated multiplication, one step at a time
for the reason `iterate` takes one step at a time. Anything fractional needs a
complex logarithm, and a complex logarithm needs a branch cut — where `arg`
jumps from `π` to `-π` — which decides `(-1 + 0i)^0.5`. There is a conventional
answer and no way for a worksheet to say it meant the other, so it reports that
it cannot rather than choosing quietly. Same reasoning for `sqrt`, `exp`, `ln`
and the trigonometric functions of a complex argument, and for a complex
exponent.

### What it did to the corpus

`ElecEngExample.sm` — a worksheet whose whole subject is the complex domain —
went from **0 of 8 answers agreeing and 6 markers** to **7 of 10 agreeing and
none**. It also turned out to define its own `conj` as `abs(X)^2/X`, which is
the first corpus worksheet to define a function whose name Nomo now also has:
the resolution order chosen in the previous commit — the worksheet's own
function first, because that is SMath's order — is what sends the call to the
right one, and it stopped being hypothetical the moment `conj` became a builtin.

In the converter worksheet the whole impedance chain imports live: `Z_LLC_eq`,
`Z_LLC_real`, `Z_LLC_im` and `Z_LLC_eq_abs` emit as ordinary definitions where
five markers stood. Its markers are down from 22 to 16, and what remains is
plots, `sys`, `diff`, SMath's range `solve`, and four decorative labels.

## Plots

`plot(f, a, b)` samples a function across a span and draws it. It needed no new
syntax and no new idea: `map`, `iterate`, `root` and `integral` already take the
**name** of a function, and `plot` joins them.

**A fixed number of samples — 257 — whatever the span.** `integral`'s rule, for
`integral`'s reasons: the drawing terminates, and it is the same drawing on
every machine, which is what lets a plot into the golden suite at all.
`compare-targets.sh` reports the new snapshot byte-identical native and in
WebAssembly, and that covers the tick positions as well as the curve, because
choosing where a gridline goes needs `log10`, `floor` and `powf` and those come
from the vendored library like everything else.

**The engine draws the SVG.** A charting library would have cost both of the
properties `nomo html` exists for: the file would need a script the reader must
trust, and the drawing would happen on the reader's machine with the reader's
floating point. Emitting geometry as markup keeps a plot in the same category as
every other result — computed once, deterministically, here.

**A plot is a value, and not an image.** `resource.rs` holds that an image
cannot be produced by an expression, and that is still true: a figure is scanned
evidence the worksheet carries, a plot is a *result* that recomputes when an
input above it changes. They are different things that end up looking similar on
the page, so a plot is a `Value::Plot` and goes nowhere near the resource
trailer. There is no arithmetic on one — every operation refuses it by name.

Two smaller decisions. The **horizontal** axis is exactly the span that was
asked for, because `plot(f, 30 kHz, 200 kHz)` said where to look; the
**vertical** one is fitted to the data and rounded out to whole ticks, because
nobody chose it. And a sample that is not finite is drawn as a **gap** — the
curve arrives as several polylines — rather than a line through values the
function never took.

### `scripts/check-plots.mjs`, and why it exists

A plot's markup carries class names and **no colours**, so what a reader sees
depends on three files agreeing: `render/plot.rs` emits the classes,
`render/html.rs` styles them for a standalone file, and `web/style.css` styles
them for the editor's output pane. The third was missing, and nothing noticed:
every Rust test passed, the golden suite was green, `nomo html` was correct,
and the editor drew a black blob on an invisible grid — because an unstyled
`<polyline>` fills its own chord and does not stroke.

That is the same shape of failure `check-figures.mjs` was written for, and it
gets the same answer. The check types a worksheet into the real editor and reads
*computed* styles: that the curve is stroked and not filled, that the grid and
labels take the page's own colour so the chart follows the theme, that the SVG
lays out with a real size, that all 257 samples are in the drawing, and that a
function which leaves the plane arrives as two polylines. Removing one line from
`web/style.css` makes it fail with "the curve is filled (rgb(0, 0, 0))", which
is the bug it was written for, stated as the reader would experience it.

### Several curves on one plot

`plot(light, nominal, heavy, 30 kHz, 200 kHz)`: every argument but the last two
names a curve. This is the corpus's `sys(...)`, and it needed no new syntax
either — the leading arguments are function names, exactly as `plot`'s first one
already was.

**The names are counted from the end, not sniffed.** `named_arity` says a
`plot` of *n* arguments has *n−2* names, so what a call means never depends on
what is in scope: `plot(f, a, b)` reads the same whether or not something called
`a` exists. Sniffing which of the leading names happen to be defined functions
would have made a worksheet's meaning change when a variable was added far
above it.

**One vertical axis still means one dimension** — now across the curves as well
as along each. A gain drawn beside a length is refused when the plot is built.
That was already the promise `PlotValue` documented about its series; this is
the first time there could be more than one to keep it about.

**A family of curves is written by naming its members.** `fn light(f) =
gain_at(f, 0.2)` and its two siblings are what a language with no lambdas has
instead of one, and it is what the engineer wanted anyway: the load a design is
nominal at ends up on the page under its own name. `examples/plots.nomo` draws
the LLC gain at three loads that way, which is the picture a resonant-converter
worksheet exists to produce — and `examples/llc.nomo` is a whole one.

**Colour is the stylesheets' business, and the legend is drawn from the same
class.** A curve carries `plot-curve` and `plot-curve-N`, cycling at six — the
Okabe–Ito colourblind-safe palette without its yellow and its black, neither of
which reads on both a white page and a dark one. Unlike the structure, a curve
cannot take `currentColor`: curves have to differ from each other as well as
from the ground. The legend sits in a strip *below* the drawing rather than
floating in a corner, because a plot's corners are where an interesting curve
usually is, and its swatch takes the curve's own class so the two cannot drift
apart. A single-curve plot is drawn exactly as before — no strip, same
viewBox — so only `plots.snap` moved, and only by the added class and the new
worksheet.

`check-plots.mjs` grew the case: two curves must compute to two *different*
strokes and the legend swatch must match the curve it names, which is again
something only a browser knows. It also had to learn to wait for the legend
count and not only the polyline count — a reciprocal is one curve in two
polylines and a pair of lines is two curves in two, so the previous chart could
answer for the next one.

### A table of measured points

`plot(m)`: an n×2 matrix, x in the first column and y in the second. That is the
shape `augment(x, y)` builds, and it is settled by the corpus rather than
guessed — `Finite differences.sm` writes `XY = augment(x, y)` and plots `XY`.

**It needs no span, which is why it is worth having.** The viewport question
below blocks a plot of a *function* of x, because nothing in the file says what
domain SMath drew it over. A table has no such gap: the points brought their own
x. Of the plotted expressions in the three corpora, about two thirds are a bare
name — `XY`, `xy`, `Points`, `polygon`, `M`, `N`, `V`, `T` — and only about a
third are `f(x)`. So this is the half of the plot problem that *can* be settled
from the files, and it is the larger half.

**Which kind of plot a call is depends only on whether a span was written.**
`named_arity` answers zero names for one or two arguments and *argc−2* for three
or more, so `plot(m)` is a table and `plot(f, a, b)` is a curve — and neither
reading depends on what a name happens to hold. A table given a span reports
that the name is not a known function, which is what it means once a span makes
the leading arguments functions.

**An axis nobody chose is fitted.** That was already the vertical axis's rule
and the reason given for it; a table's horizontal axis is in exactly the same
position, so `Extent` records where the span came from and the renderer picks
`Axis::over` or `Axis::fit` from it. A chosen span is still exactly what was
chosen.

**`Series` carries `(x, y)` now, not ordinates.** One shape for both kinds, so
the renderer has one way to ask where a point is. The snapshot still writes only
the ordinates for a sampled curve — its x is `from + i*step` and the span above
already pins it — and both coordinates for a table, where x is data. Sampled
values came out byte-identical, and `plots.snap` moved only by the added
`(chosen)`/`(measured)` tag and the new worksheets.

**Each measurement is marked**, as an open ring in the series' colour: it takes
the curve's own class, so a mark cannot drift out of step with the palette, and
a ring rather than a disc because `plot-curve` already says fill nothing. Past
200 points in one table the marks would touch and the line is the picture — well
above every table in the corpus.

### What a plotted table cost, and what it found

The importer emits `plot(XY)` for a `<plot>` of a table now. The end-to-end path
is pinned by a test in `emit.rs`, because **not one worksheet in either corpus
reaches it**, and finding that out is the useful part of this phase.

The first attempt refused a plot when the plotted expression had a free symbol
and emitted one otherwise. Measured against the corpora that emitted **32 plot
lines, of which 0 drew**: 18 failed with "`XY` has no value: the statement that
defines it failed", 13 with "`M` is not defined", and one drew a tesla, because
`T` was never bound and `T` is a unit. Shipping that would have replaced 32
honest markers with 32 red lines.

So the emitter now asks a stricter question — *does this name hold a value in
the output?* — and it is transitive. `XY : augment(x, y)` imports perfectly and
still has nothing in it when `y` was filled by a loop that mutates.
`Emitter::valued` is that set: a live definition contributes its target only
when everything it reads already has a value, and takes it back out when it does
not. It is deliberately not `Bound`, which answers whether the *SMath file*
defines a name — the right question for free symbols and the wrong one here.

**What that measurement says about the plot problem is not what this document
said before it.** The span was recorded as the blocker for every SMath plot;
that is true only of plots of a *function of x*. For the tables — the larger
half — the blocker is that **every table in both corpora is built by a loop that
mutates**: `for(line(el(M, i, 1) ← …))`. The plots are downstream of the loop
work, not of the viewport question. The corpus's tables are `augment(x, y)` at
the last step and imperative all the way up to it.

Two smaller things fell out:

- **`sys(s1, …, sn, n, 1)` carries a shape, like `mat` and `line`.** Reading its
  operands as series counted two curves as four and seven as nine. Fixed, and
  the marker now says how many series a plot really has.
- **A plot of a function of `x` is refused by name.** SMath's 2D plot variable
  is `x` and every function plot in both corpora is written in it, so mentioning
  `x` keeps the marker. Conservative in the cheap direction: a table computed
  from something called `x` reports as untranslatable, which is a marker to look
  at rather than a chart drawn over an invented domain.

The corpus gate proves the translation outcome did not move: `unsupported=` per
worksheet is unchanged, which is what it would have caught had 32 markers turned
into live lines. What it would *not* have caught is whether those live lines
evaluate — the counts see a marker becoming a line, not a line becoming an
error. Every plot line's fate here was measured by hand.

### Loops: `for` becomes `map`, and `while` says why it cannot

A worksheet is a set of definitions, not a script — so there is no loop
statement and no assignment into an element, and that is deliberate. But what
real worksheet loops were *doing* is mostly not mutation. It is building a
vector whose *i*th element is a function of *i*, one element at a time because
SMath had no other way to say it. That is `map` over a `range`, exactly, and the
importer now translates it:

```
for(i ← 1, i < 201, i ← i + 1,          fn M_col1_1(i) = i - 1
    line(el(M, i, 1) ← i - 1,     ⇒     fn M_col2_2(i) = nasaM1((i - 1)*1000)
         el(M, i, 2) ← nasaM1(…)))      M = augment(map(M_col1_1, range(1, 200)),
                                                    map(M_col2_2, range(1, 200)))
```

The body is lifted into a named function because `map` takes a name — the move
`int` already makes for an integrand — and columns written side by side are what
`augment` is. Both loop headers are read: `for(i, range(a, b), …)` and the
counted `for(i ← a, i < b, i ← i + 1, …)`, where `<` becomes `range(a, b - 1)`
because Nomo's range includes both ends.

**What was measured first.** 105 `for` loops across the three corpora, classified
before any code was written: **39 are an element-wise fill**, 25 of them standing
as a region of their own, which is what this translates. The rest are
recurrences (`el(β, i) ← … el(β, i - 1) …`), accumulators, conditional appends,
or a fill nested inside a function body — and each keeps a marker that now names
which. A fill is refused unless every element can be computed on its own: no
reading what the loop fills, no index that is not the loop variable, no starting
past the first element, no computed column, and no bound that reads the vector
being built.

**`while` is a different problem and is not solved quietly.** All 13 in the
corpora are iterative solvers that stop on a tolerance — secant, Newton,
Broyden, Richardson. `iterate` takes a *count*, because a count is reproducible
and a tolerance test is not, so translating one would mean choosing the number
of steps, and that number decides the answer. The marker says exactly that.

**A loop that translates but reads something the import never produced is
written out commented, under a marker naming what is missing.** Neither of the
two usual answers fits: emitting it live trades a marker for a red line and
blames the loop for a gap that is above it, and dropping it hides a translation
that is correct. This is what the free-symbol case already does, and the
numbers are why — emitting every fill live moved the wiki corpus from 612
markers to 588 and from 422 failing lines to 448, which is the same trade this
document refused for plots one commit earlier.

**What it bought.** `NASA_atmosphere.sm` goes from **8 markers to 0** — fully
translated — and its four `<plot>` regions now draw, because the loops build the
tables and a plotted table needs no span. That is the whole chain from the last
two commits working end to end. Across the wiki corpus: markers 612 → 603,
agreed and disagreed unchanged, failing lines 422 → 425. Across both corpora, 42
live `map`/`augment` definitions where there were 61 loop markers.

**Two things found on the way, both fixed here.**

- **A definition that shadows a builtin and calls it** is refused.
  `linfit_multiple.sm` writes its regression model as `ln(Nu) : b₁ + b₂·ln(Re) +
  b₃·ln(Pr)`, where the inner calls are plainly the logarithm — but a Nomo
  definition shadows the builtin for its own body too, so the same text would
  mean a function that calls itself. Recursion is not the objection (it works,
  and `gcd(a, b) : … gcd(b, mod(a, b))` translates); the objection is that Nomo
  cannot write "the builtin, not me". Refusing the definition leaves every other
  line's `ln` meaning the logarithm, which is what the worksheet meant — and
  `y = map(y_at_1, …)` then computes the right logs.
- **Two engine ceilings**, in their own commit: 64 nested calls and 100 000
  calls per statement. `fn f(x) = f(x) + 1` used to abort the process, and
  `fn f(x) = f(x) + f(x)` used to run forever. Both are three lines anyone can
  type, and the first also *diverged across targets* — 200 deep answered
  natively and trapped in WebAssembly.

**The seam it exposed, fixed in the commit after it.** `el(A, i, 1)` emits as
`A[i, 1]`, and a one-column SMath matrix becomes a Nomo *vector*, which took
one index — so `Calc Area Properties of Composite Rectangular Areas.sm` lost its
marker and gained three failing lines, the only worksheet that got worse. It was
a language question rather than an importer one, and the answer was already
written down everywhere else in the engine: `rows`/`cols` say a vector is *n*×1,
`augment` and `stack` line them up on it, `reshape` turns any single row or
column into one. Indexing was the corner that did not know it. A vector now
takes `v[i, 1]` as the same element as `v[i]`, and any other column is out of
bounds — which is what a column of one means. See "A vector takes its column
index" below.

### A vector takes its column index

`v[2]` and `v[2, 1]` are the same element. Not a new rule so much as the removal
of a corner that disagreed with the rest: every other part of the engine already
treats a vector as the column of *n* it is — `rows` answers *n* and `cols`
answers 1, `augment` and `stack` line vectors up on that, `reshape` collapses
any single row or column into one — and the language reference already said "which
is what indexing it already assumes", which was the one thing that was not true.

Any column but the first is out of bounds rather than an error about how many
indices were written, because a column of one is what a vector is. Three or more
indices are still refused.

**What it was worth.** The importer emits `el(A, i, 1)` as `A[i, 1]`, which is
right for a table and used to fail for a vector, and there is no way to tell
which a name holds before evaluating. Across the wiki corpus this took the
failing lines in the imported worksheets from 425 to **413** — below the 422
they stood at before the loop work — and produced the first *new agreeing stored
answers* of this whole run: `Calc Area Properties of Composite Rectangular
Areas.sm` goes from 0 of 21 to **2 of 21**, and the other 19 wait on things that
have their own markers.

### What is deliberately not here

More than two tables on one plot: two is what the arity rule leaves room for
before a span would be ambiguous, and no corpus worksheet draws more. No log
axes, and no way to choose either range.

**The importer draws a plot of a function of `x` now**, over the span its stored
viewport implies — see "Where the x-axis starts" below, and design note §8.21.
It refuses only when the viewport is not this model (an `<xyplot>`, a 3D plot)
or when the function it plots has no value here.

**A plotted *table* now imports as a drawn plot — and no corpus worksheet
qualifies.** That is the finding, and it is worth more than the feature: see
"What a plotted table cost, and what it found" above.

## The plan is finished. What is worth doing next

`/root/.claude/plans/bright-moseying-shannon.md` had ten phases and all ten are
done. Nothing below is committed to; this is a list of what the work exposed.

**`docs/roadmap.md` now says what to do about it, and in what order.** It is
drawn from this section and supersedes its *ranking*; everything below stays,
because what it holds is the evidence the ranking was made from — the CI
diagnosis, the CAS costing, the plot span, and what each measurement cost to
obtain. Read the roadmap for the order of work and this section for why each
item is where it is.

**CI runs. Four of its five jobs are green and the fifth had never passed on
this repository** — checked on 2026-08-29, on the public repository's whole run
history, which is three runs: `e3bc8ee`, `a518bc7` and `18858f0`. `check`,
`arm64`, `wasm` and `browser` pass on all three. `corpus` failed on all three, at
its **Fetch the corpora** step, before `check-corpus.sh` or any Nomo code ran.
**The cause is found and fixed** — see "The one red job" below. It was a bug in
`fetch-corpora.sh`, not an access problem, and the earlier guess in this
document that a missing secret explained it was wrong.

- **`arm64`, on real aarch64 silicon** — at `dd00363` that was 489 tests, `ok:
  16 worksheets match their snapshots`, and `ok: 16 worksheets byte-identical
  between native and WebAssembly`. That is the central claim of this project
  checked on a second architecture by a machine nobody here owns, and it is the
  one gate that cannot be run locally without qemu.
- **`check`** — fmt, clippy, the test suite, the golden suite, and the
  no-host-math guard.
- **`wasm`** — the module builds for `wasm32-unknown-unknown` and agrees with
  native byte for byte.
- **`browser`** — all nine Chrome checks, on CI's own Chrome rather than this
  machine's.
- **`corpus`** — the two `check-corpus.sh` baselines and the coverage report,
  added in `cde475f` once the worksheets were in the repository. It asserts the
  corpora are present before running, because `check-corpus.sh` skips what it
  cannot find and a skipped gate is a green job guarding nothing. **This is the
  red one.**

**The one red job — found, and it was ours.** `corpus` failed while fetching
the worksheets, and the reasonable-sounding conclusion drawn here was that it
was failing on *access* to third-party files. It was not. The script was
downloading and verifying everything correctly and then exiting 1 on the way
out:

```
fetch technical-mechanics-samples
corpora verified: 116 files under corpora
./fetch-corpora.sh: line 1: tgz: unbound variable
```

A trap body is expanded when it *fires*, not when it is set. `fetch_mechanics`
set a second `EXIT` trap naming a variable it had declared `local`, so by the
time the shell exited that name was out of scope, and under `set -u` an
unbound variable during exit is an error — which becomes the script's status.
Everything it printed was true; the exit code was not.

Two things kept it hidden. `fetch_mechanics` returns early when the mechanics
corpus is already unpacked, so the second trap is only armed on a machine that
actually downloads it — a fresh CI runner every time, and a development machine
exactly once. And the failure prints *after* the success line, which reads like
a tidy-up complaint rather than a failure.

A second bug in the same path came out of testing the first: `verify` ran
`sha256sum -c ../scripts/corpora/files.sha256` from inside the corpus root,
which resolves only when `CORPUS_ROOT` is the default directory one level below
the repository. For the absolute path this document invites, it named nothing
and reported that the corpora did not match when they were fine.

Both are fixed, and the whole CI path is verified end to end from an empty
directory: fetch, verify 116 files, and both `check-corpus.sh` baselines pass,
exit 0. The pre-fix script was confirmed to fail that same run. The method is
worth recording — a local mirror built from the corpora already on disk, so the
fetch path can be exercised in full without asking anything of the upstream
sites:

```bash
mkdir -p /tmp/mirror
cp corpora/nomo-corpus/zips/*.zip /tmp/mirror/
tar -czf /tmp/mirror/Technische-Mechanik-mit-SMath-main.tar.gz \
    -C corpora/technical-mechanics-samples Technische-Mechanik-mit-SMath-main
CORPUS_ROOT=/tmp/corpora-test NOMO_CORPORA_MIRROR=file:///tmp/mirror \
    ./scripts/fetch-corpora.sh
CORPUS_ROOT=/tmp/corpora-test ./scripts/check-corpus.sh
```

`secrets.NOMO_CORPORA_MIRROR` is therefore **not** required: the script fetches
from the upstream sites unaided, as it does locally, and the mirror stays what
it was meant to be — the reliable route when the wiki's cookie-detection
redirect or GitHub's non-byte-stable tarballs get in the way. Whether CI can
reach the wiki from its own address is the one thing still unproven, and the
next run answers it.

The Node 20 deprecation warning is **fixed** — `actions/checkout` and
`actions/setup-node` are on `v7`, the current majors, which run on Node 24;
`checkout@v7`'s one breaking change refuses a fork's head for
`pull_request_target` and `workflow_run`, and this workflow uses neither
trigger. `node-version: 22` is a different thing and stays pinned: it is the
engine the determinism scripts run under, not the actions' runtime.

**The SMath importer — continue it.** Reader, emitter and oracle all exist and
the numbers are above. Conditionals and loops were the answer to "what would
move them most", and both are now built — `if` as an expression, and a `for`
that fills a vector as `map`. What is left of the loops is the two shapes `map`
cannot say: a **recurrence** (`el(β, i) ← … el(β, i - 1) …`, 8 loops) and an
**accumulator** (`Σ ← Σ + …`), which are folds and would need `iterate` over a
pair, plus the fills nested inside function bodies (13). **SMath's summation is
translated now** — `sum(expr, i, a, b)` is `int`'s shape and lifts the same way,
into `sum(map(term, range(a, b)))`, which needed no new builtin; 77 calls across
14 worksheets, and it took agreement from 558 to 574 across both corpora at that
commit (§8.38). It also closed the item that stood here next — `i` as an index
name — for free, since a parameter shadows the imaginary unit. Original specification in
`docs/design-note.md` §8 with a 25-item checklist, corrected by
§8.9. The recorded risk is now sharper rather than weaker: the format is *known*
to have changed structurally once already, at 0.88, in a way that breaks a naive
reader outright rather than degrading it. The corpus is 0.82–0.98 and SMath is
now 1.x, so **worksheets from the current version are still needed**, ideally the
ones the users being migrated actually hold. That is the one thing here nobody
can do from this repository.

**`identity(n)` is built, and it is worth recording what it bought.** It was
listed here as one name and a whole chain, and it was: `7.3.sm` went from 27
answers agreeing to **55, which is all of them**, because everything downstream
of `I : identity(2)` — the eigenvalues by `roots`, the principal stresses, the
angle between the axes — had been failing on that one gap. Design note §8.29. `diag(v)`, the neighbouring name, is built too — on the
language's own merits rather than the corpus's ranking, which has **zero uses**
of it (§8.31).

**What the oracle still cannot compare is nothing — and 12 answers differ in
shape, down from 27, with one of the five investigations finished.** It reads
tables (§8.26) and complex answers (§8.30) now. The ones left are genuine: Nomo
computes 91 values where SMath stored 4, or 1 against 30 — worksheets whose
construct differs rather than whose answer is unreadable. `Newtoninterp.sm`,
`10.1_AS.sm`, `10.5.sm` and `5.2-4.sm` are the four left, and `10.5.sm`'s
blocker is named below.

**`Calc Area Properties…sm` is finished: 19 of 19 comparable answers agree.**
The summation (§8.38) made every line in it evaluate, and 13 of its 21 answers
then reported a shape rather than a number — all thirteen descending from one
line, `A.total ← b·h`, where `b` and `h` are both `mat(…, 9, 1)`. Reading
`SMath.Math.Numeric.dll` settled it (§8.39): `TMatrix::op_Multiply` tests for two
one-column operands of equal height **before anything else** and returns their
inner product as a scalar, falling through to the ordinary matrix product only
when that fails. So SMath's `·` and Nomo's disagree in exactly one case, and the
importer emits `dot(a, b)` for it wherever the file states both shapes. Narrow by
construction: of the 18 multiplications in either corpus with a stated shape on
both sides, 13 were matrix products both languages already agreed on and five
were this.

**The CAS items, costed rather than assumed (§8.34, §8.40).** Asked as a feasibility
question and it split into three, only the last of which is a computer algebra
system.

- **A linear solve is not algebra — built as `solve_linear` (§8.35).** The
  coefficients come out of *evaluating* the residual (`b = −r(0)`, column *j* is
  `r(eⱼ) − r(0)`), and linearity is checked by putting the answer back rather
  than assumed. One correction came out of building it: **not one `Solve` in the
  corpora stores a numeric answer** — their coefficients are symbolic, so Tier 1
  is downstream of Tier 2 there. The numeric system solves live under
  **`FindRoot` (16 numeric answers) and two-argument `roots` (4)**. Checking
  every one of those ten regions closed the question the other way (§8.36):
  **none of them says which names are its unknowns**. Every multi-unknown system
  keeps them as free symbols inside another name — Tier 2 again — and every
  region the importer *can* lift has one unknown and a nonlinear equation, which
  `solve_linear` would rightly refuse and which a guess-based root finder would
  answer only by reproducing SMath's choice of root. The mapping is not written;
  the refusals say why instead.
- **Formula-valued names are the real blocker. Attempted, measured, reverted
  (§8.37).** Widening §8.22's inference regressed six worksheets in three
  independent ways: "free in the document" is not "has no value here"; the
  transitive closure turns plotted *tables* into functions; and a definition can
  read itself. The prize was measured at about four answers, because only three
  worksheets `Clear` a single name and one of those feeds a symbolic `Solve`
  anyway. A wider inference wants to be a **second, separately fenced rule**
  using §8.22's emission-time tests rather than a widening of the first — a
  larger piece of work than four answers justify today. The three failures are
  written down so the next attempt starts from them.
- **Genuine algebra stays out**: a symbolic *answer* (`5.1.sm` stores
  `L(−3+√39)/6`, and computes the same root numerically two lines later), and
  `simp`/`ratexpand`/`assume`/Maxima — 42 calls in 11 worksheets. §8.12 stands.

One correction the measurement forced: **"uses Maxima" is not "blocked by
Maxima"**. The four Maxima-only worksheets already agree on 112 of their 157
answers. And `eval`, 71 calls that read like a CAS entry point, is SMath's
*"Evaluate numerically"* — a display directive.

**Re-costed against the built engine (§8.40).** The same question asked a third
time, now with automatic differentiation, `roots` and `solve_linear` in the tree,
and measured rather than estimated. "CAS" is four layers, and they do not cost
the same. **Symbolic values and symbolic differentiation are small**: a
`Value::Symbolic` variant was added and the workspace compiled, and it breaks
**nine exhaustive-match sites** — seven in `value.rs`, one in `render/mod.rs`,
one in `golden.rs`, and nothing at all in `eval.rs`, `doc.rs`, `graph.rs`,
`nomo-wasm` or `nomo-smath`. It is contained because `complex_pair` and
`dual_pair` already establish how a second tower joins the arithmetic at one
place, and because the trace and the AST already keep what a symbolic layer would
render.

**Symbolic linear algebra is not small, and that is the correction.** `det` and
`inv` are elimination with partial pivoting **by magnitude**, which a symbolic
matrix has none of — so that code is structurally unusable for a symbolic solve,
which needs fraction-free elimination and a **pivot zero-test**. That test is
decidable over exact rationals and a *heuristic* over this engine's `f64`
coefficients, and a heuristic zero-test is the quietly-wrong answer this project
refuses everywhere else. §8.12's "bounded and well understood" is withdrawn on
that ground, and §11's question 9a no longer claims it. Rewriting stays out for a
second reason as well: reassociation is a simplifier's whole job, and §3 makes
reduction order part of the language.

**The recommendation is unchanged — do not add one** — but on demand rather than
on effort: about 2% of the representative corpus touches anything CAS-like, and
twice now the CAS-shaped requirement has dissolved into exact numerics. If a door
is kept open it is symbolic *values* alone, fenced, with unknowns declared by
dimension the way `solve_linear`'s `kinds` already does. The trigger for
reopening is a demand question, never a capability one: the target user's own
files.

**Language features deferred to v1 scope, and where they stand.** Conditionals,
loops, complex arithmetic and plots are all built to a first phase. What is left
of each is named in `docs/language.md` under "Not yet in the language" — briefly:
complex collections and transcendentals of a complex argument; and mutation,
which is deliberate and not coming. Several curves on one plot and a plot of a
table of measured points are now both built.

**The worked example is finished.** It was a real power-electronics worksheet
and it drove eight commits. It checked **34 of its 34 stored answers, all
agreeing**, drew all three of its charts, evaluated without a single error, and
carried **7 import markers** — exactly the seven this document already listed as
*not work*. There was nothing left in it to do. It has since been removed as a
customer document (THIRD-PARTY.md); what it took, in the order it fell out:

- **All three plot regions draw** — `plot(curve_1, 53039, 125434)`,
  `plot(plot_Z_LLC_angle, 62169.2, 132624)` and `plot(Mg, Mg2, 25491.6,
  202578)`. Two things had to be read as what they are rather than as what they
  say: a definition free in `x` that a plot draws is a function of `x` (§8.22),
  and a definition of `sys(…)` is a list of curves, which is a plot rather than
  a value (§8.23). The last span is the one §8.21 derived from the viewport
  before anything could draw it.
- **`solve` is a search, not a bracket**, settled by reading
  `SpecialFunctions.dll` rather than inferring from worksheets (§8.24): 200
  samples across the range and every sign change refined. The language grew
  `roots(f, a, b)` for it, and the line imports as
  `roots(derivative_Mg, 30000, 200000)`.
- **`diff` did not need a CAS after all.** SMath's is symbolic, but this
  worksheet only ever *evaluates* its derivative — the root search samples it —
  and the value of a slope at a point is arithmetic. Forward-mode automatic
  differentiation gives it exactly, with no step size (§8.27), and
  `derivative_Mg(f) : diff(Mg(f), f)` imports as
  `fn derivative_Mg(f) = derivative(Mg, f)` (§8.28). The peak comes out
  2.7e-5 from SMath's stored `61995.1263`, which is that answer's own display
  rounding.
- **Seven markers that are not work**: four decorative labels (`jXLr`, `jXCr`
  and two `#`), `Rac` and `fsw` written as prose, and a date in the page header
  stored as arithmetic. Free symbols by design, and correct as markers.

**Where the x-axis starts — answered.** This was the most valuable open item in
this document and it is closed. A `<plot>` still records only `scale_*`,
`rotate_*` and `transpose_*`, but that is enough once SMath's own arithmetic is
known, and it was read out of `PlotRegion.dll` rather than guessed:

```
    frame = 10·(width/height)/1.66 · scale_y
    x ∈ [ (−width/2 − transpose_x)/frame , (+width/2 − transpose_x)/frame ]
```

The `10` is the frame the renderer is constructed with, the `1.66` is a literal
in the 2D branch, and `Renderer::Scale` multiplies the saved `scale_*` and the
live frame by the same factor — which is why the stored scale is a *relative*
zoom, as this document suspected, and why reloading re-applies it exactly once.
**The field names are crossed**: the horizontal extent divides by `limits_y`,
which is what `scale_y` scales, and reading `scale_x` instead is what made the
earlier attempts disagree by four orders of magnitude.

Checked against six worksheets that record their domain nowhere: the standard
normal comes out over ±4.80 and peaks at 1/√(2π); Student's *t* over −4.10…3.87;
χ² over −1.97…25.9; F over −0.41…2.78; a Newton-Raphson demo over −4.93…7.39;
and the converter worksheet's three-curve LLC plot over **25.5 kHz…202.6 kHz**,
which is the span `examples/plots.nomo` draws a gain family over.
Full derivation and evidence in `docs/design-note.md` §8.21.

It is exact to about a percent at the edges, not to the last bit: the width used
is the region's stored box, and SMath measures the canvas inside it — the same
few pixels of frame that make an imported `<picture>` slightly too large
(§8.19). Spans are written to six figures to say so.

**Two things found while looking.** The plot region has explicit axis limits
(`limits_x/y/z`, `HasLimits`, `LeftLimit`/`RightLimit`/…), and **no worksheet in
any corpus — nor any of SMath's own examples — sets them**; when one does, the
domain is in the file and none of the above is needed. And `sys(s1, …, sn, n, 1)`
is confirmed from the same source as a list of series with a shape in its last
two operands, which is how the importer already reads it.

**Product questions the engine cannot answer.** The CalcpadCE trial is the
requirements-gathering exercise and is testing whether engineers accept a text
syntax at all. That answer should shape what comes after this more than any item
above.

## Decisions taken during implementation

Beyond the plan, and worth knowing before changing anything:

- **Rational dimension exponents**, not integer. Fracture toughness is `MPa·√m`.
- **Juxtaposition is multiplication** at `*`'s precedence. This is what makes
  `9.81 m/s^2` parse correctly with a grammar that knows nothing about units, and
  it matches how SMath stores units, so the importer maps straight onto it.
- **Comments are statements**, not trivia — a worksheet's prose is part of its
  output.
- **One evaluation path.** The sequential driver was deleted when the graph
  arrived rather than kept alongside it.
- **Unit-shadowing warnings only for multi-letter names.** `V`, `h`, `A`, `F`,
  `P`, `T` are all unit symbols and all conventional variable names; warning on
  them fired on the very first real example.
- **HTML uses Unicode mathematics and CSS**, not KaTeX as the plan suggested. An
  external typesetting dependency would undercut the offline commitment.
- **`clippy::suboptimal_flops` is deliberately disabled.** It rewrites `a*b+c`
  into a fused `mul_add`, which rounds once instead of twice and is available on
  some targets but not others — exactly the drift this design exists to prevent.
  `scripts/check-no-host-math.sh` enforces the invariant instead, and has caught
  two real violations so far.
- **`Span::line_col` lives in the engine**, and the CLI's private copy was
  deleted. The snapshot needs positions for diagnostics, and two implementations
  of "where is this" would eventually disagree.
- **The HTML renderer was split into `body` and `render`.** Snapshots pin the
  body only, so a stylesheet edit does not churn every expected file while still
  leaving worksheet output impossible to change unnoticed.
- **The diff is hand-rolled** (`crates/nomo-cli/src/diff.rs`, LCS over lines
  after trimming the common head and tail). Sixty lines against a dependency in
  the build of a project whose argument is that its output is reproducible from a
  small, auditable tree.
- **`nomo-wasm` uses a plain C ABI, not `wasm-bindgen`**, which the plan named.
  The full reasoning is in that crate's module documentation. In short: the
  artifact imports nothing at all, which is both a stronger guarantee and a
  one-line check; there is no build-time generator to pin, which is the objection
  the design note raises against EngineeringPaper.xyz (§10) and would have been
  the same mistake in another language; and `cargo build --target
  wasm32-unknown-unknown` is the entire build. The cost is that the boundary
  carries bytes rather than typed values — see the phase 8 note above.
- **`check-no-host-math.sh` now guards `nomo-wasm` too.** It is the boundary
  crate, so it is the one place a call into the host would look reasonable.
- **The front end decides nothing about the language.** Highlighting is a list of
  classified ranges from the engine, not a CodeMirror mode. See the phase 8
  section above; this is the invariant that is easiest to break by accident and
  hardest to unpick later.
- **All offsets in the analysis payload are UTF-16 code units**, converted from
  the engine's byte spans by `api::Utf16Offsets`. Every `Span` inside the engine
  stays a byte range; the conversion happens once, at the edge, for the only
  consumer that counts differently.
- **`web/index.html` is committed, so `.gitignore` no longer says `*.html`.** That
  rule existed for `nomo html` output and silently swallowed the front end's
  entry point; it is now scoped to `/examples/*.html`.
- **The version pragma is written by the engine, not the front end.** See the
  phase 9 section; the same reasoning as highlighting, applied to the format
  rather than the language.
- **Storage failures are swallowed, and `loadDraft` has a timeout.** IndexedDB
  can block rather than fail, and startup waits for it.
- **The browser checks all drive Chrome over the DevTools protocol.** The one
  that used `--dump-dom --virtual-time-budget` broke as soon as startup touched
  IndexedDB, because virtual time does not advance a database request.

## Known limitations

None of these are bugs; they are scope, recorded so they are not rediscovered.

- **Incremental update matches statements by position.** An in-place edit
  invalidates one line and its dependents; inserting or deleting a line shifts
  everything below and forces a full pass — visible in the editor, where the
  status bar then reports the whole document being recalculated. Correct but
  pessimistic. Fixing it needs stable per-statement identity, not a better diff.
- **Moments render as `J`.** N·m and joules are dimensionally identical, so a
  moment displays as joules unless converted explicitly. Same class of issue as
  `W`/`VA`/`var`. There is no structural way to tell them apart.
- **Complex numbers are scalars only.** Arithmetic, `Re`, `Im`, `conj`, `arg`
  and `abs` all work; a complex *collection*, a transcendental of a complex
  argument and a complex exponent do not, and say so. `docs/language.md` has the
  reasoning — all three need a branch cut nobody can state for the worksheet.
- **No mutation, and so no loop statement.** `map`, `iterate` and `range` are
  what a worksheet's loops were doing. Deliberate, and not coming.
- **A plot's legend is a strip, not a placed key.** It sits under the drawing
  because a legend floated in a corner covers whichever curve went there. The
  axes can be scaled, windowed and named — `axis x log`, `axis y 0, 100`,
  `axis x "Frequency"`, `label "Gain", "Phase"` — but where the legend goes is
  not a choice the worksheet gets to make.
- **The editor holds one worksheet at a time.** Multiple open documents would
  mean multiple sessions, which the boundary already supports — nothing in the
  module is global — but there is no interface for it. It is the one part of
  step 19 that was not built, and deliberately: it is a piece of interface work
  — tabs, a draft per document, a file handle per document, and what "unsaved"
  means across several — with no engine question in it at all, which makes it a
  step of its own rather than a corner of this one.
- **`Save` in Firefox and Safari is a download.** Those browsers have no File
  System Access API, so there is nothing to write back to. The button says
  Download there rather than pretending otherwise.
- **The draft is per-browser, not per-device.** It lives in this browser's
  IndexedDB. Clearing site data loses it; the file on disk is the document.
- **An imported figure's size includes the region's frame.** SMath's `<picture>`
  states no size of its own, so the enclosing region's box is the only thing that
  says how large the figure stood. A region carries a few pixels of frame around
  its content — the corpus's one `<imagefile>`, which does state its own size, is
  117x100 inside a 127x108 region — so a `<picture>` imports about five pixels a
  side larger than SMath drew it. Subtracting a constant would be inventing a
  number no file states; the box is carried as the file gives it.
- **The SMath importer refuses what it cannot know.** Seven phases in and
  reading both corpora; what it still will not translate is listed under "What
  is worth doing next", and every refusal is a visible marker rather than a
  silent drop.
- **A vector whose elements carry different dimensions cannot be folded, and a
  bare `0` is dimensionless.** `10.5.sm` writes `F ← [0, M₀, F₀, 0]` — a moment
  beside a force — against a `γ` holding a dimensionless ratio beside a
  reciprocal length, so every product is a force and the sum should be one too.
  Nomo refuses at the literal zero, which it will not add to a newton. This
  surfaced when `·` between two columns became `dot` (§8.39), and it is a
  question about the engine's arithmetic rather than about the importer.
- **Nothing that lifts can be written inside a function definition.** `int`,
  `sum`, `solve`, `diff` and the `for` that becomes a `map` all lift their body
  into a named helper, and the helper is written *above* the definition it came
  from — so its names would resolve to the worksheet's globals rather than to
  the parameters in scope where they were written. Nomo has no closures, so the
  region says which parameter it would have captured instead of emitting it.
  This cost nine answers that had been agreeing and was still right: one of them
  was `simpsonrichardson.sm`, whose summand happened to be called with globals of
  the same name, and one was `normaldist.sm`, which redefines those globals
  halfway down the page. Design note §8.38.

Two mismatches between the docs and the implementation were found in phase 6 and
**settled in the implementation's favour**; `docs/language.md` now states both:

- **A computed point displays on the absolute scale.** `20°C + 5 K` shows
  `298.15 K`, not `25°C`. Same reading either way, and the absolute scale means
  the display never depends on which operand was written first.
- **A conversion target is echoed as written.** `M -> kip*ft` keeps the `*`. The
  unit you asked for is the unit you see. Exponents are the exception, since
  `in^3` has an unambiguous typeset form.

## Third-party material

Nothing third-party is committed. This repository is MIT licensed and
everything in it is the project's own work; `THIRD-PARTY.md` is the full
statement and this is the summary.

- **`corpora/` is fetched, not committed.** `./scripts/fetch-corpora.sh`
  downloads both corpora and verifies every file against
  `scripts/corpora/files.sha256`, so what the gates measure is provably the
  corpus the baselines were recorded against. The directory is gitignored. Set
  `NOMO_CORPORA_MIRROR` to fetch the same archives from a mirror instead —
  the wiki serves behind a cookie-detection redirect and GitHub's generated
  tarballs are not byte-stable, so CI wants one.
- **`reference/` is gitignored and has never been committed.** A gigabyte, and
  design note §10 says what it is for.

- `corpora/nomo-corpus/` — 54 SMath worksheets, all 48 published wiki examples,
  versions 0.82–0.98. **553 stored answers across 51 of them.** Third-party
  worksheets by named authors; the examples page states no terms.
- `corpora/technical-mechanics-samples/` — 60 worksheets from Kraska's
  *Technische Mechanik mit SMath Studio*, SMath 1.3–1.5. **877 stored answers**,
  and the only current-era corpus there is. Not representative of SMath usage:
  two thirds of it needs computer algebra (design note §8.12). Springer Nature
  book-companion material with no stated licence.
- **Two industrial worksheets have been removed.** A resonant-converter design
  and an interlock-monitor design, from an on-board-charger project. They were
  the worked examples much of the importer was built against and much of this
  document argues from. They were customer documents, not this project's to
  publish, and everything derived from them went with them: the two examples,
  their snapshots, and `tests/corpus/standalone.txt`. Comments and passages that
  cite what they measured now say "the converter worksheet" and "the interlock
  worksheet"; the measurements stand, the files do not.
- `reference/CalcpadCE/`, `reference/EngineeringPaper.xyz-main/` — the two
  codebases the design learned from. **Not to be extended**; see design note §10.

The architecture and the evidence for every decision are in `docs/design-note.md`,
which is now the only copy.
