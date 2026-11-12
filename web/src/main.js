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
// CodeMirror's own find, because the browser's cannot do the job here. The
// editor renders only the lines near the viewport — that is what keeps a
// worksheet carrying megabytes of figures responsive — so a line scrolled out
// of view is not in the page, and Ctrl+F in the browser never finds it. This
// searches the document rather than the page.
import { search, searchKeymap } from "@codemirror/search";

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
  pages: document.querySelector("#pages"),
  print: document.querySelector("#print"),
};
const outputPane = document.querySelector("#pane-output");
const pagesFrame = document.querySelector("#pages-frame");

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
  if (pages.shown) schedulePages();
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

// ---- pages ---------------------------------------------------------------
//
// The worksheet as the pages it prints on. The frame is given the document
// `nomo_render_paged` returns — the one `nomo html --paged` writes, which
// `compare-html.mjs` holds byte-identical across targets — with Folio linked
// from `folio/`, so the frame fetches it the first time pages are asked for
// and the editor costs nothing more until then.
//
// Laid out on a longer debounce than the results pane. Paginating is the
// browser laying the whole worksheet out more than once, which is far more
// than the engine's recalculation costs, and pages that reflow under every
// keystroke are not something anybody reads while typing.

/** Where the frame's document finds Folio and the fonts, relative to it. */
const FOLIO_URL = "folio/folio.polyfill.min.js";
const MATH_FONT_URL = "fonts/stix-two-math-subset.woff2";
/**
 * The prose's face: the files `web/style.css` declares for the editor. The
 * frame is a document of its own and cannot see that declaration, so without
 * these the pages were set in Georgia beside an editor set in STIX Two Text —
 * and, since a page breaks where its text does, broke in Georgia's places.
 */
const TEXT_FONT = {
  upright: "fonts/stix-two-text-subset.woff2",
  italic: "fonts/stix-two-text-italic-subset.woff2",
};

/** Milliseconds of quiet before laying the pages out again. */
const PAGES_SETTLE = 600;

/** How long Folio gets before the view says it did not finish. */
const PAGES_TIMEOUT = 30000;

const pages = {
  /** Whether the Pages view is showing in place of the results. */
  shown: false,
  timer: 0,
  /** The document the frame holds or is laying out, to skip one unchanged. */
  document: null,
  /** Settles with the page count once the frame's document is laid out. */
  done: null,
  /** Settles the previous `done` when a newer layout replaces it. */
  supersede: null,
};

/**
 * Folio and the three fonts, as `blob:` URLs made from the editor's own copies.
 *
 * The frame is given these rather than the files' paths so that it requests
 * nothing at all. It used to fetch Folio and the fonts itself, through the
 * service worker, and on the first visit after a build — while a new worker
 * was taking over — those requests could stall: Folio's script never arrived
 * and the pages never finished, or the math font did not, and Folio's probe
 * measured a fallback face and refused it as having no MATH table. A reload
 * cured it, which is how it was reported. Without the worker the same
 * sequence worked every time. The editor's own requests were never affected,
 * so the editor fetches, and a `blob:` URL is read from memory — never from the
 * worker or the network — which also means a re-layout no longer re-fetches
 * 115 kB of script and three fonts. Offline, the editor's fetch is answered
 * from the precache, as before.
 *
 * Fetched once, on the first layout. A failure is not kept, so the next
 * attempt tries again.
 */
let frameAssets = null;
function assets() {
  if (!frameAssets) {
    const local = async (url) => {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`could not fetch ${url}: ${response.status}`);
      return URL.createObjectURL(await response.blob());
    };
    frameAssets = Promise.all(
      [FOLIO_URL, MATH_FONT_URL, TEXT_FONT.upright, TEXT_FONT.italic].map(local),
    ).then(([folio, math, upright, italic]) => ({ folio, math, text: { upright, italic } }));
    frameAssets.catch(() => {
      frameAssets = null;
    });
  }
  return frameAssets;
}

/** The paged document for the buffer as it stands. */
function pagedDocument(urls) {
  const title = current.name.replace(/\.nomo$/, "");
  return engine.renderPaged(view.state.doc.toString(), title, {
    // The name the editor shows in its header — the file opened or saved, or
    // `untitled.nomo` for one never saved, which is what the header says too.
    fileName: current.name,
    mathml: typeset.checked,
    fontUrl: urls.math,
    folio: { url: urls.folio },
    textFont: urls.text,
  });
}

/**
 * Put the current worksheet into the frame and wait for Folio to finish.
 *
 * Resolves with the number of pages. The paged document marks the end of
 * pagination on its root — `data-nomo-pages`, set by Folio's `after` hook — so
 * this watches for that rather than guessing a duration. An unchanged
 * worksheet is not laid out again: typesetting toggled twice, or a save,
 * should not reflow the pages someone is reading.
 */
async function layOutPages() {
  const urls = await assets();
  let html;
  try {
    html = pagedDocument(urls);
  } catch (error) {
    void restartEngine(error);
    throw error;
  }
  if (html === pages.document && pages.done) return pages.done;
  pages.document = html;

  // Where the reader was, so new pages do not throw them back to page 1.
  const scroll = pagesFrame.contentWindow?.scrollY ?? 0;
  const previous = pages.supersede;

  const done = new Promise((resolve, reject) => {
    let settled = false;
    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      fn(value);
    };
    const timer = setTimeout(
      () => finish(reject, new Error("the pages did not finish laying out")),
      PAGES_TIMEOUT,
    );
    // A newer layout took over: whoever waits on this one waits on that.
    pages.supersede = (next) => finish(resolve, next);

    pagesFrame.addEventListener(
      "load",
      () => {
        const frame = pagesFrame.contentWindow;
        const root = pagesFrame.contentDocument.documentElement;
        // Folio reports a failure — a math font without a MATH table, say — by
        // rejecting, which in the frame is an unhandled rejection.
        frame.addEventListener("unhandledrejection", (event) =>
          finish(reject, event.reason ?? new Error("Folio failed")),
        );
        frame.addEventListener("keydown", onPrintKey, { capture: true });
        frameStyle(pagesFrame.contentDocument);
        const check = () => {
          // A waiter a newer layout superseded still hears this load, since
          // both were listening for the frame's next one; it must not touch
          // the frame on the newer layout's behalf.
          if (settled) return true;
          if (!root.hasAttribute("data-nomo-pages")) return false;
          fitPages();
          frame.scrollTo(0, scroll);
          finish(resolve, Number(root.getAttribute("data-nomo-pages")));
          return true;
        };
        if (!check()) {
          new MutationObserver((_, observer) => {
            if (check()) observer.disconnect();
          }).observe(root, { attributes: true, attributeFilter: ["data-nomo-pages"] });
        }
      },
      { once: true },
    );
  });
  pages.done = done;
  previous?.(done);
  pagesFrame.srcdoc = html;
  return done;
}

/**
 * Ctrl+P prints the pages rather than the browser's view of the editor.
 *
 * Listened for in the capture phase, so it is caught wherever the focus is in
 * the editor — and in the frame too, once a reader has clicked into the pages,
 * which is a different document whose key events never reach this window. The
 * browser's own menu still prints the results pane, which `check-print.mjs`
 * keeps honest.
 */
function onPrintKey(event) {
  if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === "p") {
    event.preventDefault();
    void commandPrint();
  }
}

/**
 * Scale the pages to the width of the pane, never up.
 *
 * An A4 page is about 800 CSS pixels wide and the results pane is half a
 * screen, so at full size a page is cut at its right edge. The scaling is the
 * editor's, added to the frame after Folio has finished, rather than written
 * into the paged document: that document is the one `nomo html --paged`
 * writes and `compare-html.mjs` holds to it byte for byte. It is screen-only
 * and set on the pages' container, so it changes neither what Folio measured
 * nor the size anything prints at.
 */
function fitPages() {
  const doc = pagesFrame.contentDocument;
  const page = doc?.querySelector(".pagedjs_page");
  if (!page) return;
  frameStyle(doc);
  // The page's own width, which zoom does not change; 32 px of ground beside it.
  const width = page.offsetWidth;
  const fit = Math.min(1, (pagesFrame.clientWidth - 32) / width);
  doc.documentElement.style.setProperty("--nomo-fit", String(Math.max(0.25, fit)));
}

/**
 * The editor's own additions to the frame's document, both screen-only.
 *
 * The pages are scaled to the pane (see `fitPages`), and the ground they sit
 * on is white. The paged document puts its pages on a grey ground, which
 * suits a document opened on its own; in the pane it showed as a grey band
 * above the first page and below the last, around white pages. Added here
 * rather than written into the document, which is the one `nomo html --paged`
 * writes and `compare-html.mjs` holds to it byte for byte. Added on load, not
 * when Folio finishes, so the grey never shows while the pages are being laid
 * out; Folio drops `@media screen` while it measures, so neither rule can move
 * a page break.
 */
function frameStyle(doc) {
  if (doc.getElementById("nomo-frame")) return;
  const style = doc.createElement("style");
  style.id = "nomo-frame";
  style.textContent =
    "@media screen {\n" +
    "  html { background: #fff; }\n" +
    "  .pagedjs_pages { zoom: var(--nomo-fit, 1); }\n" +
    "}";
  doc.head.append(style);
}

function schedulePages() {
  clearTimeout(pages.timer);
  pages.timer = setTimeout(() => {
    layOutPages().catch((error) => status(`pages: ${error.message}`, "bad"));
  }, PAGES_SETTLE);
}

/** Show or hide the Pages view. Showing lays the pages out at once. */
function showPages(shown) {
  pages.shown = shown;
  buttons.pages.checked = shown;
  output.hidden = shown;
  pagesFrame.hidden = !shown;
  outputPane.classList.toggle("paged", shown);
  clearTimeout(pages.timer);
  if (!shown) return Promise.resolve(0);
  return layOutPages();
}

/**
 * Print the worksheet as pages.
 *
 * Shows the Pages view first, so what prints is what the reader can see, and
 * prints only once Folio has finished: a print taken mid-pagination is the
 * one thing this view exists to prevent. The frame prints itself, which gives
 * the pages and nothing of the editor around them.
 */
async function commandPrint() {
  status("laying out pages…");
  let count;
  try {
    count = await showPages(true);
  } catch (error) {
    status(`could not lay out the pages: ${error.message}`, "bad");
    return;
  }
  status(`${count} page${count === 1 ? "" : "s"}`, "good", true);
  pagesFrame.contentWindow.print();
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
        // The panel at the top, where the eye already is when it looks for the
        // toolbar, rather than below a long worksheet.
        search({ top: true }),
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
          // Ctrl+F with the focus in the source opens the editor's find, which
          // sees the whole document; elsewhere it stays the browser's, which
          // is right for the results pane, since that pane is rendered whole.
          ...searchKeymap,
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
  buttons.pages.addEventListener("change", () => {
    showPages(buttons.pages.checked).catch((error) => status(`pages: ${error.message}`, "bad"));
  });
  buttons.print.addEventListener("click", () => void commandPrint());
  // The pane changes width with the window and with the layout's breakpoint.
  new ResizeObserver(() => {
    if (pages.shown) fitPages();
  }).observe(pagesFrame);
  window.addEventListener("keydown", onPrintKey, { capture: true });
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
