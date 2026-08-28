#!/usr/bin/env bash
# Enforce the determinism invariant: no transcendental may be called on the host.
#
# Every platform's libm differs in the last bits, so a worksheet that called
# `f64::sin` directly would give different answers on different machines. All
# transcendentals must go through `crate::math`, which compiles a pure-Rust
# implementation into the artifact.
#
# Fused multiply-add is banned for the same reason: it rounds once where the
# specified sequence rounds twice, and whether it is available differs by target.
#
# Exempt are the operations IEEE 754 specifies exactly — +, -, *, /, sqrt — plus
# sign and rounding manipulation, which are bit-reproducible by definition.

set -euo pipefail

cd "$(dirname "$0")/.."

# nomo-wasm is held to the same rules. It is the boundary crate, so it is the
# one place where a "just this once" call into the host would look reasonable.
CORE=crates/nomo-core/src
WASM=crates/nomo-wasm/src
GUARDED="$CORE $WASM"

# Transcendentals and anything fused. `sqrt`, `abs`, `floor`, `ceil`, `round`
# and `trunc` are deliberately absent: they are exactly specified.
BANNED='\.(sin|cos|tan|asin|acos|atan|atan2|sinh|cosh|tanh|asinh|acosh|atanh|exp|exp2|exp_m1|ln|ln_1p|log|log2|log10|powf|powi|cbrt|hypot|mul_add|recip|to_degrees|to_radians)\('

fail=0

# math.rs is where the wrapping happens, so it is the one file allowed to
# mention these names.
while IFS= read -r file; do
    [ "$(basename "$file")" = "math.rs" ] && continue
    if matches=$(grep -nE "$BANNED" "$file"); then
        echo "error: host math call in $file" >&2
        echo "$matches" | sed 's/^/    /' >&2
        echo "    -> route it through crate::math instead" >&2
        fail=1
    fi
done < <(find $GUARDED -name '*.rs')

# The engine must not reach outside itself for anything, either.
if matches=$(grep -rnE '\b(std::fs|std::net|std::time::(SystemTime|Instant)|std::thread|std::process)\b' $GUARDED); then
    echo "error: the engine must stay free of I/O, clocks and threads" >&2
    echo "$matches" | sed 's/^/    /' >&2
    fail=1
fi

if [ "$fail" -eq 0 ]; then
    echo "ok: no host math, no I/O in nomo-core or nomo-wasm"
fi
exit "$fail"
