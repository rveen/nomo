# Importing SMath worksheets

How to bring a `.sm` worksheet into Nomo, what survives the trip, and what does
not. This describes the importer as it is today, measured against the 114
worksheets `./scripts/fetch-corpora.sh` brings down into `corpora/` — they are
third-party and are not in this repository (THIRD-PARTY.md). The reasoning behind
each decision is in design note
§8, and §8.13–§8.39 are the running log of how it got here.

The governing rule, which explains most of what follows: **nothing is translated
on a guess, and nothing is dropped in silence.** A construct the importer cannot
be sure of becomes a visible marker in the output and a counted note, so the gap
is something a reviewer can see rather than something the numbers quietly
absorb. A construct that is right most of the time is worse than one that reports
it cannot tell.

---

## Importing a worksheet

Import is a command-line tool. It is not in `nomo` itself and not in the browser
editor: the importer needs the whole corpus-checking apparatus behind it, and a
migration is a thing you do once and review, not a file type the editor opens.

```bash
cargo run -p nomo-smath --bin smath-import -- worksheet.sm > worksheet.nomo
```

That writes Nomo source to stdout — the whole worksheet, including its figures.
Then open it the way you would any worksheet:

```bash
cargo run -p nomo-cli -- render worksheet.nomo   # three-column text
cargo run -p nomo-cli -- html   worksheet.nomo   # self-contained HTML, figures embedded
cargo run -p nomo-cli -- check  worksheet.nomo   # evaluate and report diagnostics
```

`--lang` picks which translation of a multilingual worksheet to keep. SMath
stores prose per language in the same region, and 97 of the wiki corpus's text
regions and 902 of the mechanics corpus's carry two or three. Codes are ISO
639-2, as SMath writes them — `eng`, `ger`, `rus`, `ita`, `dut`:

```bash
cargo run -p nomo-smath --bin smath-import -- --lang eng worksheet.sm
```

Without it each region keeps its first variant and a note records how many were
dropped.

### Read the report before you trust the output

`smath-import` on a single file prints only source. The notes — what was
refused, what was renamed, what was carried but is not displayed — are in the
check report, which works on one file as well as on a directory:

```bash
cargo run -p nomo-smath --bin smath-import -- --check worksheet.sm
```

```
1 worksheets, 0 unreadable
17 stored answers in 1 worksheets

       17  agreed

    17 of 17 comparable answers agree (100.0%)
    17 of 17 answers could be compared at all (100.0%)
    1 of 1 worksheets check out completely

Import notes
        5  Unsupported
       20  Carried
        1  ScopeFlattened

What could not be translated, by how often
        3  a stated equation that cannot be written out
        1  `…`, `…` are used here but defined nowhere in this worksheet, and SMath kept the region symbolic
        1  page header math: 04 - 04 - 2025
```

**This is the important step, and skipping it is the one way to be misled.** A
worksheet whose every line evaluates can still be wrong, because a construct the
importer refused leaves the variables it would have updated holding the values
they were initialised with — and the lines that display them go on evaluating
perfectly well. `chisquareddist.sm` converges a value in a `while` loop; without
the loop it reports its iteration count as 1 where SMath stored 5, and its answer
as 50 where SMath stored 15.9872. Nothing errors. The marker three lines above
is the only thing that says so.

That failure mode is why the report splits disagreements by whether the
worksheet translated in full. Across the wiki corpus that split is **0 in a
worksheet that translated completely, 33 in one that did not**.

### The other report: what the reader could not read

`smath-coverage` runs before translation and answers a different question — what
is in these files at all, and which gaps are worth work:

```bash
cargo run -p nomo-smath --bin smath-coverage -- corpora/nomo-corpus/sm
```

It counts regions by kind, math statements by kind, stored answers, function
calls resolved against the built-in registry versus defined by the worksheet
itself versus unknown, and units by use — then ranks the gaps by how much of the
corpus each accounts for. That ranking is how the order of the work gets decided.

### Checking a whole corpus, and keeping it honest

`--check` over a directory reads every `.sm` beneath it. `./scripts/check-corpus.sh`
is the committed regression gate: a per-worksheet baseline in `tests/corpus/`,
compared exactly, so any change that moves a result fails until the baseline is
regenerated beside the code that caused it. `--write` accepts an intended change.
Each baseline line carries verdict counts, a digest of the computed values, and a
digest of the emitted source — the third because a commit once rewrote four lines
in three worksheets while every count and every value stayed identical.

---

## What comes across

### The file itself

Both eras of the format. SMath changed structurally at 0.88 — before it, a
`<math>` holds its tokens directly; from 0.88 it holds an `<input>`, an `<area>`
*contains* the regions it collapses rather than marking them, and 1.x wraps
everything in an XML namespace. The reader detects the era from structure, never
from the version string, because version strings run from `0.85` to `1.5.0.9678`
and the break aligns with nothing nameable in them. All 114 corpus worksheets
read: 35 legacy and 19 modern in the wiki set, 60 modern in the mechanics set.

Regions are taken depth-first in file order — 442 of the wiki corpus's 3878
regions and 308 of the mechanics corpus's 4090 are nested one or two levels
inside a collapsed area, and a reader that takes only the top level looks about
eleven per cent low across every category at once.

### Mathematics

| SMath | Nomo | Note |
|---|---|---|
| `←` / `:` positional assignment | `x = …` | |
| `≡` with a name on the left | `global x = …` | Position-independent scope, collected in a pre-pass |
| `≡` with a call on the left | `fn f(x) = …` | |
| `≡` nested in an expression | `==` | An equality test there, not a binding |
| `=` display | the bare expression | Its second operand is SMath's cached answer, taken as an assertion rather than as code |
| `el(v, i)`, `el(m, i, j)` | `v[i]`, `m[i, j]` | Indexing is syntax in Nomo. The most-used function in the corpus |
| `mat(…, rows, cols)` | `[[…], […]]` | Row-major, settled by four corpus matrices that name their elements. A single row or column becomes a vector |
| `if(c, a, b)` | `if c then a else b` | Nomo evaluates only the arm it takes; SMath's does not |
| `range(a, b)` | `range(a, b)` | The two-argument form only — see below |
| `sum(e, i, a, b)` | `sum(map(term, range(a, b)))` | The summand is lifted into a named `fn`; Nomo has no lambdas |
| `int(e, x, a, b)` | `integral(f, a, b)` | Same lift |
| `solve(e, x, a, b)` | `roots(f, a, b)` | A 200-point scan with every sign change refined — read out of `SpecialFunctions.dll`, not inferred (§8.24) |
| `diff(e, x)`, `diff(e, x, 2)` | `derivative(f, x)`, `derivative(f, x, 2)` | Only where the expression actually reads `x`; see the refusals |
| `†` | `cross(a, b)` | Settled by 48 corpus uses — moment sums `r † F`, a unit normal `e.z † e.t(t)` |
| `norme` | `norm` | The CustomFunctions Euclidean norm, settled by `7.3.sm` dividing a vector by it nine times and getting unit vectors |
| `invert` | `inv` | |
| `·` between two columns of equal length | `dot(a, b)` | SMath's `*` is an inner product there and Nomo's is element-wise; read out of `SMath.Math.Numeric.dll` (§8.39) |
| `vectorize(e)` | `e` | Nomo's operators and functions are already element-wise over vectors. Dropped, but noted |
| A `for` that fills a vector | `map`, or `augment` for two columns | 39 of the 105 `for` loops across the corpora are exactly this |

Everything else resolves against the engine's own `BUILTINS` list, so the
importer cannot drift from what the language actually has. A worksheet's own
function is looked for **first**, which is SMath's resolution order — 14 names
are built-in in one worksheet and user-defined in another, and four of them are
users shadowing a built-in with their own definition.

### Units

Units are recognised by resolving the symbol, never by reading `style="unit"` —
that attribute is a display style, and in the mechanics corpus eight of the
symbols carrying it are ordinary variables, several of them the unknowns the
worksheet solves for. Units attach by multiplication in SMath and by
juxtaposition in Nomo, so `230*V` is emitted as `230 V`.

In-document unit declaration comes across in both forms SMath writes: the alias
(`VA : W`) and the magnitude (`a.0 := 1·m`). Imperial and US customary units are
first class — `in` is the most-used unit in the wiki corpus, ahead of `mm` and
`MPa` — and `°F` is affine and handled as such.

**A variable named after a unit the same worksheet also uses** is renamed rather
than left to collide. SMath tells `m := 1 kg` from `d := 1 N*(m/s)^-1` by an
attribute; Nomo resolves the name, so without the rename the second line reads
the metre and answers `4.429 m/s` — the right number with a nonsense dimension.
The rename is reported as a `Renamed` note, because the alternative reading is
the one Nomo would otherwise have taken.

### Names

SMath names are looser than Nomo's in three ways, and each is respelled: `.`
separates a subscript and becomes `_` (250 names across the corpora), `#` marks a
parameter or temporary and becomes `_` (37), and `'` is prime notation and
becomes a `_prime` suffix (10) — a suffix rather than `_`, because `f` and `f'`
are two different functions and `f_` is a third. A name that would land on a
Nomo keyword gains a trailing `_`. Two SMath names that would respell to one
Nomo name are refused and reported as a `Collision`, not merged.

### Text, figures and plots

Text regions become comments. Prose that sat *beside* a value on SMath's
two-dimensional page reads *after* it once flattened to lines; that is said once,
in a note, rather than on each of the several hundred regions it applies to.

Pictures are carried whole. The body gets a reference at the size SMath drew the
figure — `' image figure1 349x410` — and the base64 sits in a trailer at the end
of the file. Every line of it is an ordinary comment, so an imported worksheet
opens in a build that has never heard of figures. `nomo html` embeds each as a
`data:` URI. See "Figures" in `docs/language.md`.

Plots come across in both kinds: a function of `x` drawn over the span the
viewport implies, and a table of measured points, which needs no span. The span
was settled by reading SMath's plot plugin (§8.21), after two attempts to infer
it from worksheets disagreed by four orders of magnitude. `sys(…)` — SMath's way
of saying "these curves on one plot" — becomes several arguments to `plot`.

A page header is kept, clearly fenced, and **not run**: it repeats on every
printed page in SMath rather than forming part of the document, and Nomo has no
page model.

---

## What does not come across, and why

Each of these is a deliberate refusal with the evidence behind it. Counts are
marker lines in the emitted source across all 114 corpus worksheets unless said
otherwise — `grep -c '\[import\] unsupported:'` over an import reproduces them —
and they are ranked by how much of the two corpora each accounts for.

**Free symbols — 429 markers.** A worksheet may use a name it never binds. SMath
allows it where the region is set to symbolic optimization, because its CAS keeps
the name as a symbol; Nomo has no free symbols. The statement becomes a marker
plus the translated line as a comment. Most are unit labels typed as bare math
(`ft`, `s`, `lb`) and symbolic-solve unknowns, but the interlock worksheet of
design note §8.20 shows the honest case: `Vout : …*V2 - …*V1`, the generic
transfer function written for a
reader, with the same `Vout` reassigned four lines later with values substituted.
Nothing is wrong with the worksheet.

**The CAS.** Nomo has none and will not have one. That rules out the Maxima
surface (`Maxima`, `MaximaTakeover`, `assume`, `ratexpand`, `float`, `Jacob`),
the symbolic-solve idiom (`Solve`, `Assign`, `Unknowns`, `Clear`, `at`), and
`description` — 195 markers between them, and `description` alone is 86 call
sites in the mechanics corpus. Thirty of that corpus's 60 worksheets declare the
Maxima plugin before a token is parsed, and the surface costs about 7.7% of that
corpus's math regions. §8.34 prices the items in three tiers: the linear solve
came in as `solve_linear`, formula-valued names were attempted, measured and
reverted (§8.37), and genuine algebra stays out.

**Indexed assignment — 103 markers.** `el(A, i, j) ← …` mutates. A Nomo
worksheet is a set of definitions, not a script: nothing mutates, so there is no
indexed assignment to translate it into. Where the loop around it is an element-wise
fill, `map` says it exactly and the importer takes it; what is left is
recurrences and accumulators, which are folds.

**`while` loops — 13 markers.** They run until a tolerance is met. Nomo's
`iterate` takes a count, because a count is the same on every machine and a
tolerance is not, so translating one would mean inventing the number of steps.

**`range` with a step — 18 markers.** In Mathcad's lineage the third operand is
the *second element* rather than a step, and which one SMath means has not been
verified. Left unsupported rather than silently multiplied or divided by two.

**`solve` with no range — 5 markers.** It searches between `SolveFromPoint` and
`SolveToPoint`, which are program options rather than anything the worksheet
records. The range decides which roots are found, so inventing one would invent
the answer.

**`roots` and `FindRoot` — local searches, 18 and 20 markers.** SMath's
`roots(expr, x, guess)` searches from a starting point; Nomo's `roots(f, a, b)`
scans a window. Same spelling, different function. `5.1.sm` is the proof it matters: `roots(Q(x·m), x,
-1)` gives `1.08` and `roots(Q(x·m), x, -1.1)` gives `-3.08` — two guesses a
tenth apart landing on different roots, which is what a local method does and a
scan never does. Refused by name, so that a Nomo builtin acquiring a name cannot
quietly change what an imported worksheet means; that is exactly what happened
when `roots` was added to the language and 8 regions started translating wrongly
with nothing failing.

**`diff` of an expression that does not read the variable — 22 markers.**
SMath's `diff` is symbolic: `diff(y.B, t)` differentiates the *formula* `y.B`
stands for, and Nomo would differentiate the *number* it evaluated to, answering
zero. A wrong answer that looks like an answer is the one outcome this importer
must not produce.

**A binder inside a definition that captures the definition's parameter — 47
markers.** Nomo has no closures, so lifting the body out of `sum`, `int`,
`solve` or `diff` would put it above the definition, where it would read the
worksheet's global of that name instead of the parameter. Refused with that
spelled out (§8.38).

**Functions with a computed parameter — 22 markers**, and matrices with a
computed shape. Both need a value the file does not state at import time.

**The `—` operator — 6 markers.** Narrowed but not settled: every use has a
function call on the left and that function's expanded form on the right, which
reads as a symbolic-evaluation display. Not implemented until SMath confirms it.

**The `ltle` / `ltlt` / `lele` family — 29 call sites.** Their boundary
convention is unverified. `|` is logical or — all its uses sit in an `if`
condition between two comparisons (§8.11) — and is likewise not implemented on
the strength of that reading alone.

**Third-party numerics — 15 markers.** `rkfixed`, `lspline`, `sys2mat`, `ODE.2`
(Mathcad Toolbox); `eigens_by_jacobi`, `dn_LinAlgEigenvalues`,
`dn_LinAlgEigenvectors` (DotNumerics). Small counts, and each is a real algorithm
rather than a spelling.

**Region kinds with no Nomo equivalent**: `writer` (9), `cflabel` (6),
`comboboxlist` (4). An unrecognised payload is reported, never skipped. So is
plot configuration assigned through property paths like
`XYPlot'Traces#0'Name` — 24 markers, a plugin side-channel with nowhere to land.

**Document angle mode.** If a worksheet is set to degrees, the import says so
at the top and evaluates trigonometry in radians — never silently multiplied by
π/180. No corpus worksheet does, so this path is written rather than measured.

---

## Where the corpus stands

Two corpora, both fetched into `corpora/` rather than committed, both
third-party (design note §8.6 on what they are and what they are not; run
`./scripts/fetch-corpora.sh` before any of the numbers below can be reproduced).

**Wiki corpus** — 54 worksheets, SMath 0.82–0.98:

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

**Mechanics corpus** — 60 worksheets, SMath 1.3–1.5:

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

The mechanics corpus leans hard on the CAS — 30 of its 60 worksheets declare the
Maxima plugin — which is why so many lines do not evaluate and why none of the
ones that do disagrees.

**The number that matters is not the agreement rate.** It is that no
disagreement anywhere is in a worksheet that translated completely. The
non-evaluating lines are coverage, and they rank the remaining work; the
disagreements would be correctness, and there are none of that kind.

### The one place a tolerance is allowed

A stored answer is a decimal string another program wrote a decade ago, not a
value this engine produced, so the comparison gets half a unit in the last
displayed place. That tolerance is read off the stored literal itself — `1.8491`
means ±0.00005 of a mantissa, scaled by `expected / 1.8491` through the exponent
and the units at once — rather than off the document's `precision` setting, which
counts decimals of the *mantissa* and made the tolerance 100000 times too tight
when it was consulted.

This is the only tolerance in the project. The golden-file suite compares Nomo's
own output bit-exactly and must keep doing so.

---

## Reviewing an import

1. Run `--check` on the file and read the note counts first.
2. Grep the output for `' [import]`. Every refusal is on its own line, above the
   commented-out original, saying what it could not do and why.
3. Treat a worksheet with any `Unsupported` note as untrusted end to end, not
   just at the marker. A refused construct leaves stale values downstream that
   evaluate without complaint — the marker says where the trouble started, not
   how far it reached.
4. `Carried` is not a gap in the mathematics: the data survived (figures,
   mostly), but nothing displays it yet. `ScopeFlattened` and `Renamed` are
   things that changed meaning-preservingly and are worth a glance.
5. Compare the rendered worksheet against the SMath original. The stored answers
   check the numbers; nothing checks the prose or the layout.
