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

### 5. A performance report, before there is anything to regress

Measured 2026-08-29: 5 000 statements render in 40 ms, a 3 000-deep dependency
chain in 20 ms. `nomo bench` records that on fixed generated shapes and CI runs
it as a **report, not a gate** — exactly as the coverage report is run, and for
the same reason a wall-clock threshold on a shared runner is a flake generator.

*Gate:* `cargo test --workspace`; the report prints in CI.

## Phase 2 — the three things a design sheet needs

### 6. `check` statements — a worksheet that states a verdict

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

*Gate:* `cargo test`, golden suite, `scripts/build-web.sh`,
`scripts/compare-targets.sh`.

### 7. `linterp`, and table lookup — settled from SMath, not guessed

Material and section tables are what every how-to in Phase 3 needs, and the
engine cannot interpolate at all. Two questions the corpus cannot settle decide
the semantics: what happens **outside** the tabulated range, and what a lookup
returns on multiple matches. Extrapolating a material table is a real hazard, so
neither will be guessed — SMath is at `/opt/smath` and its own implementation
settles both, which is the method that settled the plot span (§8.21), `solve`
(§8.24) and `·` between two columns (§8.39). Implement what the disassembly
states; refuse with a marker whatever it leaves ambiguous. The findings go in the
design note as §8.42, and the importer maps the names it now has a target for.

*Gate:* `cargo test`, golden, `scripts/check-corpus.sh`.

### 8. The cheap missing builtins, in one batch

`mod`, `product`, `sort`, `reverse`, `submatrix`, `trace`, `rank`, `nthroot`,
`hypot`, `log(x, base)`, `cot`/`sec`/`csc`, the inverse hyperbolics, and
`mean`/`median`/`stdev`. Each is a few lines in the `eval.rs` dispatch with an
obvious dimension rule. One decision is worth stating rather than assuming:
ordering requires a common dimension, so `sort` and `median` refuse a
mixed-dimension vector rather than comparing it in base units.
`examples/functions.nomo` grows to cover them, because that file exists to put
every function through the native-versus-WebAssembly comparison.

*Gate:* `cargo test`, golden, `scripts/compare-targets.sh`,
`scripts/check-no-host-math.sh`.

### 9. Packs — shared units and constants, without I/O

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

*Gate:* `cargo test`, golden, `scripts/compare-targets.sh`.

## Phase 3 — the applications

Each worksheet is prose, inputs, the calculation, a `check` against a limit, and
a plot where one earns its place. Each gets a golden snapshot. Each is **ours to
publish**, unlike the corpora and unlike the two customer worksheets that had to
be removed — which is the other reason to write them.

### 10. Six mechanical how-tos

Bolted joint preload and torque; a shaft under combined bending and torsion;
Euler and Johnson column buckling; bearing L10 life; a helical compression
spring; thin- and thick-wall pressure vessels. One at a time, one commit each.
The acceptance criterion is not that the arithmetic runs — it is that a
practising engineer agrees with the method and with the source it cites.

### 11. What the how-tos broke

Whatever Phase 2 turns out to have missed, ranked by how many of the six wanted
it. Expected: rounding to a preferred size, a fatigue curve wanting
interpolation on a log axis, and per-line significant figures.

### 12. The gallery, and the migration story shown rather than described

`nomo html` already produces a self-contained page per worksheet; those become a
gallery. And `docs/smath.md` explains the import but never *shows* one: an SMath
worksheet beside the `.nomo` it becomes, with every marker explained, is the
artifact that persuades an SMath user to try it — and someone trying it is how
the current-era worksheets §11 question 8 still needs actually arrive.

## Phase 4 — ship it

### 13. A release

Nothing reaches a user today: no release workflow, no hosted build, no binary.
A tagged release deploys `web/dist` to Pages, builds CLI binaries for
linux-x86-64, linux-aarch64 and macOS, and publishes the wasm artifact **with
its hash**, so that whoever downloads it can check the determinism claim rather
than take it. `web/dist` gets cleaned on build first: it currently carries a
stale 650 KB `sheaf_wasm.wasm` from the old project name, which would otherwise
ship.

*Gate:* the workflow green on a tag; the published page loads, computes, and
still works offline after one visit.

## Phase 5 — the deeper engine, ranked by what Phase 3 asked for

### 14. A plot's axis limits, and a log scale

A Bode plot cannot be drawn today, which blocks the electrical direction
entirely. SMath's own `limits_x/y` attributes give the semantics for free
(§8.21) and no worksheet in either corpus sets them — so this is a language
decision rather than an import one.

### 15. A fixed-step ODE

`rkfixed`-shaped: n steps, RK4, deterministic **by construction**, because the
step count is exactly the fixed number this project's rule already demands.
Dynamics, thermal transients and control follow from it. Seven corpus calls, and
far more demand outside the corpus than in it.

### 16. Symmetric eigenvalues

Jacobi with a fixed sweep count — the same determinism argument, ten corpus
uses, and principal stresses and mode shapes are core mechanical content.

### 17. Complex vectors

So that an impedance network is one expression rather than three scalars.
Scalars-only is a recorded limitation and this is its next phase. It stops short
of transcendentals of a complex argument, which stay refused for the branch-cut
reason already written down.

## Phase 6 — how it looks, and how it edits

### 18. Typeset output

The HTML renderer emits linear text in spans — `w·L²/8`, no fraction bars, no
radicals. This project's bet is that engineers accept a text *syntax*; that bet
is much easier to win when the *output* is typeset. A deterministic MathML
emitter from the trace the engine already builds adds no dependency and is
byte-comparable in the golden suite exactly as the plot SVG is. Behind a render
option until the browser checks confirm it in Chrome, Firefox and Safari.

### 19. The editor

Unit- and function-aware completion that shows the dimension, hover giving a
name's value and dimension, go-to-definition, and more than one open document.
The boundary already supports multiple sessions — nothing in the module is
global — so the missing part is interface, not engine.

## Phase 7 — the importer, continuing its own ranking

### 20. The folds

What is left of the loops once `for` becomes `map`: a recurrence
(`el(β, i) ← … el(β, i-1) …`, 8 loops) and an accumulator. Both are `iterate`
over a pair.

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
