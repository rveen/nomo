// Render docs/language.md into the site as web/dist/language/index.html.
//
// The language reference is the one document a person using the editor actually
// reaches for, and until now it existed only in the repository. Somebody who
// opened the deployed editor and wanted to know what `->` does had to go and
// find a Markdown file on GitHub.
//
// # Why not the engine's own prose renderer
//
// Because `prose.rs` is a closed subset — headings, paragraphs and lists — and
// closed on purpose: design note §8.41 records the measurement behind every
// exclusion, and a `---` thematic break in particular is refused because
// `' --- resources ---` is the trailer sentinel. The reference needs tables (69
// rows of them), fenced code (44 blocks) and rules. Widening the worksheet
// language so that a documentation page can be built would be the tail wagging
// the dog, and it would change what worksheets mean.
//
// So the reference gets its own converter, and markdown-it is it: build-time
// only, like esbuild and subset-font, and never linked into the bundle — the
// browser is served static HTML that was rendered here. `html: false` because
// the same reasoning that makes `prose.rs` escape everything applies to a page
// built from a file in this repository: there is no passthrough to want, and
// none to go wrong.
//
// It lives here rather than under scripts/ for the same reason `font.mjs` does:
// it needs a package from `web/node_modules`, and ESM resolves from the file's
// own directory. `build.mjs` calls it.

import MarkdownIt from "markdown-it";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const source = join(here, "..", "docs/language.md");

const md = new MarkdownIt({
  // No raw HTML passthrough. See the header.
  html: false,
  // No autolinking: the reference writes about syntax, and a bare `a -> b` or a
  // unit that looks like a host name should stay the text it is.
  linkify: false,
  typographer: false,
});

/**
 * An id for a heading, so a section can be linked to.
 *
 * Slugged from the text rather than numbered, because a numbered anchor changes
 * meaning the moment a section is inserted above it, and the whole point of an
 * anchor is that it keeps pointing at the same thing.
 */
function slug(text) {
  return text
    .toLowerCase()
    .replace(/[^\w\s-]/g, "")
    .trim()
    .replace(/\s+/g, "-");
}

const escape = (s) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

// A wide table scrolls inside its own box rather than pushing the page
// sideways. Done in the renderer rather than with a CSS rule on the table,
// because the element that scrolls has to be a wrapper around it.
md.renderer.rules.table_open = () => '<div class="table-scroll"><table>';
md.renderer.rules.table_close = () => "</table></div>";

/**
 * Write `<dist>/language/index.html` from `docs/language.md`.
 *
 * Returns what it built, so the build line can say so: a page that quietly
 * stops being generated is one nobody notices is missing.
 */
export async function buildReference(dist) {
  const markdown = await readFile(source, "utf8");
  const tokens = md.parse(markdown, {});

  // Anchors on every heading, and a contents list from the `##` level. 1375 lines
  // of reference without one is a document you scroll rather than read.
  const contents = [];
  const used = new Set();
  for (let i = 0; i < tokens.length; i += 1) {
    const token = tokens[i];
    if (token.type !== "heading_open") continue;
    const text = tokens[i + 1].content;
    let id = slug(text);
    // A duplicate id silently sends every link to the first one.
    for (let n = 2; used.has(id); n += 1) id = `${slug(text)}-${n}`;
    used.add(id);
    token.attrSet("id", id);
    if (token.tag === "h2") contents.push({ id, text });
  }

  const body = md.renderer.render(tokens, md.options, {});

  const title = "The Nomo language";
  const toc = contents
    .map((h) => `      <li><a href="#${h.id}">${escape(h.text)}</a></li>`)
    .join("\n");

  // Self-contained but for the fonts, which are the ones dist already ships —
  // referenced relatively, like everything else on this site, so the page works
  // wherever the directory is unzipped. check-package.mjs is what keeps that true.
  const page = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title} — Nomo</title>
<style>
  @font-face {
    font-family: "STIX Two Text Subset";
    src: url("../fonts/stix-two-text-subset.woff2") format("woff2");
    font-weight: 400 700;
    font-style: normal;
    font-display: swap;
  }
  @font-face {
    font-family: "STIX Two Text Subset";
    src: url("../fonts/stix-two-text-italic-subset.woff2") format("woff2");
    font-weight: 400 700;
    font-style: italic;
    font-display: swap;
  }

  :root { color-scheme: light dark; }
  body {
    font-family: "STIX Two Text Subset", Georgia, serif;
    max-width: 48rem;
    margin: 0 auto;
    padding: 2rem 1.2rem 5rem;
    line-height: 1.6;
  }
  h1 { font-size: 1.9rem; margin-bottom: 0.3rem; }
  h2 { font-size: 1.4rem; margin-top: 2.4rem; }
  h3 { font-size: 1.1rem; margin-top: 1.8rem; }
  h2, h3, h4 { line-height: 1.3; }
  hr { border: 0; border-top: 1px solid rgba(128,128,128,0.35); margin: 2.4rem 0; }
  a { color: inherit; }

  code, pre {
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 0.87em;
  }
  code { background: rgba(128,128,128,0.14); padding: 0.1em 0.32em; border-radius: 3px; }
  pre {
    background: rgba(128,128,128,0.12);
    padding: 0.8rem 1rem;
    border-radius: 5px;
    /* A worksheet line is not wrapped in the editor and should not be here. */
    overflow-x: auto;
  }
  pre code { background: none; padding: 0; }

  table { border-collapse: collapse; width: 100%; font-size: 0.94rem; }
  /* The table scrolls inside this, so a wide one never widens the page. */
  .table-scroll { overflow-x: auto; margin: 1rem 0; }
  th, td {
    border: 1px solid rgba(128,128,128,0.35);
    padding: 0.35rem 0.6rem;
    text-align: left;
    vertical-align: top;
  }
  th { background: rgba(128,128,128,0.12); }

  nav.contents {
    margin: 1.5rem 0 2.5rem;
    padding: 1rem 1.2rem;
    border: 1px solid rgba(128,128,128,0.3);
    border-radius: 6px;
  }
  nav.contents p { margin: 0 0 0.5rem; font-weight: 700; }
  nav.contents ul { margin: 0; padding-left: 1.1rem; columns: 2; column-gap: 2rem; }
  nav.contents li { break-inside: avoid; }
  @media (max-width: 34rem) { nav.contents ul { columns: 1; } }

  header.site { margin-bottom: 1rem; font-size: 0.92rem; }
  header.site a { margin-right: 1rem; }
</style>
</head>
<body>
<header class="site">
  <a href="../">The editor</a>
  <a href="../examples/">Worked examples</a>
</header>

<nav class="contents" aria-label="Contents">
  <p>Contents</p>
  <ul>
${toc}
  </ul>
</nav>

${body}
</body>
</html>
`;

  const out = join(dist, "language");
  await mkdir(out, { recursive: true });
  await writeFile(join(out, "index.html"), page);
  return { sections: contents.length, bytes: page.length };
}
