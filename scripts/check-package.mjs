// Check that the packaged zip works when it is dropped somewhere arbitrary.
//
// `scripts/package-web.sh` says the site works at whatever path it is unzipped
// into, and a person following that advice finds out whether it is true by
// deploying it. Every other browser check serves `web/dist/` from the root of a
// throwaway server, so an absolute path anywhere in the front end — `/style.css`
// in a stylesheet, `/nomo_wasm.wasm` in a loader, a service worker registered at
// `/sw.js` — would pass all of them and break only the zip. That is the failure
// this exists to catch, and nothing else can catch it.
//
// So: unpack the actual archive, serve it under a nested prefix that is nothing
// like a document root, and drive it. The 404 count is the real assertion —
// any absolute path escapes the prefix and misses, whatever it was for.

import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join } from "node:path";
import { promisify } from "node:util";
import { launch } from "./chrome.mjs";
import { repoRoot } from "./wasm-host.mjs";

const run = promisify(execFile);

const version = process.argv[2] ?? "check";

// A prefix with two levels and a space in it: a document root is the case that
// already works, so the check is worth nothing unless it deploys somewhere
// awkward. The space is there because a path is a URL, and something in the
// chain will encode it or fail to.
const PREFIX = "/deployed/under here";

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8",
};

const work = await mkdtemp(join(tmpdir(), "nomo-package-"));
const failures = [];
const check = (condition, description) => {
  if (!condition) failures.push(description);
};

let browser;
let server;
try {
  // Package from whatever is in web/dist/ right now, then unpack it. Reading
  // web/dist/ directly would test the directory rather than the archive, and it
  // is the archive that gets published.
  await run("./scripts/package-web.sh", [version, work], {
    cwd: repoRoot,
  });
  const zip = join(work, `nomo-web-${version}.zip`);
  await run("unzip", ["-q", zip, "-d", join(work, "unpacked")]);
  const root = join(work, "unpacked", `nomo-web-${version}`);

  const missing = [];
  server = createServer(async (request, response) => {
    const path = decodeURIComponent(
      new URL(request.url, "http://localhost").pathname,
    );
    if (!path.startsWith(`${PREFIX}/`)) {
      // Outside the prefix is off the deployment entirely: on a real server
      // this is somebody else's application, or nothing at all. The browser's
      // unprompted favicon request is the one exception — every browser asks
      // the origin root for it, and no page asked it to.
      if (path !== "/favicon.ico") missing.push(path);
      response.writeHead(404).end();
      return;
    }
    // A directory serves its index, which is what every real server does and
    // what the gallery's own links rely on.
    const rest = path.slice(PREFIX.length);
    const name = rest.endsWith("/") ? `${rest}index.html` : rest;
    try {
      const body = await readFile(join(root, name));
      response.writeHead(200, {
        "content-type": TYPES[extname(name)] ?? "application/octet-stream",
      });
      response.end(body);
    } catch {
      missing.push(path);
      response.writeHead(404).end();
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}${encodeURI(PREFIX)}/`;

  browser = await launch();
  await browser.goto(base);
  await waitFor(
    browser,
    `document.body.dataset.ready === "true"`,
    "the editor to start",
  );

  // The engine is the file most likely to be fetched by an absolute path, and
  // an answer on the page is the only proof it both loaded and ran.
  await waitFor(
    browser,
    `document.querySelector("#output")?.textContent.includes("0.942478 dm³")`,
    "the example worksheet to evaluate",
  );

  // The stylesheet, by a property no user agent stylesheet would produce.
  check(
    (await browser.evaluate(
      `getComputedStyle(document.body).getPropertyValue("margin") === "0px"`,
    )) === true,
    "style.css did not apply, so it was not found at this path",
  );

  // The worker's scope is the directory it was served from, not the origin.
  // Registering it by an absolute path would silently claim the whole site.
  // Wrapped in an async IIFE because `evaluate` runs the expression as-is, and
  // a bare `await` is a syntax error outside one.
  await waitFor(
    browser,
    `!!(await navigator.serviceWorker.getRegistration())`,
    "the service worker to register",
  );
  const scope = await browser.evaluate(
    `(async () => (await navigator.serviceWorker.getRegistration())?.scope ?? "")()`,
  );
  check(
    typeof scope === "string" && scope.endsWith(`${encodeURI(PREFIX)}/`),
    `the service worker registered at ${scope || "nowhere"} rather than under ` +
      "the directory it was unzipped into",
  );

  // A worked example, reached the way the page links to it.
  await browser.goto(`${base}examples/`);
  const gallery = await browser.evaluate(`document.body.textContent`);
  check(
    typeof gallery === "string" && gallery.includes("worked examples"),
    "examples/index.html did not load from the packaged directory",
  );

  // The language reference, which reaches one directory *up* for its fonts —
  // the only page on the site that does, and so the one most able to break a
  // deployment that is not at a document root.
  await browser.goto(`${base}language/`);
  const reference = await browser.evaluate(`document.body.textContent`);
  check(
    typeof reference === "string" && reference.includes("A worksheet"),
    "language/index.html did not load from the packaged directory",
  );
  check(
    (await browser.evaluate(
      `document.fonts.check('1rem "STIX Two Text Subset"')`,
    )) === true,
    "the reference did not get the shipped text face; its ../fonts/ path missed",
  );

  check(
    missing.length === 0,
    `${missing.length} request(s) missed the deployment: ${[...new Set(missing)]
      .slice(0, 5)
      .join(", ")}` +
      " — something in the site is asking for an absolute path, which works " +
      "only at a document root",
  );
} catch (error) {
  failures.push(`could not check the package: ${error.message}`);
} finally {
  await browser?.close();
  server?.close();
  await rm(work, { recursive: true, force: true });
}

if (failures.length > 0) {
  console.error("check-package: the packaged site does not work as shipped\n");
  for (const failure of failures) console.error(`  error: ${failure}`);
  process.exit(1);
}

console.log(`ok: the packaged zip serves from ${PREFIX}/ and evaluates there`);

/** Poll an expression in the page until it is true. */
async function waitFor(browser, expression, what) {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (await browser.evaluate(`(async () => ${expression})()`)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}
