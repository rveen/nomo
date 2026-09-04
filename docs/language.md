# The Nomo language

The specification, written alongside the implementation. It is the source the
golden-file corpus is drawn from, and it should never lag behind the code.

This describes the language as implemented and as pinned by the golden-file
corpus, not a target. Everything below works today; what does not exist yet is
listed under "Not yet in the language" at the end.

---

## A worksheet

A worksheet is a sequence of line-oriented statements. One statement per line;
blank lines are insignificant.

```nomo
' Cylinder volume
r = 5 cm
h = 12 cm
V = pi*r^2*h
V -> dm^3
```

## Statements

| Form | Example | Meaning |
|---|---|---|
| Comment | `' Shaker specifications` | Prose, read as Markdown. Carried to the renderer as documentation, not discarded. |
| Assignment | `r = 5 cm` | Bind a name. |
| Query | `V` or `V -> dm^3` | Display an expression with its result. |
| Unit declaration | `unit kip = 1000 lbf` | Introduce a named unit. |
| Global definition | `global g = 9.81 m/s^2` | Bind a name visible *everywhere*, including above this line. |
| Function definition | `fn area(d) = pi*d^2/4` | Define a function. |
| Check | `check sigma <= sigma_allow` | State a limit and report a verdict on it. |
| Pack | `use steel` | Bring in a curated set of definitions. |
| Digits | `digits 4` | Significant figures for results, from that line down. |
| Axis | `axis x log`, `axis y 0, 100`, `axis x "Frequency"` | How the plots below are drawn. |
| Label | `label "Gain", "Phase"` | What to call the curves of the plots below. |

Only a bare name may appear on the left of `=`. `x + 1 = 2` is an error, not an
equation to solve.

## Numbers

`1`, `9.81`, `2.5e3`, `1e-6`, `.019`.

The leading-dot form is supported because it occurs in real worksheets. `2e` is
*not* a malformed exponent — it is the number `2` juxtaposed with the name `e`.

## Names

Names may contain letters, digits, `_`, `°` and `%`, and may not begin with a
digit. Letters are Unicode, so `π`, `φ`, `Ω`, `Δp`, `°C` and `Ling_rms_N` are all
ordinary names.

**Units and variables share one namespace.** `m` is lexically just a name; only
evaluation decides that it means metres. This is what lets unit expressions be
written without any special syntax.

## Operators

Loosest to tightest:

| Level | Operators | Associativity |
|---|---|---|
| 1 | `->` conversion | left |
| 2 | `or` | left |
| 3 | `and` | left |
| 4 | `not` | prefix |
| 5 | `<` `>` `<=` `>=` `==` `!=` | left |
| 6 | `+` `-` | left |
| 7 | `*` `/` and juxtaposition | left |
| 8 | unary `-` `+` | prefix |
| 9 | `^` | right |
| 10 | call `f(x)`, index `x[i]` | postfix |

`if … then … else …` binds loosest of all; see **Conditions** below.

### Juxtaposition is multiplication

Writing two things next to each other multiplies them, at the same precedence as
`*`, left associative.

```nomo
5 cm            ' 5 * cm
9.81 m/s^2      ' ((9.81 * m) / (s^2))
kg m/s^2        ' ((kg * m) / (s^2))
1/2 m           ' ((1/2) * m), not 1/(2*m)
```

This is the whole reason units need no special grammar. It is also exactly how
SMath stores units internally — a unit operand attached by multiplication — so
worksheets imported from it map onto this directly.

### Powers bind tighter than unary minus

`-x^2` is `-(x^2)`. `-x * y` is `(-x) * y`. `2^3^2` is `2^(3^2)`.

### An expression nests at most 128 levels deep

Every descent into a sub-expression counts as one level: a bracket, a call
argument, an index, a vector element, an operand, an arm of an `if`. Past 128 the
line is refused with `SH010` and the rest of it is skipped, so a worksheet says
what it cannot read rather than taking the process down with it.

It is a fixed number for the same reason every other limit here is one — the
answer, and the refusal, must not depend on the machine. It is set by the
tightest target: the WebAssembly build traps at about 750 levels on its 1 MB
stack, where the native build survives to about 6 000. Nothing written by a
person comes close. The deepest expression among the worksheets in this
repository is 13 levels, and the deepest across the 114 SMath worksheets the
importer reads is 14.

Nesting and recursion are bounded **together** as well as separately, because
they multiply: brackets 120 deep inside a definition that calls itself 64 times
respects both ceilings and is still some 7 700 nested evaluations. Evaluation
stops at 512 nested steps and says so. A worksheet reaches that only by
combining deep brackets with deep recursion — ordinary recursion is unaffected,
and `fn fact(n) = if n <= 1 then 1 else n*fact(n - 1)` still answers at the
call ceiling.

## Collections

```nomo
[1, 2, 3]              ' vector
[[1, 2], [3, 4]]       ' matrix — every element is itself a row
x[3]                   ' index
K[2, 1]                ' index a matrix
[5, 10, 15] Hz         ' units apply to the whole vector
```

Matrix rows must all be the same length.

## Conditions

Comparisons answer the dimensionless **1 or 0**. There is no separate boolean
type: SMath worksheets compute with comparisons as numbers and this language has
to be able to receive them, and adding a type to the value tower would touch
every operator to buy an error message. What *is* enforced is the part that
catches real mistakes — a condition must be dimensionless, so `if x then …` with
`x` in metres is an error rather than a coin toss.

```nomo
too_long = span > allowed      ' 1 or 0
imperial = 1 in < 1 m          ' 1 — both sides are in base SI already
```

`<=`, `>=` and `!=` may also be written `≤`, `≥` and `≠`. Equality is `==`,
because `=` already binds a name.

The connectives are the words `and`, `or` and `not`. `!` is factorial to anyone
who has met SMath and `&` is bitwise to anyone who has met C, so neither is
borrowed. **`and` and `or` short-circuit**, which is what lets a guard guard:

```nomo
n > 0 and bay[n] > 0 m         ' never indexes at zero
```

### The conditional

`if c then a else b` is an **expression**, so it composes with arithmetic and a
function body can be piecewise without any new statement form.

```nomo
overhang = if span > allowed then span - allowed else 0 m
fn stress(f, a) = if a > 0 m^2 then f/a else 0 Pa
```

**Only the arm that is taken is evaluated.** That is not an optimisation: it is
what lets a conditional guard something that would otherwise fail, and it means
the untaken arm raises no diagnostic about work nobody asked for.

```nomo
chosen = if n > 0 then bay[n] else 0 m     ' bay[0] is never reached
```

The `else` arm reaches as far as it can, so `else if` chains without brackets and
`if a then b else c + 1` puts the `+ 1` inside the arm:

```nomo
grade = if load < 5 kN then 1 else if load < 10 kN then 2 else 3
```

**Both arms are required.** An `if` with no `else` would have to mean something
when the condition is false, and in a language where every expression has a value
there is no honest answer — so it is a syntax error rather than an invented zero.

In the rendered output the arm that ran is substituted and the arm that did not
is shown as it was written, since it has no values to show. The substituted
column says which way the worksheet went.

Note that both arms are still *dependencies*. Which one runs depends on values,
and the dependency graph is built before any value exists, so editing an input
used only by the untaken arm still recalculates the line.

## Packs — definitions that arrive by name

```nomo
use steel
sigma_allow = 0.6*Fy_A992
```

A design office does not want the elastic modulus of steel typed into forty
worksheets, each free to be wrong on its own. A **pack** is a curated set of
definitions that lives in one place. `nomo packs` lists what this build carries
and what each one holds.

A pack's definitions are **global**, so where the `use` line sits does not change
what the worksheet means, and its statements are **not shown** in the output: a
worksheet that shows its work should show the work its author did, not fourteen
constants nobody typed.

The names are the pack's own and are not qualified — `use steel` brings
`E_steel`, not `steel.E`. A qualified name would need a resolution rule and a
lexer that admits `.` in an identifier; the packs suffix instead, which reads the
way an engineer writes anyway.

**Packs are compiled into the engine.** They are not files beside the worksheet
and not fetched: a browser opens a file rather than a directory, so an include
that read the disk would work on the command line and not in the editor, and a
fetched one would put a network round trip inside a claim about determinism and
break working offline. The cost is that changing a constant means a new build.
The gain is that a worksheet gives the same answer on every machine and with the
network off.

Using a pack that does not exist is an error that lists the ones that do. Using
the same one twice is a warning — harmless, and almost certainly a mistake.

## Axes

```nomo
axis x log
axis x "Frequency"                 ' what the axis measures
axis y "Gain"
plot(gain_dB, 10, 100000)          ' a Bode plot

axis y linear
axis y -90, 0                      ' the window a phase response lives in
axis y "Phase"
plot(phase, 10, 100000)
```

`axis x` and `axis y` say how the plots *below* them are drawn, the way `digits`
says how the results below it are shown. Five settings:

| | |
|---|---|
| `axis x log` | a logarithmic axis, labelled in decades |
| `axis x linear` | back to a linear one |
| `axis y 0, 100` | a window: what is drawn, in the axis's own units |
| `axis x "Frequency"` | what the axis measures, drawn beside the numbers |
| `axis y auto` | back to the extent the span or the data implies, and unlabelled |

A label is a string, or a name holding one. It is what the axis *measures*; the
unit it is measured in is drawn at the end of the axis either way, because
`Frequency` and `Hz` are different questions and a reader asks them at different
moments. `auto` clears the label along with the rest, since it means back to
what the data implies.

What follows the axis name is read by the comma and nothing else: two
expressions separated by one are a window, and a single expression is a label.
That is what lets a label be a name — `axis y what`, where `what` holds a
string — rather than only a literal.

**A logarithmic *horizontal* axis changes the sampling as well as the drawing.**
This is the one place a display directive reaches the numbers, and it is
deliberate: 257 samples spaced linearly over 10 Hz to 100 kHz put four of them
below 1 kHz, so the first decade of a Bode plot would be drawn from four points
however fine the rest was. Spaced logarithmically, every decade gets the same
number — which is what a reader of the chart assumes has happened. The ends
still land exactly where the worksheet put them.

**A window is what is shown; the span is what is computed.** `plot(f, a, b)`
decides what is sampled and `axis` decides what is drawn, and they are kept
apart so that zooming a chart cannot quietly change the curve under it.

A logarithmic axis has no place at or below zero. A span that touches zero is
refused outright; a window that starts at or below zero is refused as long as
that axis is logarithmic, in whichever order the two lines were written. On a
fitted logarithmic axis, values at or below zero are drawn as the gap they are —
the same answer a non-finite sample gets.

### Naming the curves

```nomo
label "Gain", "Phase"
plot(open_loop, closed_loop, 10, 100000)
```

A plot's legend names each curve after the function that drew it, which is the
right answer until the functions are called `h1` and `h2`. `label` says what to
call them instead: the first name goes to the first curve, the second to the
second, and a curve past the last name keeps the name it was drawn from.

Like `axis`, it is a setting for the plots *below* it rather than for the next
one only. That is not a preference: an edit above a plot recomputes that plot on
its own, and a name used up by the first drawing would be gone by the next
keystroke.

## Checks — a worksheet that states whether it holds

```nomo
sigma = M/S
sigma_allow = 0.6*Fy

check sigma <= sigma_allow          ' pass
check delta <= L/360                ' FAIL
```

A worksheet is not finished when it has computed a number; it is finished when
it says whether the number is acceptable, and against what. `check` states a
condition and reports a verdict on it — in the result column where a value would
otherwise stand, because for a check the verdict *is* the result. The 1 or 0 the
comparison produced says nothing a reader wants.

A check **binds nothing and nothing reads it**. It takes one expression and no
name: `check x = 1` is an error rather than a binding whose name is spelled after
a keyword.

**A failed check is not an error.** The arithmetic is right and the design is
not, and those are different facts about a document. So a failed check produces
no diagnostic, does not stop anything downstream, and does not make the
worksheet invalid. What it does is count, and `nomo check` reports the count and
exits **2** — where 1 means the worksheet does not evaluate at all. A script can
therefore tell "this sheet is broken" from "this part is overstressed", which is
the whole reason the statement exists.

The condition must be a **dimensionless 1 or 0** — which is exactly what
comparisons, `and`, `or` and `not` produce. Anything else is refused rather than
read as true: a length, a string, a vector, `0.5`. A check that passed because
`5 m` is "truthy" would hide the mistake it exists to catch.

A condition that cannot be evaluated — `check 1 m <= 1 s` — is **not decided**
rather than failed, and carries a diagnostic. There is a difference between a
design that does not hold and one nobody could work out.

`check` is a keyword, so it is no longer available as a name. That cost was
measured before it was spent: across the 114 SMath worksheets the importer
reads, `check` is used as a variable name exactly **zero** times. One worksheet
in this repository used it and was renamed.

## Two ambiguity rules

Both exist to avoid making whitespace significant, which would be worse than
either rule.

**`f(...)` after a name is always a call**, never multiplication. To multiply by a
parenthesised group, write `x*(a+b)`. Note that `(a+b)(c+d)` *is* multiplication,
because the left side is not a bare name.

**`x[...]` is always an index**, never multiplication. To multiply by a vector
literal, write `x*[1, 2]`.

## Comments, and the prose in them

`'` runs to the end of the line. Comments are statements, not whitespace: a
worksheet's prose is part of its output.

**The text of a comment is Markdown**, in a small closed subset — headings,
paragraphs and lists. Consecutive comment lines are one block, so prose wraps at
whatever width the file is written to and still renders as a paragraph. A blank
line, a bare `'`, a statement or a figure ends the block.

```text
' # Interlock line monitor
' A safety interlock is a loop of wire threaded through every connector that
' has to be mated before a high-voltage bus may energise.
'
' The chain measures twice:
' - the difference across the loop says whether current is flowing
' - the common-mode level says which rail the loop is shorted to
```

| Construct | Written | Notes |
|---|---|---|
| Heading | `' ## Sizing` | `#` to `######`, and the space is required. |
| Paragraph | consecutive comment lines | Joined with a space. |
| Bullet list | `' - item` | `-`, `*` or `+`, space required. |
| Numbered list | `' 1. item` or `' 1) item` | Keeps the number it was written with. |
| Literal marker | `' \# not a heading` | A backslash, before a leading marker only. |

A level-1 heading at the top of a worksheet names the document: `nomo html`
takes the page's title from it rather than from the file name.

**A numbered list keeps its numbering across the mathematics.** A worksheet
writes step 1, then the lines that compute it, then step 2 — so each step is a
list of its own, and `2.` renders as 2 rather than restarting.

### What is deliberately not in it

Nothing here is an oversight; each one is refused for a reason, and design note
§8.41 has the measurement behind it.

- **No thematic breaks (`---`) and no setext headings.** `' --- resources ---`
  opens the resource trailer, and a paragraph followed by a line of dashes must
  not become a heading. A bullet requires a space after its marker, which is
  what leaves `---` as ordinary text.
- **No indented code blocks.** Leading whitespace is a wrap: of the prose the
  SMath importer emits across both corpora, 227 lines are indented and 224 of
  them are continuations of the line above. There is no nesting either, so an
  indented list marker is an item of the same flat list.
- **No raw HTML.** Everything is escaped on its way into the document.
- **No inline emphasis yet, and `_` never.** Worksheet prose is full of
  identifiers like `V_drop`, and underscore emphasis would eat them.

A comment is still an ordinary comment: nothing about parsing, evaluation or
the dependency graph knows any of this, and a build that renders prose as flat
lines still opens every worksheet.

**`' nomo 1` is not prose.** The version pragma is metadata, and neither
renderer shows it. See *File format and versions*.

**Images keep their own line**, `' image <name> <width>x<height>`, rather than
Markdown's `![…](…)`: the size a figure is drawn at is part of the document and
Markdown's syntax has nowhere to put it. See *Figures*.

`examples/prose.nomo` is the worked example of all of this.

---

## Units

A unit is a factor onto the SI base, a dimension, and — for temperature scales —
an offset. Dimensions are exponent vectors over the seven SI base dimensions, so
`N` and `kg·m/s²` are the *same* dimension because their vectors are equal, not
because anything compares text.

### Dimensions are rational

Exponents may be fractions. This is not generality for its own sake: fracture
toughness is measured in `MPa·√m`, which needs a half-integer length exponent.
It also makes `sqrt` work on any dimension rather than only even ones.

An exponent must be a small rational for the resulting dimension to be exact.
`(5 m)^0.5` is fine; `(5 m)^π` is an error. A dimension*less* base has no such
restriction, so `2^π` is fine.

### What is built in

SI base and derived units, with prefixes; imperial and US customary units, which
are first class — `in` is the most-used unit in the surveyed SMath corpus, ahead
of `mm` and `MPa`.

Prefixes attach to SI units only: `kN`, `MPa`, `mm`, `µs`. There is no kilo-inch.
**An exact match always wins over a prefix reading**, so `min` is a minute rather
than a milli-inch, `cd` is a candela rather than a centi-day, and `T` is a tesla.

`rad` is **dimensionless** — an angle is a ratio of lengths — so `sin(2)` and
`sin(2 rad)` agree. `°` and `%` are likewise dimensionless.

`VA` and `var` exist alongside `W` and are dimensionally identical to it;
electrical worksheets distinguish them by name, and the SMath corpus declares
exactly these two.

### Declaring units

```nomo
unit kip = 1000 lbf
```

A declared unit takes no prefix: `k` before a name someone has just invented is
much more likely to be a typo than a deliberate kilo-.

### Temperature, and the affine rules

`°C` and `°F` are *offset* scales. A reading on one is a **point**; every other
quantity is an **interval**, a displacement. That distinction is what makes these
rules fall out rather than needing to be special-cased:

| Expression | Result | |
|---|---|---|
| `20°C + 5 K` | `298.15 K` | a point displaced by an interval is a point |
| `20°C - 15°C` | `5 K` | the difference of two points is an interval |
| `20°C + 5°C` | **error** | two points do not add |
| `2*(20 °C)` | **error** | a point on an offset scale cannot be scaled |
| `20°C -> K` | `293.15 K` | conversion is always allowed |

A computed point is shown on the **absolute** scale, as the first row does:
`25°C` and `298.15 K` are the same reading, and choosing the second means the
displayed scale never depends on which operand happened to be written first. Ask
for a scale explicitly when you want one — `operating -> °C`.

Note the brackets in the fourth row. Juxtaposition binds at `*`'s precedence, so
`2*20 °C` reads as `(2*20) °C`, which is the ordinary quantity 40 °C and no
error at all. Scaling a point is only attempted once the point exists.

`K` and `°R` are absolute scales and therefore linear: they carry none of these
restrictions.

Getting this wrong produces plausible-looking numbers that are silently wrong,
which is why it is settled before any operator was written. `°F` appears in real
worksheets in the corpus.

## Evaluation

Names resolve as **variable, then constant, then unit**, so a binding shadows a
unit of the same name. Shadowing a multi-letter unit warns; shadowing a
single-letter one does not, because `V`, `h`, `A`, `F`, `P` and `T` are all unit
symbols *and* the most ordinary variable names in engineering, and a warning that
is usually wrong teaches people to ignore warnings.

**A binding that fails still takes the name.** If the statement defining `x`
produces no value, uses of `x` below it say so; they do not fall through to a
constant or a unit of the same name.

```nomo
PF = missing         ' error: `missing` is not defined
PF                   ' error: `PF` has no value: the statement that defines it failed
```

Without this rule that second line answers `1e15 F`, because `PF` is peta-farads
to the unit table. The two-letter space of every SI prefix against every unit
symbol is large enough that ordinary variable names fall into it — `Zs` is
zetta-seconds — so the failure would not look like one. A name that *nothing*
binds is still a unit; only a binding that exists takes the name away.

Constants: `pi` (`π`), `e`, `tau` (`τ`), `inf`.

**Reduction order is part of the language, not an implementation detail.** Sums
and products reduce strictly left to right and nothing evaluates in parallel, so
that a worksheet gives the same last bits on every machine.

### Built-in functions

| Group | Functions |
|---|---|
| Trigonometric | `sin` `cos` `tan` `cot` `sec` `csc` `asin` `acos` `atan` `atan2` |
| Hyperbolic | `sinh` `cosh` `tanh` `asinh` `acosh` `atanh` |
| Exponential | `exp` `ln` `log` `log10` `log2` |
| Numeric | `sqrt` `nthroot` `abs` `sign` `round` `floor` `ceil` `mod` `hypot` |
| Aggregate | `sum` `product` `min` `max` `mean` `median` `length` |
| Linear algebra | `transpose` `det` `inv` `identity` `diag` `dot` `cross` `norm` `trace` `eigenvalues` `eigenvectors` |
| Shape | `rows` `cols` `row` `col` `augment` `stack` `submatrix` `sort` `reverse` |
| Repetition | `range` `map` `iterate` |
| Numerical | `root` `roots` `derivative` `integral` `solve_linear` `rk4` |
| Tables | `linterp` |

Trigonometric and exponential functions require a dimensionless argument. Since
`rad`, `°` and `%` are dimensionless, `sin(30 °)` works and gives `0.5`.

`sqrt` halves the dimension, so `sqrt(16 m^2)` is `4 m`, and `nthroot(x, n)`
divides it by `n` — which is what rational dimension exponents are for. A
negative value has a real root only for an odd whole index, so `nthroot(-8, 3)`
is `-2` and `nthroot(-8, 2)` is an error rather than a complex number.

`log` always states its base: `log(x, b)`. `log10`, `log2` and `ln` are the
shorthands. A one-argument `log` means base 10 in some worksheets and base e in
others, and every `log` call in the surveyed corpora states its base, so
requiring it costs nothing real.

`mod(a, b)` takes the sign of `a`, as SMath's does, and refuses a zero divisor.
Both operands share a dimension and so does the answer.

`sum`, `mean`, `median`, `sort`, `min` and `max` need **one dimension across the
collection**, because each is a weighted sum or an ordering of it — comparing
`5 m` against `3 s` would mean comparing magnitudes in base units, which means
nothing. `product` is the exception: dimensions multiply, so
`product([2 m, 3 m])` is `6 m²`.

**`stdev` is deliberately absent.** Dividing by *n* and dividing by *n−1* are
both called the standard deviation, the difference does not show in the answer,
and nothing here can settle which a worksheet meant. Rather than pick one
silently, the language does not offer the name.

`submatrix(m, r1, r2, c1, c2)` takes the block from row `r1` to `r2` and column
`c1` to `c2`, inclusive, counting from one. A single column comes back as a
vector.

### Eigenvalues

`eigenvalues(m)` gives the eigenvalues of a **symmetric** matrix, ascending, and
`eigenvectors(m)` the directions that go with them as the columns of a matrix in
the same order. Principal stresses and their axes, mode shapes and their
frequencies, the axes of an inertia tensor.

```nomo
state = [[79.6, 63.7], [63.7, 0]] MPa
principal = reverse(eigenvalues(state))     ' largest first, as a stress table reads
```

The elements must share one dimension, and the eigenvalues carry it; the vectors
are dimensionless, because a direction is not a stress.

**Symmetric, and exactly symmetric.** A general matrix has complex eigenvalues
and is a different problem; a matrix that is symmetric only to rounding is
refused rather than quietly symmetrised, because deciding how nearly symmetric
is near enough is a tolerance, and this engine does not have those. The message
names the remedy — `(m + transpose(m))/2` — which the worksheet then writes and
a reader can see.

The method is cyclic Jacobi at a **fixed twelve sweeps**, which is a count
rather than a convergence test for the reason every limit here is a count.

### An initial value problem

`rk4(f, y0, a, b, steps)` integrates `y' = f(x, y)` from `y(a) = y0` to `x = b`
by classical Runge–Kutta at a fixed step, and answers with a table of `(x, y)`
rows — a shape the language already has somewhere to put, since `plot` draws one
and `linterp` reads a value out of one.

```nomo
tau = 100 s
fn cool(t, T) = -(T - 300 K)/tau
history = rk4(cool, 500 K, 0 s, 200 s, 50)
plot(history)
```

**The method is in the name, and the step count is in the call.** Both change the
answer, so both are the worksheet's to state: an `odesolve` that chose a method
and an error tolerance would answer a different question whenever the tolerance
was met differently, which is the same reason `integral` counts panels rather
than testing an error. Halving the step divides a fourth-order method's error by
sixteen, and a worksheet that wants to know how much its answer moved can
integrate twice and subtract.

Dimensions travel through: `f` returns a rate — the ordinate's dimension over
the abscissa's — and every step multiplies it by a step of the abscissa, so a
temperature integrated against time comes back a temperature. An offset scale is
refused, because the equation subtracts one temperature from another and scales
the difference.

Only a first-order scalar equation, for now. A system needs a vector whose
elements carry different dimensions — a position beside a velocity — which is a
limitation this engine already has and records.

### Reading a table

`linterp(xs, ys, x)` interpolates linearly between the rows either side of `x`.
Engineering data arrives as a table — a property against a temperature, a
section against a depth, a measured curve against a load — and this is how a
worksheet reads one.

```nomo
T  = [293, 373, 473, 573] K
Fy = [250, 235, 205, 170] MPa
linterp(T, Fy, 423 K)                ' 220 MPa
```

Both columns carry units, and so does the answer. Three rules, each of which
SMath decides the other way — the differences are deliberate and design note
§8.42 records what its implementation actually does:

- **A point outside the table is refused**, not extrapolated. A table asked for a
  condition it never covered is where a confident wrong number does the most
  harm.
- **The first column must be strictly increasing.** A table whose columns were
  passed the wrong way round is a mistake worth seeing rather than sorting away.
- **An offset temperature scale is refused.** Interpolation is a weighted sum,
  and a weighted sum of readings on a relative scale means nothing; use `K`.

The columns must be the same length, at least two rows long, and each internally
of one dimension.

### Strings

A string is a value like any other: `"C24"` binds to a name, an `if` chooses
between two, and `==` compares them.

```nomo
a = 3 m
a_max = 4 m
verdict = if a <= a_max then "singly reinforced" else "doubly reinforced"
timber = "C24"
is_c24 = timber == "C24"             ' 1
```

Written between double quotes, on one line, with **no escapes** — a string that
needs a quote inside it has no spelling here, and the missing closing quote is
reported where it happens rather than swallowing the rest of the worksheet.

A string has **no arithmetic and no order**. `"a" + "b"` is refused rather than
concatenating, and `<` is refused because ordering words means choosing a
collation the worksheet has no way to state. Equality is the whole of what can
be asked, which is what a worksheet does with a string: state a verdict, and
compare a grade against a key. A string is one value — `length` of one is 1, not
its number of characters — and it does not go inside a vector or a matrix, which
hold quantities.

### Vectors and matrices

`*` is **element-wise between two vectors** of equal length, which is what a
tabulated calculation wants — `acc/(2*pi*f)^2` over parallel columns. Between two
matrices, and between a matrix and a vector, it is the **matrix product**. Use
`dot(a, b)` for an inner product.

`cross(a, b)` is the cross product, and it is **three-dimensional only**. That is
what engineering asks of it — a moment as `cross(r, F)`, a surface normal as one
tangent crossed with another — and requiring three components catches vectors
that were meant to be dotted instead of quietly returning a number. Units come
out of the products it is made of, so `cross(r, F)` of metres and newtons is in
newton-metres; write `-> N*m` if you want it shown that way rather than in
joules, which carry the same dimension.

```
r = [1, 0, 0] m
F = [0, 2, 0] N
M = cross(r, F) -> N*m               ' [0 N·m, 0 N·m, 2 N·m]
```

A scalar broadcasts over either. Indexing is **one-based**, and a matrix is
indexed row first: `K[2, 1]`. A vector takes the column index too — `v[2]` and
`v[2, 1]` are the same element — because a vector is the column of *n* that the
rest of the language already treats it as. Any column but the first is out of
bounds, which is what a column of one means.

`identity(n)` writes the n×n identity matrix. It is dimensionless, which is the
only thing it can be and the only thing that makes it useful: it exists to be
multiplied by something, and `det(S - λ*identity(2))` — the characteristic
equation of a stress tensor — needs the ones to take their dimension from `λ`.

```
S = [[10, 4], [4, 6]] MPa
S - 2 MPa*identity(2)                ' [[8 MPa, 4 MPa], [4 MPa, 4 MPa]]
```

`diag(v)` is its companion: the square matrix with `v` down the diagonal, which
is how a mass or a scaling matrix is written.

```
diag([3, 4] kg)                      ' [[3 kg, 0 kg], [0 kg, 4 kg]]
```

The zeros carry the diagonal's dimension, so the result can be added to and
multiplied by other matrices — which is why a vector of *mixed* dimensions is
refused rather than filled with dimensionless zeros that would fail on the next
line. It goes one way only: it makes a matrix from a vector and does not also
read a diagonal back out of a matrix, because a function that decides which it
means by looking at the shape of its argument changes meaning when the argument
does.

`rows` and `cols` give the shape, `row(K, i)` and `col(K, j)` take one out, and
`augment` and `stack` put values together — side by side and one above the other.
A vector answers as the column it is, so `rows([1, 2, 3])` is 3 and `cols` is 1,
which is what indexing it assumes as well. Joining refuses a mismatch rather than
padding: a table with a short column is a mistake, not a shape to be repaired.

```
K = [[1, 2], [3, 4]] m
col(K, 2)                            ' [2 m, 4 m]
augment([1, 2] m, [3, 4] m)          ' [[1 m, 3 m], [2 m, 4 m]]
stack([1, 2] m, [3, 4] m)            ' [1 m, 2 m, 3 m, 4 m]
```

`sign` is dimensionless by construction — the sign of a length is a number, not a
length — and `sign(0)` is 0 rather than positive.

## Repetition

A worksheet is a set of definitions with dependencies, not a script, so there is
no loop statement and nothing mutates. What exists instead is the three things
loops in real worksheets are doing.

**`range(a, b)` and `range(a, b, step)`** build a vector. The end is included
when the step lands on it, because `range(1, 5)` has to be able to index a
five-element vector. All three arguments share one dimension, and the implied
step is one *of that dimension* — the only reading that makes the two- and
three-argument forms agree.

```nomo
counts = range(1, 5)                  ' [1, 2, 3, 4, 5]
odd = range(1, 9, 2)                  ' [1, 3, 5, 7, 9]
stations = range(0 m, 10 m, 2.5 m)    ' [0 m, 2.5 m, 5 m, 7.5 m, 10 m]
```

Elements are computed as `a + i*step`, not by repeated addition: ten additions of
`0.1` reach `0.9999999999999999` where ten times `0.1` is exactly `1`, and the
hundredth element must have the same last bits as the second.

**`map(f, v)`** applies a function to every element.

```nomo
fn area(s) = s^2
areas = map(area, [2 m, 3 m, 4 m])    ' [4 m², 9 m², 16 m²]
total = sum(map(area, stations))      ' accumulate without a running variable
```

**`iterate(f, x, n)`** applies a function `n` times, which is what a convergence
loop is:

```nomo
fn newton(x) = x - (x^2 - 2)/(2*x)
root = iterate(newton, 1, 5)          ' 1.41421
```

A fixed count rather than a tolerance test, deliberately: it terminates, it takes
the same number of steps on every machine, and it cannot spin.

**`root(f, a, b)`** finds where `f` crosses zero between `a` and `b`, and
**`integral(f, a, b)`** is the definite integral over the same span.

```nomo
fn f(x) = x^2 - 2
root(f, 1, 2)                         ' 1.41421

fn w(x) = 10 kN/m^2 * x               ' a triangular load
integral(w, 0 m, 3 m) -> kN           ' 45 kN
```

Both follow `iterate`'s rule — a fixed amount of work rather than a tolerance
test — so both give the same bits on every machine. `root` bisects, which needs
no derivative and cannot diverge, and it **requires a bracket**: if `f` has the
same sign at both ends it says so instead of answering, because a confident wrong
root is worse than an error. `integral` is Simpson's rule over a fixed number of
panels, exact up to a cubic. Dimensions fall out of the arithmetic — `f(x)·dx` —
so a load in kN/m integrated over metres gives kN, with no rule about integration
needed.

**`roots(f, a, b)`** asks a different question: not *refine this bracket* but
*what does `f` cross zero at anywhere between `a` and `b`* — which is what a
worksheet usually wants, and where the answer may be one place, two, or none.

```nomo
fn v(x) = 5*2^2 - 6*x*(x + 2)
roots(v, 0, 2)                        ' 1.08167
roots(v, -4, 4)                       ' [-3.08167, 1.08167]
```

It samples **200 intervals** across the window, both ends included, and bisects
every sign change between neighbouring samples. One root comes back as a value,
several as a vector in increasing order, and a window with no sign change in it
is an error rather than a guess. The count is fixed for the reason every other
limit here is fixed: it decides *which* roots are found, so a machine that
sampled a different number of points would be answering a different question.
What a scan cannot see it does not claim — two roots inside one interval cancel
each other's sign change and are missed, which is a property of the method and
the reason `root` is still here for the case where you can bracket the answer
yourself. A sample that comes back infinite or NaN breaks the chain rather than
counting as a crossing, so a pole is not reported as a root.

**`derivative(f, x)`** is the slope of `f` at `x` — a number, not an
expression.

```nomo
fn area(r) = π*r^2
derivative(area, 2 m) -> m            ' 12.5664 m

fn gain(f) = f/(1 + (f/1000)^2)       ' a curve with a peak
fn slope(f) = derivative(gain, f)
roots(slope, 1, 5000)                 ' 1000, where it peaks
```

**`derivative(f, x, 2)`** is the second derivative, which is where a curve turns:
an acceleration out of a distance, or the inflection of a distribution.

```nomo
fn fall(t) = 1/2*9.81 m/s^2*t^2
derivative(fall, 3 s, 2) -> m/s^2     ' 9.81 m/s²
```

It is **exact**, and it needs no step size. Every value carries a second
component saying how fast it is changing, and every operation carries the chain
rule alongside the arithmetic it was already doing — so the slope comes out
correct to the same rounding as the value, with no step to tune and no
truncation error traded against cancellation. The dimension is arithmetic too:
an area differentiated by a length is a length.

There is no symbolic derivative here and there is not going to be one: this
gives the value at a point, which is what a worksheet wants when it plots a
slope or looks for a peak. Both orders come out of the same
evaluation, so the second costs nothing extra; the third is not written, and an
order above the second says so rather than approximating one. A function whose
derivative is not written down — `floor`, `round`, `sign` — is **refused rather
than answered**, because a missing rule reported as a slope of zero would be
believed. A comparison inside
`f` asks about the value, so a piecewise definition differentiates on the branch
it takes; at the switch itself a piecewise function has no derivative, and this
reports the side it is on.

**`solve_linear(f, kinds)`** solves a system of equations that is **linear in
its unknowns** — the shape statics has, where ΣF = 0 and ΣM = 0 are solved for
the reactions.

```nomo
mass = 10 kg
fn balance(F, A, B) = [F + B - mass*2 m/s^2, A - mass*9.81 m/s^2, B - F - A]
solve_linear(balance, [0 N, 0 N, 0 N])   ' [-39.05 N, 98.1 N, 59.05 N]
```

`f` is a **residual**: it takes the unknowns and answers with what is left over
when they are substituted, so the solution is where it is zero. `kinds` names the
unknowns by **dimension** — a vector of forces here; its magnitudes are never
read, because the engine needs to know that the first unknown is a force, not
what force it might be.

No algebra is involved. A system linear in its unknowns *is* its coefficients,
and those come out of evaluating the residual: at zero for the constant terms,
and at one unit of each unknown for each column. Nor is linearity taken on
trust — the answer is put back into the equations, and a system that does not
balance says so rather than answering. A moment equation beside a force equation
makes the coefficients dimensionally mixed by row, which is handled by taking the
dimensions off and putting them back from `kinds`.

**`plot(f, a, b)`** samples `f` across the span and draws it.

```nomo
fn moment(x) = w*x*(L - x)/2
plot(moment, 0 m, 6 m)
```

A **fixed number of samples**, whatever the span — `integral`'s rule again, and
for the same two reasons: the drawing terminates, and it is the same drawing on
every machine, which is what lets a plot into the golden suite. It costs what
fixed sampling always costs: a feature narrower than the sample spacing can fall
between two samples and not be drawn. The way to see a narrow peak is to plot a
narrower span, which is what a person does with a chart anyway.

Both axes carry their unit, taken from the dimensions so that nothing is written
twice; `axis x "Frequency"` adds what the axis measures beside it. The
span's two ends must share a dimension, and so must everything the function
returns — one vertical axis means one dimension, and a function that changes
dimension across the span is an error rather than a chart with two meanings on
it. A sample that is not finite is drawn as a **gap** rather than a line through
values the function never took.

**Several curves** go on one plot by naming each of them: every argument but the
last two is a curve, and the last two are the span.

```nomo
fn light(f)   = gain_at(f, 0.2)
fn nominal(f) = gain_at(f, 0.41)
fn heavy(f)   = gain_at(f, 0.8)
plot(light, nominal, heavy, 30 kHz, 200 kHz)
```

They are sampled over the same span at the same points, so the curves are
comparable, and the one-dimension rule applies across them as well as along each
— a gain beside a length is refused rather than drawn. A legend under the
drawing names them in the order they were written, after the functions that drew
them or after whatever a `label` line said instead. A family of curves is written
by naming its members because the language has no lambdas; that also puts the
load a design is nominal at on the page under its own name.

**`plot(m)`** draws a **table of measured points**: an n×2 matrix, x in the
first column and y in the second.

```nomo
deflection = [[0 kN, 0 mm], [10 kN, 1.9 mm], [20 kN, 4.1 mm]]
plot(deflection)

fallen = augment(t s, map(drop, t))    ' two computed columns joined
plot(fallen)
```

No span is written and none is needed — the points brought their own x — so the
horizontal axis is **fitted to the data** and rounded out to whole ticks, which
is what the vertical axis has always done for the same reason: nobody chose it.
Each point is drawn as a mark, with a line through them in the order the table
gives. Up to two tables go on one plot, and each is named by whatever it was
written as.

Which kind of plot a call is depends only on **whether a span was written**,
never on what a name happens to hold: `plot(f, a, b)` names a function whether
or not something called `a` exists, and `plot(m)` is a table whether or not
something called `m` is also a function.

A plot is a value and can be bound to a name, but it is not a number: there is
no arithmetic on one. In `nomo html` it is drawn as an SVG the engine
generates, so the output stays a single self-contained file with no script in
it; the text renderer shows one summary line.

`map`, `iterate`, `root`, `roots`, `derivative`, `integral`, `solve_linear` and `plot` take the **name** of a function
as their first argument, and `plot` takes one per curve. That
is as close to a higher-order function as the language gets — there are no
lambdas, no closures and no function values, and a call's callee is still always
a name. Ranges and repetition counts are capped at a million, because a browser
tab has no way out of a hang.

### A function may call itself

The conditional is lazy, so a definition that reaches a base case terminates and
answers:

```nomo
fn fact(n) = if n <= 1 then 1 else n*fact(n - 1)
fact(6)                        ' 720
```

One that does not reach a base case is the mistake, and it is reported rather
than run. **Two ceilings**, both fixed numbers so that every machine gets the
same answer:

- **64 nested calls.** Above that, `` `f` is nested more than 64 calls deep ``.
  It is set by the tightest target, not the roomiest: the WebAssembly build
  stops answering somewhere near 200 calls deep and the native one near a
  thousand, so without a ceiling a worksheet could compute an answer on a
  desktop and kill the browser tab that opened it.
- **A hundred thousand calls per statement.** Depth alone does not bound the
  work: `fn f(x) = f(x) + f(x)` never nests deeper than the ceiling and still
  asks for 2⁶⁴ calls. The budget is shared by everything one statement does, and
  it counts calls to the worksheet's own functions — `map(sin, …)` over the
  largest vector a range can produce costs nothing against it.

## Order of evaluation

A worksheet is not a script. It is a set of definitions with dependencies between
them, and it is evaluated in dependency order — which is why editing one line
recomputes only that line and what reads from it.

### Two kinds of binding

`x = 1` is **positional**: visible to the statements below it. Rebinding the same
name later is allowed, and each use takes the nearest definition above it.

`global x = 1` is **global**: visible everywhere in the worksheet, including
above its own definition.

```nomo
' Conclusions first, inputs below — this works.
W = m*g
W -> kN

global g = 9.81 m/s^2
global m = 2500 kg
```

Globals exist because SMath's `≡` behaves this way, and worksheets imported from
it depend on that. When both kinds define one name, the nearest preceding
positional binding wins and the global is the fallback.

### Cycles are an error

```nomo
global a = b + 1
global b = a + 1     ' error: `a` and `b` depend on each other
```

Statements outside the cycle still evaluate normally. Note that positional
bindings cannot form a cycle — they only ever reach upward — so this can only
arise with globals.

## How results are shown

A worksheet shows its work. Each line is rendered in up to three columns:

```
V = π·r²·h = π·(5 cm)²·(12 cm) = 0.942478 dm³
    ───┬──   ────────┬───────    ─────┬────
  symbolic      substituted        result
```

The substituted column is the one an engineer audits, so it uses **the units the
bindings were written in** — `(5 cm)`, not the `0.05 m` the engine stores.
Constants stay symbolic: expanding `π` to 3.14159 lengthens the line and says
nothing.

Columns that would add nothing are omitted. Substituting a bare name just
restates the result, and a literal quantity like `g = 9.81 m/s²` already *is* the
answer — but `x = 2 + 3` genuinely computes something, so its result is shown.

A conversion target is echoed **as it was written**: `M -> kip*ft` shows
`281.25 kip*ft`, keeping the `*` rather than setting it as `kip·ft`. The unit you
asked for is the unit you see, which matters when a worksheet is checked against
a specification that spells it a particular way. Exponents are the one exception,
since `in^3` has an unambiguous typeset form: it displays as `in³`.

Numbers are rounded to significant figures (six by default) at render time only.
Arithmetic stays in binary; presentation is decimal.

`digits n` changes that count from the line it appears on downwards, between 1
and 17 — one is the fewest that says anything and 17 is where binary64 runs out.
It is presentation and nothing else: nothing is recomputed, the dependency graph
does not see it, and the full-precision values a snapshot records are untouched.

Four is what most of the worksheets here use. A stress is not known to six
figures and a page of six-digit numbers invites a reader to believe the last
two.

**A conversion in an assignment is remembered.** `A_s = pi/4*d^2 -> mm^2` binds
the value *and* records that this name is written in mm², so every later use of
it substitutes as `84.27 mm²` rather than `8.427e-5 m²`. Compound targets carry
as well as named ones — `mm^2`, `MN/m`, `N*m` — which matters because those are
what engineering units mostly are. It is how a worksheet gets its verdict lines
to read in the units the reader thinks in:

```nomo
sigma = M/S -> ksi
sigma_allow = 0.6*Fy_A992 -> ksi
check sigma <= sigma_allow          ' 10 ksi ≤ 30 ksi — pass
```

A unit that does not fit the value is ignored rather than applied: `M = 500 N*m`
offers `m` as the unit it was written in, and a moment shown as `500 m` would be
worse than one shown in base units.

### Typeset output

`nomo html --mathml` sets the symbolic and substituted columns as mathematics
rather than as text: a division becomes a fraction, a power a superscript, a
`sqrt` a radical, and a bracket that only said "divide all of this" is dropped
because the bar says it. The editor has the same thing behind a **Typeset**
toggle. It is off by default.

**A name that spells a Greek letter is set as that letter.** `sigma_allow`
draws as σ_allow, `lambda` as λ, `Delta_p` as Δ_p. This is presentation only —
the two spellings stay two distinct names, so a worksheet that binds both
`sigma` and `σ` has two variables that draw alike.

The letter you get is the one Unicode gives that name, so the spelled-out form
and the typed form agree: `phi` and `φ` are both U+03C6. The `var` names —
`varepsilon`, `vartheta`, `varkappa`, `varpi`, `varrho`, `varsigma`, `varphi` —
give the symbol variants ϵ ϑ ϰ ϖ ϱ ς ϕ, as in TeX.

A name maps only where the Greek letter looks different from the Latin letter
it would otherwise be set as. So there is no `omicron` (ο is o) and no `Alpha`,
`Beta`, `Epsilon`, `Zeta`, `Eta`, `Iota`, `Kappa`, `Mu`, `Nu`, `Omicron`,
`Rho`, `Tau` or `Chi` — those capitals are Latin letters to look at, and the
names stay yours to use. The uppercase names that do map are `Gamma`, `Delta`,
`Theta`, `Lambda`, `Xi`, `Pi`, `Sigma`, `Upsilon`, `Phi`, `Psi` and `Omega`.

**Units are never read as Greek names.** `psi` is pounds per square inch, and it
draws upright as `psi`.

**The font is part of the layout, not a matter of taste.** MathML reads the
fraction bar thickness, the axis height, the script shifts and the stretchy
bracket recipes from a font's OpenType MATH table, and a font without one leaves
the browser guessing them. The editor ships one and uses it. `nomo html` has
three choices:

| | What the document does | Self-contained |
|---|---|---|
| *(default)* | names fonts the reader's machine may have | yes |
| `--font-url <url>` | references a font served beside it | no |
| `--embed-font <file>` | carries the font inside it | yes |

The default keeps `nomo html` what it has always been — one file that fetches
nothing — but it cannot promise the reader has a math font. `--embed-font` is
the one that can: it costs the font's size in every file, and it embeds the
licence file sitting beside the font along with it, because a document that
carries a font redistributes it.

Only the *math* font is ever embedded, and the difference is what each does when
it is missing. A font with no MATH table lays a fraction out from ordinary text
metrics and the mathematics comes out wrong; a missing text face gives Georgia,
which is a perfectly good book face. So the prose of a standalone document names
`"STIX Two Text", Georgia, "Times New Roman", serif` and carries nothing, while
the editor — which serves its own assets — ships the face and uses it.

**A unit stands off its number**, as ISO 80000-1 asks: `50 mm`, not `50mm`.
Ordinary algebra does not — `2x` is tight, and what tells them apart is whether
what follows the number is a unit. The plane-angle degree is the standard's own
exception, so `90°` is tight where `20 °C` is not.

**A typeset line is set whole.** The result, the `=` between the columns and the
words `check` and `pass` are part of the same statement as the formula, so they
take the same face. An *untypeset* line stays monospace, because it is linear
text whose columns line up, and that is what monospace is for.

Italic and upright follow ISO 80000-2, and you get them by writing the name the
way you would say it: a single-letter symbol is italic, and a word is upright.
That is why `sigma_allow` comes out as an italic σ with an upright `allow` —
the subscript describes the quantity, it is not one. Built-in constants are
upright too: π, e and ∞ are set in roman, which is what distinguishes them from
a variable of the same name.

Constructs with no typeset form — a conditional, a conversion, a comparison —
are carried through as the linear text instead, because a sentence in the middle
of a formula beats a gap in the middle of a worksheet. That text has no Greek
convention, so a name inside one of those is shown as written.

The worksheets under `examples/` are drawn from this document, and their rendered
output is committed in `tests/golden/` and compared byte for byte on every build
(`cargo run -p nomo-cli -- test`). A rule stated here that no example
demonstrates is a rule nothing is checking, so new language behaviour should
arrive with the example that pins it.

## File format and versions

A `.nomo` file *is* the source text: there is no second serialised form to keep
in step, and worksheets diff and review like code.

An optional first-line pragma records the format version:

```nomo
' nomo 1
```

It is an ordinary comment, and a file without one reads as version 1. It is not
prose, though, so no renderer shows it: joined to the line under it — which is
where a title usually sits — it would open the document with `nomo 1 Cylinder
volume`. A worksheet declaring a *newer* version than the build understands still
opens, with a warning — refusing outright would leave someone staring at a file they cannot
read, when it is probably mostly fine.

**Saving adds the pragma** if the worksheet has none, so a file on disk says what
format it is in and a later build can migrate it rather than guess. A worksheet
that already declares a version keeps the one it has, including a version this
build does not understand: relabelling it would turn "I cannot fully read this"
into silent corruption.

## Complex numbers

The imaginary unit is **`i`**, an ordinary constant like `pi` and `e`. It needs
no number syntax of its own: juxtaposition is already multiplication, so `4i` is
`4*i` and `4i Ω` is `4*i*Ω`, the same reading that makes `2e` mean `2*e`.

```nomo
z = 3 + 4i
Z = (1 + 2i) Ω           ' both parts share one dimension
```

A binding wins over the constant, exactly as it does for `e`, so a worksheet
that wants `i` for an index or a current keeps it by saying so.

**Any operation with a complex operand on either side answers complex**,
promoting the real one. `+`, `-`, `*`, `/` and whole powers are supported, and
`abs`, `Re`, `Im`, `conj` and `arg` take a value apart:

| | |
|---|---|
| `Re(z)`, `Im(z)` | the two parts, as **real** numbers in the same dimension |
| `conj(z)` | `3 - 4i` |
| `abs(z)` | the modulus, in the same dimension |
| `arg(z)` | the argument in radians, and **dimensionless** whatever `z` is |

`Im` is the *coefficient* of `i`, not the component with `i` still attached, so
`Re(z) + Im(z)*i` rebuilds `z`.

**The two parts share one dimension**, because they are components of one
measurement rather than two: an impedance is `(1 + 2i)·Ω`. `1 m + 2i s` is an
error, not a value with two dimensions in it. A complex value is displayed with
its unit outside the brackets, written once, for the same reason.

**A complex value never becomes real again on its own.** `(1 + 2i) - 2i` stays
complex and displays as `1 + 0i`. Demoting when the imaginary part happens to be
zero would make a result's *type* depend on its value through a floating-point
comparison, so it would fire on some worksheets and not on others differing in
the last bit. Ask for `Re(z)` and get a real.

There is no complex temperature: a reading on an offset scale is a position on a
scale, and an imaginary part would displace it in a direction the scale does not
have.

### A vector of complex numbers

A branch of impedances is one value rather than three names:

```nomo
branch = [Z_R, Z_L, Z_C]
Z_series = sum(branch)
magnitudes = abs(branch) -> ohm      ' a real vector
angles = arg(branch) -> °
halved = branch/2
```

A vector literal with a complex element in it is a complex vector, and its
elements share one dimension as a real vector's do. Arithmetic is elementwise
with the rules the real path already has: a scalar broadcasts, a real vector of
the same length joins in, another complex vector pairs up. `Re`, `Im`, `abs` and
`arg` answer a **real** vector, since a magnitude is not complex; `conj` answers
a complex one; `sum`, `length`, `reverse` and indexing work as they do
elsewhere.

Everything else refuses **by name**. Ordering complex numbers has no meaning, so
`sort`, `min` and `median` say so; `norm`, `dot` and the transcendentals say so
too. That guard exists because the alternative was worse than an error: a
complex vector holds no real elements, so an aggregate that reached for them saw
an empty collection and answered confidently — `sort` gave back `[]`.

A **matrix** of complex numbers is not built.

## Figures

A worksheet carries its images inside itself. The body refers to one by name,
and may say how large the figure is drawn:

```nomo
' Measured step response
' image figure1 749x483
```

and the data sits in a trailer at the end of the file, introduced by a marker
line and holding one block per image — a header naming it, and the base64 that
follows, indented, up to the next block or the end of the file:

```nomo
' --- resources ---
' image figure1 png 116338
'   iVBORw0KGgoAAAANSUhEUgAABIkAAALrCAIAAAD…
'   AAAgAElEQVR4nOy9d3xUVfr4f2Yy6b1XSCM9IY…
```

The size in the header is the image's own bytes, for a person reading the source;
the data is what is believed. `png`, `jpeg`, `gif`, `bmp` and `webp` are shown;
anything else is reported rather than guessed at, as is a reference whose data is
missing and a payload that is not base64.

**A reference is not Markdown's `![…](…)`, and this is why.** The size on the
reference is a different fact from the one in the header: not how many bytes the
image is, but how large the figure is drawn, in pixels. That is placement rather than
content — a photograph 1161 px wide placed at 749 is the author deciding how
large it reads beside the mathematics, which the pixels alone cannot say — so it
sits on the line that says where the figure goes, not on the one that says what
its bytes are. A reference without a size, which is every worksheet written
before the size existed, shows the figure at its own size.

It is a size to scale **down** from and never a crop. A page or a pane narrower
than the figure asks for shrinks it whole; there is no width at which a reader is
shown part of a diagram without being told so. The renderers write it as the
image's `width` and `height`, which `max-width: 100%` and `height: auto` in the
stylesheet turn into a request rather than a fixed size.

Of the two, **the width is what the figure is drawn at**; the height reserves the
space while the image decodes, but the drawn height follows the image's own
proportions. SMath lets a picture be dragged out of shape and Nomo does not
reproduce that: a stretched diagram is a wrong diagram, and nothing on the page
would say it had been stretched. For a figure scaled proportionally — nearly all
of them — the two are the same picture to within a pixel.

A third word that is not a size is not a reference at all: the line stays the
comment it is, rather than becoming a figure drawn at a size nobody wrote.

**Every line of this is an ordinary comment**, which is what the version pragma
above already is. That is the point: a worksheet carrying figures opens in a
build that has never heard of them, and shows its trailer as the comments it is
rather than failing to parse. The cost is that an image is not a statement — it
cannot be produced by an expression and nothing computes one — which is the right
trade while a figure is scanned evidence rather than a result.

Why a trailer, and not the data where the figure stands: a `.nomo` file is its
own source text, so an image can only live in it as base64, and 116 KB of base64
in the middle of a worksheet costs the format the property it was chosen for.
Collected at the end, the body stays readable and the blobs are one contiguous,
append-only region that a `.gitattributes` rule can mark `-diff`. Images beside
the worksheet as separate files were the alternative, and the browser cannot do
it: it opens a *file*, not a directory.

`nomo html` embeds each figure as a `data:` URI, so the output stays the single
self-contained document it promises to be.

## Not yet in the language

Recorded so the omissions are deliberate rather than forgotten.

- More than two tables on one plot. Two is what the arity rule leaves room for
  before a span would be ambiguous, and no corpus worksheet draws more.
- Complex **matrices**. A vector of complex numbers is built; a matrix of them
  is not, and says so rather than losing an imaginary part.
- Transcendentals of a complex argument — `sqrt`, `exp`, `ln`, the trigonometric
  functions — and a fractional or complex *exponent*. All of them need a complex
  logarithm, and that needs a branch cut: a decision about where `arg` jumps from
  `π` to `-π`, which decides what `(-1 + 0i)^0.5` is. There is one conventional
  answer and no way for a worksheet to say it means the other, so the language
  says it cannot rather than choosing quietly.
- Comparing complex numbers. `<` and `>` have no meaning on them, and `==` is
  refused with the rest of the family rather than being the one that works.
- Raising a matrix to a power.
- Strings inside a vector or a matrix, and every function of a string:
  concatenation, length in characters, searching, formatting a number as one. A
  string is a verdict or a key here, and nothing yet builds one out of another.
- Inline formatting in prose — emphasis, code spans, links. The block level is
  built; this is the layer above it, and if it is ever added it will be `` ` ``
  and `**` and never `_`, because worksheet prose is full of names like
  `V_drop`. Tables, block quotes and fenced code are not planned at all.

## Reserved

`unit`, `fn`, `global`, `check`, `use`, `digits`, `axis`, `label`, `if`, `then`,
`else`, `and`, `or` and `not` are keywords and cannot be used as names.
