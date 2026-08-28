#!/usr/bin/env bash
# Prove that the engine gives the same answers on a second CPU architecture.
#
# compare-targets.sh closes the gap between two *compilations* — native and
# WebAssembly — but runs both on whatever machine invokes it. That leaves the
# reproducibility claim (design note §3) resting on one instruction set. This
# script closes the other half locally: it builds the engine for aarch64 and
# renders the corpus under emulation, against the same committed snapshots.
#
# Three gates, each localising a different failure:
#
#   1. Build nomo-cli for aarch64-unknown-linux-musl.
#   2. No fused multiply-add in the artifact — the one instruction aarch64 has
#      and x86-64 baseline does not, and the one that would change results.
#   3. nomo test under qemu — the aarch64 build matches the committed
#      snapshots, which were produced on x86-64.
#
# Gate 3 passing means x86-64 == snapshots == aarch64, byte for byte.
#
# Why musl rather than gnu: the musl target links statically with rust-lld, so
# no cross-compiler or glibc sysroot has to be installed. The libc it links is
# irrelevant to the result — check-no-host-math.sh already forbids the engine
# from calling libc for any arithmetic, which is the whole point of vendoring a
# libm. If that guard ever fails, this one becomes meaningless too.
#
# What this does and does not establish is written up in docs/STATUS.md. The
# short version: qemu is emulation, not silicon, so CI on a real arm64 runner is
# the stronger evidence and this is the version a developer can run today.

set -euo pipefail

cd "$(dirname "$0")/.."

TARGET=aarch64-unknown-linux-musl
BIN="target/$TARGET/release/nomo"

if ! command -v qemu-aarch64 >/dev/null 2>&1; then
    echo "error: qemu-aarch64 is required to run the aarch64 build" >&2
    echo "    Fedora: dnf install qemu-user    Debian: apt install qemu-user" >&2
    exit 1
fi

# The disassembler is not optional. A gate that skips when its tool is missing
# reports success for having checked nothing.
if ! command -v llvm-objdump >/dev/null 2>&1; then
    echo "error: llvm-objdump is required to inspect the artifact" >&2
    echo "    Fedora: dnf install llvm    Debian: apt install llvm" >&2
    exit 1
fi

if ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "==> installing the $TARGET standard library"
    rustup target add "$TARGET"
fi

echo "==> building nomo-cli for $TARGET"
# rust-lld rather than the host `cc`, which cannot link aarch64 objects and
# fails with "Relocations in generic ELF (EM: 183)".
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C linker=rust-lld -C linker-flavor=ld.lld" \
    cargo build --release -p nomo-cli --target "$TARGET"

echo "==> checking the artifact for fused multiply-add"
# aarch64 has FMA in its base instruction set, so contracting `a*b + c` into a
# single `fmadd` is available to LLVM here in a way it is not on baseline
# x86-64. That instruction rounds once where the source rounds twice, which is
# exactly the drift this design forbids — the same mechanism as WebAssembly's
# `relaxed_madd`, which check-wasm.mjs rules out on the other target.
#
# Rust does not enable floating-point contraction, so this should find nothing.
# It is checked rather than assumed because it is a property of the compiler's
# defaults, which are not ours to pin, and because the failure it guards against
# is silent: results that are correct, reproducible on this machine, and
# different from every other target.
fma=$(llvm-objdump -d "$BIN" | grep -cE '[[:space:]](fmadd|fmsub|fnmadd|fnmsub|fmla|fmls)[[:space:]]' || true)
if [ "$fma" -ne 0 ]; then
    echo "error: $fma fused multiply-add instructions in $BIN" >&2
    echo "    the aarch64 build rounds differently from the x86-64 one" >&2
    echo "    -> find the site with: llvm-objdump -d $BIN | grep -E ' fmadd | fmla '" >&2
    exit 1
fi
echo "ok: no fused multiply-add in the aarch64 artifact"

echo "==> aarch64 against the snapshots committed from x86-64"
qemu-aarch64 "$BIN" test
