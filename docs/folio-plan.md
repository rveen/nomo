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
reuses `html::body()` unchanged, so **no golden file moves**. `PagedEngine`
follows `MathFont`: `Url(String)` or `Embedded(Vec<u8>)`, the bytes read by the
caller so that `nomo-core` still does no I/O.

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

### 2. CLI: `nomo html --paged`

A flag beside `--mathml`, `--font-url` and `--embed-font` rather than a new
subcommand, because it composes with them. Embeds
`web/vendor/folio/folio.polyfill.min.js` by default, so the output is still
one self-contained file; `--folio-url <url>` links it instead. A missing
bundle names `fetch-folio.sh`.

*Gate:* `cargo test --workspace`.

### 3. WebAssembly: `nomo_render_paged`

Beside `nomo_render_html`, with the same "the page is what the CLI writes"
test. `scripts/compare-html.mjs` covers the paged variant, so native and
WebAssembly agree byte for byte.

*Gate:* `node scripts/compare-html.mjs`, `scripts/compare-targets.sh`.

### 4. The editor: a Pages view, and printing through it

`web/build.mjs` copies `folio.mjs` into `dist/`; the editor `import()`s it the
first time the Pages view opens or the worksheet is printed, so the editor
costs nothing more until then. It goes in `sw.js`'s cache list and in the
notice `scripts/notice.mjs` writes. The view runs
`new Previewer({ pageDefaults, mathFont: "STIX Two Math Subset" })` into its own
container beside `#output`, and Print awaits that before `window.print()`.

*Gate:* `scripts/build-web.sh`, `check-offline.mjs`, `check-print.mjs`.

### 5. A browser gate: `scripts/check-paged.mjs`

Chrome through the existing `chrome.mjs`. For every worksheet in `examples/`:
paginate the paged output, then assert that `overflowed` is empty and there is
at least one page; that the placeholder is in the head of every page but the
first; that every `.step` appears exactly once across the pages (Folio's own
invariant, checked from outside); and that no step is split across a page. A
PDF of one worksheet through `page.pdf({ preferCSSPageSize: true })` is written
for a human to look at.

### 6. Documentation

A design-note entry (§8.55). It knowingly revisits two parts of §8.48:
MathJax was declined partly because typesetting would be asynchronous where
printing is synchronous, and Folio is asynchronous too — phases 2 and 4 are how
that is handled; and §8.48 named MathML line breaking as the condition to
revisit, which Folio provides. STATUS.md, the CLI usage text and CLAUDE.md's
command list.

## Risks to settle on the way

- **The subset math font.** Folio's `mathFont` probe throws if the family has
  no usable `MATH` table. That `STIX Two Math Subset` keeps one is to be
  verified, not assumed.
- **Wide formulas.** `.step` lines carry *inline* `<math>`, and Folio breaks
  only display math, so a very wide line still overflows. Emitting
  `display="block"` changes `body()` and every golden file; that is its own
  decision, not part of this branch.
- **Ctrl-P before pagination finishes** in a standalone file. Hide the content
  in print until Folio's `after` hook sets a class, so an early print is empty
  rather than unpaginated.

## Decided

- Folio's bundle is fetched, built and hash-verified, not committed.
- The CLI shape is the `--paged` flag, not a `nomo paged` subcommand.
- The logo is the placeholder text "Nomo" until there is a logo.
