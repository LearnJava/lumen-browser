//! Thread-local store for decoded `<img>` bitmaps used by Canvas 2D `drawImage`.
//!
//! The shell calls [`set_img_bitmap`] (via [`crate::QuickJsRuntime::register_img_bitmaps`])
//! after `fetch_and_decode_images`, keyed by DOM node id.  Canvas 2D native
//! bindings then look up pixels here when the `drawImage` source is an `<img>`
//! element rather than a `<canvas>`/OffscreenCanvas.
//!
//! The store is **thread-local** because the QuickJS runtime and all canvas
//! natives run on the dedicated JS thread (ADR-014).  The shell sends bitmaps
//! into the store via [`crate::QuickJsRuntime::run`], so no cross-thread
//! synchronisation is needed.

use std::cell::RefCell;
use std::collections::HashMap;

/// `(natural_width, natural_height, rgba8_pixels)` for a single `<img>` node.
type BitmapEntry = (u32, u32, Vec<u8>);

thread_local! {
    static IMG_BITMAPS: RefCell<HashMap<u32, BitmapEntry>> = RefCell::new(HashMap::new());
}

/// Store decoded RGBA8 pixels for an `<img>` element identified by its node id.
///
/// `rgba8` must be a row-major, 4-bytes-per-pixel RGBA8 buffer of length
/// `width * height * 4`.  Previous entry for `nid` is overwritten.
pub fn set_img_bitmap(nid: u32, width: u32, height: u32, rgba8: Vec<u8>) {
    IMG_BITMAPS.with(|m| {
        m.borrow_mut().insert(nid, (width, height, rgba8));
    });
}

/// Call `f` with `(natural_width, natural_height, rgba8_slice)` for `nid`.
///
/// Returns `Some(f(…))` when the bitmap is registered, `None` otherwise
/// (image not yet decoded or not an `<img>` element at all).
pub fn with_img_bitmap<R>(nid: u32, f: impl FnOnce(u32, u32, &[u8]) -> R) -> Option<R> {
    IMG_BITMAPS.with(|m| {
        let m = m.borrow();
        let (w, h, pixels) = m.get(&nid)?;
        Some(f(*w, *h, pixels))
    })
}

/// Remove all registered bitmaps (call at the start of each navigation to
/// release memory from the previous page).
pub fn clear_img_bitmaps() {
    IMG_BITMAPS.with(|m| m.borrow_mut().clear());
}
