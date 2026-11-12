// Check the paged document `nomo html --paged` writes, in a real browser.
//
// Everything up to here holds the document's *bytes*: the unit tests hold what
// is in it, compare-html.mjs holds the CLI and the module to the same bytes.
// Whether Folio turns those bytes into pages is a question only a browser
// answers, and the questions are these, for every worksheet in examples/:
//
//   * Folio finishes: the document marks its root `data-nomo-pages`, and
//     builds that many pages, none of them overflowed.
//   * Nothing is lost, doubled or split. The worked lines on the pages, in
//     order, are the worked lines of the document, compared by their text —
//     which a line split across two pages would fail, as two half-lines.
//   * The page furniture is there: "Nomo" heading every page but the first,
//     and "Page n of N" at the foot of each.
//   * Printed, every line is visible and no page is still marked as being
//     laid out — the guard that blanks an early print must not blank a late
//     one, which its first version did.
//   * The PDF has one A4 sheet per page. A blank sheet in front of page 1, or
//     the browser's own margin boxes around Folio's, would both show here.
//
// Twice over: the document as it stands, which names a math font and fetches
// none, and typeset with the shipped math font embedded — which also puts
// Folio's MATH-table probe in front of the font this project ships.
//
// And one thing no finished document can show: that a print taken *before*
// Folio has finished is blank rather than an unpaginated page that looks
// finished. Folio waits for fonts before it measures anything, so for that
// case the math font is served by a route that holds it back until the check
// has looked: the body is parsed, Folio is running, and nothing is laid out.
// (Holding back Folio itself does not make that window. A classic script in
// the head stops the parser, so there is no body yet to print — which is how
// the first version of this check timed out.)
//
// One PDF per worksheet is left in target/paged/ for a person to look at.

import { execFile } from "node:child_process";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { basename, extname, join } from "node:path";
import { promisify } from "node:util";
import { launch } from "./chrome.mjs";
import { repoRoot } from "./wasm-host.mjs";

const run = promisify(execFile);

const examples = join(repoRoot, "examples");
const font = join(repoRoot, "web/dist/fonts/stix-two-math-subset.woff2");
const folio = join(repoRoot, "web/vendor/folio/folio.polyfill.min.js");
const pdfDir = join(repoRoot, "target/paged");

for (const [file, hint] of [
  [folio, "./scripts/fetch-folio.sh"],
  [font, "./scripts/build-web.sh"],
]) {
  try {
    await readFile(file);
  } catch {
    console.error(`error: no ${basename(file)} — run ${hint}`);
    process.exit(1);
  }
}

/** A4 in PostScript points, and how far Chrome's rounding is let drift. */
const A4 = [595.28, 841.89];
const SHEET_TOLERANCE = 1;

const work = await mkdtemp(join(tmpdir(), "nomo-paged-"));
const names = (await readdir(examples)).filter((n) => n.endsWith(".nomo")).sort();

// Each form is a directory of documents rendered by the CLI itself, so what is
// checked is what `nomo html --paged` writes. All worksheets in one run per
// form; a worksheet with errors exits non-zero and still writes its page.
const forms = [
  { name: "plain", flags: [] },
  { name: "typeset", flags: ["--mathml", "--embed-font", font] },
];
for (const form of forms) {
  const dir = join(work, form.name);
  await mkdir(dir);
  for (const name of names) {
    await writeFile(join(dir, name), await readFile(join(examples, name)));
  }
  await run(
    "cargo",
    ["run", "--quiet", "-p", "nomo-cli", "--", "html", "--paged", ...form.flags,
     ...names.map((n) => join(dir, n))],
    { cwd: repoRoot, maxBuffer: 1 << 26 },
  ).catch(() => {});
}

// The early-print case: a typeset document whose math font comes from a
// route this server holds open until told to answer.
const early = join(work, "early");
await mkdir(early);
await writeFile(join(early, "beam.nomo"), await readFile(join(examples, "beam.nomo")));
await run(
  "cargo",
  ["run", "--quiet", "-p", "nomo-cli", "--", "html", "--paged", "--mathml",
   "--font-url", "/held/math.woff2", join(early, "beam.nomo")],
  { cwd: repoRoot },
).catch(() => {});
let release;
const held = new Promise((resolve) => (release = resolve));

const server = createServer(async (request, response) => {
  const path = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
  if (path === "/held/math.woff2") {
    await held;
    response.writeHead(200, { "content-type": "font/woff2" });
    response.end(await readFile(font));
    return;
  }
  try {
    const body = await readFile(join(work, path));
    response.writeHead(200, {
      "content-type": extname(path) === ".html" ? "text/html; charset=utf-8" : "application/octet-stream",
    });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;

const failures = [];
const check = (condition, description) => {
  if (!condition) failures.push(description);
};

async function waitFor(browser, expression, what) {
  for (let attempt = 0; attempt < 600; attempt += 1) {
    if (await browser.evaluate(`(async () => ${expression})()`)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}

/**
 * What the laid-out document says about itself.
 *
 * The worked lines it should have are read from the document as served,
 * parsed without running it, rather than from the live page: Folio has moved
 * the live body's content into pages by now, and the point is to compare the
 * two.
 */
const INSPECT = `(async () => {
  const root = document.documentElement;
  const source = new DOMParser().parseFromString(
    await (await fetch(location.href)).text(), "text/html");
  const text = (el) => el.textContent.replace(/\\s+/g, " ").trim();
  const pages = [...document.querySelectorAll(".pagedjs_page")];
  const box = (page, name) =>
    text(page.querySelector(".pagedjs_margin-" + name) || document.createElement("i"));
  return {
    count: Number(root.getAttribute("data-nomo-pages")),
    overflowed: Number(root.getAttribute("data-nomo-overflowed")),
    built: pages.length,
    expected: [...source.querySelectorAll("body .step")].map(text),
    found: [...document.querySelectorAll(".pagedjs_page .step")].map(text),
    heads: pages.map((p) => box(p, "top-left")),
    feet: pages.map((p) => box(p, "bottom-right")),
  };
})()`;

const PRINTED = `(() => ({
  hidden: [...document.querySelectorAll(".pagedjs_page .step")]
    .filter((s) => getComputedStyle(s).visibility !== "visible").length,
  marked: document.querySelectorAll("[data-nomo-paging]").length,
}))()`;

/** The sheets of a PDF, as Chrome writes it: one uncompressed MediaBox each. */
function sheets(pdf) {
  const text = pdf.toString("latin1");
  return [...text.matchAll(/\/MediaBox \[([\d.\s-]+)\]/g)].map((m) => {
    const [x0, y0, x1, y1] = m[1].trim().split(/\s+/).map(Number);
    return [x1 - x0, y1 - y0];
  });
}

let browser;
let checked = 0;
try {
  browser = await launch();
  await mkdir(pdfDir, { recursive: true });

  for (const form of forms) {
    for (const name of names) {
      const doc = name.replace(/\.nomo$/, ".html");
      const where = `${doc} (${form.name})`;
      try {
        await browser.setPrintMedia(false);
        await browser.goto(`${origin}/${form.name}/${doc}`);
        await waitFor(
          browser,
          `document.documentElement.hasAttribute("data-nomo-pages")`,
          `${where} to be laid out`,
        );
        const got = await browser.evaluate(INSPECT);

        check(got.count > 0, `${where}: no pages`);
        check(got.built === got.count, `${where}: Folio said ${got.count} pages and built ${got.built}`);
        check(got.overflowed === 0, `${where}: ${got.overflowed} page(s) overflowed`);
        check(got.expected.length > 0, `${where}: the document has no worked lines to compare`);
        const same =
          got.found.length === got.expected.length &&
          got.found.every((line, i) => line === got.expected[i]);
        check(
          same,
          `${where}: the worked lines on the pages are not the document's, in order, once each ` +
            `(${got.found.length} on the pages, ${got.expected.length} in the document)`,
        );
        check(got.heads[0] === "", `${where}: the first page carries the running head`);
        check(
          got.heads.slice(1).every((h) => h === "Nomo"),
          `${where}: a page after the first is not headed "Nomo": ${JSON.stringify(got.heads)}`,
        );
        check(
          got.feet.every((f, i) => f === `Page ${i + 1} of ${got.count}`),
          `${where}: the feet do not count the pages: ${JSON.stringify(got.feet)}`,
        );

        await browser.setPrintMedia(true);
        const printed = await browser.evaluate(PRINTED);
        check(printed.hidden === 0, `${where}: ${printed.hidden} worked line(s) hidden in print`);
        check(printed.marked === 0, `${where}: still marked as being laid out after Folio finished`);

        const pdf = await browser.pdf();
        const paper = sheets(pdf);
        check(
          paper.length === got.count,
          `${where}: ${got.count} page(s) printed on ${paper.length} sheet(s)`,
        );
        check(
          paper.every(([w, h]) =>
            Math.abs(w - A4[0]) <= SHEET_TOLERANCE && Math.abs(h - A4[1]) <= SHEET_TOLERANCE),
          `${where}: a sheet is not A4: ${JSON.stringify(paper)}`,
        );
        if (form.name === "typeset") await writeFile(join(pdfDir, doc.replace(/\.html$/, ".pdf")), pdf);
        checked += 1;
      } catch (error) {
        failures.push(`${where}: ${error.message}`);
      }
    }
  }

  // ---- printed before Folio has finished --------------------------------
  // `goto` waits for the load event, which a held font would block; so the
  // navigation is started and the check polls for the parsed document.
  await browser.setPrintMedia(false);
  void browser.goto(`${origin}/early/beam.html`).catch(() => {});
  await waitFor(
    browser,
    `location.pathname === "/early/beam.html" && document.querySelector(".step") !== null`,
    "the held document to parse",
  );
  await browser.setPrintMedia(true);
  const before = await browser.evaluate(`({
    paging: document.documentElement.hasAttribute("data-nomo-paging"),
    body: getComputedStyle(document.body).visibility,
  })`);
  check(before.paging, "early print: the root is not marked while Folio is still to come");
  check(
    before.body === "hidden",
    `early print: the body is ${before.body}; an unpaginated page would print as though finished`,
  );
  release();
  await waitFor(
    browser,
    `document.documentElement.hasAttribute("data-nomo-pages")`,
    "the held document to finish once Folio arrives",
  );
  const after = await browser.evaluate(PRINTED);
  check(
    after.hidden === 0 && after.marked === 0,
    `early print: once laid out, ${after.hidden} line(s) still hidden and ${after.marked} element(s) still marked`,
  );
} catch (error) {
  failures.push(error.message);
} finally {
  release?.();
  await browser?.close();
  server.close();
  await rm(work, { recursive: true, force: true });
}

if (failures.length > 0) {
  console.error(`check-paged: ${failures.length} problem(s):`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
console.log(
  `ok: ${checked} paged documents lay out, print one A4 sheet per page with every line ` +
    `once, and an early print is blank — PDFs in target/paged/`,
);
