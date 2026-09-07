// Prove that an SMath worksheet imports identically natively and under WebAssembly.
//
// The importer became part of the browser build, which means there are now two
// of it: the one `smath-import` runs and the one in nomo_wasm.wasm. They are the
// same source compiled twice, and the whole reason `check-no-host-math.sh` was
// extended to cover `nomo-smath` is that "the same source" is not by itself
// enough — a host `log10` would have decided the emitted decimals on two
// different libms and produced two different translations of one worksheet.
//
// This is the check that would have caught that. Both sides call
// `nomo_smath::import_json`, so nothing but the target differs, and the
// comparison is on the exact bytes of the payload: the emitted source, every
// note, and every answer checked against what SMath stored.
//
// There is no tolerance here, for the same reason there is none in
// `compare-targets.mjs`. A difference is the bug.
//
// The corpora are fetched rather than committed (see THIRD-PARTY.md), so the
// fixture below always runs and the corpora are compared when they are present.

import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { load, repoRoot } from "./wasm-host.mjs";

// The project's own bytes, and the same worksheet `check-import.mjs` uses: one
// stored answer to check against, and one construct the design note refuses to
// guess at. A cross-target check that only ran where the corpora happen to be
// downloaded would mostly not run.
const FIXTURE = `<?xml version="1.0" encoding="utf-8" standalone="yes"?>
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

const engine = await load();

// The built binary rather than `cargo run` per worksheet: the corpora are 114
// files and re-resolving the workspace for each one dominates the runtime.
const nativeImporter = path.join(repoRoot, "target/release/smath-import");
if (!existsSync(nativeImporter)) {
  console.error(
    "error: the native importer is not built\n" +
      "    cargo build --release -p nomo-smath --bin smath-import",
  );
  process.exit(1);
}

const scratch = mkdtempSync(path.join(tmpdir(), "nomo-import-"));
const fixture = path.join(scratch, "fixture.sm");
writeFileSync(fixture, FIXTURE);

const worksheets = [fixture, ...(await corpusWorksheets())];

let matched = 0;
const problems = [];

for (const file of worksheets) {
  const name = path.relative(repoRoot, file);
  const bytes = readFileSync(file);

  let native;
  try {
    // `--json` rather than the plain output: it carries the notes and the
    // checked answers as well as the source, so the comparison covers the
    // arithmetic the oracle did and not only the text the emitter wrote.
    native = execFileSync(nativeImporter, ["--json", file], {
      cwd: repoRoot,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    }).trimEnd();
  } catch (error) {
    problems.push(`${name}: the native importer failed: ${error.message}`);
    continue;
  }

  let fromWasm;
  try {
    fromWasm = engine.importSmath(bytes);
  } catch (error) {
    problems.push(`${name}: the WebAssembly importer failed: ${error.message}`);
    continue;
  }

  // Both sides go through the same JS serialiser before being compared, which
  // is not a weakening of the comparison — it is what makes it about the
  // importer. Rust writes a whole f64 as `4500.0` and JavaScript writes it as
  // `4500`, so comparing the native *text* against a re-serialised payload
  // would report every worksheet as a difference and hide any real one among
  // them. `JSON.parse` preserves key order and parses each number to the same
  // double, so what is compared is every string exactly and every number bit
  // for bit.
  let asNative;
  try {
    asNative = JSON.parse(native);
  } catch (error) {
    problems.push(`${name}: the native importer wrote unparseable JSON: ${error.message}`);
    continue;
  }

  if (JSON.stringify(asNative) === JSON.stringify(fromWasm)) {
    matched += 1;
    continue;
  }

  problems.push(
    `${name}: the WebAssembly import differs from the native one\n` +
      firstDifference(asNative, fromWasm),
  );
}

if (problems.length > 0) {
  console.error("compare-import: native and WebAssembly translate differently\n");
  for (const problem of problems) console.error(`  ${problem}\n`);
  console.error(
    "One worksheet must not have two translations. Something in the importer\n" +
      "behaved differently under WebAssembly — a host maths call is the usual\n" +
      "cause, and scripts/check-no-host-math.sh covers nomo-smath for this reason.",
  );
  process.exit(1);
}

console.log(
  `ok: ${matched} SMath worksheets import identically between native and WebAssembly` +
    (worksheets.length === 1 ? " (corpora absent; fixture only)" : ""),
);

/** Where the two reports first disagree, in the terms the report is written in. */
function firstDifference(native, wasm) {
  if (native.source !== wasm.source) {
    const a = (native.source ?? "").split("\n");
    const b = (wasm.source ?? "").split("\n");
    for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
      if (a[i] !== b[i]) {
        return (
          `    emitted source, line ${i + 1}:\n` +
          `      native: ${a[i] ?? "<end of file>"}\n` +
          `      wasm:   ${b[i] ?? "<end of file>"}`
        );
      }
    }
  }
  for (const field of ["notes", "checks"]) {
    const a = native[field] ?? [];
    const b = wasm[field] ?? [];
    for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
      const one = JSON.stringify(a[i]);
      const other = JSON.stringify(b[i]);
      if (one !== other) {
        return (
          `    ${field}[${i}]:\n` +
          `      native: ${one ?? "<missing>"}\n` +
          `      wasm:   ${other ?? "<missing>"}`
        );
      }
    }
  }
  return "    (the reports agree field by field; they differ in key order)";
}

/** Every `.sm` under `corpora/`, or none when the corpora were never fetched. */
async function corpusWorksheets() {
  const root = path.join(repoRoot, "corpora");
  try {
    return (await walk(root)).sort();
  } catch {
    return [];
  }
}

async function walk(dir) {
  const found = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) found.push(...(await walk(full)));
    else if (entry.name.toLowerCase().endsWith(".sm")) found.push(full);
  }
  return found;
}
