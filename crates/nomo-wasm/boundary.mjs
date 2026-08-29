// The host half of the WebAssembly calling convention.
//
// This file sits beside the Rust that defines the other half, because the two
// are one contract and splitting them across the repository is how they drift.
// It knows nothing about where the module came from: Node reads it from disk,
// the browser fetches it, and both hand the instantiated exports to `bind`.
//
// The convention, mirrored from `src/lib.rs`:
//
//   * strings go in as (pointer, length) pairs of UTF-8 bytes;
//   * results come back as a pointer to a little-endian u32 length followed by
//     that many bytes;
//   * the caller frees what it was given.

const HEADER = 4;

/**
 * Wrap an instantiated module in the calls a host actually wants.
 *
 * @param {WebAssembly.Exports} exports
 */
export function bind(exports) {
  const {
    memory,
    nomo_alloc,
    nomo_free,
    nomo_snapshot,
    nomo_snapshot_format,
    nomo_document_new,
    nomo_document_update,
    nomo_document_free,
    nomo_for_saving,
  } = exports;

  const encoder = new TextEncoder();
  const decoder = new TextDecoder("utf-8", { fatal: true });

  // A call may grow the module's memory, which detaches any view taken before
  // it. Always take a fresh one rather than caching.
  const bytes = () => new Uint8Array(memory.buffer);

  function write(text) {
    const encoded = encoder.encode(text);
    if (encoded.length === 0) return { ptr: 0, len: 0 };
    const ptr = nomo_alloc(encoded.length);
    if (ptr === 0) throw new Error("nomo_alloc returned null");
    bytes().set(encoded, ptr);
    return { ptr, len: encoded.length };
  }

  function read(out) {
    if (out === 0) throw new Error("the engine rejected its input");
    const view = bytes();
    const len = new DataView(view.buffer, out, HEADER).getUint32(0, true);
    // Copy before anything can reallocate the buffer underneath us.
    const body = view.slice(out + HEADER, out + HEADER + len);
    nomo_free(out, HEADER + len);
    return decoder.decode(body);
  }

  function withText(text, fn) {
    const arg = write(text);
    try {
      return fn(arg);
    } finally {
      if (arg.ptr) nomo_free(arg.ptr, arg.len);
    }
  }

  return {
    /** The snapshot format the module speaks. */
    format: () => nomo_snapshot_format(),

    /**
     * The text to write to disk: the worksheet with a version pragma.
     *
     * Asked of the engine rather than composed here, because the version number
     * and the pragma's spelling belong to the format, not to the front end.
     */
    forSaving(source) {
      return withText(source, (s) => read(nomo_for_saving(s.ptr, s.len)));
    },

    /** Render a worksheet to its golden snapshot. Stateless. */
    snapshot(name, source) {
      return withText(name, (n) =>
        withText(source, (s) => read(nomo_snapshot(n.ptr, n.len, s.ptr, s.len))),
      );
    },

    /**
     * Open an editing session.
     *
     * The session holds the engine's `Sheet` between calls, which is what makes
     * an edit recompute one statement and its dependents rather than the whole
     * worksheet. Call `close()` when finished.
     */
    open(source) {
      const handle = withText(source, (s) => {
        const h = nomo_document_new(s.ptr, s.len);
        if (h === 0) throw new Error("could not open the worksheet");
        return h;
      });

      let open = true;
      return {
        /** Apply an edit and return the analysis. */
        update(text) {
          if (!open) throw new Error("this session is closed");
          const json = withText(text, (s) =>
            read(nomo_document_update(handle, s.ptr, s.len)),
          );
          return JSON.parse(json);
        },
        close() {
          if (!open) return;
          open = false;
          nomo_document_free(handle);
        },
      };
    },
  };
}

/** Instantiate with an empty import object, which the module requires. */
export async function instantiate(source) {
  const module =
    source instanceof WebAssembly.Module ? source : new WebAssembly.Module(source);
  return fromModule(module);
}

/**
 * Bind one instance of a compiled module, and offer a replacement for it.
 *
 * An instance that traps cannot be repaired. The trap unwinds out of Rust
 * without running any of it, so the allocator is left mid-update and every
 * later call reads memory that no longer describes itself — which is why a host
 * that catches an error from any call here must replace the instance rather than
 * carry on with it. `restart()` returns that replacement; the caller has to use
 * what it hands back, because this one stays broken.
 *
 * It re-instantiates rather than re-compiling, which gives a new linear memory
 * from bytes already compiled — no fetch, so a recovery works with the network
 * off, and necessarily the same engine, because it is the same module.
 */
export function fromModule(module) {
  const api = bind(new WebAssembly.Instance(module, {}).exports);
  api.restart = () => fromModule(module);
  return api;
}
