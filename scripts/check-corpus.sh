#!/usr/bin/env bash
# The SMath corpora as a regression gate.
#
# `smath-import --check` on its own only measures: every number it prints is
# work still to do, so it exits zero however bad the news is. The baseline is
# the opposite — it says nothing about how good the import is and everything
# about whether it changed — so a change to the importer or the engine that
# moves any worksheet's result fails here until the baseline is regenerated,
# and the behavioural change lands in the diff beside the code that caused it.
#
# Each line holds the verdict counts, a digest of the values computed, and a
# digest of the emitted source. All three: counts alone let a broken `norm`
# through, and counts with values let a rewritten definition through.
#
# The corpora are third-party worksheets and are **not** in this repository —
# `./scripts/fetch-corpora.sh` brings them down and checks them against the
# committed hashes. A corpus that is not found is skipped rather than failed, so
# a fresh checkout still runs the rest of the suite; CI fetches first and then
# asserts they are there, so absence cannot turn that job green. Point
# CORPUS_ROOT elsewhere if they live somewhere else.
set -euo pipefail
cd "$(dirname "$0")/.."
root=${CORPUS_ROOT:-corpora}
extra=("$@")

fail=0
check() {
    local name=$1
    shift
    echo "==> $name"
    cargo run -q -p nomo-smath --bin smath-import -- \
        --check "$@" --baseline "tests/corpus/$name.txt" "${extra[@]}" | tail -1 || fail=1
}

for pair in "$root/nomo-corpus/sm:wiki" "$root/technical-mechanics-samples:mechanics"; do
    dir=${pair%%:*}
    name=${pair##*:}
    if [ ! -d "$dir" ]; then
        echo "skip: $name corpus not found at $dir"
        continue
    fi
    check "$name" "$dir"
done

exit $fail
