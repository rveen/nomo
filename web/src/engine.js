// Load the engine in a browser.
//
// The calling convention is in `crates/nomo-wasm/boundary.mjs`, shared with the
// Node scripts so there is one implementation of the contract. Only the fetching
// differs, and only because a browser has no filesystem.

import { bind } from "../../crates/nomo-wasm/boundary.mjs";

/**
 * Fetch, compile and bind the engine.
 *
 * The import object is empty because the module declares no imports at all —
 * asserted in CI by `scripts/check-wasm.mjs`. That is what stops the engine
 * reaching for `Math.sin` and getting a different answer here than on the
 * command line.
 */
export async function loadEngine(url = "nomo_wasm.wasm") {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`could not fetch ${url}: ${response.status}`);
  }

  // `instantiateStreaming` needs the right Content-Type and not every static
  // host sets it, so fall back rather than failing over a header.
  let instance;
  try {
    ({ instance } = await WebAssembly.instantiateStreaming(response.clone(), {}));
  } catch {
    const bytes = await response.arrayBuffer();
    ({ instance } = await WebAssembly.instantiate(bytes, {}));
  }

  return bind(instance.exports);
}
