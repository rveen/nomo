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
// The paged document is held to the same rule, in both of its forms: Folio
// carried in the page (`--paged`, which reads web/vendor/folio/) and Folio
// linked (`--folio-url`). The embedded form is the stronger check, since
// 115 kB of script and its licence cross the boundary and must come back
// untouched; the linked form is what a server would actually serve.
//
// Run after `cargo build --release -p nomo-cli`, the wasm build, and
// `scripts/fetch-folio.sh`.

import { mkdtemp, readdir, readFile, copyFile, rm } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";
import { load, repoRoot } from "./wasm-host.mjs";

const fontUrl = "/.nomo/stix-two-math-subset.woff2";
const folioUrl = "/.nomo/folio.polyfill.min.js";
// The prose's face, which the paged forms carry as the editor's pages do.
// `nomo_render_html` has no text face — its ABI predates it and a server calls
// it — so the standalone form is compared without one.
const textFont = {
  upright: "/.nomo/stix-two-text-subset.woff2",
  italic: "/.nomo/stix-two-text-italic-subset.woff2",
};
const textFlags = ["--text-font-url", textFont.upright, "--text-font-italic-url", textFont.italic];
const nomo = path.join(repoRoot, "target/release/nomo");
const examplesDir = path.join(repoRoot, "examples");
const folioDir = path.join(repoRoot, "web/vendor/folio");

// What `--paged` embeds, read the way the CLI reads it, so the WebAssembly
// call is given the same bytes.
let folio;
try {
  folio = {
    script: await readFile(path.join(folioDir, "folio.polyfill.min.js"), "utf8"),
    notice: await readFile(path.join(folioDir, "LICENSE"), "utf8"),
  };
} catch {
  console.error("compare-html: no Folio in web/vendor/folio — run ./scripts/fetch-folio.sh");
  process.exit(1);
}

// Each form of the page: the CLI's flags, and the WebAssembly call that must
// return the same bytes.
const forms = [
  {
    name: "standalone",
    flags: [],
    wasm: (source, title) => engine.renderHtml(source, title, { mathml: true, fontUrl }),
  },
  {
    name: "paged, Folio embedded",
    flags: ["--paged", ...textFlags],
    carries: textFont.upright,
    wasm: (source, title, fileName) =>
      engine.renderPaged(source, title, { mathml: true, fontUrl, folio, textFont, fileName }),
  },
  {
    name: "paged, Folio linked",
    flags: ["--folio-url", folioUrl, ...textFlags],
    carries: textFont.upright,
    wasm: (source, title, fileName) =>
      engine.renderPaged(source, title, {
        mathml: true,
        fontUrl,
        folio: { url: folioUrl },
        textFont,
        fileName,
      }),
  },
];

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
    const source = await readFile(copy, "utf8");
    const title = path.basename(name, ".nomo");
    const out = copy.replace(/\.nomo$/, ".html");
    for (const form of forms) {
      // Removed first, so a run that writes nothing cannot be compared against
      // the page the previous form left behind.
      await rm(out, { force: true });
      try {
        // From the repository root, which is where `--paged` finds Folio.
        execFileSync(nomo, ["html", "--mathml", "--font-url", fontUrl, ...form.flags, copy], {
          cwd: repoRoot,
          stdio: "ignore",
        });
      } catch {
        // A worksheet with errors exits non-zero and still writes its page.
      }
      let native;
      try {
        native = await readFile(out, "utf8");
      } catch {
        problems.push(`${name} (${form.name}): \`nomo html\` wrote nothing`);
        continue;
      }
      // Agreement alone would pass two sides that both dropped what they were
      // given, so a form that names something must be seen to carry it.
      if (form.carries && !native.includes(form.carries)) {
        problems.push(`${name} (${form.name}): the page does not carry ${form.carries}`);
      } else if (form.wasm(source, title, name) === native) matched += 1;
      else problems.push(`${name} (${form.name})`);
    }
  }
} finally {
  await rm(work, { recursive: true, force: true });
}

if (problems.length > 0) {
  console.error(`compare-html: the page differs between native and WebAssembly:\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log(
  `ok: ${matched} typeset pages byte-identical between \`nomo html\` and the module ` +
    `(${forms.map((f) => f.name).join("; ")})`,
);
