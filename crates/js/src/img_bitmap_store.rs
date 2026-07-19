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
//!
//! ## BUG-272 срез 20 — shared `Arc<Image>`, lazy RGBA8
//!
//! Every decoded `<img>` on a page used to be eagerly copied into this store as a
//! freshly-converted RGBA8 `Vec<u8>` — a full second resident copy of pixels that
//! already live in the shell's decoded-image cache (as `Arc<lumen_image::Image>`)
//! and its renderer upload, even though the vast majority of images are never a
//! `drawImage` source.  Now the store shares the shell's `Arc<Image>` across the
//! thread boundary (`Arc` is `Send + Sync`, a pointer clone, no pixel copy) and
//! materialises the RGBA8 view **lazily**, only when a canvas actually reads the
//! bitmap — cached thereafter so repeated `drawImage` of the same source (canvas
//! animation) re-converts at most once.  Images decoded but never drawn onto a
//! canvas cost zero extra bytes here.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use lumen_image::Image;

/// One registered `<img>` bitmap: the source image shared with the shell decode
/// cache (native pixel format, no copy) plus a lazily-materialised RGBA8 view.
struct BitmapEntry {
    /// Decoded source image, shared `Arc` with the shell's per-navigation cache.
    image: Arc<Image>,
    /// RGBA8 (tone-mapped) pixels, converted from `image` on the first canvas read
    /// and cached for subsequent reads. `None` until first drawn onto a canvas.
    rgba8: RefCell<Option<Vec<u8>>>,
}

thread_local! {
    static IMG_BITMAPS: RefCell<HashMap<u32, BitmapEntry>> = RefCell::new(HashMap::new());
}

/// Store a decoded `<img>` element identified by its node id.
///
/// Shares the shell's `Arc<Image>` — no pixel copy at registration.  The RGBA8
/// view is materialised lazily on the first [`with_img_bitmap`] read.  Previous
/// entry for `nid` is overwritten.
pub fn set_img_bitmap(nid: u32, image: Arc<Image>) {
    IMG_BITMAPS.with(|m| {
        m.borrow_mut()
            .insert(nid, BitmapEntry { image, rgba8: RefCell::new(None) });
    });
}

/// Call `f` with `(natural_width, natural_height, rgba8_slice)` for `nid`.
///
/// Returns `Some(f(…))` when the bitmap is registered, `None` otherwise
/// (image not yet decoded or not an `<img>` element at all).  The RGBA8 slice is
/// materialised from the shared `Arc<Image>` on the first call and cached, so the
/// closure sees the same bytes the eager path used to provide.
pub fn with_img_bitmap<R>(nid: u32, f: impl FnOnce(u32, u32, &[u8]) -> R) -> Option<R> {
    IMG_BITMAPS.with(|m| {
        let m = m.borrow();
        let entry = m.get(&nid)?;
        let mut cache = entry.rgba8.borrow_mut();
        let pixels = cache.get_or_insert_with(|| entry.image.to_rgba8());
        Some(f(entry.image.width, entry.image.height, pixels))
    })
}

/// Remove all registered bitmaps (call at the start of each navigation to
/// release memory from the previous page).
pub fn clear_img_bitmaps() {
    IMG_BITMAPS.with(|m| m.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_image::{Image, PixelFormat};

    fn rgba8_img(width: u32, height: u32, data: Vec<u8>) -> Arc<Image> {
        Arc::new(Image { width, height, format: PixelFormat::Rgba8, data, icc_profile: None })
    }

    #[test]
    fn stores_and_reads_shared_bitmap() {
        clear_img_bitmaps();
        let px = vec![10u8, 20, 30, 40];
        set_img_bitmap(7, rgba8_img(1, 1, px.clone()));
        let got = with_img_bitmap(7, |w, h, pixels| (w, h, pixels.to_vec()));
        assert_eq!(got, Some((1, 1, px)));
        clear_img_bitmaps();
    }

    #[test]
    fn missing_bitmap_returns_none() {
        clear_img_bitmaps();
        assert!(with_img_bitmap(999, |_, _, _| ()).is_none());
    }

    #[test]
    fn rgba8_materialised_lazily_and_cached() {
        clear_img_bitmaps();
        // Rgba8 source with no ICC profile: to_rgba8() is a byte-identical copy,
        // so the store hands back exactly the source pixels.
        let px = vec![1u8, 2, 3, 255, 4, 5, 6, 255];
        set_img_bitmap(3, rgba8_img(2, 1, px.clone()));
        // First read materialises the RGBA8 view; second read hits the cache.
        let first = with_img_bitmap(3, |_, _, pixels| pixels.to_vec());
        let second = with_img_bitmap(3, |_, _, pixels| pixels.to_vec());
        assert_eq!(first, Some(px.clone()));
        assert_eq!(second, Some(px));
        clear_img_bitmaps();
    }

    #[test]
    fn rgb8_source_expands_to_rgba8() {
        clear_img_bitmaps();
        // A 3-bytes-per-pixel Rgb8 source is stored without an eager copy; the
        // store expands it to opaque RGBA8 on read (matching the shell's old path).
        let rgb = vec![9u8, 8, 7];
        let img = Arc::new(Image {
            width: 1,
            height: 1,
            format: PixelFormat::Rgb8,
            data: rgb,
            icc_profile: None,
        });
        set_img_bitmap(5, img);
        let got = with_img_bitmap(5, |w, h, pixels| (w, h, pixels.to_vec()));
        assert_eq!(got, Some((1, 1, vec![9u8, 8, 7, 255])));
        clear_img_bitmaps();
    }
}
