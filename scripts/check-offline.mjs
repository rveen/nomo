// Check that work survives a reload, and that the application survives the
// network going away.
//
// These are the two claims phase 9 makes, and neither is checkable from Rust or
// from a single page load. Both are checked against the built application in a
// real browser, over the DevTools protocol.
//
// The offline check is the one that matters most and is easiest to get wrong.
// A page can look like it works offline because everything it needs is still in
// the HTTP cache; that is not offline support, it is luck with a short shelf
// life. Chrome's network emulation cuts the page off properly, so what is being
// checked is the service worker.

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
};

try {
  await readFile(join(dist, "sw.js"));
} catch {
  console.error("error: web/dist is not built — see scripts/build-web.sh");
  process.exit(1);
}

let requests = 0;
const server = createServer(async (request, response) => {
  requests += 1;
  const path = new URL(request.url, "http://localhost").pathname;
  const name = path === "/" ? "/index.html" : path;
  try {
    const body = await readFile(join(dist, name));
    response.writeHead(200, {
      "content-type": TYPES[extname(name)] ?? "application/octet-stream",
      // Defeat the HTTP cache, so that anything still working with the network
      // off is working because of the service worker and not by accident.
      "cache-control": "no-store, no-cache, must-revalidate",
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

  // ---- the draft survives a reload -------------------------------------

  await browser.goto(url);
  await ready(browser);

  const before = await docText(browser);
  check(
    before.includes("Cylinder volume"),
    "a fresh browser profile should start on the example worksheet",
  );

  // Type at the end of the document, the way a user would.
  await browser.evaluate(`document.querySelector(".cm-content").focus()`);
  await browser.evaluate(`
    (() => {
      const selection = window.getSelection();
      selection.selectAllChildren(document.querySelector(".cm-content"));
      selection.collapseToEnd();
    })()
  `);
  await browser.type("\nwidth = 42 mm\n");

  check(
    (await docText(browser)).includes("width = 42 mm"),
    "typing did not reach the editor; the rest of this check would prove nothing",
  );
  // Wait on the *output*, not the editor. The text lands immediately and the
  // analysis runs on a debounce behind it, so checking the editor here would
  // race the engine and fail intermittently.
  await waitFor(
    browser,
    `document.querySelector("#output").textContent.includes("width=42 mm")`,
    "the new line to evaluate",
  );

  // The draft is written on a debounce; wait for it rather than for a duration.
  await waitFor(browser, `(await nomoDraft()) !== null`, "the draft to be written");

  await browser.reload();
  await ready(browser);

  check(
    (await docText(browser)).includes("width = 42 mm"),
    "the worksheet was not restored after a reload; unsaved work is being lost",
  );

  // ---- the application works offline -----------------------------------

  await waitFor(
    browser,
    `!!navigator.serviceWorker.controller`,
    "the service worker to take control",
  );

  const requestsBefore = requests;
  await browser.setOffline(true);

  await browser.reload();
  await ready(browser);

  check(
    (await outputText(browser)).includes("0.942478 dm³"),
    "the worksheet did not evaluate with the network off; the engine did not load",
  );
  check(
    (await docText(browser)).includes("width = 42 mm"),
    "the draft was not restored with the network off",
  );
  check(
    requests === requestsBefore,
    `the page reached the server ${requests - requestsBefore} time(s) while offline, ` +
      "so this did not test what it claims to",
  );

  await browser.setOffline(false);
} catch (error) {
  failures.push(`could not drive the browser: ${error.message}`);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error("check-offline: persistence or offline support is broken\n");
  for (const failure of failures) console.error(`  error: ${failure}`);
  process.exit(1);
}

console.log("ok: work survives a reload, and the application runs offline");

/** Wait for the editor to have started. */
function ready(browser) {
  return waitFor(browser, `document.body.dataset.ready === "true"`, "the editor");
}

function docText(browser) {
  return browser.evaluate(
    `document.querySelector(".cm-content")?.textContent ?? ""`,
  );
}

function outputText(browser) {
  return browser.evaluate(`document.querySelector("#output")?.textContent ?? ""`);
}

/**
 * Poll an expression in the page until it is true.
 *
 * Polling rather than sleeping: a fixed wait is either too short on a loaded
 * machine, which makes a check flaky, or too long everywhere, which makes it
 * slow. `nomoDraft` is defined here rather than in the application, so nothing
 * is added to the shipped code for the benefit of a test.
 */
async function waitFor(browser, expression, what) {
  const helper = `
    globalThis.nomoDraft = () => new Promise((resolve) => {
      const request = indexedDB.open("nomo", 1);
      request.onsuccess = () => {
        const db = request.result;
        if (!db.objectStoreNames.contains("drafts")) { db.close(); return resolve(null); }
        const get = db.transaction("drafts", "readonly").objectStore("drafts").get("current");
        get.onsuccess = () => { db.close(); resolve(get.result ?? null); };
        get.onerror = () => { db.close(); resolve(null); };
      };
      request.onerror = () => resolve(null);
    });
  `;
  await browser.evaluate(helper);

  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (await browser.evaluate(`(async () => ${expression})()`)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}
