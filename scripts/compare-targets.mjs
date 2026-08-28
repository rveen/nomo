// The test of the numeric thesis.
//
// Render every worksheet under `examples/` through the WebAssembly build and
// require the result to be byte-identical to the snapshot in `tests/golden/`,
// which the native build produced and `nomo test` independently verifies.
// Native == golden and WASM == golden together mean native == WASM, byte for
// byte, which is the claim design note §3 makes.
//
// If §3 is right this passes trivially. If it fails, something reached the host:
// a transcendental, a fused multiply-add, or a NaN whose bits crossed a boundary
// unnormalised. There is no tolerance here for the same reason there is none in
// `nomo test` — a difference is the bug, not noise around it.

import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { load, repoRoot } from "./wasm-host.mjs";

const examplesDir = path.join(repoRoot, "examples");
const goldenDir = path.join(repoRoot, "tests/golden");

const engine = await load();

const worksheets = (await walk(examplesDir)).sort();
if (worksheets.length === 0) {
  console.error(`compare-targets: no .nomo files under ${examplesDir}`);
  process.exit(1);
}

let matched = 0;
const problems = [];

for (const file of worksheets) {
  const name = path.basename(file, ".nomo");
  const relative = path.relative(examplesDir, file);
  const goldenFile = path.join(goldenDir, relative.replace(/\.nomo$/, ".snap"));

  const source = await readFile(file, "utf8");
  const fromWasm = engine.snapshot(name, source);

  let expected;
  try {
    expected = await readFile(goldenFile, "utf8");
  } catch {
    problems.push(
      `${path.relative(repoRoot, goldenFile)}: missing; run \`cargo run -p nomo-cli -- test --write\` first`,
    );
    continue;
  }

  if (fromWasm === expected) {
    matched += 1;
    continue;
  }
  problems.push(
    `${relative}: WebAssembly output differs from the native snapshot\n` +
      firstDifference(expected, fromWasm),
  );
}

if (problems.length > 0) {
  console.error("compare-targets: native and WebAssembly disagree\n");
  for (const problem of problems) console.error(`  ${problem}\n`);
  console.error(
    "This is the failure the phase exists to detect. Something in the engine\n" +
      "behaved differently under WebAssembly: a host maths call, a fused\n" +
      "multiply-add, or an unnormalised NaN. It is not a tolerance problem.",
  );
  process.exit(1);
}

console.log(
  `ok: ${matched} worksheets byte-identical between native and WebAssembly ` +
    `(snapshot format v${engine.format()})`,
);

/** The first differing line, with its number, so a diff is readable at a glance. */
function firstDifference(expected, actual) {
  const a = expected.split("\n");
  const b = actual.split("\n");
  for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
    if (a[i] !== b[i]) {
      return (
        `    line ${i + 1}:\n` +
        `      native: ${a[i] ?? "<end of file>"}\n` +
        `      wasm:   ${b[i] ?? "<end of file>"}`
      );
    }
  }
  return "    (identical line by line; they differ in trailing bytes)";
}

async function walk(dir) {
  const found = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) found.push(...(await walk(full)));
    else if (entry.name.endsWith(".nomo")) found.push(full);
  }
  return found;
}
