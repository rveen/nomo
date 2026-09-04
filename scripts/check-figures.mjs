// Prove that a worksheet's figures reach the screen as pictures.
//
// Every layer below this one can be right while the reader still sees base64.
// That is not hypothetical: the engine, the renderer and the golden suite all
// agreed that a figure was embedded, and Chrome showed the raw text anyway,
// because the service worker was cache-first under a name no build ever changed
// and the tab was running an engine from before images existed. Only a browser
// can say what a browser is showing.
//
// So this asserts the two things the layers below cannot:
//
//   1. `naturalWidth > 0` — the browser fetched, decoded and sized the image.
//      A broken `data:` URI, an unusable media type or a mangled payload all
//      leave a rendered `<img>` behind, and only this distinguishes them.
//   2. The trailer's base64 appears nowhere in the output's text.
//   3. The figure is laid out at the size its reference asks for, and a figure
//      wider than the pane is shrunk whole rather than cropped. Only a browser
//      can say this either: the attributes, the `max-width` and the `height:
//      auto` are three separate files agreeing, and getting any one of them
//      wrong shows a reader part of a diagram with nothing to say so.

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

// A 2×1 PNG. Small enough to type into the editor the way a user would, which
// is what keeps this a test of the application rather than of a fixture; two
// pixels wide rather than one so it has a shape, and every size below is one
// the reference asked for rather than one the image brought with it.
const PIXEL =
  "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAIAAAB7QOjdAAAAC0lEQVR4nGNgAAMAAAcAAbKGrPQAAAAASUVORK5CYII=";

const worksheet = (reference) =>
  [
    "' A figure",
    `' ${reference}`,
    "' --- resources ---",
    `' image dot png 68`,
    `'   ${PIXEL}`,
  ].join("\n");

const WORKSHEET = worksheet("image dot");

/// The size the reference asks for, in the image's own 2:1 so that honouring it
/// and refusing to distort it are the same picture. Neither number is one the
/// image or the pane could have produced by itself.
const DRAWN = { width: 240, height: 120 };

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

// The cache name is the thing that went wrong, so check it directly rather than
// only through its symptom. A literal here means every future build ships bytes
// a returning browser will not fetch.
const worker = await readFile(join(dist, "sw.js"), "utf8");
check(
  /nomo-[0-9a-f]{12}/.test(worker) && !worker.includes("nomo-v1"),
  "the service worker's cache name is not stamped from the shell's content;\n" +
    "    a cache-first worker under a fixed name serves the previous build for ever",
);

let browser;
try {
  browser = await launch();
  await browser.goto(url);
  await waitFor(browser, `document.body.dataset.ready === "true"`, "the editor");

  // The pane is re-rendered wholesale, so waiting for "an image exists" would
  // measure the *previous* worksheet's figure and pass on stale layout. Wait
  // for the width this source asks for to be the one in the document.
  const render = async (source, width) => {
    await browser.evaluate(`document.querySelector(".cm-content").focus()`);
    await browser.evaluate(`
      (() => {
        const selection = window.getSelection();
        selection.selectAllChildren(document.querySelector(".cm-content"));
      })()
    `);
    await browser.type(source);
    const asked = width === null ? "null" : `"${width}"`;
    await waitFor(
      browser,
      `(document.querySelector("#output img")?.getAttribute("width") ?? null)` +
        ` === ${asked}` +
        ` && (document.querySelector("#output img")?.complete ?? false)`,
      `the figure to render at ${width ?? "its own size"}`,
    );
  };

  await render(WORKSHEET, null);

  const width = await browser.evaluate(
    `document.querySelector("#output img").naturalWidth`,
  );
  check(
    width > 0,
    `the <img> is in the document but the browser could not decode it (naturalWidth ${width})`,
  );

  const src = await browser.evaluate(
    `document.querySelector("#output img").getAttribute("src")`,
  );
  check(
    typeof src === "string" && src.startsWith("data:image/png;base64,"),
    `the figure is not embedded as a data: URI (src began ${String(src).slice(0, 40)})`,
  );

  // The complaint that started this: the trailer shown as prose.
  const text = await browser.evaluate(
    `document.querySelector("#output").textContent`,
  );
  check(
    !text.includes(PIXEL.slice(0, 32)),
    "the resource trailer is being displayed as text instead of as a picture",
  );

  // A reference that says nothing must still lay out at the image's own size:
  // every worksheet written before the size existed is this one.
  const own = await measure(browser);
  check(
    own.width === own.naturalWidth,
    `a reference with no size was not drawn at the image's own width` +
      ` (${own.width} against ${own.naturalWidth})`,
  );

  await render(
    worksheet(`image dot ${DRAWN.width}x${DRAWN.height}`),
    DRAWN.width,
  );

  const drawn = await measure(browser);
  check(
    Math.abs(drawn.width - DRAWN.width) < 1 &&
      Math.abs(drawn.height - DRAWN.height) < 1,
    `the figure was not drawn at the size its reference asks for` +
      ` (${drawn.width}x${drawn.height} against ${DRAWN.width}x${DRAWN.height})`,
  );
  check(
    drawn.pane > DRAWN.width,
    `this check needs a pane wider than ${DRAWN.width}px to mean anything` +
      ` (it was ${drawn.pane}px)`,
  );

  // A reference whose shape disagrees with the image's. SMath lets a picture
  // region be dragged out of proportion, and the honest answer is the width it
  // asks for at the image's own shape: a stretched diagram is a wrong diagram,
  // and nothing on the page would say it had been stretched.
  await render(worksheet("image dot 240x240"), "240");
  const squashed = await measure(browser);
  check(
    Math.abs(squashed.width - 240) < 1 && Math.abs(squashed.height - 120) < 1,
    `a reference out of proportion with its image distorted the figure` +
      ` (${squashed.width}x${squashed.height}, not 240x120)`,
  );

  // Wider than any pane. The figure must shrink whole: fit the pane, keep the
  // 2:1 the reference asked for, and leave nothing to scroll sideways to.
  await render(worksheet("image dot 20000x10000"), "20000");

  const huge = await measure(browser);
  check(
    huge.width <= huge.pane + 1,
    `a figure wider than the pane was not limited to it` +
      ` (${huge.width} in a pane of ${huge.pane})`,
  );
  check(
    Math.abs(huge.width / huge.height - 2) < 0.02,
    `the shrunk figure lost the shape its reference asked for` +
      ` (${huge.width}x${huge.height} is not 2:1)`,
  );
  // Cropping is the failure this whole path exists to rule out, and a figure
  // clipped by its container reads as a complete one to anybody who does not
  // think to scroll.
  check(
    huge.overflow <= 1,
    `the output scrolls sideways by ${huge.overflow}px, so the figure is cut off`,
  );
} catch (error) {
  failures.push(`could not drive the browser: ${error.message}`);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error("check-figures: figures did not reach the screen\n");
  for (const failure of failures) console.error(`  error: ${failure}\n`);
  process.exit(1);
}

console.log("ok: figures render as pictures in Chrome, and the trailer does not");

/// What the browser actually laid the figure out as, and the room it had.
async function measure(browser) {
  return JSON.parse(
    await browser.evaluate(`
      (() => {
        const img = document.querySelector("#output img");
        const output = document.querySelector("#output");
        const box = img.getBoundingClientRect();
        return JSON.stringify({
          width: box.width,
          height: box.height,
          naturalWidth: img.naturalWidth,
          pane: output.clientWidth,
          overflow: output.scrollWidth - output.clientWidth,
        });
      })()
    `),
  );
}

async function waitFor(browser, expression, what) {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (await browser.evaluate(`(async () => ${expression})()`)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}
