//! Process-global subresource prefetch cache (BUG-171, этап 1).
//!
//! The page pipeline (`parse_and_layout`) fetches external scripts, stylesheets,
//! images and fonts. Until BUG-171 those network round-trips ran synchronously on
//! the UI thread inside the `LoadEvent::LoadDone` handler, freezing the window for
//! the whole load. This cache lets the **streaming** background thread warm
//! subresource bytes (via the preload scanner) *while the HTML is still arriving*,
//! so the UI-thread consumer in `parse_and_layout` reads them instantly instead of
//! waiting on the socket.
//!
//! Design invariants:
//!
//! * **Same bytes.** A cache *hit* must return exactly the bytes the consumer would
//!   have fetched itself. Both producer (streaming thread) and consumer
//!   (`parse_and_layout`) warm/read the slot via the identical
//!   `http_client_for_subresource(...).fetch_subresource(url, dest)` path, so script
//!   order and the CSS cascade are unaffected. A cache *miss* degrades to the old
//!   behaviour (the consumer fetches the resource itself) — never wrong bytes.
//! * **In-flight dedup.** The first caller for a URL runs the fetch and fills the
//!   slot; concurrent callers block until it finishes and share one `Arc<Vec<u8>>`.
//!   This prevents the streaming warm-up and the final consumer from fetching the
//!   same URL twice.
//! * **Generation-scoped.** Each navigation bumps a generation; [`PrefetchCache::reset`]
//!   clears all slots and adopts the new generation. A stale producer thread (from a
//!   superseded navigation) bypasses the cache and never pollutes the current page.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, LazyLock, Mutex};

/// Per-slot stored outcome: shared resource bytes or an error message.
///
/// Cloneable so every waiter on an in-flight slot receives the same result.
type FetchResult = Result<Arc<Vec<u8>>, String>;

/// One cache entry. `state` is `None` while the fetch is in flight and `Some` once
/// the first caller finished; `cv` wakes blocked waiters.
struct Slot {
    state: Mutex<Option<FetchResult>>,
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
    /// Resolved-absolute-URL → slot. Key must match between producer and consumer.
    slots: HashMap<String, Arc<Slot>>,
}

/// Shared, generation-scoped byte cache for page subresources. See module docs.
pub struct PrefetchCache {
    inner: Mutex<Inner>,
}

impl PrefetchCache {
    fn new() -> Self {
        Self { inner: Mutex::new(Inner { generation: 0, slots: HashMap::new() }) }
    }

    /// Drop all cached entries and adopt navigation `generation`.
    ///
    /// Called on the UI thread at navigation start (before the streaming thread is
    /// spawned), so producer warm-ups and the consumer all observe the same
    /// generation for one navigation.
    pub fn reset(&self, generation: u64) {
        let mut inner = self.inner.lock().unwrap();
        inner.generation = generation;
        inner.slots.clear();
    }

    /// The navigation generation the cache is currently scoped to.
    pub fn current_generation(&self) -> u64 {
        self.inner.lock().unwrap().generation
    }

    /// Fetch `url` through the cache for navigation `generation`.
    ///
    /// The first caller for a given `(generation, url)` runs `fetch` and fills the
    /// slot; concurrent callers block until it completes and share the same
    /// `Arc<Vec<u8>>`. When `generation` no longer matches the cache's current
    /// generation (a newer navigation already reset it), the call bypasses the cache
    /// entirely and runs `fetch` directly — a stale producer never pollutes the
    /// current page's entries.
    ///
    /// `fetch` returns raw bytes on success or an error string; the error is cached
    /// too so waiters share one outcome instead of stampeding the network.
    pub fn fetch(
        &self,
        generation: u64,
        url: &str,
        fetch: impl FnOnce() -> Result<Vec<u8>, String>,
    ) -> FetchResult {
        let (slot, is_filler) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.generation != generation {
                drop(inner);
                return fetch().map(Arc::new);
            }
            if let Some(existing) = inner.slots.get(url) {
                (Arc::clone(existing), false)
            } else {
                let slot = Arc::new(Slot::new());
                inner.slots.insert(url.to_owned(), Arc::clone(&slot));
                (slot, true)
            }
        };

        if is_filler {
            // Run the (potentially slow, network-bound) fetch WITHOUT holding any
            // lock, then publish the result and wake waiters.
            let result = fetch().map(Arc::new);
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

    /// Convenience for the UI-thread consumer (`parse_and_layout`): fetch using the
    /// cache's current generation.
    ///
    /// The consumer runs synchronously on the UI thread for the navigation being
    /// rendered, so the current generation is stable for the duration of the call —
    /// no newer navigation can interleave between reading the generation and the
    /// fetch.
    pub fn fetch_current(
        &self,
        url: &str,
        fetch: impl FnOnce() -> Result<Vec<u8>, String>,
    ) -> FetchResult {
        let generation = self.current_generation();
        self.fetch(generation, url, fetch)
    }
}

/// Process-global prefetch cache shared between the streaming thread and the
/// UI-thread page pipeline. Reset per navigation via [`PrefetchCache::reset`].
pub static PREFETCH_CACHE: LazyLock<PrefetchCache> = LazyLock::new(PrefetchCache::new);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;

    #[test]
    fn first_caller_fills_then_hit_reuses() {
        let cache = PrefetchCache::new();
        cache.reset(1);
        let calls = AtomicUsize::new(0);

        let first = cache.fetch(1, "http://x/a.js", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(b"body".to_vec())
        });
        assert_eq!(&**first.unwrap(), b"body");

        // Second call for the same URL must NOT run the closure again.
        let second = cache.fetch(1, "http://x/a.js", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(b"DIFFERENT".to_vec())
        });
        assert_eq!(&**second.unwrap(), b"body");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reset_clears_and_changes_generation() {
        let cache = PrefetchCache::new();
        cache.reset(1);
        let _ = cache.fetch(1, "http://x/a.js", || Ok(b"v1".to_vec()));

        cache.reset(2);
        assert_eq!(cache.current_generation(), 2);
        // Same URL under the new generation re-runs the fetch (old slot cleared).
        let after = cache.fetch(2, "http://x/a.js", || Ok(b"v2".to_vec()));
        assert_eq!(&**after.unwrap(), b"v2");
    }

    #[test]
    fn stale_generation_bypasses_cache() {
        let cache = PrefetchCache::new();
        cache.reset(5);
        let calls = AtomicUsize::new(0);

        // Producer from generation 4 (superseded) — must run uncached and NOT
        // insert a slot for the current generation.
        let stale = cache.fetch(4, "http://x/a.js", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(b"stale".to_vec())
        });
        assert_eq!(&**stale.unwrap(), b"stale");

        // Current generation 5 sees no entry → runs its own fetch.
        let fresh = cache.fetch(5, "http://x/a.js", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(b"fresh".to_vec())
        });
        assert_eq!(&**fresh.unwrap(), b"fresh");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn cached_error_is_shared() {
        let cache = PrefetchCache::new();
        cache.reset(1);
        let calls = AtomicUsize::new(0);

        let first = cache.fetch(1, "http://x/bad", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("404".to_string())
        });
        assert_eq!(first.unwrap_err(), "404");

        let second = cache.fetch(1, "http://x/bad", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(b"would-succeed".to_vec())
        });
        assert_eq!(second.unwrap_err(), "404");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn concurrent_callers_dedup_to_one_fetch() {
        let cache = Arc::new(PrefetchCache::new());
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
                    let bytes = cache.fetch(1, "http://x/shared.js", || {
                        // First filler does a "slow" fetch; others must block on it.
                        calls.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(30));
                        Ok(b"once".to_vec())
                    });
                    assert_eq!(&**bytes.unwrap(), b"once");
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
