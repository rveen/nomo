#!/usr/bin/env bash
# Package web/dist/ as a zip that can be dropped onto any web server.
#
# The application is static files and nothing else — no backend, no database, no
# build step at the far end — so "install" is copying a directory somewhere a
# server can read it. That is worth shipping as an artifact rather than leaving
# a reader to clone the repository, install a Rust toolchain and a node
# toolchain, and build it themselves to get the same bytes CI already built.
#
# Everything in the directory refers to everything else *relatively* — the
# stylesheet, the bundle, the engine, the fonts, the service worker's precache
# list — so the site works at whatever path it is unzipped into. It does not
# have to be a document root. That is a property worth keeping: if an absolute
# path is ever introduced, this zip silently stops working anywhere but the
# root, and check-offline.mjs will not notice because it serves from the root.
#
#   ./scripts/package-web.sh v0.4.1                  # into dist/
#   ./scripts/package-web.sh v0.4.1 somewhere/else
#
# Run ./scripts/build-web.sh first; this packages what that produced rather than
# building it, so the zip holds bytes that passed the browser checks.

set -euo pipefail
cd "$(dirname "$0")/.."

version=${1:-}
out=${2:-dist}

if [ -z "$version" ]; then
    echo "usage: $0 <version> [output-dir]" >&2
    exit 2
fi

if [ ! -f web/dist/index.html ]; then
    echo "error: no build in web/dist/" >&2
    echo "  build it first:" >&2
    echo "    ./scripts/build-web.sh" >&2
    exit 1
fi

# The notice the MIT licences require to travel with the bundle. It is written
# by the front-end build, so its absence means dist/ was assembled some other
# way and must not be published. See scripts/notice.mjs.
if [ ! -f web/dist/NOTICE.txt ]; then
    echo "error: web/dist/NOTICE.txt is missing — refusing to package" >&2
    echo "  Rebuild with ./scripts/build-web.sh, which writes it." >&2
    exit 1
fi

name="nomo-web-$version"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

# One directory inside the zip rather than loose files. Unzipping in the wrong
# place then costs one `rm -r` instead of picking twelve names out of whatever
# was already there — and because every path in the site is relative, serving
# the extracted directory as-is works without renaming it.
cp -R web/dist "$stage/$name"

cat > "$stage/$name/README.txt" <<README
Nomo $version — the worksheet editor as a static site.

Copy this directory anywhere a web server can read it. There is nothing to
install and nothing to configure: no backend, no database, no build step. The
engine is nomo_wasm.wasm and it runs in the browser, so no worksheet leaves the
machine it is opened on and the server is never asked to compute anything.

  cp -R $name /var/www/html/nomo

Every path inside the site is relative, so it works at any URL — a document root
or a subdirectory, whichever suits.

Two things a server can get wrong:

  * .wasm should be served as application/wasm. If yours does not know the
    extension, the engine still loads — it falls back to a non-streaming
    compile — but it loads more slowly.

  * sw.js is a service worker, and browsers register one only over HTTPS or on
    localhost. Without it the application works but does not run offline.

examples/ holds the worked examples as pre-rendered pages; open examples/ for
the index. index.html is the editor.

SMATH WORKSHEETS

Open handles SMath Studio .sm files as well as .nomo ones. The translation runs
in the browser, in the same engine module, so a worksheet is never uploaded
anywhere — and it works offline like everything else here. What lands in the
editor is a translation with no file behind it: review it, then Save as. A panel
above the editor lists every construct that could not be translated, against the
line it belongs to, and checks the answers the worksheet already stored against
what Nomo computes.

LICENCES

NOTICE.txt carries every licence this directory ships under — Nomo's own MIT
terms, the MIT terms of the packages bundled into bundle.js, and libm's from the
engine. fonts/OFL.txt is the SIL Open Font License the font subsets are under.
Both files have to travel with these bytes if you redistribute them.
README

mkdir -p "$out"
# `zip` writes paths relative to its working directory, so it runs in the
# staging directory rather than being given absolute paths that would be stored
# in the archive.
zip_out=$(cd "$stage" && zip -qr9 "$stage/$name.zip" "$name" && echo ok)
[ "$zip_out" = ok ]
mv "$stage/$name.zip" "$out/$name.zip"

files=$(find "$stage/$name" -type f | wc -l)
size=$(wc -c < "$out/$name.zip")
echo "web: $out/$name.zip — $files files, $((size / 1024)) kB"
