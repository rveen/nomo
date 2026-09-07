// The Nomo worksheet editor.
//
// Everything runs in this tab. There is no server: the page is static files, the
// engine is a WebAssembly module, and nothing a worksheet contains leaves the
// browser. Any backend this project ever grows stores documents and does not
// compute.

import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
// `lintGutter` and `setDiagnostics` only — deliberately not `linter()`. That
// extension owns the diagnostic set and would overwrite the engine's with the
// result of its own source on the next tick.
import { lintGutter, setDiagnostics } from "@codemirror/lint";

import { assist, goToDefinition, setSymbols, setVocabulary } from "./assist.js";
import { loadEngine } from "./engine.js";
import { highlighting, setTokens } from "./highlight.js";
import { imagePaste } from "./image.js";
import {
  canWriteFiles,
  clearDraft,
  download,
  loadDraft,
  openWorksheet,
  saveDraft,
  saveWorksheetAs,
  writeWorksheet,
} from "./storage.js";

const STARTING_WORKSHEET = `' Cylinder volume
' Edit anything — results update as you type.

r = 5 cm
h = 12 cm

V = pi*r^2*h
V -> dm^3
`;

/** Milliseconds of quiet before re-analysing. */
const SETTLE = 60;

/** Milliseconds of quiet before writing the draft to IndexedDB. */
const DRAFT_SETTLE = 800;

/** How long a message that asks to stay put gets the status line for. */
const STATUS_HOLD = 4000;

const output = document.querySelector("#output");
const statusLine = document.querySelector("#status");
const editorHost = document.querySelector("#editor");
const fileName = document.querySelector("#file-name");
const typeset = document.querySelector("#typeset");
const importPanel = {
  root: document.querySelector("#import"),
  summary: document.querySelector("#import-summary"),
  list: document.querySelector("#import-list"),
  dismiss: document.querySelector("#import-dismiss"),
};
const buttons = {
  open: document.querySelector("#open"),
  save: document.querySelector("#save"),
  saveAs: document.querySelector("#save-as"),
  new: document.querySelector("#new"),
};

/**
 * The engine.
 *
 * Module scope rather than a local in `main`, and not passed to the commands
 * that use it, because a restart *replaces* it: a handle captured in a closure
 * would go on calling the instance that already failed.
 */
let engine = null;
let session = null;
let view = null;

/** True while a restart is in flight, so a burst of edits queues one. */
let restarting = false;

/**
 * How many times the engine may be restarted before the page gives up.
 *
 * A restart that immediately fails again would otherwise loop: the recovery
 * ends by analysing the same buffer that just brought the engine down. Three is
 * enough for a fault that is transient and few enough to notice one that is not.
 */
const MAX_RESTARTS = 3;
let restarts = 0;

/** The document currently being edited. */
const current = {
  name: "untitled.nomo",
  /** A FileSystemFileHandle, or null when the browser cannot write back. */
  handle: null,
  /** Whether the buffer differs from what was last written to disk. */
  dirty: false,
};

/** When the message on the status line stops asking to be left alone. */
let heldUntil = 0;

/**
 * Say something on the status line.
 *
 * `hold` is for a message the next analysis cannot say again. Pasting a figure
 * is the case that needed it: how big the image came out and whether it was
 * shrunk are facts about that one paste, and the analysis it triggers is 60 ms
 * behind it with an `ok` that would wipe them off the screen unread.
 *
 * Anything a person just did is said immediately either way. Only `analysed`
 * below defers, because only it can repeat itself.
 */
function status(text, kind = "", hold = false) {
  heldUntil = hold ? Date.now() + STATUS_HOLD : 0;
  statusLine.textContent = text;
  statusLine.className = kind;
}

/**
 * What the analysis has to say, which a held message outranks for a moment.
 *
 * An error is never held back: a worksheet that stopped computing matters more
 * than anything a message has to say about a picture.
 */
function analysed(text, kind = "") {
  if (kind !== "bad" && Date.now() < heldUntil) return;
  status(text, kind);
}

function showFileName() {
  fileName.textContent = `${current.name}${current.dirty ? " •" : ""}`;
  fileName.title = current.handle
    ? "Saved to the file you opened"
    : "This browser cannot write back to a file; use Download";
  buttons.save.disabled = !current.handle;
}

function markDirty(dirty) {
  if (current.dirty === dirty) return;
  current.dirty = dirty;
  showFileName();
}

/**
 * Analyse the current text and update everything downstream of it.
 *
 * Called on a short debounce rather than on every keystroke: the engine is fast
 * enough to run inline, but re-rendering the output pane mid-word is visually
 * noisy and costs more than the analysis does.
 */
function analyse() {
  if (!session || !view) return;

  const source = view.state.doc.toString();
  let result;
  try {
    result = session.update(source, { mathml: typeset.checked });
  } catch (error) {
    // The engine is gone, not merely unhappy: an errored worksheet comes back
    // as diagnostics, so anything thrown here means the instance itself failed.
    // Whatever else happens, the buffer is safe — it belongs to CodeMirror and
    // the engine never held it.
    void restartEngine(error);
    return;
  }
  output.innerHTML = result.html;
  // What the engine now knows about this worksheet's names, for completion,
  // hover and go-to-definition. Replaced wholesale rather than merged: a name
  // deleted from the worksheet must stop being offered.
  setSymbols(result.symbols);

  view.dispatch({ effects: setTokens.of(result.tokens) });
  view.dispatch(
    setDiagnostics(
      view.state,
      result.diagnostics.map((d) => ({
        from: Math.min(d.from, view.state.doc.length),
        to: Math.min(d.to, view.state.doc.length),
        severity: d.severity,
        message: `${d.message}  [${d.code}]`,
      })),
    ),
  );

  const errors = result.diagnostics.filter((d) => d.severity === "error").length;
  const warnings = result.diagnostics.length - errors;
  const checks = result.checks ?? { total: 0, failed: 0 };
  if (errors > 0) {
    analysed(`${errors} error${errors === 1 ? "" : "s"}`, "bad");
  } else if (checks.failed > 0) {
    // Amber rather than red, and said before anything else that is not an
    // error: the worksheet is correct and the design does not hold, which is a
    // result the engineer has to see rather than a fault to fix.
    analysed(
      `${checks.failed} of ${checks.total} check${checks.total === 1 ? "" : "s"} failed`,
      "warn",
    );
  } else if (warnings > 0) {
    analysed(`${warnings} warning${warnings === 1 ? "" : "s"}`, "warn");
  } else if (checks.total > 0) {
    analysed(
      `ok — ${checks.total} check${checks.total === 1 ? "" : "s"} passed`,
      "good",
    );
  } else {
    // `recalculated` is how many statements the dependency graph actually
    // re-evaluated. Surfaced because it is the visible proof that editing one
    // line does not recompute the worksheet.
    analysed(
      result.recalculated > 0 ? `ok — recalculated ${result.recalculated}` : "ok",
      "good",
    );
  }
}

/**
 * Put the engine back after it failed mid-edit.
 *
 * Before this existed, one failure was permanent. The engine is a WebAssembly
 * instance, a trap leaves its linear memory describing something that is no
 * longer true, and every later call failed the same way — so the editor
 * reported `engine error` once and then quietly stopped recalculating for the
 * life of the tab, while still looking like it was working. The parser's
 * nesting limit removed the one way a worksheet could cause that; this is what
 * happens if a way is ever found again.
 *
 * The replacement instance starts from the buffer on screen, so nothing typed
 * is lost — the text was never the engine's to hold.
 */
async function restartEngine(cause) {
  if (restarting) return;
  restarting = true;
  try {
    if (restarts >= MAX_RESTARTS) {
      status(
        `the engine failed repeatedly (${cause.message}) — reload the page`,
        "bad",
      );
      return;
    }
    restarts += 1;

    // The old session's handle points into memory that no longer exists.
    // Freeing it is what would crash; dropping it on the floor is correct.
    session = null;

    try {
      engine = engine.restart();
      session = engine.open(view.state.doc.toString());
    } catch (error) {
      status(`the engine could not be restarted: ${error.message}`, "bad");
      return;
    }
  } finally {
    restarting = false;
  }

  // Analyse again so the results catch up with the buffer. If this fails too it
  // comes back here, and the counter above is what stops that being a loop.
  analyse();
}

/**
 * Replace the buffer wholesale, as opening a file does.
 *
 * `dirty` is a parameter rather than always false because an import is not an
 * open: what lands in the buffer is a translation that exists nowhere on disk,
 * and calling that clean would invite closing the tab on it.
 */
function setDocument(text, name, handle, dirty = false) {
  current.name = name;
  current.handle = handle;
  current.dirty = dirty;
  showFileName();

  view.dispatch({
    changes: { from: 0, to: view.state.doc.length, insert: text },
  });
  analyse();
}

// ---- commands ------------------------------------------------------------

async function commandOpen() {
  let opened;
  try {
    opened = await openWorksheet();
  } catch (error) {
    status(`could not open: ${error.message}`, "bad");
    return;
  }
  if (!opened) return;

  if (opened.bytes) {
    await importSmath(opened.bytes, opened.name);
    return;
  }

  hideImportReport();
  setDocument(opened.text, opened.name, opened.handle);
  // Opening replaces the draft: the draft is a safety net for unsaved work, and
  // what was just loaded from disk is not unsaved.
  await saveDraft(opened.text, opened.name);
}

// ---- importing an SMath worksheet ----------------------------------------
//
// The translation runs here, in this tab, in the same WebAssembly module as the
// engine. That is the whole design: the files this feature exists to accept are
// an engineer's existing work, and uploading them to a server to be translated
// would break the one promise the application makes about them.

/**
 * Translate a `.sm` file into the editor, and report what the translation did.
 *
 * The report is not decoration. The importer's rule is that nothing is ever
 * silently dropped — an untranslatable construct becomes a marker comment in the
 * output — and a marker nobody is pointed at is only half of that. So every note
 * gets a line a reader can click, and the stored answers SMath itself saved are
 * checked against what Nomo computes, which is the only evidence available that
 * the translation is faithful.
 */
async function importSmath(bytes, name) {
  let report;
  try {
    report = engine.importSmath(bytes);
  } catch (error) {
    // A throw here is the module failing, not the file being bad — a bad file
    // comes back as `report.error`. So the engine has to be replaced, exactly
    // as a failed analysis does it.
    void restartEngine(error);
    return;
  }

  if (report.error) {
    hideImportReport();
    status(`${name} could not be read: ${report.error}`, "bad");
    return;
  }

  // `.sm` becomes `.nomo`, and the handle is deliberately dropped in
  // `openWorksheet`: Save must never write Nomo source over the original.
  const imported = name.replace(/\.sm$/i, "") + ".nomo";
  setDocument(report.source, imported, null, true);
  // The draft matters more here than after an ordinary open. There is no file
  // on disk holding this text — losing the tab would mean importing again.
  await saveDraft(report.source, imported);
  showImportReport(report, name);
}

/** Move the cursor to a line and put it in view. */
function goToLine(number) {
  const total = view.state.doc.lines;
  const line = view.state.doc.line(Math.min(Math.max(number, 1), total));
  view.dispatch({
    selection: { anchor: line.from },
    scrollIntoView: true,
  });
  view.focus();
}

/** Prose for a note kind. The wire names are stable; these are not. */
const NOTE_LABELS = {
  unsupported: "not translated",
  carried: "carried but not shown",
  renamed: "renamed",
  collision: "name collision",
  "scope-flattened": "scope flattened",
};

/** Prose for the verdicts worth showing. `agreed` is counted, never listed. */
const VERDICT_LABELS = {
  disagreed: "answer differs from SMath's",
  "line-failed": "line did not evaluate",
  "answer-unreadable": "SMath's stored answer could not be read",
  "shape-differs": "answer is a different shape",
};

function hideImportReport() {
  importPanel.root.hidden = true;
  importPanel.list.replaceChildren();
}

function showImportReport(report, name) {
  const agreed = report.checks.filter((c) => c.verdict === "agreed").length;
  const unsupported = report.notes.filter((n) => n.kind === "unsupported").length;

  // Two numbers, in the order a reader needs them: how much of the worksheet
  // came across, and whether what came across is right. They are deliberately
  // not averaged into one score — "how much was translated" and "is the
  // translation correct" are different questions and a single figure hides both.
  const parts = [];
  parts.push(
    unsupported === 0
      ? `${name}: every construct translated`
      : `${name}: ${unsupported} construct${unsupported === 1 ? "" : "s"} not translated`,
  );
  if (report.checks.length > 0) {
    parts.push(
      `${agreed} of ${report.checks.length} answer${report.checks.length === 1 ? "" : "s"} SMath stored agree${agreed === 1 ? "s" : ""} with Nomo`,
    );
  } else {
    parts.push("this worksheet stored no answers to check against");
  }
  importPanel.summary.textContent = parts.join(" — ");
  importPanel.summary.className =
    unsupported === 0 && agreed === report.checks.length ? "clean" : "";

  // Every note, then every check that is not an agreement, in line order. A
  // clean check is a number in the summary and nothing more; a reader needs the
  // list for what they have to look at.
  const rows = [
    ...report.notes.map((n) => ({
      line: n.line,
      kind: n.kind,
      label: NOTE_LABELS[n.kind] ?? n.kind,
      detail: n.detail,
    })),
    ...report.checks
      .filter((c) => c.verdict !== "agreed")
      .map((c) => ({
        line: c.line,
        kind: c.verdict,
        label: VERDICT_LABELS[c.verdict] ?? c.verdict,
        detail: detailOf(c),
      })),
  ].sort((a, b) => a.line - b.line);

  importPanel.list.replaceChildren(
    ...rows.map((row) => {
      const item = document.createElement("li");
      item.className = `import-${row.kind}`;
      const button = document.createElement("button");
      button.type = "button";
      button.className = "import-line";
      button.textContent = `line ${row.line}`;
      button.addEventListener("click", () => goToLine(row.line));
      const label = document.createElement("span");
      label.className = "import-kind";
      label.textContent = row.label;
      const detail = document.createElement("span");
      detail.className = "import-detail";
      detail.textContent = row.detail;
      item.append(button, label, detail);
      return item;
    }),
  );

  importPanel.root.hidden = false;
  status(`imported ${name}`, unsupported === 0 ? "good" : "");
}

/**
 * What to say about a check that did not agree.
 *
 * A disagreement is the one case where the two numbers are the message, so they
 * are shown rather than described. Both are in base SI, which is what the oracle
 * compares.
 */
function detailOf(check) {
  if (check.verdict === "disagreed" && check.computed !== null) {
    return `Nomo ${check.computed}, SMath ${check.expected}`;
  }
  return check.detail;
}

async function commandSave() {
  const text = engine.forSaving(view.state.doc.toString());
  if (!current.handle) return commandSaveAs();

  try {
    await writeWorksheet(current.handle, text);
  } catch (error) {
    status(`could not save: ${error.message}`, "bad");
    return;
  }
  afterWrite(text);
}

async function commandSaveAs() {
  const text = engine.forSaving(view.state.doc.toString());

  if (!canWriteFiles) {
    // Nothing to write back to, so the honest option is a download. The button
    // says Download in this browser, so this is not a surprise.
    download(text, current.name);
    afterWrite(text);
    return;
  }

  let handle;
  try {
    handle = await saveWorksheetAs(text, current.name);
  } catch (error) {
    status(`could not save: ${error.message}`, "bad");
    return;
  }
  if (!handle) return; // cancelled

  current.handle = handle;
  current.name = handle.name;
  afterWrite(text);
}

/**
 * Reconcile the buffer with what was just written.
 *
 * Saving stamps a version pragma, so the text on disk can differ from the text
 * on screen by one line. Putting it back into the editor keeps the two the same
 * — a buffer that silently differs from its file is how an editor loses an edit.
 */
function afterWrite(text) {
  if (text !== view.state.doc.toString()) {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
    });
    analyse();
  }
  markDirty(false);
  void saveDraft(text, current.name);
  status(`saved ${current.name}`, "good");
}

async function commandNew() {
  hideImportReport();
  setDocument(STARTING_WORKSHEET, "untitled.nomo", null);
  await clearDraft();
}

// ---- startup -------------------------------------------------------------

/**
 * Register the service worker that makes the application work offline.
 *
 * Deliberately not awaited and deliberately not fatal. The application is
 * perfectly usable without it — it only stops being usable *next* time, with the
 * network off — so a registration failure is worth neither a delay at startup
 * nor an error in the user's face.
 */
function registerServiceWorker() {
  if (!("serviceWorker" in navigator)) return;
  // A `file://` page has no origin a worker can be scoped to.
  if (!location.protocol.startsWith("http")) return;

  navigator.serviceWorker.register("./sw.js").catch(() => {
    /* offline support is unavailable; everything else still works */
  });
}

async function main() {
  status("loading the engine…");

  try {
    engine = await loadEngine();
  } catch (error) {
    status(`could not load the engine: ${error.message}`, "bad");
    return;
  }

  // Read once: the units, functions, packs and keywords are facts about the
  // build rather than about the document.
  try {
    setVocabulary(engine.vocabulary());
  } catch (error) {
    // Completion is a convenience; an editor that refused to start without it
    // would be trading the whole application for part of one.
    status(`completions unavailable: ${error.message}`, "warn");
  }

  // Whatever was being edited last time, if anything.
  const draft = await loadDraft();
  const initial = draft?.text ?? STARTING_WORKSHEET;
  current.name = draft?.name ?? "untitled.nomo";

  session = engine.open(initial);
  // A tab that closes takes the module with it, but an explicit close keeps the
  // session's lifetime honest and makes a leak visible if one is ever added.
  window.addEventListener("pagehide", () => session?.close());

  let analyseTimer = null;
  let draftTimer = null;
  const onChange = EditorView.updateListener.of((update) => {
    if (!update.docChanged) return;
    markDirty(true);

    clearTimeout(analyseTimer);
    analyseTimer = setTimeout(analyse, SETTLE);

    // Written on a longer debounce than the analysis: a draft that is a second
    // out of date costs nothing, and a database write per keystroke is waste.
    clearTimeout(draftTimer);
    draftTimer = setTimeout(
      () => void saveDraft(view.state.doc.toString(), current.name),
      DRAFT_SETTLE,
    );
  });

  view = new EditorView({
    parent: editorHost,
    state: EditorState.create({
      doc: initial,
      extensions: [
        lineNumbers(),
        lintGutter(),
        highlightActiveLine(),
        history(),
        keymap.of([
          {
            key: "Mod-s",
            run: () => {
              void commandSave();
              return true;
            },
          },
          {
            // The convention every editor uses for this.
            key: "F12",
            run: goToDefinition,
          },
          {
            key: "Mod-o",
            run: () => {
              void commandOpen();
              return true;
            },
          },
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        highlighting,
        assist,
        imagePaste({
          // Read at call time, never captured: a restart replaces the engine.
          ready: () => engine !== null && !restarting,
          attach: (source, cursor, image) => engine.attachImage(source, cursor, image),
          failed: (error) => void restartEngine(error),
          status,
        }),
        EditorView.lineWrapping,
        onChange,
      ],
    }),
  });

  buttons.open.addEventListener("click", () => void commandOpen());
  buttons.save.addEventListener("click", () => void commandSave());
  buttons.saveAs.addEventListener("click", () => void commandSaveAs());
  buttons.new.addEventListener("click", () => void commandNew());
  importPanel.dismiss.addEventListener("click", hideImportReport);
  // Re-render rather than recompute: typesetting changes how the answer is
  // drawn and not what it is.
  typeset.addEventListener("change", analyse);

  if (!canWriteFiles) {
    // Say what the button does rather than offering a Save that silently
    // produces a second file in ~/Downloads.
    buttons.saveAs.textContent = "Download";
    buttons.save.hidden = true;
  }

  // The draft is written on a debounce, so a tab closed mid-edit could lose the
  // last second of typing. This is the last chance to write it.
  window.addEventListener("pagehide", () => {
    if (current.dirty) void saveDraft(view.state.doc.toString(), current.name);
  });

  showFileName();
  analyse();
  view.focus();

  // Last, so a failure here cannot delay a working editor.
  registerServiceWorker();

  // Announced so the browser checks can wait for a specific thing rather than
  // for a duration.
  document.body.dataset.ready = "true";
}

main();
