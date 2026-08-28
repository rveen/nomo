//! The WebAssembly boundary.
//!
//! `nomo-core` compiles to `wasm32-unknown-unknown` unchanged — it has no I/O by
//! construction — so this crate is only the edge: it moves strings across the
//! linear-memory boundary and does nothing else. No calculation happens here, and
//! nothing here may make a decision that could differ from the native build.
//!
//! # No wasm-bindgen
//!
//! The plan named `wasm-bindgen`, and this uses a plain C ABI instead. Three
//! reasons, in order of weight:
//!
//! 1. **The artifact imports nothing at all.** `scripts/check-wasm.mjs`
//!    asserts an empty import section, which is a far stronger statement than
//!    "imports no math functions" and is checkable in one line. With generated JS
//!    glue in between, the guarantee would be about the glue as much as the
//!    module.
//! 2. **No build-time tool to pin.** `wasm-bindgen` needs its CLI at a version
//!    matched to the crate. The design note's objection to EngineeringPaper.xyz
//!    is a build that depends on a pinned external generator (§10); adopting one
//!    here would be the same mistake in a different language. `cargo build
//!    --target wasm32-unknown-unknown` is the whole build.
//! 3. It runs today in any host with a `WebAssembly` implementation, including
//!    plain Node, which is what makes the cross-target comparison possible
//!    without a browser.
//!
//! The cost is that the boundary is bytes rather than typed values. For a
//! worksheet application that is barely a cost: source text goes in, rendered
//! text comes out. If phase 8 wants richer traffic it can serialise, and adding
//! `wasm-bindgen` later changes nothing about the engine.
//!
//! # The calling convention
//!
//! Strings cross as `(pointer, length)` pairs of UTF-8 bytes. Results come back
//! as a single pointer to a buffer laid out as a little-endian `u32` length
//! followed by that many bytes; the caller reads the length, copies the bytes,
//! and calls [`nomo_free`] with the same pointer. One allocation per call, no
//! globals, and nothing retained between calls.

use std::alloc::{alloc, dealloc, Layout};

/// Bytes prefixed to a returned buffer to carry its length.
const HEADER: usize = 4;

/// Reserve `len` bytes for the host to write into.
///
/// # Safety
///
/// The caller must eventually pass the returned pointer to [`nomo_free`] with
/// the same length.
#[no_mangle]
pub extern "C" fn nomo_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    match Layout::from_size_align(len, 1) {
        // SAFETY: the layout has a non-zero size, checked above.
        Ok(layout) => unsafe { alloc(layout) },
        Err(_) => std::ptr::null_mut(),
    }
}

/// Release a buffer obtained from [`nomo_alloc`] or [`nomo_snapshot`].
///
/// # Safety
///
/// `ptr` must have come from this module with the same `len`, and must not be
/// used afterwards.
#[no_mangle]
pub unsafe extern "C" fn nomo_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    if let Ok(layout) = Layout::from_size_align(len, 1) {
        dealloc(ptr, layout);
    }
}

/// Render a worksheet to its golden snapshot.
///
/// This is deliberately the same `nomo_core::golden::snapshot` the native CLI
/// calls. The cross-target comparison is only meaningful if both sides run the
/// same code: if this crate reimplemented any part of the rendering, the
/// comparison would be testing this crate rather than the arithmetic.
///
/// Returns a length-prefixed buffer, or null if either input is not UTF-8.
///
/// # Safety
///
/// Both pointers must address at least the stated number of readable bytes.
#[no_mangle]
pub unsafe extern "C" fn nomo_snapshot(
    name_ptr: *const u8,
    name_len: usize,
    source_ptr: *const u8,
    source_len: usize,
) -> *mut u8 {
    let Some(name) = str_from(name_ptr, name_len) else {
        return std::ptr::null_mut();
    };
    let Some(source) = str_from(source_ptr, source_len) else {
        return std::ptr::null_mut();
    };
    into_buffer(nomo_core::snapshot(name, source))
}

/// The engine's format version, so a host can refuse a module it does not
/// understand instead of misreading its output.
#[no_mangle]
pub extern "C" fn nomo_snapshot_format() -> u32 {
    nomo_core::golden::FORMAT
}

// ---- the editing session -------------------------------------------------
//
// `nomo_snapshot` above is stateless, which is right for rendering a file. An
// editor is not: it edits one worksheet many times, and phase 4 built a
// dependency graph precisely so that an edit recomputes one statement and its
// dependents rather than the document. Reaching that requires the `Sheet` to
// live between calls, so the host holds an opaque handle to one.
//
// The handle is a leaked `Box`. Nothing else in the module retains state, and a
// host that drops a handle without freeing it leaks that worksheet and nothing
// more — in a browser tab that ends when the tab does.

/// Open an editing session on `source`. Returns a handle, or null on bad input.
///
/// Pair every call with [`nomo_document_free`].
///
/// # Safety
///
/// `ptr` must address at least `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn nomo_document_new(ptr: *const u8, len: usize) -> *mut nomo_core::Sheet {
    match str_from(ptr, len) {
        Some(source) => Box::into_raw(Box::new(nomo_core::Sheet::new(source))),
        None => std::ptr::null_mut(),
    }
}

/// Apply an edit and return the analysis as length-prefixed JSON.
///
/// Goes through `Sheet::update`, so only the statements that changed and the
/// statements downstream of them are re-evaluated.
///
/// # Safety
///
/// `handle` must come from [`nomo_document_new`] and not yet be freed; `ptr`
/// must address at least `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn nomo_document_update(
    handle: *mut nomo_core::Sheet,
    ptr: *const u8,
    len: usize,
) -> *mut u8 {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    let Some(source) = str_from(ptr, len) else {
        return std::ptr::null_mut();
    };
    let sheet = &mut *handle;
    let recalculation = sheet.update(source);
    into_buffer(with_recalculation(
        nomo_core::api::analysis_json(sheet),
        &recalculation,
    ))
}

/// Close an editing session.
///
/// # Safety
///
/// `handle` must come from [`nomo_document_new`] and must not be used again.
#[no_mangle]
pub unsafe extern "C" fn nomo_document_free(handle: *mut nomo_core::Sheet) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}

/// The text to write to disk for `source`: the worksheet with a version pragma.
///
/// The front end asks rather than composing the line itself. The version number
/// and the pragma's spelling are facts about the format, and a JavaScript
/// function writing `' nomo 1` would still be writing `1` long after the engine
/// moved on.
///
/// # Safety
///
/// `ptr` must address at least `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn nomo_for_saving(ptr: *const u8, len: usize) -> *mut u8 {
    match str_from(ptr, len) {
        Some(source) => into_buffer(nomo_core::doc::stamp_version(source)),
        None => std::ptr::null_mut(),
    }
}

/// Splice the recalculation counts into the analysis payload.
///
/// They belong to the editing session rather than to the sheet, so the engine's
/// `analysis_json` does not know them. Kept out here rather than threading an
/// optional argument through the engine for the benefit of one caller.
fn with_recalculation(json: String, r: &nomo_core::Recalculation) -> String {
    let tail = format!(
        ",\"recalculated\":{},\"changed\":{},\"structural\":{}}}",
        r.evaluated.len(),
        r.changed.len(),
        r.structural
    );
    let mut json = json;
    json.pop(); // the closing brace
    json.push_str(&tail);
    json
}

/// # Safety
///
/// `ptr` must address at least `len` readable bytes.
unsafe fn str_from<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if len == 0 {
        return Some("");
    }
    if ptr.is_null() {
        return None;
    }
    std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).ok()
}

/// Copy `text` into a freshly allocated little-endian length-prefixed buffer.
fn into_buffer(text: String) -> *mut u8 {
    let bytes = text.as_bytes();
    let total = HEADER + bytes.len();
    let Ok(layout) = Layout::from_size_align(total, 1) else {
        return std::ptr::null_mut();
    };

    // SAFETY: `total` is at least HEADER, so the layout is non-zero, and the
    // writes below stay inside the allocation.
    unsafe {
        let buffer = alloc(layout);
        if buffer.is_null() {
            return buffer;
        }
        let len = bytes.len() as u32;
        std::ptr::copy_nonoverlapping(len.to_le_bytes().as_ptr(), buffer, HEADER);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.add(HEADER), bytes.len());
        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the boundary the way a host does, so the round trip is tested on
    /// the native target too rather than only under a WebAssembly engine.
    fn round_trip(name: &str, source: &str) -> Option<String> {
        // SAFETY: both slices outlive the call.
        let buffer =
            unsafe { nomo_snapshot(name.as_ptr(), name.len(), source.as_ptr(), source.len()) };
        if buffer.is_null() {
            return None;
        }
        // SAFETY: the buffer is the layout `into_buffer` wrote.
        unsafe {
            let mut header = [0u8; HEADER];
            std::ptr::copy_nonoverlapping(buffer, header.as_mut_ptr(), HEADER);
            let len = u32::from_le_bytes(header) as usize;
            let bytes = std::slice::from_raw_parts(buffer.add(HEADER), len).to_vec();
            nomo_free(buffer, HEADER + len);
            Some(String::from_utf8(bytes).expect("snapshots are UTF-8"))
        }
    }

    #[test]
    fn the_boundary_returns_what_the_engine_returns() {
        let source = "r = 5 cm\nh = 12 cm\nV = pi*r^2*h\n";
        assert_eq!(
            round_trip("cylinder", source).as_deref(),
            Some(nomo_core::snapshot("cylinder", source).as_str())
        );
    }

    #[test]
    fn an_empty_worksheet_crosses_cleanly() {
        assert!(round_trip("empty", "").is_some());
    }

    #[test]
    fn non_utf8_input_is_rejected_rather_than_guessed() {
        let bad = [0xffu8, 0xfe];
        // SAFETY: the slice outlives the call.
        let buffer = unsafe { nomo_snapshot(bad.as_ptr(), bad.len(), bad.as_ptr(), bad.len()) };
        assert!(buffer.is_null());
    }

    #[test]
    fn multibyte_text_survives_the_round_trip() {
        // The boundary counts bytes; the engine counts characters. A worksheet
        // full of `π`, `°` and `·` is the case where confusing the two shows.
        let source = "θ = 30 °\nΔ = 5 µm\nV = sin(θ)\n";
        let crossed = round_trip("unicode", source).expect("crossed");
        assert!(crossed.contains('°'), "{crossed}");
        assert_eq!(crossed, nomo_core::snapshot("unicode", source));
    }

    #[test]
    fn allocation_of_nothing_is_null_not_a_dangling_pointer() {
        assert!(nomo_alloc(0).is_null());
        // Freeing it must be harmless.
        // SAFETY: null with zero length is the documented no-op.
        unsafe { nomo_free(std::ptr::null_mut(), 0) };
    }

    #[test]
    fn the_format_version_matches_the_engine() {
        assert_eq!(nomo_snapshot_format(), nomo_core::golden::FORMAT);
    }

    /// Drive an editing session the way the browser does.
    fn session(initial: &str, edits: &[&str]) -> Vec<String> {
        // SAFETY: the slice outlives the call.
        let handle = unsafe { nomo_document_new(initial.as_ptr(), initial.len()) };
        assert!(!handle.is_null());
        let mut out = Vec::new();
        for edit in edits {
            // SAFETY: the handle is live and the slice outlives the call.
            let buffer = unsafe { nomo_document_update(handle, edit.as_ptr(), edit.len()) };
            assert!(!buffer.is_null());
            // SAFETY: the buffer is the layout `into_buffer` wrote.
            unsafe {
                let mut header = [0u8; HEADER];
                std::ptr::copy_nonoverlapping(buffer, header.as_mut_ptr(), HEADER);
                let len = u32::from_le_bytes(header) as usize;
                let bytes = std::slice::from_raw_parts(buffer.add(HEADER), len).to_vec();
                nomo_free(buffer, HEADER + len);
                out.push(String::from_utf8(bytes).expect("payload is UTF-8"));
            }
        }
        // SAFETY: the handle is live and is not used again.
        unsafe { nomo_document_free(handle) };
        out
    }

    #[test]
    fn an_edit_returns_an_analysis() {
        let payloads = session("r = 5 cm\n", &["r = 6 cm\n"]);
        assert!(payloads[0].contains("\"tokens\":["), "{}", payloads[0]);
        assert!(
            payloads[0].contains("\"hasErrors\":false"),
            "{}",
            payloads[0]
        );
    }

    #[test]
    fn editing_one_line_does_not_recompute_the_worksheet() {
        // The point of holding a Sheet across calls. `a` changes, `b` reads it,
        // `c` does not — so two statements are re-evaluated, not three.
        let payloads = session("a = 1\nb = a*2\nc = 99\n", &["a = 5\nb = a*2\nc = 99\n"]);
        assert!(
            payloads[0].contains("\"changed\":1"),
            "one line changed:\n{}",
            payloads[0]
        );
        assert!(
            payloads[0].contains("\"recalculated\":2"),
            "`a` and its dependent `b`, but not `c`:\n{}",
            payloads[0]
        );
        assert!(
            payloads[0].contains("\"structural\":false"),
            "{}",
            payloads[0]
        );
    }

    #[test]
    fn adding_a_line_is_reported_as_structural() {
        let payloads = session("a = 1\n", &["a = 1\nb = 2\n"]);
        assert!(
            payloads[0].contains("\"structural\":true"),
            "{}",
            payloads[0]
        );
    }

    #[test]
    fn a_session_survives_a_worksheet_that_stops_parsing_midway() {
        // Every keystroke is an edit, so most of them land on a document that is
        // briefly nonsense. The session must keep going and keep reporting.
        let payloads = session("x = 1\n", &["x = 1 +\n", "x = 1 + \n", "x = 1 + 2\n"]);
        assert!(
            payloads[0].contains("\"hasErrors\":true"),
            "{}",
            payloads[0]
        );
        assert!(
            payloads[2].contains("\"hasErrors\":false"),
            "{}",
            payloads[2]
        );
    }

    #[test]
    fn a_null_handle_is_refused_rather_than_dereferenced() {
        let source = "x = 1\n";
        // SAFETY: passing null is the documented rejection path.
        let buffer =
            unsafe { nomo_document_update(std::ptr::null_mut(), source.as_ptr(), source.len()) };
        assert!(buffer.is_null());
        // SAFETY: freeing null is a documented no-op.
        unsafe { nomo_document_free(std::ptr::null_mut()) };
    }
}
