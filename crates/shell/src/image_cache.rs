//! Process-global per-navigation decoded-image cache (BUG-172).
//!
//! `<img src>` resources are fetched and decoded by two independent code paths
//! that did not know about each other, so every image was downloaded and decoded
//! **twice** per navigation:
//!
//! 1. **Progressive, during streaming** ([`spawn_stream_image_loads`]): background
//!    threads spawned per intermediate frame fetch+decode `<img>` found in the
//!    partial DOM and push `LoadEvent::ImageDecoded` so images appear as they
//!    arrive.
//! 2. **Final full pipeline** ([`fetch_and_decode_images`] inside
//!    `parse_and_layout` on `LoadEvent::LoadDone`): re-fetches and re-decodes
//!    *all* non-lazy images via `parallel_map`.
//!
//! This cache makes both paths share one fetch+decode per image. Whichever path
//! decodes a given `src` first fills the slot; the other reads the decoded pixels
//! instead of touching the network or the decoder.
//!
//! Design invariants (mirroring [`crate::prefetch`]):
//!
//! * **Decode once.** A cache *hit* returns the already-decoded image; the consumer
//!   only clones the pixels (a memcpy) instead of re-running the network round-trip
//!   and the decoder. A *miss* runs the supplied decode closure — never wrong
//!   pixels.
//! * **In-flight dedup.** The first caller for a `src` runs the decode and fills the
//!   slot; concurrent callers block until it finishes and share the result. This is
//!   what prevents the streaming warm-up and the final pass from doing the same work
//!   when their timing overlaps.
//! * **Shared outcome (including failure).** Both paths fetch through the identical
//!   `fetch_image_bytes` → decode path, so a failure for one is a failure for the
//!   other; caching the `None` outcome lets waiters share it instead of stampeding a
//!   resource that just failed.
//! * **Generation-scoped.** Each navigation bumps a generation;
//!   [`DecodedImageCache::reset`] clears all slots. A stale producer thread (from a
//!   superseded navigation) bypasses the cache and never pollutes the current page.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, LazyLock, Mutex};

/// Decoded image payload shared between the streaming progressive loader and the
/// final page pipeline. Held behind `Arc` so a cache hit clones a pointer, not the
/// pixel buffer, until a consumer actually needs an owned copy.
#[derive(Clone)]
pub enum DecodedImage {
    /// Static image (includes a single-frame GIF flattened to one image).
    Static(Arc<lumen_image::Image>),
    /// Multi-frame animated GIF: first frame plus the full animation.
    Animated {
        /// First frame, used as the initial paint before the animation ticks.
        first: Arc<lumen_image::Image>,
        /// Full decoded animation, ticked on the UI thread.
        gif: Arc<lumen_image::AnimatedGif>,
    },
}

/// Per-slot stored outcome: `Some` on a successful decode, `None` when fetch or
/// decode failed. Cloneable so every waiter on an in-flight slot sees the same
/// result.
type CachedDecode = Option<DecodedImage>;

/// One cache entry. `state` is `None` while the decode is in flight and `Some`
/// once the first caller finished; `cv` wakes blocked waiters.
struct Slot {
    state: Mutex<Option<CachedDecode>>,
    cv: Condvar,
}

impl Slot {
    fn new() -> Self {
        Self { state: Mutex::new(None), cv: Condvar::new() }
    }
}

/// Mutable cache contents guarded by a single lock.
struct Inner {
    /// Navigation generation these slots belong to.
    generation: u64,
    /// Image `src` (as emitted by layout in `DrawImage`) → slot. Key must match
    /// between producer and consumer; both use the raw `src` from
    /// `collect_image_requests`, resolved against the same per-navigation base.
    slots: HashMap<String, Arc<Slot>>,
}

/// Shared, generation-scoped decoded-image cache for page `<img>` resources.
/// See module docs.
pub struct DecodedImageCache {
    inner: Mutex<Inner>,
}

impl DecodedImageCache {
    fn new() -> Self {
        Self { inner: Mutex::new(Inner { generation: 0, slots: HashMap::new() }) }
    }

    /// Drop all cached entries and adopt navigation `generation`.
    ///
    /// Called on the UI thread at navigation start (alongside
    /// [`crate::prefetch::PrefetchCache::reset`]) so the streaming producers and the
    /// final consumer all observe the same generation for one navigation.
    pub fn reset(&self, generation: u64) {
        let mut inner = self.inner.lock().unwrap();
        inner.generation = generation;
        inner.slots.clear();
    }

    /// Drop all cached entries and bump to a fresh generation.
    ///
    /// For headless render entry points (`--screenshot`, the `--ipc-server`
    /// `Screenshot` command) that have no `load_generation`: each render is its own
    /// "navigation", so this clears the previous render's images (bounding memory in
    /// the long-lived IPC server) and guarantees no stale cross-page reuse.
    pub fn reset_new(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.generation = inner.generation.wrapping_add(1);
        inner.slots.clear();
    }

    /// The navigation generation the cache is currently scoped to.
    pub fn current_generation(&self) -> u64 {
        self.inner.lock().unwrap().generation
    }

    /// Decode `src` through the cache for navigation `generation`.
    ///
    /// The first caller for a given `(generation, src)` runs `decode` and fills the
    /// slot; concurrent callers block until it completes and share the result. When
    /// `generation` no longer matches the cache's current generation (a newer
    /// navigation already reset it), the call bypasses the cache entirely and runs
    /// `decode` directly — a stale producer never pollutes the current page.
    pub fn get_or_decode(
        &self,
        generation: u64,
        src: &str,
        decode: impl FnOnce() -> CachedDecode,
    ) -> CachedDecode {
        let (slot, is_filler) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.generation != generation {
                drop(inner);
                return decode();
            }
            if let Some(existing) = inner.slots.get(src) {
                (Arc::clone(existing), false)
            } else {
                let slot = Arc::new(Slot::new());
                inner.slots.insert(src.to_owned(), Arc::clone(&slot));
                (slot, true)
            }
        };

        if is_filler {
            // Run the (slow: network + decode) work WITHOUT holding any lock, then
            // publish the result and wake waiters.
            let result = decode();
            let mut state = slot.state.lock().unwrap();
            *state = Some(result.clone());
            slot.cv.notify_all();
            result
        } else {
            let mut state = slot.state.lock().unwrap();
            while state.is_none() {
                state = slot.cv.wait(state).unwrap();
            }
            state.clone().unwrap()
        }
    }

    /// Convenience for the UI-thread consumer ([`fetch_and_decode_images`]): decode
    /// using the cache's current generation.
    ///
    /// The consumer runs synchronously on the UI thread for the navigation being
    /// rendered, so the current generation is stable for the duration of the call.
    pub fn get_or_decode_current(
        &self,
        src: &str,
        decode: impl FnOnce() -> CachedDecode,
    ) -> CachedDecode {
        let generation = self.current_generation();
        self.get_or_decode(generation, src, decode)
    }
}

/// Process-global decoded-image cache shared between the streaming image loader and
/// the final page pipeline. Reset per navigation via [`DecodedImageCache::reset`].
pub static IMAGE_CACHE: LazyLock<DecodedImageCache> = LazyLock::new(DecodedImageCache::new);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;

    fn dummy_image(w: u32) -> DecodedImage {
        DecodedImage::Static(Arc::new(lumen_image::Image {
            width: w,
            height: 1,
            format: lumen_image::PixelFormat::Rgba8,
            data: vec![0; (w * 4) as usize],
            icc_profile: None,
        }))
    }

    fn width_of(d: &DecodedImage) -> u32 {
        match d {
            DecodedImage::Static(img) => img.width,
            DecodedImage::Animated { first, .. } => first.width,
        }
    }

    #[test]
    fn first_caller_fills_then_hit_reuses() {
        let cache = DecodedImageCache::new();
        cache.reset(1);
        let calls = AtomicUsize::new(0);

        let first = cache.get_or_decode(1, "a.png", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Some(dummy_image(10))
        });
        assert_eq!(width_of(&first.unwrap()), 10);

        // Second call for the same src must NOT run the closure again.
        let second = cache.get_or_decode(1, "a.png", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Some(dummy_image(99))
        });
        assert_eq!(width_of(&second.unwrap()), 10);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reset_clears_and_changes_generation() {
        let cache = DecodedImageCache::new();
        cache.reset(1);
        let _ = cache.get_or_decode(1, "a.png", || Some(dummy_image(1)));

        cache.reset(2);
        assert_eq!(cache.current_generation(), 2);
        let after = cache.get_or_decode(2, "a.png", || Some(dummy_image(2)));
        assert_eq!(width_of(&after.unwrap()), 2);
    }

    #[test]
    fn reset_new_bumps_generation() {
        let cache = DecodedImageCache::new();
        cache.reset(5);
        let _ = cache.get_or_decode(5, "a.png", || Some(dummy_image(1)));
        cache.reset_new();
        assert_eq!(cache.current_generation(), 6);
        // Old slot cleared → closure runs again under the new generation.
        let calls = AtomicUsize::new(0);
        let _ = cache.get_or_decode(6, "a.png", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Some(dummy_image(1))
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn stale_generation_bypasses_cache() {
        let cache = DecodedImageCache::new();
        cache.reset(5);
        let calls = AtomicUsize::new(0);

        // Producer from generation 4 (superseded) — runs uncached, inserts no slot.
        let stale = cache.get_or_decode(4, "a.png", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Some(dummy_image(4))
        });
        assert_eq!(width_of(&stale.unwrap()), 4);

        // Current generation 5 sees no entry → runs its own decode.
        let fresh = cache.get_or_decode(5, "a.png", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Some(dummy_image(5))
        });
        assert_eq!(width_of(&fresh.unwrap()), 5);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn cached_failure_is_shared() {
        let cache = DecodedImageCache::new();
        cache.reset(1);
        let calls = AtomicUsize::new(0);

        let first = cache.get_or_decode(1, "bad.png", || {
            calls.fetch_add(1, Ordering::SeqCst);
            None
        });
        assert!(first.is_none());

        let second = cache.get_or_decode(1, "bad.png", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Some(dummy_image(1))
        });
        assert!(second.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn concurrent_callers_dedup_to_one_decode() {
        let cache = Arc::new(DecodedImageCache::new());
        cache.reset(1);
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(8));

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let calls = Arc::clone(&calls);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    let img = cache.get_or_decode(1, "shared.png", || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(30));
                        Some(dummy_image(7))
                    });
                    assert_eq!(width_of(&img.unwrap()), 7);
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
