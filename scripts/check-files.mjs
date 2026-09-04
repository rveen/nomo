// Check opening and saving worksheets, on both sides of the browser split.
//
// The File System Access API gives a handle, so Save writes back to the file
// that was opened. Chrome and Edge have it; Firefox and Safari do not, and
// supporting all of them is a hard requirement inherited from
// EngineeringPaper.xyz (design note §11 item 6). So there are two paths through
// this code, and a check that exercised only one would leave half the users
// untested.
//
// Both paths are produced deliberately rather than inherited from whatever the
// running browser happens to support: one injected script reads the query string
// and either removes the API or stubs it. Chrome does expose it on 127.0.0.1,
// which is a secure context, so leaving this to chance would have silently tested
// the same branch twice.
//
// The stubs stand in for the pickers — the part a user operates — and nothing
// else, so what is under test is this application's use of the API, not the API.

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

const failures = [];
const check = (condition, description) => {
  if (!condition) failures.push(description);
};

// Runs before the application in every page. `?nofsa` removes the File System
// Access API to produce the Firefox and Safari path; otherwise it installs
// pickers that record what the application asks to have written.
const HARNESS = `
  if (location.search.includes("nofsa")) {
    delete window.showOpenFilePicker;
    delete window.showSaveFilePicker;
  } else {
  globalThis.__written = [];
  globalThis.__opened = "' opened worksheet\\nq = 7 kg\\n";

  function fakeHandle(name) {
    return {
      name,
      async getFile() {
        return { name, text: async () => globalThis.__opened };
      },
      async createWritable() {
        let buffer = "";
        return {
          async write(chunk) { buffer += chunk; },
          async close() { globalThis.__written.push({ name, text: buffer }); },
        };
      },
    };
  }

  window.showOpenFilePicker = async () => [fakeHandle("opened.nomo")];
  window.showSaveFilePicker = async ({ suggestedName }) =>
    fakeHandle(suggestedName ?? "untitled.nomo");
  }
`;

let browser;
try {
  browser = await launch();
  await browser.onNewDocument(HARNESS);

  // ---- the fallback path, which is what Firefox and Safari get ---------

  await browser.goto(`${url}?nofsa`);
  await ready(browser);

  check(
    (await browser.evaluate(`document.querySelector("#save").hidden`)) === true,
    "without a File System Access API there is nothing to save back to, " +
      "so Save must not be offered",
  );
  check(
    (await browser.evaluate(
      `document.querySelector("#save-as").textContent`,
    )) === "Download",
    "the fallback must say Download rather than offering a Save that quietly " +
      "produces a second file",
  );

  // ---- the File System Access path -------------------------------------

  await browser.goto(url);
  await ready(browser);

  check(
    (await browser.evaluate(`document.querySelector("#save").hidden`)) === false,
    "with a File System Access API, Save should be offered",
  );

  // Open.
  await browser.evaluate(`document.querySelector("#open").click()`);
  await waitFor(
    browser,
    `document.querySelector(".cm-content").textContent.includes("q = 7 kg")`,
    "the opened worksheet to reach the editor",
  );
  check(
    (await browser.evaluate(`document.querySelector("#file-name").textContent`))
      .includes("opened.nomo"),
    "the file name should be shown after opening",
  );
  await waitFor(
    browser,
    `document.querySelector("#output").textContent.includes("7 kg")`,
    "the opened worksheet to evaluate",
  );

  // Editing marks the document dirty.
  await browser.evaluate(`document.querySelector(".cm-content").focus()`);
  await browser.evaluate(`
    (() => {
      const selection = window.getSelection();
      selection.selectAllChildren(document.querySelector(".cm-content"));
      selection.collapseToEnd();
    })()
  `);
  await browser.type("\nw = q*2\n");
  await waitFor(
    browser,
    `document.querySelector("#file-name").textContent.includes("•")`,
    "the unsaved marker",
  );

  // Save.
  await browser.evaluate(`document.querySelector("#save").click()`);
  await waitFor(browser, `globalThis.__written.length > 0`, "the file to be written");

  const written = await browser.evaluate(`globalThis.__written[0]`);
  check(
    written.name === "opened.nomo",
    `Save must write back to the file that was opened, not to ${written.name}`,
  );
  check(
    written.text.includes("w = q*2"),
    "the edit did not reach the file",
  );
  // The worksheet opened without a pragma, so saving must have added one.
  check(
    written.text.startsWith("' nomo 1\n"),
    "saving must stamp the version pragma, so the file says what format it is in; " +
      `got ${JSON.stringify(written.text.slice(0, 30))}`,
  );

  check(
    !(await browser.evaluate(`document.querySelector("#file-name").textContent`))
      .includes("•"),
    "the unsaved marker should clear after a successful save",
  );

  // The pragma the engine added must be in the editor too. A buffer that
  // silently differs from its file is how an editor loses an edit.
  check(
    (await browser.evaluate(`document.querySelector(".cm-content").textContent`))
      .startsWith("' nomo 1"),
    "the editor was not reconciled with what was written to disk",
  );

  // Saving again must not stack a second pragma.
  await browser.evaluate(`document.querySelector("#save").click()`);
  await waitFor(browser, `globalThis.__written.length > 1`, "the second write");
  const again = await browser.evaluate(`globalThis.__written[1]`);
  check(
    (again.text.match(/' nomo 1/g) ?? []).length === 1,
    "saving twice stacked version pragmas",
  );
} catch (error) {
  failures.push(`could not drive the browser: ${error.message}`);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error("check-files: opening or saving is broken\n");
  for (const failure of failures) console.error(`  error: ${failure}`);
  process.exit(1);
}

console.log("ok: worksheets open and save, on both sides of the browser split");

function ready(browser) {
  return waitFor(browser, `document.body.dataset.ready === "true"`, "the editor");
}

async function waitFor(browser, expression, what) {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (await browser.evaluate(`(async () => ${expression})()`)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}
