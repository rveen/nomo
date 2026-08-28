// Getting worksheets on and off the machine.
//
// Two separate concerns that are easy to confuse:
//
//   * **Files** are the user's documents. They live wherever the user put them
//     and the application does not own them.
//   * **The draft** is whatever is currently in the editor, kept in IndexedDB so
//     that closing the tab and coming back does not lose work. It is a safety
//     net, not storage — the file on disk is the document.
//
// # Why there are two ways to open a file
//
// The File System Access API is the good one: it yields a handle, so Save writes
// back to the file that was opened instead of dropping another copy in
// ~/Downloads. Chrome and Edge have it; Firefox and Safari do not, and
// CONTRIBUTING-style cross-browser support is a hard requirement inherited from
// EngineeringPaper.xyz (design note §11 item 6). So there is a fallback built on
// `<input type="file">` and a download link, which works everywhere and cannot
// write back.
//
// The difference is visible in the interface rather than hidden: with a handle
// there is a Save; without one there is only Download.

const DB_NAME = "nomo";
const DB_VERSION = 1;
const STORE = "drafts";
const DRAFT_KEY = "current";

/** Whether this browser can write back to a file the user opened. */
export const canWriteFiles =
  typeof window !== "undefined" &&
  typeof window.showOpenFilePicker === "function" &&
  typeof window.showSaveFilePicker === "function";

const FILE_TYPES = [
  {
    description: "Nomo worksheet",
    accept: { "text/plain": [".nomo"] },
  },
];

/**
 * Ask the user for a worksheet.
 *
 * Resolves to `{name, text, handle}`, or null if the user cancelled. `handle` is
 * null when the browser cannot write back.
 */
export async function openWorksheet() {
  if (canWriteFiles) {
    let handles;
    try {
      handles = await window.showOpenFilePicker({ types: FILE_TYPES });
    } catch (error) {
      // The picker rejects with AbortError when dismissed, which is not a
      // failure and must not surface as one.
      if (error.name === "AbortError") return null;
      throw error;
    }
    const [handle] = handles;
    const file = await handle.getFile();
    return { name: file.name, text: await file.text(), handle };
  }

  const file = await pickWithInput();
  if (!file) return null;
  return { name: file.name, text: await file.text(), handle: null };
}

/** The `<input type="file">` fallback, wrapped to look like a picker. */
function pickWithInput() {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".nomo,text/plain";
    input.style.display = "none";
    document.body.append(input);

    // There is no cancel event for a file input in any browser, so a dismissed
    // dialog leaves this promise pending forever unless something else settles
    // it. `focus` fires when the dialog closes either way; a change event beats
    // it when a file was chosen.
    let settled = false;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      input.remove();
      resolve(value);
    };

    input.addEventListener("change", () => finish(input.files[0] ?? null));
    window.addEventListener(
      "focus",
      () => setTimeout(() => finish(input.files[0] ?? null), 300),
      { once: true },
    );
    input.click();
  });
}

/**
 * Write `text` back to `handle`.
 *
 * Only reachable when [`canWriteFiles`] is true.
 */
export async function writeWorksheet(handle, text) {
  const writable = await handle.createWritable();
  await writable.write(text);
  await writable.close();
}

/**
 * Ask where to put a worksheet, then write it.
 *
 * Returns the new handle, or null if the user cancelled.
 */
export async function saveWorksheetAs(text, suggestedName) {
  if (!canWriteFiles) {
    download(text, suggestedName);
    return null;
  }
  let handle;
  try {
    handle = await window.showSaveFilePicker({
      suggestedName,
      types: FILE_TYPES,
    });
  } catch (error) {
    if (error.name === "AbortError") return null;
    throw error;
  }
  await writeWorksheet(handle, text);
  return handle;
}

/** The everywhere-fallback for getting a file out of the browser. */
export function download(text, name) {
  const url = URL.createObjectURL(
    new Blob([text], { type: "text/plain;charset=utf-8" }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  document.body.append(link);
  link.click();
  link.remove();
  // Revoking immediately can cancel the download in some browsers; a tick is
  // enough and the object is small.
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

// ---- the draft -----------------------------------------------------------

function openDatabase() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE);
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

function transact(db, mode, fn) {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, mode);
    const request = fn(tx.objectStore(STORE));
    tx.oncomplete = () => resolve(request?.result);
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}

/**
 * Remember the editor's contents.
 *
 * Failure is swallowed: private browsing, a full disk and a denied quota all
 * make IndexedDB throw, and none of them is a reason to stop the user editing.
 * The draft is a convenience; the file is the document.
 */
export async function saveDraft(text, name) {
  try {
    const db = await openDatabase();
    await transact(db, "readwrite", (store) =>
      store.put({ text, name, at: Date.now() }, DRAFT_KEY),
    );
    db.close();
    return true;
  } catch {
    return false;
  }
}

/**
 * The remembered contents, or null if there are none or storage is unusable.
 *
 * Bounded by a timeout because startup waits for this. IndexedDB can block
 * indefinitely rather than fail — a database still open in another tab, a
 * blocked upgrade, a storage backend that never answers — and a worksheet
 * application that shows nothing at all because it is waiting to find out
 * whether there was a draft has failed at the only job that matters. After the
 * timeout the editor opens on the starting worksheet, which is the same thing a
 * first-time visitor sees.
 */
export async function loadDraft(timeoutMs = 2000) {
  const read = (async () => {
    const db = await openDatabase();
    const draft = await transact(db, "readonly", (store) =>
      store.get(DRAFT_KEY),
    );
    db.close();
    return draft ?? null;
  })();

  try {
    return await Promise.race([
      read,
      new Promise((resolve) => setTimeout(() => resolve(null), timeoutMs)),
    ]);
  } catch {
    return null;
  }
}

/** Forget the draft, when the user starts a new worksheet. */
export async function clearDraft() {
  try {
    const db = await openDatabase();
    await transact(db, "readwrite", (store) => store.delete(DRAFT_KEY));
    db.close();
  } catch {
    /* nothing to do about it */
  }
}
