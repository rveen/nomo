#!/usr/bin/env bash
# Obtain Folio, the paged-media engine the paged output is laid out by.
#
# `nomo html --paged` writes a CSS Paged Media document; Folio is what turns it
# into pages, in the browser. It is a separate project in its own repository,
# so it is treated like the fonts and the corpora: **not committed**. What is
# committed is the provenance — the repository and commit in
# scripts/folio/upstream.source, and the SHA-256 of every file used in
# scripts/folio/upstream.sha256.
#
# Built rather than downloaded, because Folio publishes no built files. The
# build is reproducible for the files used here — two clean builds of the pinned
# commit gave identical bytes — and that is what makes a hash over a build a
# pin rather than a hope. See docs/folio-plan.md, phase 0.
#
#   ./scripts/fetch-folio.sh          # build what is missing, then verify
#   ./scripts/fetch-folio.sh --verify # verify what is there, build nothing
#   ./scripts/fetch-folio.sh --force  # rebuild even if present
#
# NOMO_FOLIO_REPO   where to clone from instead of the URL in upstream.source —
#                   a local checkout or a mirror. The commit and the hashes are
#                   checked either way, which is what makes a mirror trustworthy.
#
# Needs git, Node 20 or later and corepack. Corepack because Folio's
# package.json names the pnpm it is built with, and an older pnpm cannot read
# its lockfile: pnpm 8 refuses it outright.
set -euo pipefail
cd "$(dirname "$0")/.."

dest=web/vendor/folio
manifest=scripts/folio
mode=fetch
case ${1:-} in
    --verify) mode=verify ;;
    --force)  mode=force ;;
    "")       ;;
    *) echo "usage: $0 [--verify|--force]" >&2; exit 2 ;;
esac

field() { awk -v k="$1" '$1 == k { print $2 }' "$manifest/upstream.source"; }
repo=${NOMO_FOLIO_REPO:-$(field repo)}
commit=$(field commit)
files=$(grep -v '^\s*\(#.*\)\?$' "$manifest/upstream.sha256" | awk '{ print $2 }')

verify() {
    ( cd "$dest" && sha256sum --quiet --check "../../../$manifest/upstream.sha256" ) || {
        echo "error: web/vendor/folio does not match scripts/folio/upstream.sha256" >&2
        echo "  The build changed, or the files are damaged. Nothing is used until" >&2
        echo "  the hash agrees; re-run with --force to build again." >&2
        exit 1
    }
    echo "ok: $(echo "$files" | wc -l) Folio files verified in $dest (commit ${commit:0:7})"
}

missing=
for f in $files; do
    [ -f "$dest/$f" ] || missing=yes
done

if [ "$mode" = verify ]; then
    [ -z "$missing" ] || { echo "missing: files in $dest — run $0" >&2; exit 1; }
    verify
    exit 0
fi
if [ -z "$missing" ] && [ "$mode" != force ]; then
    verify
    exit 0
fi

for tool in git node corepack; do
    command -v "$tool" >/dev/null || { echo "error: $tool is required to build Folio" >&2; exit 1; }
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

echo "cloning Folio from $repo"
git clone --quiet --no-checkout "$repo" "$work/folio"
# Asked before the checkout so that a mirror which lacks the commit says so,
# rather than leaving git's "unable to read tree" to be deciphered.
git -C "$work/folio" cat-file -e "$commit^{commit}" 2>/dev/null || {
    echo "error: $repo does not have commit $commit" >&2
    exit 1
}
git -C "$work/folio" -c advice.detachedHead=false checkout --quiet "$commit"

echo "building Folio ${commit:0:7}"
# --frozen-lockfile: the lockfile is part of what the hashes were taken over,
# and a resolver allowed to update it would build different bytes.
( cd "$work/folio" &&
  export COREPACK_ENABLE_DOWNLOAD_PROMPT=0 &&
  corepack pnpm install --frozen-lockfile --reporter=silent &&
  corepack pnpm --silent build >/dev/null )

mkdir -p "$dest"
for f in $files; do
    case $f in
        LICENSE) cp "$work/folio/LICENSE" "$dest/$f" ;;
        *)       cp "$work/folio/dist/$f" "$dest/$f" ;;
    esac
done
verify
