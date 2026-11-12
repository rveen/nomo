// Check that Ctrl+F in the source finds text the browser's find cannot.
//
// The editor renders only the lines near the viewport, so a line scrolled out
// of view is not in the page and the browser's Ctrl+F does not find it —
// reported from use, on a worksheet longer than the screen. Ctrl+F with the
// focus in the source now opens CodeMirror's own find, which searches the
// document. This checks both halves of that sentence: that a word far down a
// long worksheet really is absent from the page (or the check proves nothing),
// and that the editor's find reaches it, selects it and scrolls it into view.

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
  await readFile(join(dist, "bundle.js"));
} catch {
  console.error("error: web/dist is not built — see scripts/build-web.sh");
  process.exit(1);
}

const server = createServer(async (request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  const name = path === "/" ? "/index.html" : path;
  try {
    const body = await readFile(join(dist, name));
    response.writeHead(200, { "content-type": TYPES[extname(name)] ?? "application/octet-stream" });
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
  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (await browser.evaluate(`(async () => ${expression})()`)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}

// A word that occurs once, near the end of a worksheet far longer than the
// pane. Typed at the start of the buffer, so the view stays at the top.
const NEEDLE = "zanzibar_length";
const WORKSHEET =
  Array.from({ length: 400 }, (_, i) => `a${i} = ${i + 1} m`).join("\n") +
  `\n${NEEDLE} = 3 m\n`;

let browser;
try {
  browser = await launch();
  await browser.goto(url);
  await waitFor(browser, `document.body.dataset.ready === "true"`, "the editor");

  await browser.evaluate(`document.querySelector(".cm-content").focus()`);
  await browser.type(WORKSHEET);
  await waitFor(
    browser,
    `document.querySelector("#output").textContent.includes(${JSON.stringify(NEEDLE)})`,
    "the worksheet to compute",
  );
  // Back to the top, cursor and all: the editor keeps the cursor's line in the
  // page wherever it is, so scrolling alone would leave the needle there.
  await browser.key("Home", { ctrl: true, code: "Home", keyCode: 36 });
  await waitFor(
    browser,
    `!document.querySelector(".cm-content").textContent.includes(${JSON.stringify(NEEDLE)})`,
    "the far line to leave the page",
  );

  // The premise: the browser's find has nothing to find in the source.
  check(
    !(await browser.evaluate(
      `document.querySelector(".cm-content").textContent.includes(${JSON.stringify(NEEDLE)})`,
    )),
    "the far line is in the page after all, so this check proves nothing about finding it",
  );

  await browser.evaluate(`document.querySelector(".cm-content").focus()`);
  await browser.key("f", { ctrl: true, code: "KeyF", keyCode: 70 });
  await waitFor(browser, `document.querySelector(".cm-search input[name=search]") !== null`, "the find panel");
  check(
    await browser.evaluate(
      `document.activeElement === document.querySelector(".cm-search input[name=search]")`,
    ),
    "Ctrl+F should put the cursor in the find field",
  );

  await browser.type(NEEDLE);
  // The panel takes its query from the field on keyup, which a person's typing
  // produces and `insertText` does not; End moves the caret within the field
  // and gives it one.
  await browser.key("End", { code: "End", keyCode: 35 });
  await browser.key("Enter", { code: "Enter", keyCode: 13 });
  await waitFor(
    browser,
    `document.querySelector(".cm-searchMatch-selected")?.textContent === ${JSON.stringify(NEEDLE)}`,
    "the match to be selected",
  );

  // Selected and scrolled to: the line is in the page now, and in view.
  const seen = await browser.evaluate(`(() => {
    const match = document.querySelector(".cm-searchMatch-selected").getBoundingClientRect();
    const pane = document.querySelector(".cm-scroller").getBoundingClientRect();
    return { top: match.top, bottom: match.bottom, paneTop: pane.top, paneBottom: pane.bottom };
  })()`);
  check(
    seen.top >= seen.paneTop && seen.bottom <= seen.paneBottom,
    `the match was selected but not scrolled into view: ${JSON.stringify(seen)}`,
  );
} catch (error) {
  failures.push(error.message);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error(`check-find: ${failures.length} problem(s):`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
console.log(
  "ok: Ctrl+F in the source finds a line far outside the viewport, which the browser's find cannot",
);
