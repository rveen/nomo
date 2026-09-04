#!/usr/bin/env bash
# Fetch the math font web/dist/ ships a subset of.
#
# Typeset output needs a font with an OpenType MATH table: the fraction bar
# thickness, the axis height, the script shifts and the stretchy bracket recipes
# are all read from it, and a font without one leaves the browser guessing them
# from ordinary text metrics. Until now the stylesheets *named* such fonts and
# hoped the reader's machine had one. This obtains one so the question does not
# arise.
#
# The font is third-party and is **not** committed. What is committed is the
# provenance — the URL each file comes from and its SHA-256 — so what lands here
# is provably the file this repository was built against. Same rule as the
# corpora, same reason. See THIRD-PARTY.md.
#
#   ./scripts/fetch-font.sh          # fetch what is missing, then verify
#   ./scripts/fetch-font.sh --verify # verify what is there, fetch nothing
#   ./scripts/fetch-font.sh --force  # re-download even if present
#
# NOMO_FONT_MIRROR   base URL holding the same files by name, for a build that
#                    should not reach GitHub. The hashes are checked either way,
#                    which is what makes a mirror trustworthy.
set -euo pipefail
cd "$(dirname "$0")/.."

dest=web/vendor
manifest=scripts/fonts
mirror=${NOMO_FONT_MIRROR:-}
mode=fetch
case ${1:-} in
    --verify) mode=verify ;;
    --force)  mode=force ;;
    "")       ;;
    *) echo "usage: $0 [--verify|--force]" >&2; exit 2 ;;
esac

mkdir -p "$dest"

# The licence travels with the font because the OFL says it must: a build that
# shipped the glyphs and left OFL.txt behind would be redistributing the font
# without the terms it is offered under.
while read -r name url; do
    case $name in ''|\#*) continue ;; esac
    target="$dest/$name"
    if [ "$mode" = verify ]; then
        [ -f "$target" ] || { echo "missing: $target — run $0" >&2; exit 1; }
        continue
    fi
    if [ -f "$target" ] && [ "$mode" != force ]; then
        continue
    fi
    from=$url
    [ -n "$mirror" ] && from="${mirror%/}/$name"
    echo "fetching $name"
    if ! curl -fsSL --retry 3 --retry-delay 2 "$from" -o "$target.part"; then
        rm -f "$target.part"
        echo "could not fetch $from" >&2
        [ -z "$mirror" ] && echo "  Set NOMO_FONT_MIRROR to fetch from elsewhere." >&2
        exit 1
    fi
    mv "$target.part" "$target"
done < "$manifest/upstream.sources"

# Verified from the manifest rather than from whatever was downloaded, so a
# file that drifted upstream fails here instead of quietly changing how every
# worksheet is set.
( cd "$dest" && sha256sum --quiet --check "../../$manifest/upstream.sha256" ) || {
    echo "error: the fetched font does not match scripts/fonts/upstream.sha256" >&2
    echo "  Upstream changed, or the download is damaged. Nothing is used until" >&2
    echo "  the hash agrees; re-run with --force to fetch again." >&2
    exit 1
}
echo "ok: $(grep -cv '^\s*\(#.*\)\?$' "$manifest/upstream.sha256") font files verified in $dest"
