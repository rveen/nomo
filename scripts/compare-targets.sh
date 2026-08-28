#!/usr/bin/env bash
# Prove that the engine gives the same answers natively and under WebAssembly.
#
# This is the verification the numeric model exists for (design note §3). It runs
# four gates in order, each of which localises a different failure:
#
#   1. Build the engine for wasm32-unknown-unknown.
#   2. check-wasm.mjs   — the artifact imports nothing and enables no SIMD.
#   3. nomo test       — the native build matches the committed snapshots.
#   4. compare-targets  — the WebAssembly build matches those same snapshots.
#
# Native == snapshots and WebAssembly == snapshots together mean native ==
# WebAssembly, byte for byte, across the whole corpus.
#
# Needs Node for its WebAssembly engine; nothing is installed and no package is
# fetched. Everything under scripts/ is dependency-free on purpose, because these
# scripts are part of the evidence for the determinism claim.

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v node >/dev/null 2>&1; then
    echo "error: node is required to run the WebAssembly build" >&2
    echo "    it is the WebAssembly engine; nothing is installed from npm" >&2
    exit 1
fi

echo "==> building for wasm32-unknown-unknown"
cargo build -p nomo-wasm --release --target wasm32-unknown-unknown

echo "==> checking the artifact"
node scripts/check-wasm.mjs

echo "==> native against the committed snapshots"
cargo run --quiet -p nomo-cli -- test

echo "==> WebAssembly against the same snapshots"
node scripts/compare-targets.mjs
