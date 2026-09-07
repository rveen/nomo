// Check that an SMath worksheet can be imported in the browser, and that the
// import says what it did.
//
// The point of the feature is not the translation on its own — `check-corpus.sh`
// measures that against 114 worksheets and is a far better test of it. The point
// here is the part only a browser has: that a `.sm` goes in through the ordinary
// Open command, that what comes out is the *translation* rather than the file,
// and that every construct the importer could not translate is put in front of
// the reader with a line they can click.
//
// The last of those is the one worth a browser check. The importer's rule is
// that nothing is ever silently dropped, and until this panel existed the only
// place that rule was visible was a marker comment somewhere in a thousand-line
// file. A rule nobody is pointed at is only half a rule.
//
// # The fixture is written here
//
// The corpora are fetched rather than committed and are not present on every
// machine, so a check that read one would be a check that mostly does not run.
// This worksheet is the project's own bytes: five regions, one stored answer
// that Nomo should agree with, and one `range` with a step — a construct the
// design note refuses to guess at, so it is a reliably untranslatable one.

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

const WORKSHEET = `<?xml version="1.0" encoding="utf-8" standalone="yes"?>
<?application progid="SMath Studio" version="0.98"?>
<regions>
  <settings><calculation><precision>4</precision><angle>radians</angle></calculation></settings>
  <region id="0" left="0" top="0">
    <text lang="eng"><p>A beam under load</p></text>
  </region>
  <region id="1" left="0" top="40">
    <math><input>
      <e type="operand">L</e><e type="operand">3</e>
      <e type="operand" style="unit">m</e><e type="operator" args="2">*</e>
      <e type="operator" args="2">:</e>
    </input></math>
  </region>
  <region id="2" left="0" top="80">
    <math><input>
      <e type="operand">w</e><e type="operand">12</e>
      <e type="operand" style="unit">kN</e><e type="operator" args="2">*</e>
      <e type="operator" args="2">:</e>
    </input></math>
  </region>
  <region id="3" left="0" top="120">
    <math><input>
      <e type="operand">M</e><e type="operand">w</e><e type="operand">L</e>
      <e type="operator" args="2">*</e><e type="operand">8</e>
      <e type="operator" args="2">/</e><e type="operator" args="2">:</e>
    </input></math>
  </region>
  <region id="4" left="0" top="160">
    <math>
      <input><e type="operand">M</e></input>
      <result action="numeric">
        <e type="operand">4.5</e>
        <e type="operand" style="unit">kN</e><e type="operator" args="2">*</e>
        <e type="operand" style="unit">m</e><e type="operator" args="2">*</e>
      </result>
    </math>
  </region>
  <region id="5" left="0" top="200">
    <math><input>
      <e type="operand">s</e><e type="operand">0</e><e type="operand">1</e>
      <e type="operand">10</e><e type="function" args="3">range</e>
      <e type="operator" args="2">:</e>
    </input></math>
  </region>
</regions>
`;

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

// The picker stub hands back a `.sm`. Its `getFile` offers `arrayBuffer` and a
// `text` that throws: the application must read an SMath worksheet as bytes,
// because SMath writes a BOM and the reader strips it itself. A stub that
// offered both would let a regression to `file.text()` pass unnoticed.
const HARNESS = `
  globalThis.__worksheet = ${JSON.stringify(WORKSHEET)};
  globalThis.__saved = [];

  window.showOpenFilePicker = async () => [{
    name: "beam.sm",
    async getFile() {
      const bytes = new TextEncoder().encode(globalThis.__worksheet);
      return {
        name: "beam.sm",
        async arrayBuffer() { return bytes.buffer; },
        async text() { throw new Error("an .sm must be read as bytes, not text"); },
      };
    },
  }];

  window.showSaveFilePicker = async ({ suggestedName }) => ({
    name: suggestedName,
    async createWritable() {
      let buffer = "";
      return {
        async write(chunk) { buffer += chunk; },
        async close() { globalThis.__saved.push({ name: suggestedName, text: buffer }); },
      };
    },
  });
`;

let browser;
try {
  browser = await launch();
  await browser.onNewDocument(HARNESS);
  await browser.goto(url);
  await ready(browser);

  check(
    (await browser.evaluate(`document.querySelector("#import").hidden`)) === true,
    "the import panel must stay out of the way until there is an import",
  );

  // ---- the import ------------------------------------------------------

  await browser.evaluate(`document.querySelector("#open").click()`);
  await waitFor(
    browser,
    `document.querySelector("#import").hidden === false`,
    "the import report",
  );

  const source = await browser.evaluate(
    `document.querySelector(".cm-content").textContent`,
  );
  check(
    source.includes("M = w*L/8"),
    `the translation should be in the editor; got ${JSON.stringify(source.slice(0, 80))}`,
  );
  check(
    !source.includes("<e type="),
    "the editor is holding the .sm file rather than a translation of it",
  );

  // The document is now a Nomo worksheet with no file behind it. Both halves
  // matter: the extension, so Save As suggests the right name, and the absent
  // handle, so nothing can write Nomo source over the user's original .sm.
  const shown = await browser.evaluate(
    `document.querySelector("#file-name").textContent`,
  );
  check(
    shown.includes("beam.nomo"),
    `an imported worksheet should be named beam.nomo; got ${JSON.stringify(shown)}`,
  );
  check(
    shown.includes("•"),
    "a translation exists nowhere on disk, so it must count as unsaved work",
  );
  check(
    (await browser.evaluate(`document.querySelector("#save").disabled`)) === true,
    "Save must not offer to write back over the .sm that was opened",
  );

  // ---- what the report says --------------------------------------------

  const summary = await browser.evaluate(
    `document.querySelector("#import-summary").textContent`,
  );
  check(
    summary.includes("1 construct not translated"),
    `the summary should count the untranslated construct; got ${JSON.stringify(summary)}`,
  );
  // The oracle is the whole reason the importer is in this module rather than a
  // smaller one of its own: it is the only evidence a reader has that the
  // translation is faithful, and it comes from the worksheet's own stored answer.
  check(
    summary.includes("1 of 1 answer"),
    `the summary should report the checked answer; got ${JSON.stringify(summary)}`,
  );

  const rows = await browser.evaluate(`
    Array.from(document.querySelectorAll("#import-list li")).map((li) => ({
      className: li.className,
      line: li.querySelector(".import-line").textContent,
      detail: li.querySelector(".import-detail").textContent,
    }))
  `);
  check(rows.length === 1, `expected one note, got ${rows.length}`);
  check(
    rows[0]?.className === "import-unsupported",
    `the note should be marked unsupported; got ${rows[0]?.className}`,
  );
  check(
    rows[0]?.detail.includes("`range` with a step"),
    `the note should say what was refused; got ${JSON.stringify(rows[0]?.detail)}`,
  );

  // The marker has to be in the file too, not only in a panel that can be
  // dismissed. This is the importer's rule about silent loss, and the panel is
  // an addition to it rather than a replacement for it.
  check(
    source.includes("[import] unsupported"),
    "the untranslatable construct left no marker in the worksheet",
  );

  // ---- the line number goes somewhere ----------------------------------

  const noted = Number(rows[0].line.replace(/\D/g, ""));
  await browser.evaluate(
    `document.querySelectorAll("#import-list li .import-line")[0].click()`,
  );
  // Read where the cursor went from the gutter rather than from CodeMirror's
  // internals. `highlightActiveLine` marks the line the cursor is on, so the
  // active gutter number is both the thing under test and the thing the reader
  // actually sees — and nothing has to be exposed on `window` for a test's sake.
  await waitFor(
    browser,
    `document.querySelector(".cm-activeLine") !== null`,
    "the cursor to land",
  );
  const cursorLine = await browser.evaluate(`
    (() => {
      const lines = Array.from(document.querySelectorAll(".cm-line"));
      return lines.indexOf(document.querySelector(".cm-activeLine")) + 1;
    })()
  `);
  check(
    cursorLine === noted,
    `clicking the note should put the cursor on line ${noted}; it is on ${cursorLine}`,
  );

  // ---- and it can be put away ------------------------------------------

  await browser.evaluate(`document.querySelector("#import-dismiss").click()`);
  check(
    (await browser.evaluate(`document.querySelector("#import").hidden`)) === true,
    "the report should be dismissible — it describes an event, not the document",
  );

  // ---- a file that is not a worksheet ----------------------------------
  //
  // Reported, not thrown. The wrong file is a mistake a user makes, and the
  // application has to say which mistake it was.
  await browser.evaluate(`globalThis.__worksheet = "this is not a worksheet"`);
  await browser.evaluate(`document.querySelector("#open").click()`);
  await waitFor(
    browser,
    `document.querySelector("#status").className === "bad"`,
    "the failure to be reported",
  );
  const failed = await browser.evaluate(
    `document.querySelector("#status").textContent`,
  );
  check(
    failed.includes("beam.sm") && failed.includes("could not be read"),
    `a bad file should be named and explained; got ${JSON.stringify(failed)}`,
  );
  check(
    (await browser.evaluate(`document.querySelector(".cm-content").textContent`))
      .includes("M = w*L/8"),
    "a failed import must leave the worksheet that was already open alone",
  );
} catch (error) {
  failures.push(`could not drive the browser: ${error.message}`);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error("check-import: importing SMath worksheets is broken\n");
  for (const failure of failures) console.error(`  error: ${failure}`);
  process.exit(1);
}

console.log(
  "ok: an SMath worksheet imports in the browser, and every refusal is shown against its line",
);

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
