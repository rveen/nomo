// Pasting a picture into a worksheet.
//
// A figure arrives on the clipboard as bytes and has to land in two places at
// once: a `' image <name> <w>x<h>` reference where the cursor is, and a base64
// block in the trailer at the end of the file. The engine decides both of those
// — where a reference may legally go is a question about the file format, not
// about the editor, and `crates/nomo-core/src/resource.rs` is where the format
// is described — so what is left here is the part a browser can do and Rust
// cannot: get the bytes out of a paste event, decode the image far enough to
// know how big it is, and shrink it when it is too big to place.
//
// ## Why this shrinks anything at all
//
// A screenshot off a modern display is 2560 px across and a worksheet column is
// nowhere near that wide. Carried untouched it costs several megabytes of
// base64 in the file — text the editor re-analyses after every keystroke — to
// draw a picture at a quarter of its size. `PLACED` is the width past which
// that trade stops being worth making.
//
// ## Determinism, and why re-encoding is not an exception to it
//
// The rest of this project goes to some length so that two machines get the same
// answer, and this module hands an image to the host's PNG encoder, which is not
// the same program in two browsers. That is consistent rather than a hole in the
// rule: the promise is that a *worksheet computes* identically everywhere, and
// the bytes of a photograph are an input the author supplied, like a number
// typed on a line. Two pastes of one screenshot in two browsers make two
// different files; each of those files then computes the same answer
// everywhere, which is the whole of the claim.

import { EditorView } from "@codemirror/view";

/**
 * The widest a pasted figure is placed, in pixels.
 *
 * A fixed number rather than something derived from the window: the size goes
 * into the file, and a worksheet whose figures came out at whatever width the
 * author's browser happened to be would place them differently on the next
 * machine that opened it.
 */
const PLACED = 700;

/** JPEG quality, for the one case where a re-encode stays a JPEG. */
const QUALITY = 0.9;

/**
 * Handle a pasted image; leave every other paste alone.
 *
 * The dependencies are passed in rather than imported because the engine is
 * *replaced* by a restart — a handle captured here would go on calling the
 * instance that already failed, which is the same reason `main.js` keeps it at
 * module scope.
 *
 * @param {object} deps
 * @param {() => boolean} deps.ready   whether the engine can be called at all
 * @param {Function} deps.attach       `engine.attachImage`, read at call time
 * @param {Function} deps.failed       what to do when the engine itself throws
 * @param {Function} deps.status       the status line
 */
export function imagePaste(deps) {
  return EditorView.domEventHandlers({
    paste(event, view) {
      const data = event.clipboardData;
      // The blob is taken out here, synchronously, and everything slow happens
      // afterwards: `getAsFile` only works while the event is being dispatched,
      // and the clipboard's contents are gone by the first `await`.
      const blob = imageOn(data);

      if (!blob) {
        if (data && onlyALinkToAnImage(data)) {
          // Nothing would be pasted at all otherwise, and an editor that
          // swallows a paste is worse than one that says why it cannot.
          event.preventDefault();
          deps.status(
            "the clipboard has a link to that image and not the image itself — " +
              "copy it with the browser's Copy image, or save it and paste the file",
            "warn",
          );
          return true;
        }
        return false; // ordinary text: CodeMirror pastes it as it always did
      }

      event.preventDefault();
      deps.status("placing the image…");
      void place(view, blob, deps);
      return true;
    },
  });
}

/**
 * The image on the clipboard, if there is one.
 *
 * Both lists are read because they are not the same list everywhere: a
 * screenshot taken by the operating system arrives as an item of kind `file`,
 * and a file dragged out of a file manager arrives in `files`.
 */
function imageOn(data) {
  if (!data) return null;
  for (const item of data.items ?? []) {
    if (item.kind === "file" && item.type.startsWith("image/")) {
      const file = item.getAsFile();
      if (file) return file;
    }
  }
  for (const file of data.files ?? []) {
    if (file.type.startsWith("image/")) return file;
  }
  return null;
}

/**
 * Whether this paste is a picture on a web page and nothing else.
 *
 * Copying part of a page puts HTML on the clipboard, and if the selection was
 * one image the HTML is an `<img>` whose `src` is a URL — no bytes anywhere,
 * and no plain text either, so an editor that pasted the text would paste
 * nothing. Fetching the URL is not the answer: it would be this application's
 * first network request for a document's content, and the reason nothing a
 * worksheet contains leaves the browser is that nothing is ever fetched for it.
 *
 * Deliberately narrow. A selection with an image *and* text still pastes the
 * text, because that is what was asked for.
 */
function onlyALinkToAnImage(data) {
  const html = data.getData("text/html");
  if (!html || !/<img\b/i.test(html)) return false;
  return data.getData("text/plain").trim() === "";
}

/** Decode, shrink if need be, ask the engine where it goes, and write it. */
async function place(view, blob, deps) {
  let image;
  try {
    image = await fit(blob);
  } catch (error) {
    deps.status(`that image could not be read: ${error.message}`, "bad");
    return;
  }

  // Checked here rather than before the decode because that is where it
  // matters: the decode can take a moment and the engine can fail during it.
  if (!deps.ready()) {
    deps.status("the engine is restarting — the image was not written, paste again", "warn");
    return;
  }

  // Read after the decode, not before. The author may well have gone on typing
  // while it ran, and an offset into the text as it was would put the reference
  // on the wrong line.
  const source = view.state.doc.toString();
  const cursor = view.state.selection.main.head;

  let attached;
  try {
    attached = deps.attach(source, cursor, image);
  } catch (error) {
    // A throw is the module failing, not the image being bad — a bad image
    // comes back as `error` in the answer.
    deps.failed(error);
    return;
  }
  if (attached.error) {
    deps.status(attached.error, "bad");
    return;
  }

  // Two insertions rather than a rewritten document: a worksheet carrying a few
  // figures is megabytes of text, and replacing all of it would throw away the
  // undo history and the scroll position to add two lines.
  const { reference, trailer } = attached;
  view.dispatch({
    changes: [
      { from: reference.at, insert: reference.text },
      { from: trailer.at, insert: trailer.text },
    ],
    // Measured in the document the changes produce, which is what a
    // transaction's selection is measured in. The reference is inserted first,
    // so its own text is the only thing that has moved this point.
    selection: { anchor: reference.at + reference.text.length },
    scrollIntoView: true,
  });

  deps.status(describe(attached.name, image), "good", true);
}

/**
 * Decode the image, and shrink it if it is wider than a figure is placed.
 *
 * An image small enough to place is carried byte for byte. That matters beyond
 * saving the work: it is the only path that keeps whatever the source format
 * was good at — an animated GIF goes on animating, a screenshot keeps its exact
 * pixels — and a re-encode is a lossy step taken only when the alternative is
 * worse. (An animated GIF *wider* than `PLACED` does lose its animation here,
 * which is a known gap and not a decision.)
 */
async function fit(blob) {
  const bitmap = await createImageBitmap(blob);
  try {
    if (bitmap.width <= PLACED) {
      return {
        width: bitmap.width,
        height: bitmap.height,
        bytes: new Uint8Array(await blob.arrayBuffer()),
      };
    }

    const width = PLACED;
    // Rounded, and never to zero: a 4000×3 rule is a strange thing to paste but
    // it is not an error, and the engine refuses an image placed at no height.
    const height = Math.max(1, Math.round((bitmap.height * PLACED) / bitmap.width));

    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d");
    context.imageSmoothingQuality = "high";
    context.drawImage(bitmap, 0, 0, width, height);

    // A photograph stays a photograph, and everything else becomes a PNG.
    // Re-encoding a JPEG as PNG would take a 60 KB picture to ten times that,
    // and encoding anything else as JPEG would put ringing around the text in
    // the screenshots and diagrams that are most of what anyone pastes.
    const type = blob.type === "image/jpeg" ? "image/jpeg" : "image/png";
    const shrunk = await encode(canvas, type, QUALITY);

    return {
      width,
      height,
      bytes: new Uint8Array(await shrunk.arrayBuffer()),
      from: { width: bitmap.width, height: bitmap.height },
    };
  } finally {
    // The decoded frame can be tens of megabytes and nothing else releases it.
    bitmap.close();
  }
}

/** `canvas.toBlob` as a promise, with its one failure reported rather than null. */
function encode(canvas, type, quality) {
  return new Promise((resolve, reject) => {
    canvas.toBlob(
      (blob) =>
        blob ? resolve(blob) : reject(new Error(`this browser cannot write ${type}`)),
      type,
      quality,
    );
  });
}

/**
 * What the status line says about a figure that was just written.
 *
 * The name is there because it is what the reference says and what the author
 * will type if they want the figure somewhere else too; the shrink is there
 * because it happened to their image without being asked for, and a size that
 * silently changed is exactly the kind of thing an engineering document should
 * never do quietly.
 */
function describe(name, image) {
  const shrunk = image.from ? `${image.from.width}×${image.from.height} shrunk to ` : "";
  return `${name} — ${shrunk}${image.width}×${image.height}, ${sizeOf(image.bytes.length)}`;
}

/** A size a person reads rather than a count of bytes. */
function sizeOf(bytes) {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
