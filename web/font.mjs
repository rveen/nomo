// Subset the math font that `web/dist/` ships.
//
// # Why a font is shipped at all
//
// MathML Core reads the fraction bar thickness, the axis height, the script
// shifts and the stretchy bracket recipes from a font's OpenType MATH table.
// A font without one leaves the browser guessing all of them from ordinary text
// metrics, which is the worst case for a fraction. Until now the stylesheets
// *named* such fonts — Latin Modern Math, STIX Two Math, Cambria Math — and a
// reader whose machine had none got the guess. Shipping one ends the question,
// and the named stack stays behind it as the fallback it always was.
//
// # Why a subset, and why this subset
//
// The upstream face is 552 kB; what dist ships is 162 kB, and the difference is
// almost entirely the Mathematical Alphanumeric Symbols block. That block
// cannot simply be dropped: MathML Core italicises a one-character `<mi>` by
// `text-transform: math-auto`, which *remaps* the character into that block
// rather than slanting it, so a subset without it loses every italic variable.
//
// But `math-auto` produces only the **italic** alphabets, and `render/mathml.rs`
// emits no `mathvariant` other than `normal`. The bold, script, fraktur,
// double-struck, sans and monospace alphabets in that block are therefore
// unreachable by anything this renderer can write, and they are what the saving
// is made of. If the renderer ever emits another `mathvariant`, this list is
// what has to grow with it.
//
// The stretchy pieces — the tall brackets a matrix needs, the radical that grows
// with what is under it — are *not* enumerated here and must not be. They are
// reached through the MATH table's variant records rather than through a
// character code, and hb-subset follows that closure from the base glyphs: 45
// vertical variants survive a subset that names none of them.
//
// # Why hb-subset and not fontTools
//
// `subset-font` is the harfbuzz subsetter compiled to WebAssembly, so it is an
// npm dependency of a build that already has node and esbuild, rather than a
// third toolchain. It is pinned in package-lock.json by integrity hash, which is
// what makes the output reproducible: the same input font and the same codepoint
// list give the same bytes.

import { readFile, mkdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import subsetFont from "subset-font";

const here = dirname(fileURLToPath(import.meta.url));

/**
 * The name the subset ships under, in the file system and in CSS.
 *
 * Not "STIX Two Math". A subset is a Modified Version under the OFL, and while
 * the licence's Reserved Font Name is "TM Math" rather than "STIX" — so keeping
 * the original name would be permitted — STIX Fonts is a trademark of the IEEE
 * and this file is not what that name refers to. Naming it for what it is
 * settles both, and it means a reader whose machine has the *full* STIX Two Math
 * keeps it as the next entry in the stack for anything outside the subset.
 */
export const FONT_FILE = "stix-two-math-subset.woff2";
export const FONT_FAMILY = "STIX Two Math Subset";

/**
 * Every code point the renderer can put inside a `<math>` element.
 *
 * Whole blocks rather than a hand-picked list wherever a block is small, because
 * a hand-picked list is a thing that is quietly wrong for one worksheet in a
 * hundred: the failure is not a crash but a single character in a different
 * design in the middle of a formula.
 */
const RANGES = [
  [0x0020, 0x007e], // ASCII: digits, names, operators, brackets
  [0x00a0, 0x00ff], // ° µ · × ÷ and the superscript digits ¹ ² ³
  [0x0100, 0x017f], // Latin Extended-A, for an accented name
  [0x0370, 0x03ff], // Greek and Coptic, whole block — §8.47's table lives here
  [0x2000, 0x206f], // punctuation: the invisible operators U+2061–U+2064
  [0x2070, 0x209f], // superscripts and subscripts
  [0x2100, 0x214f], // letterlike: ℃ ℉ Ω ℓ
  [0x2200, 0x22ff], // mathematical operators: √ ∞ ≤ ≥ ≠ ∑ ∫
  [0x2308, 0x230b], // ceiling and floor
  // Mathematical Alphanumeric Symbols, italic only — see the note above.
  [0x1d434, 0x1d467], // italic Latin
  [0x1d6a4, 0x1d6a5], // dotless i and j, which math-auto also produces
  [0x1d6e2, 0x1d71b], // italic Greek
];

const text = RANGES.map(([a, b]) => {
  let run = "";
  for (let c = a; c <= b; c += 1) run += String.fromCodePoint(c);
  return run;
}).join("");

/**
 * Subset `web/vendor/` into `<dist>/fonts/`, and carry the licence with it.
 *
 * The OFL requires the licence to travel with the font, so `OFL.txt` is copied
 * beside it rather than merely referenced from NOTICE. A build that shipped the
 * glyphs and left the terms behind would be redistributing the font without
 * them.
 */
export async function buildFont(dist) {
  const vendor = join(here, "vendor");
  let source;
  try {
    source = await readFile(join(vendor, "STIXTwoMath-Regular.woff2"));
  } catch {
    console.error(
      "error: no math font in web/vendor/\n" +
        "  fetch it first:\n" +
        "    ./scripts/fetch-font.sh",
    );
    process.exit(1);
  }
  const subset = await subsetFont(source, text, { targetFormat: "woff2" });
  const fonts = join(dist, "fonts");
  await mkdir(fonts, { recursive: true });
  await writeFile(join(fonts, FONT_FILE), subset);
  await writeFile(
    join(fonts, "OFL.txt"),
    await readFile(join(vendor, "OFL.txt")),
  );
  return subset.length;
}
