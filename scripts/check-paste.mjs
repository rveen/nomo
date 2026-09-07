// Prove that an image pasted into the editor becomes a figure in the worksheet.
//
// Every layer below this one can be right while the paste still does nothing.
// The engine's half is tested in Rust and proven identical on both targets, but
// what it is given comes out of a `ClipboardEvent`, and no Rust test and no Node
// script can produce one. Nor can they say whether the two insertions the engine
// returns land where it said they would once CodeMirror has applied them.
//
// So this asserts the things only a browser can:
//
//   1. The reference lands on the line after the cursor, and the block lands in
//      the trailer, from one paste — and the figure the two of them describe is
//      then decoded and drawn, which is the only proof the base64 is intact.
//   2. An image wider than a figure is placed is shrunk to 700 px, in
//      proportion, and the file carries the shrunk bytes rather than the
//      original. This is the one step that rewrites the author's data, so it is
//      the one worth watching.
//   3. A second paste is a second figure under the same single marker. Two
//      markers would be a file the reader has to guess about.
//   4. A clipboard with a link to an image and no bytes is refused in words.
//      Nothing can be pasted there, and an editor that swallows a paste is
//      worse than one that says why it cannot.
//   5. Pasting text still pastes text. This handler sees every paste in the
//      application, so the check that it keeps its hands off is not optional.
//
// The images are drawn by the page itself rather than shipped as fixtures: a
// canvas is the one image source that is certainly the project's own, and a
// paste is the same event whatever made the bytes.

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

/// The width past which a pasted figure is shrunk. `web/src/image.js` owns this
/// number; repeating it here is what makes a change to it fail loudly.
const PLACED = 700;

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

  await replaceAll(browser, "' A worksheet\nr = 5 cm\nh = 12 cm\n");
  await waitFor(
    browser,
    `document.querySelector("#output").textContent.includes("12 cm")`,
    "the worksheet to evaluate",
  );
  // The cursor at the end of the second line, which is where the reference has
  // to appear under: a figure written above the line it was asked for is as
  // wrong as one written at the end of the file.
  await putCursorAtEndOfLine(browser, 2);

  // ---- a picture too wide to place ---------------------------------------

  await paste(browser, 1400, 700);
  await waitFor(browser, `document.querySelector("#output img")`, "the figure");

  const drawn = await measure(browser);
  check(
    drawn.width === String(PLACED) && drawn.height === "350",
    `the pasted figure was not placed at ${PLACED} px in proportion` +
      ` (the reference says ${drawn.width}x${drawn.height})`,
  );
  check(
    drawn.natural === `${PLACED}x350`,
    `the file carries the original pixels rather than the shrunk ones` +
      ` (the image decodes to ${drawn.natural})`,
  );
  check(
    drawn.src.startsWith("data:image/png;base64,"),
    `the figure is not embedded as a data: URI (src began ${drawn.src})`,
  );

  const said = await status(browser);
  check(
    said.includes("figure1") && said.includes("shrunk"),
    `a paste that rewrote the author's image did not say so ("${said}")`,
  );

  const text = await draft(browser);
  const lines = text.split("\n");
  check(
    lines[2] === `' image figure1 ${PLACED}x350`,
    `the reference did not land on the line after the cursor` +
      ` (line 3 is ${JSON.stringify(lines[2])})`,
  );
  check(
    lines[3] === "h = 12 cm",
    `the paste displaced the line below it (line 4 is ${JSON.stringify(lines[3])})`,
  );
  check(
    /^' image figure1 png \d+$/.test(blockHeaders(text)[0] ?? ""),
    `the trailer has no block for the figure (headers: ${JSON.stringify(blockHeaders(text))})`,
  );
  // The base64 belongs in the file and nowhere a reader can see it.
  check(
    !(await pane(browser)).includes("iVBOR"),
    "the worksheet's base64 is being shown as prose",
  );

  // One transaction, so one undo. A paste that takes two undos to remove has
  // left the trailer behind, and an orphaned block is a file that grows.
  await browser.key("z", { ctrl: true, code: "KeyZ", keyCode: 90 });
  const undone = await draft(browser);
  check(
    !undone.includes("figure1") && !undone.includes("--- resources ---"),
    "one undo did not take back the whole paste",
  );
  await browser.key("y", { ctrl: true, code: "KeyY", keyCode: 89 });
  await waitFor(
    browser,
    `document.querySelector("#output img") !== null`,
    "the figure to come back",
  );

  // ---- a second figure, and a picture small enough to keep ----------------

  // Below the first figure, so the order the two references come out in says
  // something: a paste goes where the cursor is, not after the last figure.
  await putCursorAtEndOfLine(browser, 4);
  await paste(browser, 200, 100);
  await waitFor(
    browser,
    `document.querySelectorAll("#output img").length === 2`,
    "the second figure",
  );

  const two = await draft(browser);
  check(
    references(two).join(" | ") ===
      `' image figure1 ${PLACED}x350 | ' image figure2 200x100`,
    `a second paste did not become a second figure (${JSON.stringify(references(two))})`,
  );
  check(
    two.split("\n").filter((l) => l.includes("--- resources ---")).length === 1,
    "a second paste wrote a second resource marker",
  );
  check(
    blockHeaders(two).length === 2,
    `the trailer does not hold both blocks (${JSON.stringify(blockHeaders(two))})`,
  );

  // ---- what a clipboard cannot give --------------------------------------

  await browser.evaluate(`
    (() => {
      const data = new DataTransfer();
      data.setData("text/html", '<img src="https://example.invalid/a.png">');
      const target = document.querySelector(".cm-content");
      target.focus();
      target.dispatchEvent(
        new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }),
      );
      return true;
    })()
  `);
  await waitFor(
    browser,
    `document.querySelector("#status").textContent.includes("link to that image")`,
    "the refusal",
  );
  const after = await draft(browser);
  check(
    after === two,
    "a paste that could not be carried changed the worksheet anyway",
  );

  // ---- and text is still text --------------------------------------------

  await browser.evaluate(`
    (() => {
      const data = new DataTransfer();
      data.setData("text/plain", "w = 7 kg");
      const target = document.querySelector(".cm-content");
      target.focus();
      target.dispatchEvent(
        new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }),
      );
      return true;
    })()
  `);
  await waitFor(
    browser,
    `document.querySelector("#output").textContent.includes("7 kg")`,
    "the pasted text",
  );
} catch (error) {
  failures.push(`could not drive the browser: ${error.message}`);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error("check-paste: a pasted image did not become a figure\n");
  for (const failure of failures) console.error(`  error: ${failure}\n`);
  process.exit(1);
}

console.log(
  "ok: an image pasted into the editor becomes a figure, shrunk to fit and said out loud",
);

/**
 * Paste an image the page draws for itself.
 *
 * Two colours rather than one so that a shrink that dropped the picture and
 * kept the canvas would still be a different image, and `File` rather than a
 * bare `Blob` because that is what a real clipboard carries.
 */
function paste(browser, width, height) {
  return browser.evaluate(`
    (async () => {
      const canvas = document.createElement("canvas");
      canvas.width = ${width};
      canvas.height = ${height};
      const context = canvas.getContext("2d");
      context.fillStyle = "#2266cc";
      context.fillRect(0, 0, ${width}, ${height});
      context.fillStyle = "#ffcc00";
      context.fillRect(0, 0, ${width} / 2, ${height} / 2);
      const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));

      const data = new DataTransfer();
      data.items.add(new File([blob], "pasted.png", { type: "image/png" }));
      const target = document.querySelector(".cm-content");
      target.focus();
      target.dispatchEvent(
        new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }),
      );
      return true;
    })()
  `);
}

/**
 * The whole document, read back out of the draft.
 *
 * Not out of the DOM: CodeMirror renders the lines it can see, and a worksheet
 * carrying an image is thousands of lines of trailer that are not among them.
 * The draft is the one place the entire text exists outside the editor, and
 * waiting for it also proves the draft survives a paste.
 */
async function draft(browser) {
  await waitFor(
    browser,
    `(document.querySelector("#file-name").textContent ?? "").length > 0`,
    "the editor",
  );
  // Longer than DRAFT_SETTLE in `web/src/main.js`, which is what this waits for.
  await new Promise((resolve) => setTimeout(resolve, 1000));
  return browser.evaluate(`
    (async () => {
      const db = await new Promise((resolve, reject) => {
        const request = indexedDB.open("nomo", 1);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      return await new Promise((resolve, reject) => {
        const tx = db.transaction("drafts", "readonly");
        const got = tx.objectStore("drafts").get("current");
        tx.oncomplete = () => resolve(got.result?.text ?? "");
        tx.onerror = () => reject(tx.error);
      });
    })()
  `);
}
/** Every body reference, in document order. */
function references(text) {
  return text.split("\n").filter((line) => /^' image \S+ \d+x\d+$/.test(line));
}

/** Every block header in the trailer, in document order. */
function blockHeaders(text) {
  return text.split("\n").filter((line) => /^' image \S+ [a-z]+ \d+$/.test(line));
}

function status(browser) {
  return browser.evaluate(`document.querySelector("#status").textContent`);
}

function pane(browser) {
  return browser.evaluate(`document.querySelector("#output").textContent`);
}

/** What the figure was written as, and what the browser made of it. */
async function measure(browser) {
  return JSON.parse(
    await browser.evaluate(`
      (() => {
        const img = document.querySelector("#output img");
        return JSON.stringify({
          width: img.getAttribute("width"),
          height: img.getAttribute("height"),
          natural: img.naturalWidth + "x" + img.naturalHeight,
          src: img.getAttribute("src").slice(0, 24),
        });
      })()
    `),
  );
}

/** Type a worksheet in, as a user would, over whatever was there. */
async function replaceAll(browser, source) {
  await browser.evaluate(`document.querySelector(".cm-content").focus()`);
  await browser.evaluate(`
    (() => {
      window.getSelection().selectAllChildren(document.querySelector(".cm-content"));
      return true;
    })()
  `);
  await browser.type(source);
}

/** Put the cursor at the end of line `number`, counting from one. */
function putCursorAtEndOfLine(browser, number) {
  return browser.evaluate(`
    (() => {
      const line = document.querySelectorAll(".cm-line")[${number - 1}];
      const range = window.document.createRange();
      range.selectNodeContents(line);
      range.collapse(false);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      return true;
    })()
  `);
}

async function waitFor(browser, expression, what) {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (await browser.evaluate(`(async () => ${expression})()`)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}
