#!/usr/bin/env bash
# Fetch the SMath corpus the importer is measured against.
#
# One corpus: the 60 worksheets of Martin Kraska's *Technische Mechanik mit
# SMath Studio*, from GitHub. There used to be a second — the 48 examples the
# SMath wiki published — and it is no longer fetched: in November 2026 the wiki
# was replaced by docs.smath.com, every one of its download links now answers
# with that site's home page, and no public source holds the files (the
# Wayback Machine has 3 of the 48). Its provenance is kept, retired, in
# scripts/corpora/wiki.sources and wiki.sha256, because those are what finding
# another source needs: the names to look for and the hashes that prove a copy
# is the same file. docs/roadmap.md has the item.
#
# The worksheets are third-party and are **not** in this repository: they are
# other people's documents, published without redistribution terms, and this
# repository is MIT licensed. What is committed instead is the provenance —
# where each archive comes from and the SHA-256 of every file it must yield — so
# a corpus fetched here is demonstrably the same corpus the baselines in
# `tests/corpus/` were recorded against. See THIRD-PARTY.md.
#
#   ./scripts/fetch-corpora.sh          # fetch what is missing, then verify
#   ./scripts/fetch-corpora.sh --verify # verify what is already there, fetch nothing
#   ./scripts/fetch-corpora.sh --force  # re-download even if present
#
# CORPUS_ROOT           where the corpora live (default: corpora/)
# NOMO_CORPORA_MIRROR  base URL holding the same archive by name. Set this and
#                       nothing is asked of GitHub. GitHub's generated tarballs
#                       are not byte-stable, which is why the hashes are taken
#                       per file rather than over the archive; they are checked
#                       either way, which is what makes a mirror trustworthy.
set -euo pipefail
cd "$(dirname "$0")/.."
repo=$(pwd)

root=${CORPUS_ROOT:-corpora}
manifest=scripts/corpora
mirror=${NOMO_CORPORA_MIRROR:-}
mode=fetch
case ${1:-} in
    --verify) mode=verify ;;
    --force)  mode=force ;;
    "")       ;;
    *) echo "usage: $0 [--verify|--force]" >&2; exit 2 ;;
esac

MECH_URL='https://codeload.github.com/sn-code-inside/Technische-Mechanik-mit-SMath/tar.gz/refs/heads/main'

# Scratch files for the whole run, and one trap that cleans up all of them.
#
# One trap, declared here, because a trap body is expanded when it *fires* and
# not when it is set. A second `trap` inside a function, naming a variable that
# function had declared `local`, therefore expanded a name that no longer
# existed by the time the shell exited — and under `set -u` that is an error
# during exit, so the script returned 1 having done everything correctly. The
# symptom was `tgz: unbound variable` printed after `corpora verified`, and it
# only appeared on a machine that actually downloaded the mechanics corpus,
# which is why it survived on every development machine and failed every CI run.
tgz=
trap 'rm -f ${tgz:+"$tgz"}' EXIT

get() { # get <url> <destination>
    if ! curl -fsSL --retry 3 --retry-delay 2 "$1" -o "$2"; then
        echo "could not fetch $1" >&2
        if [ -z "$mirror" ]; then
            echo "  The upstream is third-party and not under this project's control." >&2
            echo "  Set NOMO_CORPORA_MIRROR to a base URL holding the same archive by" >&2
            echo "  name; every file is hash-checked either way, which is what makes a" >&2
            echo "  mirror as trustworthy as the original." >&2
        else
            echo "  NOMO_CORPORA_MIRROR is set to $mirror — check it serves this name." >&2
        fi
        exit 1
    fi
}

fetch_mechanics() {
    local dir="$root/technical-mechanics-samples"
    if [ "$mode" != force ] && [ -d "$dir/Technische-Mechanik-mit-SMath-main" ]; then
        return
    fi
    echo "fetch technical-mechanics-samples"
    mkdir -p "$dir"
    # Not `local`: the trap set at the top of the script cleans this up, and it
    # can only do that if the name is still in scope when the shell exits.
    tgz=$(mktemp)
    if [ -n "$mirror" ]; then
        get "$mirror/Technische-Mechanik-mit-SMath-main.tar.gz" "$tgz"
    else
        get "$MECH_URL" "$tgz"
    fi
    # The archive's top directory is already the name used here, so it unpacks
    # into place. Hashes are checked below rather than on the tarball: GitHub's
    # generated tarballs are recompressed and are not byte-stable.
    tar -xzf "$tgz" -C "$dir"
}

verify() {
    # An absolute path, because the hashes are relative to the corpus root and
    # so the check has to run from inside it. `../$manifest` worked only while
    # CORPUS_ROOT was the default directory one level below the repository —
    # anywhere else, and for the absolute path the documentation invites, it
    # named nothing and the corpora "did not match" when they were fine.
    if ! (cd "$root" && sha256sum -c --quiet "$repo/$manifest/files.sha256"); then
        echo >&2
        echo "The corpora under $root do not match scripts/corpora/files.sha256." >&2
        echo "Re-fetch with --force, or check what changed upstream." >&2
        exit 1
    fi
    echo "corpora verified: $(grep -cv '^#' "$manifest/files.sha256") files under $root"
}

if [ "$mode" != verify ]; then
    fetch_mechanics
fi
verify
