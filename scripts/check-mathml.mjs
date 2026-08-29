// Check that typeset output is actually typeset, in a real browser.
//
// The engine can be tested on the markup it emits, and unit tests do that. What
// they cannot tell is whether a browser *lays it out* — and that is the whole
// question here, because a browser without MathML does not fail: it draws the
// numerator and the denominator side by side, in order, as though the markup
// were a paragraph. The worksheet then reads as `w · L 2 8`, which is worse
// than the linear text it replaced and would pass every check that looked only
// at the HTML.
//
// So this asks the page where things ended up. A fraction has its numerator
// above its denominator and is taller than a plain letter; a radical is wider
// than what it contains. Those are the two facts that distinguish typesetting
// from a run of characters.
//
// Chrome only, because this machine has one browser. Firefox and Safari
// implement MathML Core and are not checked here, which is a gap in the
// evidence rather than a claim about them.

import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { execFile } from "node:child_process";
import { join } from "node:path";
import { promisify } from "node:util";
import { launch } from "./chrome.mjs";
import { repoRoot } from "./wasm-host.mjs";

const run = promisify(execFile);

const WORKSHEET = `' # Typeset
w = 2.5 kN/m
L = 6 m
M = w*L^2/8
r = sqrt(M/(2 MPa))
`;

const dir = await mkdtemp(join(tmpdir(), "nomo-mathml-"));
const source = join(dir, "typeset.nomo");
await writeFile(source, WORKSHEET);

// Rendered by the shipped binary rather than by a library call, so that what is
// checked is what `nomo html --mathml` actually writes.
await run("cargo", ["run", "--quiet", "-p", "nomo-cli", "--", "html", "--mathml", source], {
  cwd: repoRoot,
});
const page = await readFile(join(dir, "typeset.html"), "utf8");

const server = createServer((_, response) => {
  response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  response.end(page);
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

  const geometry = JSON.parse(
    await browser.evaluate(`
      (() => {
        const frac = document.querySelector("mfrac");
        const sqrt = document.querySelector("msqrt");
        const letter = document.querySelector("mi");
        const box = (el) => {
          if (!el) return null;
          const r = el.getBoundingClientRect();
          return { top: r.top, bottom: r.bottom, left: r.left, right: r.right,
                   width: r.width, height: r.height };
        };
        const parts = frac ? [...frac.children].map(box) : [];
        return JSON.stringify({
          frac: box(frac),
          numerator: parts[0] ?? null,
          denominator: parts[1] ?? null,
          sqrt: box(sqrt),
          sqrtInner: sqrt ? box(sqrt.firstElementChild) : null,
          letter: box(letter),
          mathCount: document.querySelectorAll("math").length,
        });
      })()
    `),
  );

  check(geometry.mathCount > 0, "the page carries no <math> at all");
  check(geometry.frac !== null, "no fraction was rendered");

  if (geometry.numerator && geometry.denominator) {
    // The fact that says "typeset" rather than "a run of characters": the
    // numerator sits entirely above the denominator.
    check(
      geometry.numerator.bottom <= geometry.denominator.top + 0.5,
      `a fraction should stack: numerator bottom ${geometry.numerator.bottom} ` +
        `is not above denominator top ${geometry.denominator.top} — ` +
        "this is what a browser without MathML does",
    );
  }

  if (geometry.frac && geometry.letter) {
    check(
      geometry.frac.height > geometry.letter.height * 1.5,
      `a fraction should be taller than a letter: ${geometry.frac.height} vs ${geometry.letter.height}`,
    );
  }

  if (geometry.sqrt && geometry.sqrtInner) {
    // The radical sign and its overbar take room the contents do not.
    check(
      geometry.sqrt.width > geometry.sqrtInner.width,
      `a radical should be wider than what it encloses: ${geometry.sqrt.width} vs ${geometry.sqrtInner.width}`,
    );
  }
} catch (error) {
  failures.push(`could not drive the browser: ${error.message}`);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error("check-mathml: typeset output did not lay out as mathematics\n");
  for (const failure of failures) console.error(`  error: ${failure}\n`);
  process.exit(1);
}

console.log("ok: fractions stack and radicals enclose, in Chrome");
