# Roadmap

What to build next, in the order to build it, and why each item is where it is.
`docs/STATUS.md` says where the work stands; this says where it goes. The ten
numbered phases of the original plan are all done, so what follows is the first
list in this repository that was drawn up from a finished engine rather than
from a specification.

The ranking rests on one observation: **capability is no longer the constraint.**
The engine renders 114 third-party worksheets byte-identically on two
architectures and two targets, and the importer reads both corpora. What is
missing is a crash, three language features that every real design sheet needs,
content that shows what the tool is for, and any way at all for a user to obtain
it. `docs/STATUS.md` records that the most valuable open question is a *demand*
question — and no demand question can be answered while there is nothing to
download.

One commit per step, as everywhere else here. Each step names the gate it has to
pass, and no step is finished until that gate is green.

## Two directions, interleaved on purpose

**The applications** — worksheets an engineer would actually keep — are Phase 3.
**The tool** is everything else. They alternate because they need each other:
Phase 2 exists because the how-tos of Phase 3 cannot be written without it, and
Phase 3 is what will show whether Phase 5's ranking is right. A feature list
drawn up without content to test it is how a worksheet program acquires forty
functions nobody calls.

## Phase 1 — safety, and the gates that are not running

### 1. A fixed nesting limit in the parser

`eval.rs` caps call recursion at `MAX_DEPTH = 64`. The parser caps nothing, and
that asymmetry is a crash:

| `x = ((((…1…))))` | Native, 8 MB stack | WebAssembly, 1 MB stack |
|---|---|---|
| where it dies | aborts between 5 000 and 7 000 | **traps between 750 and 800** |

Measured 2026-08-29 against `web/dist/nomo_wasm.wasm`. In the browser the trap
is not recoverable: it leaves the instance's allocator in an undefined state, so
every later `update()` throws `memory access out of bounds` — checked by opening
a session, feeding it one deep expression, and watching every subsequent edit
fail. The editor reports `engine error: …` once and then quietly stops
recalculating for the life of the tab.

The fix is a `depth` field on `Parser`, a new code `SH010`, and recovery through
the existing `recover_to_line_end`. The number is chosen the way `MAX_DEPTH` was
— **from the tightest target, which is WebAssembly** — and then verified rather
than assumed: a worksheet nested to exactly the limit must survive parse,
evaluation, rendering and snapshotting in a **debug** wasm build as well as a
release one, since debug frames are larger. The deepest expression in
`examples/` and in the importer's emitted output is measured at the same time,
so the limit sits far above anything real.

*Gate:* `cargo test --workspace`, `nomo test`, `scripts/compare-targets.sh`.

### 2. The front end survives an engine trap

Prevention is step 1; this is the backstop. The engine handle moves to module
scope in `web/src/main.js`, and a failure in `analyse()` re-instantiates the
module and reopens the session from the buffer — reopening on the same instance
is not enough, because the instance is what was damaged. The buffer itself was
never at risk: it belongs to CodeMirror. `scripts/check-session.mjs` gains an
assertion that a session survives a trapping edit, so this is gated rather than
believed.

*Gate:* `scripts/build-web.sh`.

### 3. A robustness test that would have found step 1

There is no fuzzing, no property test and no benchmark in the workspace. A
zero-dependency randomized test — a small xorshift with a fixed seed, in the
style the determinism scripts already use, and **no new dependency in a crate
that carries only `libm`** — generates token soup and malformed worksheets and
asserts that the parser never panics, always terminates, and that everything it
accepts renders. Fixed seed, fixed case count: a gate, not a flake.

*Gate:* `cargo test --workspace`.

### 4. Fix the one red CI job — **done**

`corpus` had never passed, and the guess recorded here was that a missing
`NOMO_CORPORA_MIRROR` secret explained it. It did not. `fetch-corpora.sh`
downloaded and verified everything correctly and then exited 1 on the way out,
because an `EXIT` trap set inside a function named a variable that function had
declared `local`: a trap body is expanded when it fires, by which time the name
is out of scope, and under `set -u` that is an error during exit. It only
appeared on a machine that actually downloaded the mechanics corpus — a fresh
runner every time, a development machine once. Testing the fix turned up a
second bug beside it: `verify` resolved the hash manifest relative to the corpus
root, so any `CORPUS_ROOT` but the default reported a mismatch that was not
there.

Verified end to end from an empty directory through a local mirror, and the
pre-fix script confirmed to fail the same run. `docs/STATUS.md` records the
method. The secret is not required.

*Gate:* a green `corpus` job on the next push — the one thing still unproven is
whether a runner can reach the wiki from its own address.

### 5. A performance report, before there is anything to regress — **done**

`nomo bench` times six fixed shapes through the same `snapshot` function the
WebAssembly build exports, and CI prints it on every push as a report rather
than a gate. The numbers and what they say are in `docs/STATUS.md`; two are
worth acting on eventually — a user-function call costs about 8 µs because it
copies the environment it runs in, and an edit to one line of a 5 000-line
worksheet costs 5 ms because only the evaluation is incremental, not the parse
or the graph rebuild. Neither is urgent, and both are now measured rather than
suspected.

## Phase 2 — the three things a design sheet needs

### 6. `check` statements — a worksheet that states a verdict — **done**

```nomo
check sigma <= sigma_allow
```

renders the usual columns and then a verdict, and counts into a summary. This is
the genre engineers deliver — inputs, calculation, limit, pass or fail — and
nothing in the language expresses it today.

- `Stmt::Check` in `ast.rs`; `check` joins `unit`, `fn` and `global` as a
  keyword.
- Evaluation reuses comparisons, which already yield 1 and 0 dimensionlessly. An
  expression that is not a comparison, or that fails, is a diagnostic rather than
  a silent pass.
- **A failed check is not an error.** The worksheet is right; the design is not.
  So the CLI separates them by exit code — 0 ok, 1 the worksheet does not
  evaluate, 2 a check failed — and prints `n checks, m failed`. That is what
  makes `nomo check` usable as a gate on an engineering document in CI.
- `analysis_json` grows `checks: {total, failed}`, so the editor's status bar can
  say it without parsing HTML.

Built as specified. Two things came out of building it. The condition rule is
strict — a dimensionless 1 or 0, which is what comparisons produce — because
anything looser means a check can pass on `5 m` being "truthy" and hide the
mistake it exists to catch; a condition that *cannot* be evaluated is reported
as undecided rather than failed, since a design that does not hold and one
nobody could work out are different. And the keyword's cost was measured before
it was spent: `check` is used as a name in **zero** of the 114 SMath worksheets
the importer reads, and in exactly one worksheet here, which was renamed.

### 7. `linterp`, and table lookup — settled from SMath, not guessed — **done**

Material and section tables are what every how-to in Phase 3 needs, and the
engine cannot interpolate at all. Two questions the corpus cannot settle decide
the semantics: what happens **outside** the tabulated range, and what a lookup
returns on multiple matches. Extrapolating a material table is a real hazard, so
neither will be guessed — SMath is at `/opt/smath` and its own implementation
settles both, which is the method that settled the plot span (§8.21), `solve`
(§8.24) and `·` between two columns (§8.39). Implement what the disassembly
states; refuse with a marker whatever it leaves ambiguous. The findings go in the
design note as §8.42, and the importer maps the names it now has a target for.

Done, and the disassembly answered more than it was asked. SMath extrapolates
outside the table silently, sorts an unsorted column silently, and drops units
altogether; Nomo decides all three the other way, and design note §8.42 records
the evidence and the reasoning. The lookup family turned out **not to be SMath
functions at all** — `SMath.Manager`'s name table has no lookup of any kind — so
they are unimplementable from here on principle rather than on priority, and the
importer's registry no longer claims them.

### 8. The cheap missing builtins, in one batch — **done**

`mod`, `product`, `sort`, `reverse`, `submatrix`, `trace`, `rank`, `nthroot`,
`hypot`, `log(x, base)`, `cot`/`sec`/`csc`, the inverse hyperbolics, and
`mean`/`median`/`stdev`. Each is a few lines in the `eval.rs` dispatch with an
obvious dimension rule. One decision is worth stating rather than assuming:
ordering requires a common dimension, so `sort` and `median` refuse a
mixed-dimension vector rather than comparing it in base units.
`examples/functions.nomo` grows to cover them, because that file exists to put
every function through the native-versus-WebAssembly comparison.

Done, with two of the batch deliberately dropped and one convention question
answered by measurement rather than by taste. `rank` is out because a matrix
rank needs a pivot zero-test, which over `f64` is a heuristic — the same ground
§8.40 refused symbolic linear algebra on. `stdev` is out because dividing by *n*
and by *n−1* are both called the standard deviation and nothing here settles
which a worksheet meant. `log` requires its base, which cost nothing: all six
`log` calls in either corpus state one.

The names were checked against the corpus rather than assumed — every one is
spelled the way real worksheets spell it, so the importer needed no renames —
and against SMath's own implementation where a convention was at stake: `mod`
takes the sign of its dividend (`rem` on two doubles) and `submatrix` is
inclusive and one-based. Wiki agreement went from 304/337 to 312/344.

### 9. Packs — shared units and constants, without I/O — **done**

Twelve how-tos must not each redeclare steel. A fetched include would break the
offline story and put a network round trip inside a determinism claim, and the
browser opens a *file*, not a directory — so an include that reads the disk
cannot work in the front end at all. What does work is **packs compiled into the
engine** as source text and resolved with no I/O:

```nomo
use materials.steel     ' E, the grades, densities, and their unit declarations
```

Resolved in `doc.rs` before the graph is built, versioned with the file format,
listed by `nomo packs`, and rendered as one line rather than a hundred
definitions. A user-authored pack directory for the CLI is a later step; it is
not this one, because the browser is the constraint and the CLI is not.

Built as designed, with one simplification and one thing that had to be got
right. The simplification: `use steel`, not `use materials.steel` — `.` is not a
character an identifier may contain here, and a namespace for two packs would
have been a lexer change in exchange for nothing.

The thing to get right: a pack's statements are spliced into the tree where the
`use` stands, and they arrive carrying spans that point into the *pack's* source
— a different string from the one the editor slices and the highlighter indexes.
Left alone they are not merely wrong but out of bounds. They take the span of
the `use` line instead, which is also the right answer for a reader: that is the
line they wrote.

## Phase 3 — the applications

Each worksheet is prose, inputs, the calculation, a `check` against a limit, and
a plot where one earns its place. Each gets a golden snapshot. Each is **ours to
publish**, unlike the corpora and unlike the two customer worksheets that had to
be removed — which is the other reason to write them.

### 10. Six mechanical how-tos — **done**

Bolted joint preload and torque; a shaft under combined bending and torsion;
Euler and Johnson column buckling; bearing L10 life; a helical compression
spring; thin- and thick-wall pressure vessels. One commit each, and every
figure checked against a hand calculation before it was committed.

Three habits came out of writing them and are worth keeping in the ones that
follow. **Each says what it leaves out**, at the same length as the calculation
where that is honest — a shaft worksheet that does not mention keyways and
fatigue is more dangerous than one that answers nothing. **Each names its least
certain number** rather than presenting six digits of equal weight: the nut
factor, the allowable shear of a drawn wire, the estimated bearing load.
**A plot stops where its equation does** — the first bolted-joint plot ran a
straight line through separation and the first spring plot ran one through solid
length, both of them a wrong picture of a right equation.

### 11. What the how-tos broke — **done**

Measured rather than guessed, which changed the answer. Of the three things
expected here, **rounding to a preferred size was wanted by none of the six**
and is not built.

What all six wanted was **fewer significant figures** — every worksheet showed
`36.4031 kN` where `36.4 kN` is the whole of what is known. `digits n` sets them
from a line downwards; it is presentation only, so the full-precision values the
cross-target comparison uses are untouched. The corpus wanted it too, which was
not part of the original argument: 1279 regions across 34 SMath worksheets carry
an explicit `decimalPlaces` or `significantDigitsMode`, and Nomo had no way to
express any of it.

The second thing all six wanted turned out **not to be a missing feature but a
missing idiom**: a conversion in an assignment — `sigma = M/S -> ksi` — already
recorded the unit, so verdict lines could have read `10 ksi ≤ 30 ksi` all along
rather than in pascals. It only half worked: a *compound* target like `mm^2` or
`MN/m` could not become a hint, which is most engineering units, and that is now
fixed. Fixing it nearly shipped a much worse bug — see the commit — and the
guard against it is a test.

### 12. The gallery, and the migration story shown rather than described — **done**

Both done, and the second had a constraint that shaped it: the corpora are other
people's documents, so a before-and-after cannot show one. The worksheet in
`docs/smath.md` is therefore an SMath 1.x file **written here** — our numbers,
our prose, ours to publish — carrying one of each thing that matters: prose, a
global `≡`, positional definitions with units, two stored answers, and one
construct that is out of scope so a refusal is shown too.

It turned out to be worth more than the document it was written for.
`check-corpus.sh` skips what it cannot find, so on any machine without the
corpora — every CI runner, so far — the importer had **no test at all**. That
fixture is now four, and they run anywhere.

## Phase 4 — ship it

### 13. A release — **written; the runners have not run it**

Nothing reaches a user today: no release workflow, no hosted build, no binary.
A tagged release deploys `web/dist` to Pages, builds CLI binaries for
linux-x86-64, linux-aarch64 and macOS, and publishes the wasm artifact **with
its hash**, so that whoever downloads it can check the determinism claim rather
than take it. `web/dist` gets cleaned on build first: it currently carries a
stale 650 KB `sheaf_wasm.wasm` from the old project name, which would otherwise
ship.

Written, with its shell extracted and run against stand-in artifacts so that the
packaging, the checksums and the collection into one `SHA256SUMS.txt` are known
to work. What cannot be checked from here is the runner, the Pages deployment
and the `gh` call — and one setting has to be changed by hand: the repository's
Pages source must be "GitHub Actions".

`web/dist` is emptied before each build rather than merged into, which is what
finally removed the stale `sheaf_wasm.wasm` — 650 kB of an engine from before
the project was renamed, which every deployment would otherwise have published.

*Gate:* the workflow green on a tag; the published page loads, computes, and
still works offline after one visit.

## Phase 5 — the deeper engine, ranked by what Phase 3 asked for

### 14. A plot's axis limits, and a log scale — **done**

Done as `axis x log` / `axis y 0, 100`, a statement in the shape `digits`
established. Reading `PlotRegion.dll` settled what there was to follow and what
there was not: SMath has explicit axis limits — `HasLimits` with a `Left`,
`Right`, `Top` and `Bottom` — and **no logarithmic scale of any kind**. So the
limits have a precedent and the scale is Nomo's own decision, taken with no
import pressure behind it.

The part worth remembering: a logarithmic *horizontal* axis had to change the
sampling and not only the drawing, so it lives on the plot value rather than in
the renderer. 257 samples spaced linearly across four decades put four of them
in the first one.

### 15. A fixed-step ODE — **done**

Built as `rk4(f, y0, a, b, steps)`, and *not* `rkfixed`-shaped after all: that
name is in no plugin this installation carries, and the corpus calls it with
three arguments four times and four arguments once, so its signature cannot be
read here any more than the lookup family's could. With nothing to copy, the
name states the method — which is the honest thing anyway, since the answer to
an initial value problem depends on how it was integrated and how finely.

The result is a table of `(x, y)` rows, which cost nothing to design because the
language already had somewhere to put one: `plot` draws a table and `linterp`
reads a value out of one.

Two things fell out of it. A first-order scalar equation only, because a system
needs a vector whose elements carry different dimensions — an existing recorded
limitation. And a collection over twenty values now renders as its shape rather
than its contents: an `rk4` over a hundred steps is two hundred and two numbers,
printed in the substituted column of every line that touches the table.

### 16. Symmetric eigenvalues — **done**

Cyclic Jacobi at twelve sweeps — a count rather than a convergence test, and
the count is measured: the two slowest cases are repeated eigenvalues and
eigenvalues six orders of magnitude apart, and both are exact well before the
twelfth sweep at 8x8, which is larger than anything a worksheet holds.

The decision worth recording is the refusal. A matrix that is symmetric only to
rounding is **not** symmetrised: deciding how nearly symmetric is near enough is
exactly the heuristic zero-test §8.40 refused symbolic linear algebra over, and
the message names the remedy instead so the worksheet writes it where a reader
can see it.

`examples/shaft.nomo` gained its principal stresses, and they check against the
Tresca stress the worksheet already had from a closed formula — an independent
check of the solver inside a real calculation rather than in a test.

### 17. Complex vectors — **done**

Built, and it cost exactly what §8.40 predicted a second value tower would:
**nine exhaustive matches**, no more. A complex vector is its own variant rather
than a complex element inside the real one, for the reason the scalar tower is
separate — every real worksheet stays on the code it was already on.

The lesson is in what it nearly shipped. A complex vector holds no real
elements, so every aggregate that reaches for `elements()` saw an *empty*
collection: `sort` of a complex vector answered `[]` rather than refusing. One
guard now catches every name that is not explicitly handled, which is a better
shape than remembering to check in thirty places.

A complex matrix is still not built, and transcendentals of a complex argument
stay refused for the branch-cut reason already written down.

## Phase 6 — how it looks, and how it edits

### 18. Typeset output — **first phase done**

Built as `render/mathml.rs`, walking the same trace the linear renderer walks:
fractions, superscripts, radicals, upright units against italic names, and a
subscript for the underscore in `sigma_allow`. A bracket that only existed to
say "divide all of this" is dropped, because the fraction bar says it — which is
the whole visual difference between typeset output and linear text with a bar
drawn through it.

Off by default (`nomo html --mathml`), and the reason to keep it off is the
verification: `check-mathml.mjs` confirms it in **Chrome only**, because this
machine has one browser. Firefox and Safari implement MathML Core and are not
checked, which is a gap in the evidence rather than a claim about them.

What that browser check is for, and why an assertion on the markup would not do:
a browser without MathML does not fail. It draws `<mfrac>` as a run of
characters, so the worksheet reads `w · L 2 8` and every markup test still
passes. The check therefore asks the page where the numerator *ended up* —
above the denominator, and taller than a letter — and was confirmed to fail
against output rendered without the flag.

### 19. The editor — **done, less the multi-document part**

Completion, hover and go-to-definition, all from a symbol table the engine now
reports after every edit: each name a worksheet binds, what it came to, and
*where it was written*. No parsing in the front end — a second answer to "what
does this worksheet mean" is exactly what design note §10 records CalcpadCE
paying for.

Two details worth keeping. A completion shows a **unit's dimension**, because
`ksi` and `kip` are one letter apart and mean different things, and the moment
of choosing is the moment that matters. And a name a *pack* supplied points at
the `use` line that brought it — the line in this worksheet responsible for it —
rather than into a file the reader cannot see.

The Typeset toggle landed here too: step 18 put MathML behind a CLI flag, and
this is where a reader meets it. It is a per-call option rather than a session
setting, because how a worksheet is drawn is a property of the view — the same
document is typeset in one pane and plain in a printout.

**Multiple open documents was not built.** It is pure interface work — tabs, a
draft and a file handle per document, and a decision about what "unsaved" means
across several — with no engine question in it, which makes it a step of its own
rather than a corner of this one.

## Phase 7 — the importer, continuing its own ranking

### 20. The folds — **measured, and not built**

The expectation was that these were folds. Reading all eleven says otherwise,
and design note §8.43 records it: nine are multi-statement programs, two are a
two-dimensional recurrence filling a matrix in place, and exactly **one** is a
genuine fold — over a triple, because its body reads the loop counter as well as
both accumulators.

Translating them needs local bindings inside an expression, indexed assignment,
or `iterate` over a tuple of mixed dimensions: two of those are mutation by
another name and the third is a limitation already recorded. And the one true
fold would emit a synthesised three-element state function that no engineer
would keep — an import that produces that has not translated the worksheet, it
has obfuscated it.

The largest group is the useful finding: three worksheets hand-write a
fixed-step Runge–Kutta because SMath's core has none. Nomo has `rk4` now, so
their *intent* is one line here — but recognising a hand-rolled integrator in a
twelve-statement body is pattern-matching on a program, which is a far less
trustworthy translation than anything else this importer does. The marker names
the construct and leaves the two lines to a person.

### 21. `description` (86 uses), `at` (45), `cases` (9)

The first is prose and nearly free, the second is substitute-then-evaluate, and
the third is a conditional in disguise. Together they are the largest remaining
non-CAS block in the mechanics corpus.

### 22. `ltle`, `ltlt`, `lele` — 29 uses, settled by disassembly

Refused today because the boundary convention is unknown, and refusing was
right. `/opt/smath` can say what it is, exactly as `PlotRegion.dll` settled the
plot span. If the disassembly is ambiguous they stay refused, and the marker
says why.

## What this list does not contain

- **A CAS.** §8.40 costed it against the built engine and the recommendation
  stands on demand rather than on effort: about 2% of the representative corpus
  touches anything CAS-like, and twice the CAS-shaped requirement dissolved into
  exact numerics. The trigger for reopening is the target user's own files.
- **Mutation, or a loop statement.** A worksheet is a set of definitions.
- **Anything third-party in the tree.** Every item above is this project's own
  work to write.
