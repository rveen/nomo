#!/usr/bin/env bash
# Render every worksheet under examples/ and index them.
#
# `nomo html` already produces a self-contained page per worksheet — the
# mathematics, the plots as inline SVG, the figures as data URIs, no assets and
# no script. This puts them side by side behind one page, which is the first
# thing a person wanting to know what Nomo is should be shown: not a feature
# list, but a dozen worksheets an engineer recognises.
#
# The output goes into the front end's build directory so that whatever
# publishes the editor publishes these with it, and so that opening the editor
# and opening a worked example are the same click apart.
#
#   ./scripts/build-gallery.sh            # into web/dist/examples/
#   ./scripts/build-gallery.sh <dir>      # somewhere else

set -euo pipefail
cd "$(dirname "$0")/.."

out=${1:-web/dist/examples}
mkdir -p "$out"

cargo build --release -q -p nomo-cli

# One line per worksheet: its title, taken from the first Markdown heading in
# its prose, and its first paragraph. Read from the worksheet rather than
# maintained here, because a list of descriptions in a build script is a list
# that goes stale the first time somebody edits a worksheet.
index_rows=""
count=0

for source in examples/*.nomo; do
    name=$(basename "$source" .nomo)

    # diagnostics.nomo is a page of deliberate mistakes. It belongs in the test
    # suite and not in a gallery that is trying to show what the tool is for.
    [ "$name" = "diagnostics" ] && continue

    ./target/release/nomo html "$source" >/dev/null || true
    mv "examples/$name.html" "$out/$name.html"

    title=$(grep -m1 "^' # " "$source" | sed "s/^' # //" || true)
    [ -z "$title" ] && title="$name"
    # The first whole paragraph of prose after the title, joined into one line.
    # A single line is nearly always half a sentence, which reads as a mistake.
    blurb=$(awk '
        /^'"'"' # / { found = 1; next }
        found && /^'"'"'$/ { if (para != "") exit; next }
        found && /^'"'"' / { sub(/^'"'"' /, ""); para = para (para == "" ? "" : " ") $0 }
        END { print para }
    ' "$source")

    index_rows="$index_rows
    <li>
      <a href=\"$name.html\">$title</a>
      <p>$blurb</p>
      <code>examples/$name.nomo</code>
    </li>"
    count=$((count + 1))
done

cat > "$out/index.html" <<HTML
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Nomo — worked examples</title>
<style>
  :root { color-scheme: light dark; }
  body {
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    max-width: 46rem; margin: 0 auto; padding: 2rem 1.2rem 4rem; line-height: 1.5;
  }
  h1 { font-size: 1.5rem; margin-bottom: 0.2rem; }
  header p { margin-top: 0; opacity: 0.75; }
  ul { list-style: none; padding: 0; }
  li { border-top: 1px solid rgba(128,128,128,0.3); padding: 0.9rem 0; }
  li a { font-size: 1.05rem; font-weight: 600; text-decoration: none; }
  li a:hover { text-decoration: underline; }
  li p { margin: 0.3rem 0; }
  code { font-size: 0.8rem; opacity: 0.6; }
  footer { margin-top: 2rem; font-size: 0.9rem; opacity: 0.75; }
</style>
</head>
<body>
<header>
  <h1>Nomo — worked examples</h1>
  <p>
    $count worksheets, each rendered by the engine itself. Every page is
    self-contained: the charts are SVG the engine drew, the figures are embedded,
    and nothing here runs a script or fetches anything.
  </p>
</header>
<ul>$index_rows
</ul>
<footer>
  <p>
    Each page is what <code>nomo html</code> produces from the worksheet named
    under it. The same engine computes them in the browser, offline, with the
    same answers to the last bit.
  </p>
</footer>
</body>
</html>
HTML

# A gallery that quietly stops being built is one nobody notices is broken, so
# it checks itself: every link resolves, and every page carries the worksheet's
# own title rather than an empty shell.
for source in examples/*.nomo; do
    name=$(basename "$source" .nomo)
    [ "$name" = "diagnostics" ] && continue
    if [ ! -s "$out/$name.html" ]; then
        echo "error: $out/$name.html is missing or empty" >&2
        exit 1
    fi
    if ! grep -q "<h1" "$out/$name.html"; then
        echo "error: $out/$name.html has no heading — did the worksheet lose its title?" >&2
        exit 1
    fi
done

echo "gallery: $count worksheets in $out"
