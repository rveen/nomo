# Folio: a paged typesetting output

The plan for the `nomo-folio` branch: a second typeset output, laid out as
pages by [Folio](https://github.com/trukeio/folio). It is written in the
roadmap's form — one commit per phase, and each phase names the gate it has to
pass.

## What the two pieces are

**Nomo** has one typeset output. `render::html::render` wraps `html::body()` in
a self-contained page with a light `@media print` block, and the editor prints
its output pane through `web/style.css`. Nothing breaks the worksheet into
pages, so there is no page number, no running head and nowhere for a logo to go.

**Folio** is a client-side paged-media engine in TypeScript, MIT licensed. Give
it HTML and CSS Paged Media and it lays the document out as pages in the
browser: margin boxes, `counter(pages)`, tables that split, and line breaking
of MathML. There are two ways in: a drop-in script (`folio.polyfill.js`) and a
`Previewer` the caller drives (`folio.mjs`).

The division of work follows from that. **Nomo emits a Paged-Media document;
Folio paginates it in the browser.** The Rust engine never paginates, so the
determinism guard is not touched: pagination is downstream of the numbers, the
same way the esbuild bundle of the user interface is downstream of the
WebAssembly module.

## The boundary this draws

The numbers stay bit-reproducible. **Where the pages break does not**: it
depends on the fonts the browser lays the text out in, and on the browser's
own line breaking. That is a real limit, and it is the right one — a page
break is presentation, the same class of thing as the colour of a link — but it
has to be written down rather than discovered. Embedding the math font
(`--embed-font`) is what narrows it in practice.

## Phases

### 0. Folio, obtained reproducibly

`scripts/fetch-folio.sh` clones Folio at a pinned commit, builds it with the
`pnpm` its `package.json` names (through corepack — the system `pnpm` 8 cannot
read its lockfile), and copies the three files Nomo uses, with Folio's
`LICENSE`, into the gitignored `web/vendor/folio/`. They are checked against
hashes in `scripts/folio/`. `--verify` only checks.

Committed: the provenance (repository URL, commit, hashes). Not committed: the
bundle. Folio is by the same author under the same licence, but it is a
separate project with its own repository, and "nothing third-party gets
committed" is easier to keep as a rule with no exceptions.

Measured before pinning: two clean builds of commit `a73682f` gave
byte-identical `folio.polyfill.js`, `folio.polyfill.min.js` and `folio.mjs`.
Only the TeX bundles differed between the two builds; Nomo does not use those.
The minified polyfill is 115 kB, 40 kB gzipped. The bundles Nomo uses contain
only Folio's own code (its `THIRD-PARTY-LICENSES.md`: Temml is only in the TeX
bundles).

*Gate:* `./scripts/fetch-folio.sh --verify`.

### 1. Core: `render/paged.rs`

`pub fn render(sheet, opts, title, engine: &PagedEngine) -> String`, which
reuses `html::body()` unchanged, so **no golden file moves**. `PagedEngine` is
built by `PagedEngine::url` or `PagedEngine::embedded(script)`. The second
takes the script as text, read by the caller so that `nomo-core` still does no
I/O, and returns an error if the script could close its own `<script>` element.
`html::render` now shares a `document()` with it, and its output was checked
byte-identical for every example, plain and `--mathml`, before and after.

The page chrome is fixed CSS — nothing in it varies, not even a date — so the
document stays byte-reproducible:

- `@page { size: A4; margin: … }`;
- `@top-left { content: "Nomo" }` — **the logo placeholder**. Its own rule and
  comment, so a real logo replaces it later through `content: url(…)` or a
  running element;
- `@top-right` the worksheet's title, through `string-set` on the title `h1`
  (`opening_heading` already decides what that is);
- `@bottom-right` `"Page " counter(page) " of " counter(pages)`;
- `@page :first` drops the running head.

Print-media fixes the paged document needs and the plain one does not: a
forced light scheme (Folio drops `@media screen` but not
`prefers-color-scheme`), no `max-width` or margins on `body`, `.step` set
`pre-wrap` with `break-inside: avoid`, `break-after: avoid` on headings.

Unit tests: the placeholder, the `@page` rules and the script are there, and
an embedded engine cannot close its own `<script>` — the pinned bundle
contains no `</script` and no `<!--`, and the test makes sure a later one
cannot slip through either.

*Gate:* `cargo test --workspace`, `nomo test` (unchanged), clippy, fmt,
`check-no-host-math.sh`.

*Found while doing it.* Folio builds each page from a **clone of the source's
`html` and `body`, attributes included**, and rewrites `:root` to match the
clone. So a marker attribute set on the root while pagination runs is copied
into every page, and the print guard keyed on it hid the content of every
finished page. Only the margin boxes printed. Neither `:root` nor a child
combinator scopes the rule away from the clones; the `after` hook clears the
attribute from every element that carries it. Smoke-tested before committing:
all 29 examples paginate with nothing overflowed, every step is visible under
print media, and a PDF printed from Chrome shows the head, the running title
and the foot.

### 2. CLI: `nomo html --paged`

A flag beside `--mathml`, `--font-url` and `--embed-font` rather than a new
subcommand, because it composes with them. Embeds
`web/vendor/folio/folio.polyfill.min.js` by default, so the output is still
one self-contained file; `--folio-url <url>` links it instead, and
`--embed-folio <file>` carries a different build. Either of the two implies
`--paged`. A missing bundle names `fetch-folio.sh`.

The default path is relative to the working directory, the convention
`nomo test` already follows, rather than baked in at build time, which would
have put the build machine's layout into every release binary.

Embedding Folio redistributes it, so the `LICENSE` beside the script is
written into the document as a comment, as `--embed-font` does with a font's
OFL. Unlike the font, a missing licence is **refused** rather than warned
about: Folio's terms are known and they ask for the notice, so there is
nothing to be unsure of. `PagedEngine::embedded` takes the notice for that
reason.

*Gate:* `cargo test --workspace`. An end-to-end test (`tests/paged.rs`) runs
the binary against a stand-in script. Smoke-tested as well: the documents the
real CLI writes for all 29 examples paginate in Chrome, with nothing hidden
in print and nothing overflowed.

### 3. WebAssembly: `nomo_render_paged`

Beside `nomo_render_html`, with the same "the page is what the CLI writes"
test. `scripts/compare-html.mjs` covers the paged variant, so native and
WebAssembly agree byte for byte.

One entry point for both forms of the engine: `embed` chooses whether `folio`
is a URL or the script, with its licence in `notice`. The script crosses the
boundary rather than living in the module. The module is the artifact the
determinism claim rests on, and carrying a layout engine inside it would make
every Folio release a new engine build. A script that could end its element
comes back null, as non-UTF-8 input does. `boundary.mjs` gains
`renderPaged(source, title, { mathml, fontUrl, folio })`, where `folio` is
`{ url }` or `{ script, notice }`.

`compare-html.mjs` compares three forms per worksheet: the standalone page,
the paged document with Folio embedded, and with Folio linked. That is 87
pages. Each output is removed before the CLI writes it, so a run that writes
nothing cannot be compared against the previous form's page. Since the
script compares the paged document, `compare-targets.sh` runs
`fetch-folio.sh` before it. That script builds only if the files are
missing, so CI's three `compare-targets.sh` jobs obtain Folio with nothing
added to the workflows. They use Node 22, which still bundles corepack
(Node 25 drops it; that is when `fetch-folio.sh` will need another way to
the pinned pnpm).

*Gate:* `node scripts/compare-html.mjs`, `scripts/compare-targets.sh` — all
six of its gates green.

### 4. The editor: a Pages view, and printing through it

*Changed from the first draft.* The draft had the editor import `folio.mjs`
and drive a `Previewer` in its own page. What was built instead puts the
document `nomo_render_paged` returns into an `<iframe srcdoc>`, with Folio
linked from `folio/folio.polyfill.min.js`. Three reasons:

- Folio installs `@page` rules and a print stylesheet for the whole document
  it runs in. In the editor's page those collide with the editor's own.
- A frame prints itself, so Print gives exactly the pages on screen with
  nothing of the editor around them.
- The editor's pages are then byte-for-byte the document phase 3 gates.

The frame only fetches Folio when Pages is first opened, so nothing extra is
needed to load it on demand.

What was built:

- **Buttons.** *Pages* toggles the results pane between results and pages.
  *Print*, and Ctrl+P, show the pages, wait for Folio to finish, and call the
  frame's `print()`. Ctrl+P is caught in the frame as well as the editor: after
  a click into the pages, the key events go to the frame's document and never
  reach the editor's window. `check-pages` found this on its first run.
  Printing from the browser's own menu still prints the results pane, as
  `check-print` has always required.
- **Layout timing.** The pages are laid out again on a 600 ms debounce. An
  unchanged document is not laid out again, and the reader's scroll position
  survives a re-layout.
- **Fitting the pane.** An A4 page is about 800 px wide, wider than the pane.
  The editor scales the pages down to fit with a screen-only `zoom` that it
  adds to the frame after Folio has finished. It is not written into the
  document, which has to stay the CLI's byte for byte.
- **Build and licence.** `web/build.mjs` copies the script and its `LICENSE`
  to `dist/folio/` and includes them in the service worker's cache digest.
  `sw.js` precaches the script, and `NOTICE.txt` carries Folio's licence
  verbatim, read from the shipped copy. `build-web.sh` runs `fetch-folio.sh`.
- **Math font.** The math font is the shipped subset, named to Folio as
  `mathFont`. The plan's first risk is settled: Folio's MATH-table probe
  accepts `STIX Two Math Subset`.

*Gate:* `scripts/build-web.sh`, which now runs `check-pages.mjs`. That check
covers:

- Folio is not fetched before Pages is asked for.
- A long worksheet lays out on more than one page, with nothing overflowed,
  every step exactly once, the "Nomo" head, the running title and the foot.
- No page is wider than the pane.
- Typeset pages pass the math-font probe.
- Print, Ctrl+P in the editor and Ctrl+P in the pages each reach the frame's
  `print()`. A headless print dialogue cannot be opened, so `print` is
  replaced by a counter.
- The browser menu's print prints the results.

`check-offline.mjs` adds the Pages view opened for the first time with the
network already off. (Since the follow-up "Pages in the worker's first
moments", the editor fetches Folio and the fonts and hands the frame `blob:`
URLs; the frame requests nothing.)

### 5. A browser gate: `scripts/check-paged.mjs`

Chrome through the existing `chrome.mjs`. For every worksheet in `examples/`:
paginate the paged output, then assert that `overflowed` is empty and there is
at least one page; that the placeholder is in the head of every page but the
first; that every `.step` appears exactly once across the pages (Folio's own
invariant, checked from outside); and that no step is split across a page. A
PDF of one worksheet through `page.pdf({ preferCSSPageSize: true })` is written
for a human to look at.

*As built.* All 29 examples, in two forms: the document as it stands, and
typeset with the shipped math font embedded, which puts Folio's MATH-table
probe in front of that font. Each of the 58 documents is rendered by the CLI
itself, and the check asserts:

- Folio finished, built as many pages as it reported, and none overflowed.
- The worked lines on the pages, compared by text and in order, are exactly
  the document's. The document is re-parsed without running it to get them.
  A line split across two pages would fail this as two half-lines.
- "Nomo" heads every page but the first, and each page's foot reads
  "Page n of N".
- Under print media, nothing is hidden and no element still carries the
  marker.
- The PDF has exactly one A4 sheet per page. The sheets are counted from the
  bytes, since Chrome writes one uncompressed `/MediaBox` per page. A blank
  sheet in front of page 1, or Chrome's own margin boxes around Folio's,
  would show up here.

`chrome.mjs` gains `pdf()` for this. Every typeset PDF is left in
`target/paged/`, not just one.

The early print is checked too. The math font is served from a route that
holds it back, and Folio waits for fonts, so the body is parsed and nothing
is laid out yet. In that state the body must be hidden in print, and visible
once the font is released. Holding back Folio itself does not create that
state: a classic script in the head stops the parser, so there is no body to
print. That is how the first version of this check timed out.

The gate was tested by breaking what it guards. With the phase 1 bug put
back, so the marker is cleared from the root but not from the pages' clones,
it reports 117 problems. With the print guard removed, it reports the early
print. It runs in `build-web.sh`; all 18 gates pass.

### 6. Documentation

*Done.* Design note §8.55, and an entry in §12 saying what was verified in a
browser and what was not. STATUS.md: a summary, a row in "Where things
stand", the commands, and the two new browser checks. The CHANGELOG has an
Unreleased section, the README a build line, and CLAUDE.md a pointer here.

Writing it turned up one more gap, recorded in §8.55. The paged document
names the *text* face and does not carry it, and the editor's pages inherit
that: the frame is a document of its own, and the `@font-face` that gives the
editor STIX Two Text Subset is in the editor's stylesheet. So both are set in
Georgia on a machine without STIX Two Text. The remedy is a
`--text-font-url` beside `--font-url`, and it is the first thing worth doing
next on this branch.

### Follow-up: the text face

*Done.* `TextFont` in `nomo-core`, a policy beside `MathFont`, with a URL form
and no embedded one; `--text-font-url` and `--text-font-italic-url`, which
work with or without `--paged`; and two more arguments to
`nomo_render_paged`, which the editor fills with the files it already serves.

- **The document.** It gets the editor's two `@font-face` rules and a body
  stack with the subset first, after `STYLE`'s own body rule. The margin boxes
  name the subset first too. With no text face nothing is added, and `nomo
  html` was re-checked byte-identical to before the branch across all 58
  plain and `--mathml` documents.
- **The module.** `nomo_render_html` keeps its signature, because a server
  calls it.
- **`compare-html.mjs`.** The two paged forms carry the face on both targets.
  The script now also asserts that the face is in the page, since two sides
  that both dropped it would still agree.
- **`check-pages.mjs`.** Asserts the face loaded in the frame and is first in
  the prose's stack. Removing the editor's argument fails both assertions.
- **`check-paged.mjs`.** Serves the face beside the typeset documents and
  asserts the same.

*Gates:* 743 tests, `compare-targets.sh` with all six gates green, and
`build-web.sh` with all 18 green.

The original intent of this phase:

A design-note entry (§8.55). It knowingly revisits two parts of §8.48:
MathJax was declined partly because typesetting would be asynchronous where
printing is synchronous, and Folio is asynchronous too — phases 2 and 4 are how
that is handled; and §8.48 named MathML line breaking as the condition to
revisit, which Folio provides. STATUS.md, the CLI usage text and CLAUDE.md's
command list.

## Risks to settle on the way

- **The subset math font.** Settled in phase 4: Folio accepts it. Its `mathFont` probe throws if the family has
  no usable `MATH` table. That `STIX Two Math Subset` keeps one is to be
  verified, not assumed.
- **Wide formulas.** `.step` lines carry *inline* `<math>`, and Folio breaks
  only display math, so a very wide line still overflows. Emitting
  `display="block"` changes `body()` and every golden file; that is its own
  decision, not part of this branch.
- **Ctrl-P before pagination finishes** in a standalone file. Settled in
  phase 1: the content is hidden in print while `data-nomo-paging` is on the
  root, and Folio's `after` hook removes it, so an early print is empty rather
  than unpaginated. The hook also sets `data-nomo-pages` and
  `data-nomo-overflowed`, which phase 5 waits on and reads.

## Decided

- Folio's bundle is fetched, built and hash-verified, not committed.
- The CLI shape is the `--paged` flag, not a `nomo paged` subcommand.
- The logo is the placeholder text "Nomo" until there is a logo.

### Follow-up: Pages in the service worker's first moments

*Reported from use.* Opening a 23-page worksheet and choosing Pages with
Typeset on showed "folio: "STIX Two Math Subset" is laying out math without a
MATH table", and a reload cured it.

*Found.*
- **First guess, disproved.** Two layouts racing for the frame's `load`
  event. Instrumenting showed one document, never loaded,
  stuck in `<head>` waiting for Folio's script.
- **The cause.** The frame's requests went through the service worker, and
  in the first moments after a new worker takes over (the first visit after
  every build) they could stall. Folio's script never arrived, or the math
  font did not, and the probe refused the fallback face.
- **The proof.** With `/sw.js` answering 404, the same sequence worked every
  time.

*Fixed.* The editor fetches Folio and the three fonts once and hands the
frame `blob:` URLs, so the frame requests nothing. The repro with the
reported worksheet, run ten times, never failed after the change and always
failed before it.

*Gate.* `check-pages` opens typeset pages straight after the editor starts.
It asserts that the frame requested nothing but `blob:` URLs and that the math
font loaded. It fails against the old code, which fetched both over HTTP.
`build-web.sh`: all 18 gates green.

### Follow-up: the head and the foot

*Asked for.* Remove "Nomo", move the title from the head's right to its left,
and put the source file's name, extension included, at the foot's left.

*Done.*
- **The page.** The head carries the title at the left from the second page
  on, and nothing at the right. The foot carries the file name at the left
  and "Page n of N" at the right, on every page.
- **Where the name comes from.** It is a new argument, threaded through
  `paged::render`, `nomo_render_paged` (after the title) and `renderPaged`'s
  `fileName`. The CLI takes it from the path; the editor passes the name in
  its header, which is `untitled.nomo` for a buffer never saved. Empty leaves
  the corner blank. It is escaped like the title.
- **The margin boxes.** Each box gets its own rule: a margin rule is an
  at-rule, and the first draft's `@top-left, @bottom-left { … }` was invalid
  CSS that a browser would have dropped. The first page loses the title by a
  rule written after the one that sets it, so it holds whether `:first` is
  weighed by specificity or by order.

*Gates.* The unit tests assert the new boxes, that nothing is at the head's
right, and that "Nomo" is gone. `tests/paged.rs` and the module's test assert
the file name in the foot. `compare-html.mjs` passes it on both sides.
`check-paged` asserts, for all 58 documents, that the title (as `<title>`
resolves it) heads every page but the first, that the head's right is empty,
and that every foot names the file. `check-pages` asserts the same for
`untitled.nomo`.
