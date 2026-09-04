// Build the static site into web/dist/.
//
// esbuild bundles the front end; `cargo` builds the engine. The two are
// deliberately separate: the WebAssembly artifact whose reproducibility this
// project rests on is produced by cargo alone, and no JavaScript tool is in that
// path. A bundler for the user interface is downstream of the guarantee and
// cannot affect it — which is exactly why `wasm-bindgen` was refused and esbuild
// is fine. See `crates/nomo-wasm/src/lib.rs`.
//
//   node build.mjs           build once into dist/
//   node build.mjs --serve   build, watch, and serve on :8000

import * as esbuild from "esbuild";
import { buildFont, FONT_FILE, TEXT_FILES } from "./font.mjs";
import { createHash } from "node:crypto";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..");
const dist = join(here, "dist");
const wasm = join(
  repoRoot,
  "target/wasm32-unknown-unknown/release/nomo_wasm.wasm",
);

const serve = process.argv.includes("--serve");

// Emptied first, not merged into. A build directory that only ever gains files
// ships whatever anybody ever put there: this one was still carrying a
// `sheaf_wasm.wasm` from before the project was renamed — 650 kB of a dead
// engine that every deployment would have published, and that a service worker
// told to cache the whole origin would have handed to a browser.
await rm(dist, { recursive: true, force: true });
await mkdir(dist, { recursive: true });

try {
  await cp(wasm, join(dist, "nomo_wasm.wasm"));
} catch {
  console.error(
    `error: no engine at ${wasm}\n` +
      "  build it first:\n" +
      "    cargo build -p nomo-wasm --release --target wasm32-unknown-unknown",
  );
  process.exit(1);
}

await cp(join(here, "index.html"), join(dist, "index.html"));
await cp(join(here, "style.css"), join(dist, "style.css"));

// The fonts, subset here rather than committed. `dist/` is emptied above, so
// this has to run on every build rather than only when the files are missing.
const fontBytes = await buildFont(dist);

const options = {
  entryPoints: [join(here, "src/main.js")],
  bundle: true,
  format: "esm",
  target: ["es2022"],
  outfile: join(dist, "bundle.js"),
  sourcemap: serve,
  minify: !serve,
  logLevel: "info",
};

/**
 * A digest of the shell the service worker is about to cache.
 *
 * The worker is cache-first, so what a returning browser runs is decided
 * entirely by its cache name: an unchanged name means the new bytes on the
 * server are never fetched. Naming the cache after the *content* is what makes
 * that safe — any change to the engine or the interface changes the name, the
 * old cache is deleted on activate, and a reload picks the new build up.
 *
 * `sw.js` is excluded because the digest ends up inside it, which cannot include
 * itself. Everything the worker actually serves is covered.
 */
async function shellVersion() {
  const hash = createHash("sha256");
  for (const name of [
    "index.html",
    "style.css",
    "bundle.js",
    "nomo_wasm.wasm",
    // The fonts are part of the shell the worker precaches, so a change to one
    // has to change the cache name like anything else. Leaving them out would
    // be the same fault the literal `"nomo-v1"` above was: new bytes on the
    // server that a returning browser never asks for.
    `fonts/${FONT_FILE}`,
    `fonts/${TEXT_FILES.upright}`,
    `fonts/${TEXT_FILES.italic}`,
  ]) {
    hash.update(await readFile(join(dist, name)));
  }
  return hash.digest("hex").slice(0, 12);
}

// The service worker is a separate script at the root of dist/, not part of the
// bundle: it runs in its own global scope, and its scope on the server is
// determined by where the file sits. `iife` because it is registered as a
// classic worker, which cannot use `import`.
const workerOptions = (version) => ({
  entryPoints: [join(here, "src/sw.js")],
  bundle: true,
  format: "iife",
  target: ["es2022"],
  outfile: join(dist, "sw.js"),
  minify: !serve,
  logLevel: "warning",
  define: { __SHELL_VERSION__: JSON.stringify(version) },
});

if (!serve) {
  // The bundle first: the worker is named after a digest that covers it.
  await esbuild.build(options);
  await esbuild.build(workerOptions(await shellVersion()));
  const { size } = await readFile(join(dist, "bundle.js")).then((b) => ({
    size: b.length,
  }));
  console.log(
    `built dist/ — bundle ${(size / 1024).toFixed(0)} kB, ` +
      `math font ${(fontBytes.math / 1024).toFixed(0)} kB, ` +
      `text font ${(fontBytes.text / 1024).toFixed(0)} kB`,
  );
  process.exit(0);
}

const context = await esbuild.context(options);
await context.watch();
// Watch mode stamps the moment the server started rather than a digest: the
// bundle changes under it as you edit, so a digest taken now would be a claim
// about bytes that no longer exist. A rebuild is what refreshes it, which is the
// same thing a developer testing the worker has to do anyway.
const workerContext = await esbuild.context(
  workerOptions(`dev-${Date.now().toString(36)}`),
);
await workerContext.watch();

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".map": "application/json; charset=utf-8",
  // Serving this correctly is what lets `instantiateStreaming` work.
  ".wasm": "application/wasm",
  ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8",
};

createServer(async (request, response) => {
  const url = new URL(request.url, "http://localhost");
  const name = url.pathname === "/" ? "/index.html" : url.pathname;
  try {
    const body = await readFile(join(dist, name));
    response.writeHead(200, {
      "content-type": TYPES[extname(name)] ?? "application/octet-stream",
      "cache-control": "no-store",
    });
    response.end(body);
  } catch {
    response.writeHead(404, { "content-type": "text/plain" });
    response.end("not found\n");
  }
}).listen(8000, () => console.log("serving http://localhost:8000"));

// Keep the copied engine current while watching.
await writeFile(join(dist, ".watching"), new Date().toISOString());
