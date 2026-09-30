// A worksheet rendered as a page, native against WebAssembly.
//
// `nomo_render_html` exists for a host that serves stored worksheets — gserver's
// plugin runs the module under wazero and answers a request for `beam.nomo`
// with the page. That page has to be the one `nomo html --mathml` writes, byte
// for byte: `compare-targets.mjs` proves the *calculation* agrees across
// targets, and this proves the wrapping around it adds nothing that differs.
// It is separate because no golden snapshot covers the document chrome — the
// stylesheet, the title, the `@font-face` rule — which is exactly what the new
// entry point is responsible for.
//
// Run after `cargo build --release -p nomo-cli` and the wasm build.

import { mkdtemp, readdir, readFile, copyFile, rm } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";
import { load, repoRoot } from "./wasm-host.mjs";

const fontUrl = "/.nomo/stix-two-math-subset.woff2";
const nomo = path.join(repoRoot, "target/release/nomo");
const examplesDir = path.join(repoRoot, "examples");

const engine = await load();
const work = await mkdtemp(path.join(tmpdir(), "nomo-html-"));
const problems = [];
let matched = 0;

try {
  const names = (await readdir(examplesDir)).filter((n) => n.endsWith(".nomo")).sort();
  for (const name of names) {
    // `nomo html` writes beside its input, so it is given a copy to write beside.
    const copy = path.join(work, name);
    await copyFile(path.join(examplesDir, name), copy);
    try {
      execFileSync(nomo, ["html", "--mathml", "--font-url", fontUrl, copy], { stdio: "ignore" });
    } catch {
      // A worksheet with errors exits non-zero and still writes its page.
    }
    const native = await readFile(copy.replace(/\.nomo$/, ".html"), "utf8");
    const source = await readFile(copy, "utf8");
    const wasm = engine.renderHtml(source, path.basename(name, ".nomo"), { mathml: true, fontUrl });
    if (wasm === native) matched += 1;
    else problems.push(name);
  }
} finally {
  await rm(work, { recursive: true, force: true });
}

if (problems.length > 0) {
  console.error(`compare-html: the page differs between native and WebAssembly:\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log(`ok: ${matched} typeset pages byte-identical between \`nomo html\` and nomo_render_html`);
