// Check that the page prints as a worksheet rather than as an editor.
//
// Printing is a hard requirement, not a finishing touch: engineering worksheets
// get printed, signed and filed, and EngineeringPaper.xyz treats browser
// printing the same way. The plan says to write the print styles from the start
// because retrofitting them means rebuilding the layout around them — so there
// should be a check, or "from the start" is only an intention.
//
// Asks Chrome to emulate print media and then asks the page what is actually
// visible. Reading `getComputedStyle` under emulation is the closest thing to
// looking at the paper that does not involve looking at paper.

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
  // The math font the typeset columns are laid out with.
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

let browser;
try {
  browser = await launch();
  await browser.goto(url);

  // Wait for the engine to finish, so the output pane has something in it.
  await browser.evaluate(`
    new Promise((resolve) => {
      const done = () => document.querySelector("#output").children.length > 0;
      if (done()) return resolve(true);
      const timer = setInterval(() => { if (done()) { clearInterval(timer); resolve(true); } }, 50);
      setTimeout(() => { clearInterval(timer); resolve(false); }, 8000);
    })
  `);

  const visible = (selector) =>
    browser.evaluate(`
      (() => {
        const el = document.querySelector(${JSON.stringify(selector)});
        if (!el) return null;
        const style = getComputedStyle(el);
        return style.display !== "none" && style.visibility !== "hidden";
      })()
    `);

  // On screen, everything is there.
  check(await visible("header"), "the header should be visible on screen");
  check(await visible("#pane-editor"), "the editor should be visible on screen");
  check(await visible("#output"), "the output should be visible on screen");

  await browser.setPrintMedia(true);

  // On paper, only the worksheet.
  check(
    (await visible("header")) === false,
    "the header is still printed; it is application chrome, not the document",
  );
  check(
    (await visible("footer")) === false,
    "the footer is still printed",
  );
  check(
    (await visible("#pane-editor")) === false,
    "the editor is still printed; the source is not the deliverable",
  );
  check(await visible("#output"), "the worksheet itself must still print");

  // Long expressions have to wrap. A scrollbar is not a thing paper has, and a
  // clipped calculation is worse than no calculation.
  const wrapping = await browser.evaluate(`
    (() => {
      const step = document.querySelector("#output .step");
      if (!step) return null;
      const style = getComputedStyle(step);
      return { overflowX: style.overflowX, whiteSpace: style.whiteSpace };
    })()
  `);
  check(wrapping !== null, "the printed output has no worked steps in it");
  check(
    wrapping?.overflowX === "visible",
    `worked steps must not scroll on paper, got overflow-x: ${wrapping?.overflowX}`,
  );
  check(
    wrapping?.whiteSpace === "pre-wrap",
    `worked steps must wrap on paper, got white-space: ${wrapping?.whiteSpace}`,
  );

  // The output pane must not be a scrolling box on paper either, or only the
  // first screenful reaches the page.
  const pane = await browser.evaluate(
    `getComputedStyle(document.querySelector("#pane-output")).overflow`,
  );
  check(
    pane === "visible",
    `the results pane must not clip on paper, got overflow: ${pane}`,
  );
} catch (error) {
  failures.push(`could not drive the browser: ${error.message}`);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error("check-print: the page does not print as a worksheet\n");
  for (const failure of failures) console.error(`  error: ${failure}`);
  process.exit(1);
}

console.log("ok: printing gives the worksheet without the editor");
