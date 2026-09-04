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
// It also checks the font, which is half of the layout. MathML Core reads the
// fraction bar thickness, the axis height and the stretchy bracket recipes from
// an OpenType MATH table, so a page that names fonts and finds none of them
// installed gets a fraction laid out from ordinary text metrics. The document
// is therefore rendered with `--embed-font`, and the page is asked whether that
// font actually loaded — which also exercises the embed path end to end.
//
// And with a font guaranteed present, the italic can finally be checked. §8.47
// leaves the italic and upright entirely to MathML Core: a one-character `<mi>`
// is remapped into the Mathematical Alphanumeric Symbols block, a longer one is
// not. That claim was measured by hand when it was written but not gated,
// because the measurement depended on the machine having a math font. It does
// not any more.
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
const font = join(repoRoot, "web/dist/fonts/stix-two-math-subset.woff2");
try {
  await readFile(font);
} catch {
  console.error(
    "error: no math font at web/dist/fonts — see scripts/build-web.sh\n" +
      "  ./scripts/fetch-font.sh && (cd web && node build.mjs)",
  );
  process.exit(1);
}
await run(
  "cargo",
  [
    "run",
    "--quiet",
    "-p",
    "nomo-cli",
    "--",
    "html",
    "--mathml",
    "--embed-font",
    font,
    source,
  ],
  { cwd: repoRoot },
);
const rendered = await readFile(join(dir, "typeset.html"), "utf8");

// A probe appended to the real document, so it inherits the same stylesheet and
// the same embedded font. The same letter twice: once as a bare `<mi>`, which
// MathML Core remaps to U+1D449 MATHEMATICAL ITALIC CAPITAL V, and once marked
// upright, which it leaves alone. If the remapping is not happening the two set
// to exactly the same width, which is the failure this catches — and it is the
// failure a page would have if the font that carries U+1D449 never loaded.
const PROBE = `<div id="probe" style="font-size: 40px">
<math display="inline"><mi id="italic">V</mi></math>
<math display="inline"><mi id="upright" mathvariant="normal">V</mi></math>
</div>
`;
const page = rendered.replace("</body>", `${PROBE}</body>`);

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

  // Layout is measured after the fonts settle: `font-display: swap` means the
  // first paint can be in a fallback face, and a width measured then is a width
  // from the wrong font.
  await browser.evaluate("document.fonts.ready.then(() => true)");

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
          italic: box(document.getElementById("italic")),
          upright: box(document.getElementById("upright")),
          // The gap a unit is set off from its number by. The renderer asks
          // for it with rspace on the invisible-times operator; whether a
          // browser honours that is a different question from whether the
          // attribute is in the markup, and only this can answer it.
          // (No backticks in here: this is inside a template literal.)
          unitGap: (() => {
            const op = document.querySelector("math mo[rspace]");
            if (!op) return null;
            const before = op.previousElementSibling;
            const after = op.nextElementSibling;
            if (!before || !after) return null;
            return after.getBoundingClientRect().left -
                   before.getBoundingClientRect().right;
          })(),
          // Whether the embedded face is loaded, not merely named. A stack
          // that resolves to nothing reports the same family string as one
          // that resolves to the shipped font.
          fontLoaded: [...document.fonts].some(
            (f) => f.family === "STIX Two Math Subset" && f.status === "loaded",
          ),
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
  check(
    geometry.fontLoaded,
    "the embedded math font did not load — a fraction is then laid out from " +
      "ordinary text metrics, which is what shipping a font is meant to end",
  );

  // ISO 80000-1 §7.1.3 wants a space between a numerical value and its unit,
  // and U+2062 INVISIBLE TIMES is exactly zero wide, so the renderer asks for
  // one explicitly. A browser that ignored `rspace` would set `6m` and every
  // assertion about the markup would still pass.
  check(
    geometry.unitGap !== null,
    "no spaced juxtaposition on the page — this check is measuring nothing",
  );
  if (geometry.unitGap !== null) {
    check(
      geometry.unitGap > 0.5,
      `a unit should be set off from its number, but the gap is ` +
        `${geometry.unitGap} — the browser is ignoring rspace, so this reads "6m"`,
    );
  }

  if (geometry.italic && geometry.upright) {
    check(
      Math.abs(geometry.italic.width - geometry.upright.width) > 0.5,
      `a one-character <mi> should be italicised and an upright one not, but ` +
        `both set to ${geometry.italic.width} — MathML Core's math-auto is not ` +
        "remapping, so every variable on the page is upright",
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

console.log(
  "ok: fractions stack, radicals enclose, units stand off their numbers, the " +
    "embedded font loads and variables are italic, in Chrome",
);
