// Check the editor's Pages view and its printing.
//
// The Pages view puts the paged document `nomo_render_paged` writes into a
// frame, where Folio lays it out. Everything that makes that worth having
// happens in a browser — whether Folio loads at all from where the editor
// serves it, whether the shipped math font passes Folio's MATH-table probe,
// whether the head and foot are on the pages, whether Print waits for them —
// so this drives Chrome over the DevTools protocol, as the other checks do.
//
// The browser's print dialogue cannot be driven headless, and a call to
// `print()` that opened one would hang the check. So `print` is replaced,
// before any page's own scripts run, by a counter in the editor's window. That
// tests the thing the editor is responsible for — that Print and Ctrl+P reach
// the frame's `print()`, once, and only after the pages exist — and leaves
// what the paper looks like to `check-paged.mjs`, which prints a PDF.

import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, join } from "node:path";
import { launch } from "./chrome.mjs";
import { repoRoot } from "./wasm-host.mjs";

const dist = join(repoRoot, "web/dist");
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".woff2": "font/woff2",
};

try {
  await readFile(join(dist, "folio/folio.polyfill.min.js"));
} catch {
  console.error("error: web/dist has no Folio — see scripts/build-web.sh");
  process.exit(1);
}

/** Every path the editor asked the server for, in order. */
const requested = [];
const server = createServer(async (request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  requested.push(path);
  const name = path === "/" ? "/index.html" : path;
  try {
    const body = await readFile(join(dist, name));
    response.writeHead(200, {
      "content-type": TYPES[extname(name)] ?? "application/octet-stream",
    });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const url = `http://127.0.0.1:${server.address().port}/`;

const failures = [];
const check = (condition, description) => {
  if (!condition) failures.push(description);
};

async function waitFor(browser, expression, what) {
  for (let attempt = 0; attempt < 400; attempt += 1) {
    if (await browser.evaluate(`(async () => ${expression})()`)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}

/** The frame's laid-out document, read from the editor's window. */
const PAGES = `(() => {
  const doc = document.querySelector("#pages-frame").contentDocument;
  const root = doc && doc.documentElement;
  if (!root || !root.hasAttribute("data-nomo-pages")) return null;
  const pages = [...doc.querySelectorAll(".pagedjs_page")];
  const box = (page, name) =>
    (page.querySelector(".pagedjs_margin-" + name) || {}).textContent || "";
  return {
    count: Number(root.getAttribute("data-nomo-pages")),
    overflowed: Number(root.getAttribute("data-nomo-overflowed")),
    built: pages.length,
    steps: doc.querySelectorAll(".pagedjs_page .step").length,
    math: doc.querySelectorAll(".pagedjs_page math").length,
    heads: pages.map((p) => box(p, "top-left")),
    tops: pages.map((p) => box(p, "top-right")),
    files: pages.map((p) => box(p, "bottom-left")),
    // The family the prose is set in, and whether that face actually loaded:
    // a stack that names a face nobody declared reports the same family
    // string as one that resolves to it.
    prose: getComputedStyle(doc.querySelector(".pagedjs_page p.prose")).fontFamily,
    textLoaded: [...doc.fonts].some(
      (f) => f.family === "STIX Two Text Subset" && f.status === "loaded"),
    feet: pages.map((p) => box(p, "bottom-right")),
  };
})()`;

// Enough worksheet to need more than one page, so the running head — which
// the first page leaves off — is somewhere to be checked.
const LONG = Array.from(
  { length: 70 },
  (_, i) => `' Step ${i + 1}, with a sentence of prose above it.\nq${i} = ${i + 1} m * 2\n`,
).join("");

let browser;
try {
  browser = await launch();
  // Before the editor's scripts and before the frame's: a print dialogue
  // would hang a headless browser, and this is what is being asserted anyway.
  await browser.onNewDocument(
    "window.print = () => { window.top.__printed = (window.top.__printed || 0) + 1; };",
  );
  await browser.goto(url);
  await waitFor(browser, `document.body.dataset.ready === "true"`, "the editor");

  check(
    !requested.some((p) => p.startsWith("/folio/")),
    "Folio was fetched before anybody asked for pages; it should cost nothing until then",
  );

  // ---- in the service worker's first moments --------------------------
  // Pages asked for at once, while the worker this first visit installed is
  // still taking over. The frame used to fetch Folio and the fonts through
  // the worker, and here those requests stalled: Folio never arrived and the
  // pages never finished, or the math font did not and Folio's probe refused
  // the fallback face as having no MATH table. Reported as "reloading fixes
  // it". The frame now requests nothing; the editor hands it blob: URLs.
  // Typeset and Pages together, as it was reported: typeset pages are the
  // ones that set formulas in the math font in the document itself.
  await browser.evaluate(`document.querySelector("#typeset").click()`);
  await browser.evaluate(`document.querySelector("#pages").click()`);
  await waitFor(
    browser,
    `(() => {
      const s = document.querySelector("#status").textContent;
      if (/^pages:|could not lay out/.test(s)) throw new Error(s);
      const p = ${PAGES};
      return p !== null && p.math > 0;
    })()`,
    "typeset pages laid out in the service worker's first moments",
  );
  // Resource timing does not record blob: loads, so what is asserted is that
  // nothing else was requested — with the pages laid out, which needed
  // Folio's script, and the math font loaded, which is what the probe reads.
  const frame = await browser.evaluate(`(() => {
    const w = document.querySelector("#pages-frame").contentWindow;
    return {
      requested: w.performance.getEntriesByType("resource").map((e) => e.name)
        .filter((name) => !name.startsWith("blob:")),
      math: [...w.document.fonts].some(
        (f) => f.family === "STIX Two Math Subset" && f.status === "loaded"),
    };
  })()`);
  check(
    frame.requested.length === 0,
    `the frame should request nothing but blob: URLs: ${JSON.stringify(frame.requested)}`,
  );
  check(frame.math, "the math font did not load in the pages laid out first");
  // Back to where the rest of this check starts: results, not typeset.
  await browser.evaluate(`document.querySelector("#pages").click()`);
  await browser.evaluate(`document.querySelector("#typeset").click()`);

  await browser.type(LONG);
  await waitFor(
    browser,
    `document.querySelectorAll("#output .step").length > 70`,
    "the long worksheet to compute",
  );
  const steps = await browser.evaluate(`document.querySelectorAll("#output .step").length`);

  // ---- the view -------------------------------------------------------
  await browser.evaluate(`document.querySelector("#pages").click()`);
  await waitFor(browser, `${PAGES} !== null`, "the pages to be laid out");
  let laid = await browser.evaluate(PAGES);

  check(
    requested.includes("/folio/folio.polyfill.min.js"),
    "the frame did not fetch Folio from folio/",
  );
  check(laid.count >= 2, `the long worksheet should take two pages or more, took ${laid.count}`);
  check(laid.built === laid.count, `Folio said ${laid.count} pages and built ${laid.built}`);
  check(laid.overflowed === 0, `${laid.overflowed} pages overflowed`);
  check(
    laid.steps === steps,
    `every worked line should be on the pages exactly once: ${laid.steps} on the pages, ${steps} in the results`,
  );
  check(laid.heads[0] === "", "the first page should not carry the running head");
  check(
    laid.heads.slice(1).every((h) => h === "untitled"),
    `every page after the first should be headed with the worksheet's title: ${JSON.stringify(laid.heads)}`,
  );
  check(
    laid.tops.every((t) => t === ""),
    `nothing belongs at the head's right: ${JSON.stringify(laid.tops)}`,
  );
  check(
    laid.files.every((f) => f === "untitled.nomo"),
    `every page's foot should name the file, extension included: ${JSON.stringify(laid.files)}`,
  );
  check(
    laid.feet.every((f, i) => f === `Page ${i + 1} of ${laid.count}`),
    `every page should say which it is: ${JSON.stringify(laid.feet)}`,
  );
  check(
    (await browser.evaluate(`document.querySelector("#output").hidden`)) === true,
    "the results should give way to the pages",
  );
  // The frame cannot see the editor's @font-face, so the pages carry their
  // own; before they did, the prose here was set in Georgia.
  check(
    laid.prose.startsWith('"STIX Two Text Subset"'),
    `the pages' prose should be set in the shipped face first: ${laid.prose}`,
  );
  check(laid.textLoaded, "the shipped text face did not load in the pages");
  // An A4 page is wider than half a screen. The editor scales the pages to the
  // pane, so none is cut at its right edge.
  const fit = await browser.evaluate(`(() => {
    const frame = document.querySelector("#pages-frame");
    const page = frame.contentDocument.querySelector(".pagedjs_page");
    return { right: page.getBoundingClientRect().right, width: frame.clientWidth };
  })()`);
  check(
    fit.right <= fit.width,
    `a page runs past the pane: its right edge at ${fit.right}px, the pane ${fit.width}px wide`,
  );
  // White around the pages: the paged document's own grey ground showed as a
  // band above the first page and below the last.
  const ground = await browser.evaluate(
    `getComputedStyle(document.querySelector("#pages-frame").contentDocument.documentElement).backgroundColor`,
  );
  check(ground === "rgb(255, 255, 255)", `the ground around the pages should be white, is ${ground}`);

  // ---- typeset pages --------------------------------------------------
  // The case Folio's MATH-table probe exists for: the editor names the font it
  // ships, and Folio refuses to lay out with one it cannot prove. A subset that
  // had lost its MATH table would fail here and nowhere else.
  await browser.evaluate(`document.querySelector("#typeset").click()`);
  await waitFor(
    browser,
    `(() => { const p = ${PAGES}; return p !== null && p.math > 0; })()`,
    "the typeset pages",
  );
  laid = await browser.evaluate(PAGES);
  check(laid.overflowed === 0, `${laid.overflowed} typeset pages overflowed`);
  check(
    !/^pages:|could not lay out/.test(
      await browser.evaluate(`document.querySelector("#status").textContent`),
    ),
    "the status line reports a failure laying out typeset pages",
  );

  // ---- printing -------------------------------------------------------
  await browser.evaluate(`document.querySelector("#print").click()`);
  await waitFor(browser, `window.__printed === 1`, "Print to reach the frame");
  // Ctrl+P twice: with the focus in the editor, and with it in the pages,
  // which is a document of its own whose keys never reach the editor's window.
  await browser.evaluate(`document.querySelector(".cm-content").focus()`);
  await browser.key("p", { ctrl: true, code: "KeyP", keyCode: 80 });
  await waitFor(browser, `window.__printed === 2`, "Ctrl+P in the editor to print the pages");
  await browser.evaluate(`document.querySelector("#pages-frame").contentWindow.focus()`);
  await browser.key("p", { ctrl: true, code: "KeyP", keyCode: 80 });
  await waitFor(browser, `window.__printed === 3`, "Ctrl+P in the pages to print them");

  // The browser's own menu, in the Pages view, still prints the results:
  // the frame would print as one clipped screenful.
  await browser.setPrintMedia(true);
  const media = await browser.evaluate(`({
    output: getComputedStyle(document.querySelector("#output")).display,
    frame: getComputedStyle(document.querySelector("#pages-frame")).display,
  })`);
  check(media.output !== "none", "printed from the menu, the results should print");
  check(media.frame === "none", "printed from the menu, the frame should not");
  await browser.setPrintMedia(false);

  // ---- back -----------------------------------------------------------
  await browser.evaluate(`document.querySelector("#pages").click()`);
  check(
    (await browser.evaluate(`document.querySelector("#output").hidden`)) === false &&
      (await browser.evaluate(`document.querySelector("#pages-frame").hidden`)) === true,
    "leaving the Pages view should bring the results back",
  );

  // Print from the results view goes through the pages too, and says so.
  await browser.evaluate(`document.querySelector("#print").click()`);
  await waitFor(browser, `window.__printed === 4`, "Print from the results view");
  check(
    (await browser.evaluate(`document.querySelector("#pages").getAttribute("aria-pressed")`)) ===
      "true",
    "Print should show the pages it prints",
  );
} catch (error) {
  failures.push(error.message);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error(`check-pages: ${failures.length} problem(s):`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
console.log(
  "ok: the Pages view lays the worksheet out with Folio, typeset or not, " +
    "and Print and Ctrl+P print the frame once the pages exist",
);
