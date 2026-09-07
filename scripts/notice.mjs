// Write the licence notices that have to travel with a built artifact.
//
//   node scripts/notice.mjs --cli <dir>   the notice for the nomo binary
//
// `web/build.mjs` imports `buildNotice` from here for `web/dist/`.
//
// # Why this exists
//
// What this repository releases — `web/dist/`, and the `nomo` tarball — are
// redistributable things. Everything in them permits that: Nomo's own code and
// the CodeMirror packages are MIT, the engine's one crate is MIT, and the fonts
// are OFL 1.1. But MIT permits it *on a condition* — "the above copyright
// notice and this permission notice shall be included in all copies or
// substantial portions of the Software" — and until this file existed, only the
// fonts met the equivalent condition.
//
// The fonts were already right, and for the reason stated in `web/font.mjs`:
// the OFL says the licence must travel with the bytes, so `OFL.txt` is copied
// beside them "rather than merely referenced from NOTICE". MIT says the same
// thing about code. `bundle.js` is a minified concatenation of twelve MIT
// packages carrying no copyright line at all — none of them writes an
// `@license` comment, so esbuild's `legalComments` has nothing to preserve and
// faithfully preserves it — and the release tarball was the binary alone, with
// neither Nomo's licence nor libm's beside it.
//
// # Why it is generated rather than written
//
// The repository's own NOTICE named five CodeMirror packages. The bundle
// actually contains twelve: `@codemirror/autocomplete` and `@lezer/highlight`
// arrived with features, and `crelt`, `style-mod`, `w3c-keyname` and
// `@marijn/find-cluster-break` are transitive and were never going to be
// noticed by hand. A hand-kept list of what a bundler pulled in is a list that
// is quietly wrong, and being wrong here is a licence breach rather than a
// cosmetic staleness — so the list comes from the bundler's own metafile and
// the crate graph, and the licence texts are read verbatim from the packages
// themselves. Nothing here restates a licence from memory.
//
// This is the same choice `build-gallery.sh` makes about worksheet titles, for
// the same reason, with more riding on it.
//
// # Why one module and two artifacts
//
// The two notices share the crate walk, and a crate walk written twice is the
// thing `builtins_match_the_dispatch` exists to catch elsewhere: two lists that
// agree until the day they do not. The npm half is the browser build's alone,
// the crate half is used by both, and neither is duplicated.

import { execFile } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const run = promisify(execFile);

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..");

export const NOTICE_FILE = "NOTICE.txt";

const RULE = "-".repeat(78);

/** The file names a package might keep its licence text under. */
const LICENSE_NAMES = [
  "LICENSE",
  "LICENSE.txt",
  "LICENSE.md",
  "LICENCE",
  "LICENCE.txt",
  "LICENSE-MIT",
];

/**
 * The licence text a package ships, verbatim.
 *
 * A package that permits redistribution only on the condition that its notice
 * travels, and then ships no notice to travel, is a case this build cannot
 * paper over: it throws rather than emit an attribution it made up.
 */
async function licenseText(dir, what) {
  for (const name of LICENSE_NAMES) {
    try {
      return (await readFile(join(dir, name), "utf8")).trim();
    } catch {
      // Try the next spelling.
    }
  }
  throw new Error(
    `no licence file in ${dir} for ${what}\n` +
      "  Its terms have to ship with the artifact, and this build will not " +
      "invent them.",
  );
}

/**
 * Every npm package whose code esbuild put into the output, from its metafile.
 *
 * The metafile lists the actual inputs of the actual build, so this is what was
 * linked rather than what package.json asks for — devDependencies that never
 * reach the bundle stay out, and transitive packages that do reach it come in.
 */
async function bundledPackages(metafiles) {
  const names = new Set();
  for (const metafile of metafiles) {
    for (const input of Object.keys(metafile.inputs)) {
      const match = input.match(/node_modules\/((?:@[^/]+\/)?[^/]+)/);
      if (match) names.add(match[1]);
    }
  }

  const packages = [];
  for (const name of [...names].sort()) {
    const dir = join(repoRoot, "web/node_modules", name);
    const manifest = JSON.parse(
      await readFile(join(dir, "package.json"), "utf8"),
    );
    packages.push({
      name,
      version: manifest.version,
      license: manifest.license ?? "(unstated)",
      text: await licenseText(dir, name),
    });
  }
  return packages;
}

/**
 * Every crate compiled into a binary, from cargo's own resolve graph.
 *
 * Walked from `root` across normal dependencies only, so a dev-dependency
 * somewhere in the workspace cannot be attributed as though it shipped. That
 * filter is what separates the two artifacts honestly: `roxmltree` is a real
 * dependency of `nomo-smath`, whose binaries are not released, and it must not
 * appear in a notice for a binary that does not contain it.
 *
 * Today both artifacts resolve to exactly one crate — libm — which is the whole
 * of the engine's third-party surface and the reason `check-no-host-math.sh`
 * has anything to check.
 */
async function crates(root, target) {
  let metadata;
  try {
    const args = ["metadata", "--format-version", "1"];
    if (target) args.push("--filter-platform", target);
    const { stdout } = await run("cargo", args, {
      cwd: repoRoot,
      maxBuffer: 64 * 1024 * 1024,
    });
    metadata = JSON.parse(stdout);
  } catch (error) {
    throw new Error(
      `could not read ${root}'s dependencies: ${error.message}\n` +
        "  cargo is needed here to attribute what is compiled into the binary.",
    );
  }

  const byId = new Map(metadata.packages.map((p) => [p.id, p]));
  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
  const rootPackage = metadata.packages.find((p) => p.name === root);
  if (!rootPackage) throw new Error(`no ${root} package in the cargo metadata`);

  const seen = new Set();
  const stack = [rootPackage.id];
  while (stack.length > 0) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      // `kind: null` is a normal dependency; "dev" and "build" are not shipped.
      if (dep.dep_kinds.some((k) => k.kind === null)) stack.push(dep.pkg);
    }
  }

  const found = [];
  for (const id of seen) {
    const p = byId.get(id);
    // A crate with no source is one of this repository's own, covered by LICENSE.
    if (!p.source) continue;
    found.push({
      name: p.name,
      version: p.version,
      license: p.license ?? "(unstated)",
      text: await licenseText(
        dirname(p.manifest_path),
        `${p.name} ${p.version}`,
      ),
    });
  }
  return found.sort((a, b) => a.name.localeCompare(b.name));
}

function section({ name, version, license, text }) {
  return `${name} ${version} — ${license}\n\n${text}\n`;
}

const from = (sources) =>
  `Generated by scripts/notice.mjs from ${sources}, so it lists what was\n` +
  "actually linked rather than what was remembered.";

/**
 * Write `<dist>/NOTICE.txt` for the browser build.
 *
 * `metafiles` are esbuild metafiles for everything JavaScript that dist ships —
 * the bundle and the service worker — so a third-party package entering either
 * one is attributed without anybody remembering to add it here.
 */
export async function buildNotice(dist, metafiles) {
  const own = (await readFile(join(repoRoot, "LICENSE"), "utf8")).trim();
  const packages = await bundledPackages(metafiles);
  const linked = await crates("nomo-wasm", "wasm32-unknown-unknown");

  const parts = [
    "Nomo — licences for everything in this directory",
    "",
    "This directory is the whole application: index.html, style.css, bundle.js,",
    "nomo_wasm.wasm, sw.js, the fonts under fonts/ and the worked examples under",
    "examples/. It is redistributable, and this file is part of what makes it so —",
    "the MIT licences below require their notices to travel with the code, exactly",
    "as the OFL requires fonts/OFL.txt to travel with the fonts.",
    "",
    from("the bundler's metafile and cargo's\nresolve graph"),
    "",
    RULE,
    "Nomo itself — index.html, style.css, sw.js, the source of bundle.js, the",
    "engine in nomo_wasm.wasm, and the worksheets under examples/.",
    "",
    own,
    "",
    RULE,
    "Bundled into bundle.js. Each is used unmodified under its own licence.",
    "",
    packages.map(section).join(`\n${RULE}\n`),
    RULE,
    "Compiled into nomo_wasm.wasm.",
    "",
    linked.map(section).join(`\n${RULE}\n`),
    RULE,
    "The fonts under fonts/ are subsets of STIX Two Math and STIX Two Text,",
    "renamed because a subset is a Modified Version under their licence and",
    "because STIX Fonts is a trademark of the IEEE. The Reserved Font Name",
    '"TM Math" is not used. Their licence is fonts/OFL.txt, beside them.',
    "",
    "  Copyright 2001-2021 The STIX Fonts Project Authors",
    '  (https://github.com/stipub/stixfonts), with Reserved Font Name "TM Math".',
    "  SIL Open Font License 1.1 — https://scripts.sil.org/OFL",
    "",
    RULE,
    "esbuild and subset-font build this directory and are not part of it. The",
    "SMath worksheet corpora the importer is measured against are not here and",
    "are not redistributed; see THIRD-PARTY.md in the source repository.",
    "",
  ];

  const text = parts.join("\n");
  await writeFile(join(dist, NOTICE_FILE), text);
  return { packages: packages.length, crates: linked.length, bytes: text.length };
}

/**
 * The notice for a released binary: Nomo's own terms, then every crate in it.
 *
 * The two released binaries differ only in which graph they resolve and what
 * they are called, so they share this. `title` and `holds` are the two
 * sentences that would be wrong if one were used to describe the other.
 */
async function binaryNotice({ title, holds, root, target, compiledInto }) {
  const own = (await readFile(join(repoRoot, "LICENSE"), "utf8")).trim();
  const linked = await crates(root, target);

  const text = [
    title,
    "",
    ...holds,
    "",
    from("cargo's resolve graph"),
    "",
    "The SMath importer's crates are deliberately absent. roxmltree belongs to",
    "nomo-smath, whose binaries are not released, and attributing it here would",
    "describe a binary other than this one.",
    "",
    RULE,
    "Nomo itself.",
    "",
    own,
    "",
    RULE,
    compiledInto,
    "",
    linked.map(section).join(`\n${RULE}\n`),
  ].join("\n");

  return { text, own, crates: linked.length };
}

/**
 * Write `<dir>/LICENSE` and `<dir>/NOTICE.txt` for the command-line tool.
 *
 * Both files, not just the notice: LICENSE is Nomo's own permission notice, and
 * a tarball carrying an attribution for its dependency but not its own terms
 * tells the person who downloaded it less than nothing about what they may do
 * with it.
 */
export async function buildCliNotice(dir) {
  const { text, own, crates: n } = await binaryNotice({
    title: "Nomo — licences for the nomo command-line tool",
    holds: [
      "This archive holds the `nomo` binary, Nomo's own licence in LICENSE, and this",
      "file. It is redistributable, and this file is part of what makes it so: the",
      "MIT licence below requires its notice to travel with the code, and a compiled",
      "binary is a copy of the code.",
    ],
    root: "nomo-cli",
    target: null,
    compiledInto: "Compiled into the nomo binary.",
  });

  await writeFile(join(dir, NOTICE_FILE), text);
  await writeFile(join(dir, "LICENSE"), `${own}\n`);
  return { crates: n, bytes: text.length };
}

/**
 * Write the notice for the standalone WebAssembly module, to `file`.
 *
 * One file rather than a directory and two, because the asset it accompanies is
 * one file: the module is published on its own so that the determinism claim
 * can be checked against the exact bytes. Nomo's whole licence text is inside
 * this notice for that reason — there is no archive to put a LICENSE in, so it
 * has to be here or nowhere.
 */
export async function buildWasmNotice(file) {
  const { text, crates: n } = await binaryNotice({
    title: "Nomo — licences for the nomo_wasm WebAssembly module",
    holds: [
      "This file accompanies nomo_wasm.wasm, the Nomo engine as it runs in a browser.",
      "The module is redistributable, and this notice is what its licences require to",
      "travel with it: a compiled module is a copy of the code it was built from.",
      "",
      "The module is the engine alone. It carries no interface, no fonts and none of",
      "the npm packages web/dist/ bundles — those are covered by the NOTICE.txt in",
      "that directory.",
    ],
    root: "nomo-wasm",
    target: "wasm32-unknown-unknown",
    compiledInto: "Compiled into nomo_wasm.wasm.",
  });

  await writeFile(file, text);
  return { crates: n, bytes: text.length };
}

// Run directly: the packaging steps for the two released binaries, neither of
// which has a JavaScript build to hang this off.
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [flag, where] = process.argv.slice(2);
  const usage =
    "usage: node scripts/notice.mjs --cli <dir>\n" +
    "       node scripts/notice.mjs --wasm <file>";
  if (!where || (flag !== "--cli" && flag !== "--wasm")) {
    console.error(usage);
    process.exit(2);
  }
  if (flag === "--cli") {
    const { crates: n, bytes } = await buildCliNotice(where);
    console.log(
      `${where}/${NOTICE_FILE}: ${n} crate${n === 1 ? "" : "s"}, ` +
        `${bytes} bytes, beside LICENSE`,
    );
  } else {
    const { crates: n, bytes } = await buildWasmNotice(where);
    console.log(`${where}: ${n} crate${n === 1 ? "" : "s"}, ${bytes} bytes`);
  }
}
