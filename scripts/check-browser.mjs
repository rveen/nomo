// Load the built page in a real browser and check what it actually rendered.
//
// The unit tests can say the engine classifies `cm` as a unit; only a browser
// can say the editor coloured the characters that spell `cm`. The two came apart
// once already and nothing else noticed: the engine emits UTF-8 byte offsets and
// CodeMirror counts UTF-16 code units, so every highlight after the first
// non-ASCII character in a worksheet landed two columns to the right. Every
// assertion below was green at the time.
//
// Drives Chrome over the DevTools protocol and waits for the application to say
// it is ready. It used to use `--dump-dom --virtual-time-budget`, which was
// simpler but stopped working the moment startup touched IndexedDB: virtual time
// advances timers, and a database request is not a timer, so the page never
// finished starting and every assertion here failed at once. Waiting for a
// specific signal beats waiting for a clock.

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
  await readFile(join(dist, "bundle.js"));
} catch {
  console.error(
    "error: web/dist is not built\n" +
      "  cargo build -p nomo-wasm --release --target wasm32-unknown-unknown\n" +
      "  cd web && npm install && node build.mjs",
  );
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
let browser;

try {
  browser = await launch();
  await browser.goto(url);
  await waitFor(browser, `document.body.dataset.ready === "true"`, "the editor");

  const dom = await browser.evaluate(`document.documentElement.outerHTML`);

  const has = (needle, why) => {
    if (!dom.includes(needle)) failures.push(`${why}\n    looked for: ${needle}`);
  };

  // The engine loaded and evaluated. `status` reports any failure to load, so a
  // good status is also proof that nothing threw on the way.
  has(
    'id="status" class="good"',
    "the status line did not end up in its success state",
  );

  // The worksheet computed, and to the same value the CLI produces.
  has("0.942478 dm³", "the rendered worksheet does not show the expected result");
  has('<span class="result">', "the result markup is missing");

  // Highlighting is applied, and — the part that regressed — applied to the
  // right characters. These check the spans wrap exactly the intended text.
  has(
    '<span class="tok-unit">cm</span>',
    "`cm` is not highlighted as a unit; offsets may be misaligned",
  );
  has(
    '<span class="tok-constant">pi</span>',
    "`pi` is not highlighted as a constant",
  );
  has(
    '<span class="tok-variable">r</span>',
    "`r` is not highlighted as a variable",
  );
  has('class="tok-comment">\' ', "comments are not highlighted");

  // The document contains an em dash before any of the above, so if the offset
  // conversion regressed these would be off by two and the checks above would
  // fail. Assert the dash is really there, or the check stops testing that.
  has("—", "the fixture lost its em dash and no longer covers offset conversion");

  // The editor mounted.
  has('class="cm-editor', "CodeMirror did not mount");
} catch (error) {
  failures.push(`could not drive the browser: ${error.message}`);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error("check-browser: the page did not render as expected\n");
  for (const failure of failures) console.error(`  error: ${failure}\n`);
  process.exit(1);
}

console.log("ok: the page loads, evaluates and highlights correctly in Chrome");

async function waitFor(browser, expression, what) {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (await browser.evaluate(`(async () => ${expression})()`)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}
