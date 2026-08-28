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

import { loadEngine } from "./engine.js";
import { highlighting, setTokens } from "./highlight.js";
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

const output = document.querySelector("#output");
const statusLine = document.querySelector("#status");
const editorHost = document.querySelector("#editor");
const fileName = document.querySelector("#file-name");
const buttons = {
  open: document.querySelector("#open"),
  save: document.querySelector("#save"),
  saveAs: document.querySelector("#save-as"),
  new: document.querySelector("#new"),
};

let session = null;
let view = null;

/** The document currently being edited. */
const current = {
  name: "untitled.nomo",
  /** A FileSystemFileHandle, or null when the browser cannot write back. */
  handle: null,
  /** Whether the buffer differs from what was last written to disk. */
  dirty: false,
};

function status(text, kind = "") {
  statusLine.textContent = text;
  statusLine.className = kind;
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
    result = session.update(source);
  } catch (error) {
    status(`engine error: ${error.message}`, "bad");
    return;
  }
  output.innerHTML = result.html;

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
  if (errors > 0) {
    status(`${errors} error${errors === 1 ? "" : "s"}`, "bad");
  } else if (warnings > 0) {
    status(`${warnings} warning${warnings === 1 ? "" : "s"}`, "warn");
  } else {
    // `recalculated` is how many statements the dependency graph actually
    // re-evaluated. Surfaced because it is the visible proof that editing one
    // line does not recompute the worksheet.
    status(
      result.recalculated > 0 ? `ok — recalculated ${result.recalculated}` : "ok",
      "good",
    );
  }
}

/** Replace the buffer wholesale, as opening a file does. */
function setDocument(text, name, handle) {
  current.name = name;
  current.handle = handle;
  current.dirty = false;
  showFileName();

  view.dispatch({
    changes: { from: 0, to: view.state.doc.length, insert: text },
  });
  analyse();
}

// ---- commands ------------------------------------------------------------

async function commandOpen(engine) {
  let opened;
  try {
    opened = await openWorksheet();
  } catch (error) {
    status(`could not open: ${error.message}`, "bad");
    return;
  }
  if (!opened) return;

  setDocument(opened.text, opened.name, opened.handle);
  // Opening replaces the draft: the draft is a safety net for unsaved work, and
  // what was just loaded from disk is not unsaved.
  await saveDraft(opened.text, opened.name);
  void engine;
}

async function commandSave(engine) {
  const text = engine.forSaving(view.state.doc.toString());
  if (!current.handle) return commandSaveAs(engine);

  try {
    await writeWorksheet(current.handle, text);
  } catch (error) {
    status(`could not save: ${error.message}`, "bad");
    return;
  }
  afterWrite(text);
}

async function commandSaveAs(engine) {
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

  let engine;
  try {
    engine = await loadEngine();
  } catch (error) {
    status(`could not load the engine: ${error.message}`, "bad");
    return;
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
              void commandSave(engine);
              return true;
            },
          },
          {
            key: "Mod-o",
            run: () => {
              void commandOpen(engine);
              return true;
            },
          },
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        highlighting,
        EditorView.lineWrapping,
        onChange,
      ],
    }),
  });

  buttons.open.addEventListener("click", () => void commandOpen(engine));
  buttons.save.addEventListener("click", () => void commandSave(engine));
  buttons.saveAs.addEventListener("click", () => void commandSaveAs(engine));
  buttons.new.addEventListener("click", () => void commandNew());

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
