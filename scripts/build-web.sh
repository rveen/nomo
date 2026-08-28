#!/usr/bin/env bash
# Build the browser application into web/dist/, then check it in a real browser.
#
# Two build systems meet here, and the split between them is deliberate:
#
#   cargo   builds the engine. Nothing else touches it.
#   esbuild bundles the user interface.
#
# The WebAssembly artifact whose reproducibility this project rests on is
# produced by cargo alone, so no JavaScript tool is anywhere in that path. That
# is the difference between esbuild, which is fine, and `wasm-bindgen`, which was
# refused: a bundler for the interface is downstream of the guarantee and cannot
# affect it. See `crates/nomo-wasm/src/lib.rs`.

set -euo pipefail

cd "$(dirname "$0")/.."

for tool in node npm; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: $tool is required to build the browser front end" >&2
        exit 1
    fi
done

echo "==> engine"
cargo build -p nomo-wasm --release --target wasm32-unknown-unknown
node scripts/check-wasm.mjs

echo "==> front end"
if [ ! -d web/node_modules ]; then
    (cd web && npm install --no-audit --no-fund)
fi
(cd web && node build.mjs)

echo "==> editing sessions"
node scripts/check-session.mjs

echo "==> in a browser"
node scripts/check-browser.mjs
node scripts/check-print.mjs
node scripts/check-files.mjs
node scripts/check-figures.mjs
node scripts/check-plots.mjs
node scripts/check-offline.mjs
