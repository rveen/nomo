// Drive an editing session against the WebAssembly build.
//
// `check-browser.mjs` proves the page loads and renders; it cannot type. This
// exercises the path a keystroke takes — `open`, then `update` per edit — over
// the same module the browser loads, through the same `boundary.mjs` the browser
// uses. What is left untested by both is only CodeMirror's own behaviour.

import { load } from "./wasm-host.mjs";

const engine = await load();
const failures = [];

function check(condition, description) {
  if (!condition) failures.push(description);
}

const session = engine.open("r = 5 cm\nh = 12 cm\nV = pi*r^2*h\n");

// A no-op edit.
let result = session.update("r = 5 cm\nh = 12 cm\nV = pi*r^2*h\n");
check(result.format === 1, "the payload should declare format 1");
check(result.changed === 0, "an identical document changed nothing");
check(result.hasErrors === false, "a valid worksheet has no errors");
check(Array.isArray(result.tokens), "tokens should be a list");
check(result.html.includes("0.000942478"), "the html should carry the result");

// An edit to one line. `V` reads `r`, so both are re-evaluated; `h` is not.
result = session.update("r = 6 cm\nh = 12 cm\nV = pi*r^2*h\n");
check(result.changed === 1, `one line changed, got ${result.changed}`);
check(
  result.recalculated === 2,
  `r and its dependent V, got ${result.recalculated}`,
);
check(result.structural === false, "editing in place is not structural");
check(
  result.html.includes("0.00135717"),
  "the result should follow the edit",
);

// Every keystroke is an edit, and most land on a document that is briefly
// nonsense. The session must survive that and recover.
result = session.update("r = 6 cm\nh = 12 cm\nV = pi*r^2*\n");
check(result.hasErrors === true, "an incomplete expression is an error");
check(
  result.diagnostics.length > 0,
  "an error should come with a diagnostic",
);
check(
  result.diagnostics.every((d) => d.to > d.from),
  "every diagnostic needs a range the editor can draw",
);

result = session.update("r = 6 cm\nh = 12 cm\nV = pi*r^2*h\n");
check(result.hasErrors === false, "the session should recover");

// Verdicts travel beside the diagnostics rather than among them: the editor's
// status line has to say "1 of 2 checks failed" without treating it as an
// error, because the worksheet is right and the design is not.
result = session.update("d = 12 mm\ncheck d >= 16 mm\ncheck d >= 10 mm\n");
check(result.checks !== undefined, "the payload should carry the verdicts");
check(
  result.checks?.total === 2 && result.checks?.failed === 1,
  `two checks, one failed, got ${JSON.stringify(result.checks)}`,
);
check(
  result.hasErrors === false,
  "a failed check must not make the worksheet an error",
);
check(
  result.html.includes('class="step check fail"'),
  "the failed check should be marked in the rendered output",
);

// Offsets are UTF-16, because the only consumer counts in UTF-16. A worksheet
// full of `π` and `°` is the normal case for this language, not an edge one.
result = session.update("' π°—\nr = 5 cm\n");
const source = "' π°—\nr = 5 cm\n";
const units = [...source].length; // no surrogate pairs here, so this is the count
check(
  result.tokens.every((t) => t.to <= units),
  "a token ran past the end of the document measured in UTF-16 units",
);
const rToken = result.tokens.find((t) => t.class === "variable");
check(
  rToken !== undefined && source[rToken.from] === "r",
  `the variable token should start on \`r\`, found "${source[rToken?.from]}"`,
);

// Classification the engine can do and a grammar cannot.
result = session.update("m = 4\nx = m*2\n");
check(
  result.tokens.filter((t) => t.class === "unit").length === 0,
  "`m` is bound here, so nothing should be coloured as a unit",
);
result = session.update("x = 4 m\n");
check(
  result.tokens.some((t) => t.class === "unit"),
  "`m` is metres here and should be coloured as a unit",
);

// The text written to disk is the engine's business, not the front end's: the
// version number and the pragma's spelling belong to the format.
check(
  engine.forSaving("r = 5 cm\n") === "' nomo 1\nr = 5 cm\n",
  "saving should stamp a version pragma",
);
check(
  engine.forSaving("' nomo 1\nr = 5 cm\n") === "' nomo 1\nr = 5 cm\n",
  "stamping twice should not stack pragmas",
);
check(
  engine.forSaving("' nomo 99\nx = 1\n") === "' nomo 99\nx = 1\n",
  "a worksheet from the future must keep its own pragma rather than be relabelled",
);

session.close();

let threw = false;
try {
  session.update("x = 1\n");
} catch {
  threw = true;
}
check(threw, "a closed session should refuse further edits");

// Recovery. A WebAssembly instance that has trapped stays broken — its memory
// no longer describes itself — so the front end replaces it rather than
// carrying on, and `restart()` is what it replaces it with. The editor's own
// recovery is checked in a browser by `check-browser.mjs`; this checks the
// mechanism underneath it, which is the half that lives in `boundary.mjs`.
const replacement = engine.restart();
check(
  typeof replacement.restart === "function",
  "a restarted engine must itself be restartable, or recovery works once",
);
const afterRestart = replacement.open("r = 5 cm\nV = pi*r^2\n");
check(
  afterRestart.update("r = 5 cm\nV = pi*r^2\n").html.includes("0.00785398"),
  "a session on the restarted engine should compute",
);

// The replacement is a *different* instance, not a reset of the old one: the
// old engine's sessions keep working, which is what proves the memories are
// separate rather than shared.
const onTheOld = engine.open("x = 2 m\n");
check(
  onTheOld.update("x = 3 m\n").html.includes("3 m"),
  "restarting must not disturb the engine it was made from",
);
onTheOld.close();
afterRestart.close();

// The worksheet that used to take the whole instance down with it. 2 000 nested
// brackets trapped this module before the parser had a nesting limit, and every
// later call then failed too; now it is an ordinary diagnostic and the session
// carries on. This is the regression test for that crash at the boundary.
const deep = replacement.open("x = 1\n");
const refused = deep.update(`x = ${"(".repeat(2000)}1${")".repeat(2000)}\n`);
check(
  refused.diagnostics.some((d) => d.code === "SH010"),
  "a deeply nested expression should be refused by the parser",
);
check(
  deep.update("y = 2 m + 3 m\n").html.includes("5 m"),
  "the session must survive the worksheet that used to trap it",
);
deep.close();

if (failures.length > 0) {
  console.error("check-session: the editing path misbehaved\n");
  for (const failure of failures) console.error(`  error: ${failure}`);
  process.exit(1);
}

console.log("ok: editing sessions behave, and recalculate incrementally");
