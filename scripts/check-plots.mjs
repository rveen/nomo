// Prove that a plot reaches the screen as a picture, and not as an invisible one.
//
// A plot is drawn by the engine as SVG carrying **class names and no colours**,
// so what a reader sees depends on three files agreeing: `render/plot.rs` emits
// the classes, `render/html.rs` styles them for a standalone `nomo html` file,
// and `web/style.css` styles them for the editor's output pane. Every Rust test
// in the repository can pass while the third one is missing — and it *was*
// missing: the markup was correct, the golden suite was green, and the editor
// showed a black blob on an invisible grid, because an unstyled `<polyline>`
// fills and does not stroke.
//
// So this asserts the things only a browser can see:
//
//   1. The curve is stroked and not filled. That is the whole failure above,
//      and it is a computed style rather than a rule anyone can read off a
//      stylesheet by eye.
//   2. The grid and the axes are drawn in the page's own colour, so the chart
//      follows the theme instead of being legible on one ground and not the
//      other.
//   3. The SVG lays out with a real width and height. An SVG with no intrinsic
//      size collapses to nothing, and every check on its markup still passes.
//   4. A worksheet whose function leaves the plane is drawn as more than one
//      polyline — the gap is the visible part of the promise that nothing is
//      joined across values the function never took.
//   5. A table of measured points is drawn with a mark at every measurement.
//      The mark is a ring rather than a disc — it takes the curve's own class,
//      so a stylesheet that filled it would hide the line under six blobs.
//   6. Two curves on one plot are drawn in two different colours, and the
//      legend swatch beside a name is the colour of the curve it names. Both
//      are computed styles again: the markup only carries `plot-curve-N`, and
//      a stylesheet that defines one of those and not the next draws two
//      curves the reader cannot tell apart.

import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, join } from "node:path";
import { launch } from "./chrome.mjs";
import { repoRoot } from "./wasm-host.mjs";

const dist = join(repoRoot, "web/dist");
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
};

// A parabola: one unbroken curve, and a span whose ends are not round numbers
// so the outermost tick labels have to be pulled inside the drawing.
const SMOOTH = ["' nomo 1", "fn f(x) = x^2 - 2", "plot(f, 0 - 3, 3)"].join("\n");
// A reciprocal through zero: finite either side, not finite at the sample on
// it, so the curve must arrive as two polylines rather than one.
const BROKEN = ["' nomo 1", "fn g(x) = 1/x", "plot(g, 0 - 1, 1)"].join("\n");
// Two curves on one span, which is the case that needs colours to differ and a
// legend to say which is which.
const PAIR = [
  "' nomo 1",
  "fn up(x) = x",
  "fn down(x) = 0 - x",
  "plot(up, down, 0 - 1, 1)",
].join("\n");

// A table of six measured points: no span, marks at each one, and an axis
// fitted to the data.
const TABLE = [
  "' nomo 1",
  "d = [[0, 0], [1, 1.9], [2, 4.1], [3, 6.2], [4, 8.5], [5, 11.4]]",
  "plot(d)",
].join("\n");

const server = createServer(async (request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  const name = path === "/" ? "/index.html" : path;
  try {
    const body = await readFile(join(dist, name));
    response.writeHead(200, {
      "content-type": TYPES[extname(name)] ?? "application/octet-stream",
      "cache-control": "no-store",
    });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const url = `http://127.0.0.1:${server.address().port}/`;

const failures = [];
const check = (condition, description) => {
  if (!condition) failures.push(description);
};

let browser;
try {
  browser = await launch();
  await browser.goto(url);
  await waitFor(browser, `document.body.dataset.ready === "true"`, "the editor");

  // The pane is re-rendered wholesale, so waiting for "a curve exists" would
  // measure the previous worksheet's chart. Wait for the number of polylines
  // this source produces to be the number in the document — and, where two
  // worksheets in a row draw the same number of them, for the legend count
  // that tells them apart. A reciprocal is one curve in two polylines and a
  // pair of lines is two curves in two, so the polyline count alone would let
  // the previous chart answer for this one.
  const render = async (source, curves, legends = 0) => {
    await browser.evaluate(`document.querySelector(".cm-content").focus()`);
    await browser.evaluate(`
      (() => {
        const selection = window.getSelection();
        selection.selectAllChildren(document.querySelector(".cm-content"));
      })()
    `);
    await browser.type(source);
    await waitFor(
      browser,
      `document.querySelectorAll("#output figure.plot polyline.plot-curve").length === ${curves}
       && document.querySelectorAll("#output figure.plot .plot-legend").length === ${legends}`,
      `the plot to render as ${curves} polyline(s) and ${legends} legend entries`,
    );
  };

  await render(SMOOTH, 1);

  const style = JSON.parse(
    await browser.evaluate(`
      (() => {
        const svg = document.querySelector("#output figure.plot svg");
        const box = svg.getBoundingClientRect();
        const curve = getComputedStyle(svg.querySelector(".plot-curve"));
        const grid = getComputedStyle(svg.querySelector(".plot-grid"));
        const axis = getComputedStyle(svg.querySelector(".plot-axis"));
        const label = getComputedStyle(svg.querySelector(".plot-label"));
        const body = getComputedStyle(document.body).color;
        return JSON.stringify({
          width: Math.round(box.width),
          height: Math.round(box.height),
          curveStroke: curve.stroke,
          curveFill: curve.fill,
          curveWidth: parseFloat(curve.strokeWidth),
          gridStroke: grid.stroke,
          axisStroke: axis.stroke,
          labelFill: label.fill,
          body,
        });
      })()
    `),
  );

  check(
    style.width > 100 && style.height > 100,
    `the plot collapsed instead of laying out (${style.width}x${style.height});` +
      " an SVG with no intrinsic size takes up none",
  );
  check(
    style.curveFill === "none",
    `the curve is filled (${style.curveFill}); an unstyled polyline fills its` +
      " own chord and reads as a black blob",
  );
  check(
    style.curveStroke !== "none" && style.curveWidth > 0,
    `the curve is not stroked (${style.curveStroke}, width ${style.curveWidth})`,
  );
  // `currentColor` computes to whatever the body colour is, which is how the
  // chart follows the theme. A literal here would be legible on one ground.
  check(
    style.gridStroke === style.body && style.axisStroke === style.body,
    `the grid and axes do not take the page's colour (grid ${style.gridStroke},` +
      ` axis ${style.axisStroke}, page ${style.body})`,
  );
  check(
    style.labelFill === style.body,
    `the tick labels do not take the page's colour (${style.labelFill})`,
  );

  // Every sample the engine took is in the drawing, so the picture is the data
  // rather than a subsample of it chosen by the renderer.
  const points = await browser.evaluate(
    `document.querySelector("#output .plot-curve").getAttribute("points").trim().split(" ").length`,
  );
  check(
    points === 257,
    `the curve carries ${points} points, not the 257 the engine sampled`,
  );

  // The outermost tick labels are pulled inside, or the last one is clipped by
  // the edge of the drawing and `200000` reads as `20000`.
  const anchors = JSON.parse(
    await browser.evaluate(`
      JSON.stringify([...document.querySelectorAll("#output .plot-label.plot-x, #output .plot-label.plot-start, #output .plot-label.plot-end")]
        .map((t) => getComputedStyle(t).textAnchor))
    `),
  );
  check(
    anchors.length > 2 && anchors.includes("middle"),
    `the tick labels are not anchored as expected (${anchors.join(", ")})`,
  );

  // A curve that leaves the plane is two polylines with a hole between them.
  await render(BROKEN, 2);

  // Two curves, two colours, and a legend that names them in the colour they
  // were drawn in.
  await render(PAIR, 2, 2);
  const pair = JSON.parse(
    await browser.evaluate(`
      (() => {
        const svg = document.querySelector("#output figure.plot svg");
        const curves = [...svg.querySelectorAll("polyline.plot-curve")]
          .map((c) => getComputedStyle(c).stroke);
        const swatches = [...svg.querySelectorAll("line.plot-curve")]
          .map((c) => getComputedStyle(c).stroke);
        const names = [...svg.querySelectorAll(".plot-legend")]
          .map((t) => t.textContent);
        return JSON.stringify({ curves, swatches, names });
      })()
    `),
  );
  check(
    pair.curves.length === 2 && pair.curves[0] !== pair.curves[1],
    `the two curves are the same colour (${pair.curves.join(", ")}); a` +
      " stylesheet missing `plot-curve-2` draws them indistinguishably",
  );
  check(
    pair.names.join(", ") === "up, down",
    `the legend names are ${pair.names.join(", ") || "missing"}, not "up, down"`,
  );
  check(
    pair.swatches.join("|") === pair.curves.join("|"),
    `a legend swatch is not the colour of its curve (swatches` +
      ` ${pair.swatches.join(", ")}, curves ${pair.curves.join(", ")})`,
  );
  // A table is drawn with its measurements marked, in the curve's own colour
  // and not filled — a filled mark is a blob sitting on the line it belongs to.
  await render(TABLE, 1);
  const marks = JSON.parse(
    await browser.evaluate(`
      (() => {
        const svg = document.querySelector("#output figure.plot svg");
        const rings = [...svg.querySelectorAll("circle.plot-mark")];
        const style = rings.length > 0 ? getComputedStyle(rings[0]) : null;
        const curve = getComputedStyle(svg.querySelector("polyline.plot-curve"));
        return JSON.stringify({
          count: rings.length,
          fill: style && style.fill,
          stroke: style && style.stroke,
          curveStroke: curve.stroke,
        });
      })()
    `),
  );
  check(
    marks.count === 6,
    `the table drew ${marks.count} marks, not one per measured point`,
  );
  check(
    marks.fill === "none" && marks.stroke === marks.curveStroke,
    `a mark is not an open ring in the curve's colour (fill ${marks.fill},` +
      ` stroke ${marks.stroke}, curve ${marks.curveStroke})`,
  );

  // Back to two curves, so the viewBox check below measures the legend case.
  await render(PAIR, 2, 2);
  // The legend is under the drawing rather than on top of it, so the viewBox
  // has to have grown to hold it.
  const box = await browser.evaluate(
    `document.querySelector("#output figure.plot svg").getAttribute("viewBox")`,
  );
  check(
    box === "0 0 640 402",
    `the legend strip is not in the viewBox (${box}); a legend drawn outside it` +
      " is clipped away",
  );
} catch (error) {
  failures.push(`could not drive the browser: ${error.message}`);
} finally {
  await browser?.close();
  server.close();
}

if (failures.length > 0) {
  console.error("check-plots: a plot did not reach the screen as a picture\n");
  for (const failure of failures) console.error(`  error: ${failure}\n`);
  process.exit(1);
}

console.log("ok: plots draw as stroked, themed charts in Chrome");

async function waitFor(browser, expression, what) {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (await browser.evaluate(`(async () => ${expression})()`)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}
