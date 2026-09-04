// Check that the editor survives an engine that fails mid-edit.
//
// The failure this exists for was permanent and quiet. A worksheet nested deep
// enough to overflow the module's 1 MB stack trapped it, and a trapped
// WebAssembly instance stays broken: the trap unwinds out of Rust without
// running any of it, so the allocator is left mid-update and every later call
// reads memory that no longer describes itself. The editor reported `engine
// error` once and then went on looking like it was working while recalculating
// nothing — for the life of the tab.
//
// The parser's nesting limit closed the one route a worksheet had to that, so
// nothing a user can type reaches it any more. Which leaves the recovery itself
// untestable by ordinary means: there is no worksheet that breaks the engine.
// So this stands in for one. `Page.addScriptToEvaluateOnNewDocument` — the same
// mechanism `check-files.mjs` uses to give headless Chrome a File System Access
// API it does not have — wraps the first instance so that one `update` call
// throws, exactly as a trap would, and then asserts the editor comes back.
//
// What it proves: the front end replaces a failed instance rather than carrying
// on with it, the replacement re-reads the buffer on screen, and nothing typed
// is lost. What it does not prove: that a real trap looks like a thrown
// `RuntimeError` at this boundary. It does — every WebAssembly trap surfaces to
// JavaScript that way — but this check takes that from the specification rather
// than from a crash it caused.

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
      "cache-control": "no-store",
    });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const url = `http://127.0.0.1:${server.address().port}/`;

// Fail the second `nomo_document_update` of the first instance, and only that
// one. The second instance — the replacement the front end builds — is left
// alone, because a recovery that needed a patched engine would prove nothing.
const SABOTAGE = `
(() => {
  const RealInstance = WebAssembly.Instance;
  let built = 0;
  window.__nomoInstances = () => built;
  window.__nomoFailed = false;
  WebAssembly.Instance = function (module, imports) {
    const real = new RealInstance(module, imports);
    built += 1;
    if (built > 1) return real;
    const exports = { ...real.exports };
    // Both entry points, because the editor picks one and this check must not
    // quietly stop failing when it picks the other. It did exactly that when
    // the typeset-aware update arrived: the patched function was no longer
    // called, the simulated trap never fired, and the check timed out — which
    // is the right failure, and the reason to patch both now.
    let calls = 0;
    for (const name of ["nomo_document_update", "nomo_document_update_as"]) {
      const real = exports[name];
      if (!real) continue;
      exports[name] = (...args) => {
        calls += 1;
        if (calls === 2) {
          window.__nomoFailed = true;
          throw new WebAssembly.RuntimeError("simulated trap");
        }
        return real(...args);
      };
    }
    return Object.create(RealInstance.prototype, { exports: { value: exports } });
  };
})();
`;

const failures = [];
let browser;

try {
  browser = await launch();
  await browser.onNewDocument(SABOTAGE);
  await browser.goto(url);
  await waitFor(browser, `document.body.dataset.ready === "true"`, "the editor");

  // Type at the start of the starting worksheet. The engine fails on this edit.
  await browser.type("q = 2 m + 3 m\n");

  await waitFor(
    browser,
    `window.__nomoFailed === true`,
    "the engine to fail on an edit",
  );
  await waitFor(
    browser,
    `document.querySelector("#output").textContent.includes("5 m")`,
    "the editor to recover and recalculate",
  );

  const instances = await browser.evaluate(`window.__nomoInstances()`);
  if (instances !== 2) {
    failures.push(
      `the front end should have built exactly one replacement instance, it built ${instances - 1}`,
    );
  }

  const output = await browser.evaluate(
    `document.querySelector("#output").textContent`,
  );
  if (!output.includes("0.942478")) {
    failures.push(
      "the worksheet that was already on screen stopped computing after the recovery",
    );
  }

  const text = await browser.evaluate(
    `document.querySelector(".cm-content").textContent`,
  );
  if (!text.startsWith("q = 2 m + 3 m")) {
    failures.push(`the buffer lost what was typed: ${JSON.stringify(text.slice(0, 40))}`);
  }

  const statusClass = await browser.evaluate(
    `document.querySelector("#status").className`,
  );
  if (statusClass !== "good") {
    const statusText = await browser.evaluate(
      `document.querySelector("#status").textContent`,
    );
    failures.push(
      `after recovering, a valid worksheet should read as ok; the status line said "${statusText}"`,
    );
  }

  // And the recovered editor still edits: the session behind it is a live one,
  // not a corpse that happened to render once.
  await browser.type("w = 10 m\n");
  await waitFor(
    browser,
    `document.querySelector("#output").textContent.includes("10 m")`,
    "an edit after the recovery",
  );
} catch (error) {
  failures.push(`could not drive the browser: ${error.message}`);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error("check-recovery: the editor did not survive an engine failure\n");
  for (const failure of failures) console.error(`  error: ${failure}\n`);
  process.exit(1);
}

console.log("ok: the editor replaces a failed engine and keeps the work");

async function waitFor(browser, expression, what) {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (await browser.evaluate(`(async () => ${expression})()`)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}
