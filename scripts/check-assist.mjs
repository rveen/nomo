// Completion, hover and go-to-definition, in a real browser.
//
// These are the three features whose whole existence is a thing appearing on a
// screen. A unit test can say the engine reports that `r` is `0.05 m` and was
// written at offset 12; only a browser can say that typing `si` offers
// `sigma_allow`, that pointing at `r` says what it holds, and that F12 moves the
// cursor to where it was bound.
//
// Chrome only, like the rest of the browser checks on this machine.

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

let browser;
try {
  browser = await launch();
  await browser.goto(url);
  await waitFor(browser, `document.body.dataset.ready === "true"`, "the editor");

  // A worksheet with a name worth completing, one worth explaining, and a
  // definition worth jumping to. Typed at the start of the document, so the
  // starting worksheet stays below it and nothing depends on clearing it.
  await browser.evaluate(`document.querySelector(".cm-content").focus()`);
  await browser.type("sigma_allow = 24 ksi\n");
  await waitFor(
    browser,
    `document.querySelector("#output").textContent.includes("24 ksi")`,
    "the worksheet to evaluate",
  );

  // ---- completion ------------------------------------------------------
  //
  // Typing the first letters of a name the worksheet binds should offer it,
  // with what it holds beside it: `ksi` and `kip` are one letter apart and mean
  // different things, and the detail is what tells them apart.
  await browser.type("sig");
  await waitFor(
    browser,
    `document.querySelectorAll(".cm-tooltip-autocomplete li").length > 0`,
    "a completion list",
  );
  const offered = await completionItems(browser);
  check(
    offered.some((t) => t.includes("sigma_allow")),
    `completion should offer a name the worksheet binds, got ${JSON.stringify(offered)}`,
  );
  check(
    offered.some((t) => t.includes("ksi")),
    `completion should show what the name holds, got ${JSON.stringify(offered)}`,
  );

  // A unit is offered too, with its dimension — the thing a reader needs at the
  // moment of choosing between two units whose names are one letter apart.
  await replaceDocument(browser, "x = 5 ks");
  await browser.type("i");
  await waitFor(
    browser,
    `document.querySelectorAll(".cm-tooltip-autocomplete li").length > 0`,
    "a unit completion",
  );
  const units = await completionItems(browser);
  check(
    units.some((t) => t.includes("ksi")),
    `a unit should be offered, got ${JSON.stringify(units)}`,
  );

  // ---- hover -----------------------------------------------------------
  //
  // The question a reader asks of a worksheet they did not write: what is that,
  // and in what units. Driven by pointing at the character, through the DOM
  // rather than through CodeMirror's own API — the editor does not publish its
  // view object, and a test hook in the application to let this reach it would
  // be a worse trade than measuring what the page actually shows.
  await replaceDocument(browser, "r = 5 cm\nd = 2*r");
  await waitFor(
    browser,
    `document.querySelector("#output").textContent.includes("0.1 m")`,
    "the worksheet to evaluate",
  );

  // The final `r` of `d = 2*r`, which is the use rather than the binding.
  const target = await charPoint(browser, 2, "d = 2*r".length - 1);
  const hovered = await browser.evaluate(`
    (async () => {
      const el = document.elementFromPoint(${target.x}, ${target.y});
      for (const type of ["mousemove", "mouseover"]) {
        el.dispatchEvent(new MouseEvent(type, {
          bubbles: true, clientX: ${target.x}, clientY: ${target.y},
        }));
      }
      await new Promise((r) => setTimeout(r, 700));
      const tip = document.querySelector(".cm-nomo-tooltip");
      return tip ? tip.textContent : "";
    })()
  `);
  check(
    hovered.includes("r") && hovered.includes("5 cm"),
    `hovering a name should say what it holds, got ${JSON.stringify(hovered)}`,
  );

  // ---- go to definition ------------------------------------------------
  //
  // Click on the `r` in `2*r`, press F12, and the cursor should end up on the
  // line that binds it.
  await browser.click(target.x, target.y);
  const before = Number(
    await browser.evaluate(`document.querySelectorAll(".cm-line")[1].classList.contains("cm-activeLine") ? 2 : 0`),
  );
  check(before === 2, "the click should put the cursor on the second line");
  // Dispatched into the page rather than through the operating system's key
  // routing: a browser keeps F12 for its own developer tools, and what this
  // check is about is whether the editor's binding does the right thing.
  await browser.evaluate(`
    (() => {
      const content = document.querySelector(".cm-content");
      content.dispatchEvent(new KeyboardEvent("keydown", {
        key: "F12", code: "F12", keyCode: 123, which: 123, bubbles: true,
      }));
    })()
  `);
  await new Promise((r) => setTimeout(r, 100));
  const landed = Number(
    await browser.evaluate(`
      (() => {
        const lines = [...document.querySelectorAll(".cm-line")];
        return lines.findIndex((l) => l.classList.contains("cm-activeLine")) + 1;
      })()
    `),
  );
  check(
    landed === 1,
    `F12 should jump to the line that binds the name, landed on line ${landed}`,
  );
  // ---- the typeset toggle ----------------------------------------------
  //
  // Step 18 put MathML behind a CLI flag; this is where a reader meets it. The
  // pane must change when the box is ticked and change back when it is not,
  // because the alternative — a toggle that does nothing visible — is the kind
  // of feature that looks present and is not.
  await replaceDocument(browser, "w = 2 kN/m\nL = 6 m\nM = w*L^2/8");
  await waitFor(
    browser,
    `document.querySelector("#output").textContent.includes("9000")`,
    "the worksheet to evaluate",
  );
  check(
    (await browser.evaluate(`document.querySelectorAll("#output math").length`)) === 0,
    "typesetting should be off until it is asked for",
  );

  await browser.evaluate(`document.querySelector("#typeset").click()`);
  await waitFor(
    browser,
    `document.querySelectorAll("#output mfrac").length > 0`,
    "the results to be typeset",
  );
  const stacked = JSON.parse(
    await browser.evaluate(`
      (() => {
        const frac = document.querySelector("#output mfrac");
        const [num, den] = [...frac.children].map((c) => c.getBoundingClientRect());
        return JSON.stringify({ numeratorBottom: num.bottom, denominatorTop: den.top });
      })()
    `),
  );
  check(
    stacked.numeratorBottom <= stacked.denominatorTop + 0.5,
    `the toggle should produce a real fraction, got ${JSON.stringify(stacked)}`,
  );

  await browser.evaluate(`document.querySelector("#typeset").click()`);
  await waitFor(
    browser,
    `document.querySelectorAll("#output math").length === 0`,
    "the results to go back to text",
  );

} catch (error) {
  failures.push(`could not drive the browser: ${error.message}`);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error("check-assist: the editor did not assist\n");
  for (const failure of failures) console.error(`  error: ${failure}\n`);
  process.exit(1);
}

console.log("ok: completion offers, hover explains, and F12 finds the definition");

/** Replace the whole document, the way a user would: select all, then type. */
async function replaceDocument(browser, text) {
  await browser.evaluate(`document.querySelector(".cm-content").focus()`);
  await browser.key("a", { ctrl: true, keyCode: 65 });
  await browser.type(text);
}

/** The completion list as the reader sees it. */
async function completionItems(browser) {
  return JSON.parse(
    await browser.evaluate(`
      (() => {
        const items = [...document.querySelectorAll(".cm-tooltip-autocomplete li")];
        return JSON.stringify(items.map((li) => li.textContent));
      })()
    `),
  );
}

/**
 * Where a character sits on screen, measured from the rendered text.
 *
 * A `Range` over the line's own text node, which needs nothing from the editor
 * beyond what it has drawn.
 */
async function charPoint(browser, line, column) {
  return JSON.parse(
    await browser.evaluate(`
      (() => {
        const el = document.querySelectorAll(".cm-line")[${line - 1}];
        const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
        let node = walker.nextNode();
        let seen = 0;
        while (node && seen + node.textContent.length <= ${column}) {
          seen += node.textContent.length;
          node = walker.nextNode();
        }
        const range = document.createRange();
        range.setStart(node, ${column} - seen);
        range.setEnd(node, ${column} - seen + 1);
        const r = range.getBoundingClientRect();
        return JSON.stringify({ x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) });
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
