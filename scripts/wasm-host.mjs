// Load the Nomo engine as a WebAssembly module, under Node.
//
// Only the loading is here. The calling convention lives in
// `crates/nomo-wasm/boundary.mjs`, next to the Rust that defines the other half
// of it, and is shared with the browser front end so there is one implementation
// of the contract rather than two that can disagree.
//
// Deliberately dependency-free and small enough to read in one sitting: it is
// part of the evidence for the determinism claim, so it should not require
// trusting a package to believe the result.

import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { bind } from "../crates/nomo-wasm/boundary.mjs";

export const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

export const wasmPath = path.join(
  repoRoot,
  "target/wasm32-unknown-unknown/release/nomo_wasm.wasm",
);

/** Read and compile the module, with a legible error if it was never built. */
export async function compile(file = wasmPath) {
  let bytes;
  try {
    bytes = await readFile(file);
  } catch {
    throw new Error(
      `no WebAssembly build at ${path.relative(repoRoot, file)}\n` +
        "run: cargo build -p nomo-wasm --release --target wasm32-unknown-unknown",
    );
  }
  return { bytes, module: new WebAssembly.Module(bytes) };
}

/**
 * Instantiate and return a `snapshot(name, source)` function.
 *
 * The import object is empty on purpose. If the module ever grows an import,
 * instantiation fails here rather than silently binding to something the native
 * build does not have — which is the whole failure this phase exists to prevent.
 */
export async function load(file = wasmPath) {
  const { module } = await compile(file);
  return bind(new WebAssembly.Instance(module, {}).exports);
}

/** Every import the module declares. Expected to be empty. */
export function imports(module) {
  return WebAssembly.Module.imports(module);
}

/**
 * The WebAssembly features LLVM recorded in the artifact's `target_features`
 * custom section.
 *
 * This is what makes the SIMD gate authoritative rather than a guess: the module
 * states which features it was compiled against, so the check reads the artifact
 * instead of inferring from build flags that may have been overridden.
 */
export function targetFeatures(bytes) {
  for (const section of customSections(bytes)) {
    if (section.name !== "target_features") continue;
    const r = reader(bytes, section.start);
    const count = r.leb();
    const features = [];
    for (let i = 0; i < count; i += 1) {
      const prefix = String.fromCharCode(bytes[r.at()]);
      r.skip(1);
      const len = r.leb();
      features.push({ prefix, name: r.text(len) });
    }
    return features;
  }
  return null;
}

function* customSections(bytes) {
  let offset = 8; // magic + version
  while (offset < bytes.length) {
    const id = bytes[offset];
    const r = reader(bytes, offset + 1);
    const size = r.leb();
    const body = r.at();
    if (id === 0) {
      const nameLen = r.leb();
      const name = r.text(nameLen);
      yield { name, start: r.at(), end: body + size };
    }
    offset = body + size;
  }
}

function reader(bytes, start) {
  let offset = start;
  return {
    at: () => offset,
    skip: (n) => {
      offset += n;
    },
    leb() {
      let result = 0;
      let shift = 0;
      let byte;
      do {
        byte = bytes[offset++];
        result |= (byte & 0x7f) << shift;
        shift += 7;
      } while (byte & 0x80);
      return result >>> 0;
    },
    text(len) {
      const slice = bytes.subarray(offset, offset + len);
      offset += len;
      return Buffer.from(slice).toString("utf8");
    },
  };
}
