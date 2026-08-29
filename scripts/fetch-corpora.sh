#!/usr/bin/env bash
# Fetch the SMath corpora the importer is measured against.
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
# NOMO_CORPORA_MIRROR  base URL holding the same archives by name. Set this and
#                       nothing is asked of the upstream sites. The wiki serves
#                       files behind an ASP.NET cookie-detection redirect that
#                       loops without a cookie jar, and GitHub's generated
#                       tarballs are not byte-stable, so a mirror is the reliable
#                       route for CI. Per-file hashes are checked either way,
#                       which is what makes a mirror trustworthy.
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

WIKI_BASE='https://smath.com/wiki/GetFile.aspx?File='
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
jar=$(mktemp)
tgz=
trap 'rm -f "$jar" ${tgz:+"$tgz"}' EXIT

get() { # get <url> <destination>
    if ! curl -fsSL --retry 3 --retry-delay 2 -c "$jar" -b "$jar" "$1" -o "$2"; then
        echo "could not fetch $1" >&2
        if [ -z "$mirror" ]; then
            echo "  The upstream sites are third-party and neither is under this" >&2
            echo "  project's control. Set NOMO_CORPORA_MIRROR to a base URL holding" >&2
            echo "  the same archives by name; every file is hash-checked either way," >&2
            echo "  which is what makes a mirror as trustworthy as the original." >&2
        else
            echo "  NOMO_CORPORA_MIRROR is set to $mirror — check it serves this name." >&2
        fi
        exit 1
    fi
}

sha() { sha256sum "$1" | cut -d' ' -f1; }

unpack() { # unpack <zip> <dir> <pattern>; 11 means "nothing matched", which is fine
    local rc=0
    unzip -j -q -o "$1" "$3" -d "$2" >/dev/null 2>&1 || rc=$?
    if [ "$rc" != 0 ] && [ "$rc" != 11 ]; then
        echo "unzip failed on $1 ($3): exit $rc" >&2
        exit 1
    fi
}

fetch_wiki() {
    local zips="$root/nomo-corpus/zips" sm="$root/nomo-corpus/sm"
    mkdir -p "$zips" "$sm"
    local want remote name url
    while IFS=$'\t' read -r want remote name; do
        case ${want:-} in ""|\#*) continue ;; esac
        local dest="$zips/$name"
        if [ "$mode" != force ] && [ -f "$dest" ] && [ "$(sha "$dest")" = "$want" ]; then
            continue
        fi
        if [ -n "$mirror" ]; then
            url="$mirror/${name// /%20}"
        else
            # The remote path is already URL-encoded where it needs to be; only
            # the spaces that survived in the wiki's own filenames need doing.
            url="$WIKI_BASE${remote// /%20}"
        fi
        echo "fetch $name"
        get "$url" "$dest"
        local got; got=$(sha "$dest")
        if [ "$got" != "$want" ]; then
            echo "checksum mismatch for $name" >&2
            echo "  expected $want" >&2
            echo "  got      $got" >&2
            exit 1
        fi
        # Flatten: a few archives nest their worksheets one directory down, and
        # `sm/` is flat. The .csv is the data file `importDataCSV.sm` reads, and
        # it is in exactly one archive — unzip answers 11 for a pattern that
        # matches nothing, which is not an error here and must not trip `set -e`.
        unpack "$dest" "$sm" '*.sm'
        unpack "$dest" "$sm" '*.csv'
    done < "$manifest/wiki.sources"
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
    fetch_wiki
    fetch_mechanics
fi
verify
