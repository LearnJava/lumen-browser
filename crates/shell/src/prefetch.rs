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
//!   `http_client_for_subresource(...).fetch_subresource_with_content_type(url, dest)`
//!   path, so script order and the CSS cascade are unaffected. A cache *miss*
//!   degrades to the old behaviour (the consumer fetches the resource itself) —
//!   never wrong bytes. The cached [`CachedResource`] carries the response's
//!   `Content-Type` alongside the body (BUG-509) — both producer and consumer
//!   must go through this same cache for the header to be reliably available,
//!   since whichever one loses the fetch race never runs its own closure at all.
//! * **In-flight dedup.** The first caller for a URL runs the fetch and fills the
//!   slot; concurrent callers block until it finishes and share one
//!   `Arc<CachedResource>`. This prevents the streaming warm-up and the final
//!   consumer from fetching the same URL twice.
//! * **Generation-scoped.** Each navigation bumps a generation; [`PrefetchCache::reset`]
//!   clears all slots and adopts the new generation. A stale producer thread (from a
//!   superseded navigation) bypasses the cache and never pollutes the current page.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, LazyLock, Mutex};

/// One resource fetched through the cache: body bytes plus the response's
/// `Content-Type` header value (BUG-509: the CSS "determine the fallback
/// encoding" algorithm needs the transport-layer `charset=` parameter).
/// Carried for every resource kind, not just stylesheets — the producer
/// (streaming preload scanner) and the consumer (`parse_and_layout`) must
/// share exactly one fetch regardless of which one ends up needing the
/// header, or CSS charset detection would silently see no header whenever
/// the producer wins the race (which it does, by design — see module docs).
#[derive(Debug)]
pub(crate) struct CachedResource {
    pub(crate) body: Vec<u8>,
    pub(crate) content_type: Option<String>,
}

/// Per-slot stored outcome: shared resource bytes or an error message.
///
/// Cloneable so every waiter on an in-flight slot receives the same result.
type FetchResult = Result<Arc<CachedResource>, String>;

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

/// Outcome of reserving a URL's slot ([`PrefetchCache::claim`]).
enum Claim {
    /// `generation` is not the cache's current one — bypass the cache.
    Stale,
    /// This caller created the slot and must fill it.
    Filler(Arc<Slot>),
    /// Someone else owns the fetch; wait for its result.
    Waiter(Arc<Slot>),
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

    /// TEMP BUG-272 diagnostics: (entries, cached body bytes) currently held.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn debug_stats(&self) -> (usize, usize) {
        let inner = self.inner.lock().unwrap();
        let mut bytes = 0usize;
        for slot in inner.slots.values() {
            if let Some(Ok(resource)) = slot.state.lock().unwrap().as_ref() {
                bytes += resource.body.len();
            }
        }
        (inner.slots.len(), bytes)
    }

    /// Drop all cached entries and adopt navigation `generation`.
    ///
    /// Called on the UI thread at navigation start (before the streaming thread is
    /// spawned), so producer warm-ups and the consumer all observe the same
    /// generation for one navigation.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn reset(&self, generation: u64) {
        let mut inner = self.inner.lock().unwrap();
        inner.generation = generation;
        inner.slots.clear();
    }

    /// The navigation generation the cache is currently scoped to.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn current_generation(&self) -> u64 {
        self.inner.lock().unwrap().generation
    }

    /// Fetch `url` through the cache for navigation `generation`.
    ///
    /// The first caller for a given `(generation, url)` runs `fetch` and fills the
    /// slot; concurrent callers block until it completes and share the same
    /// `Arc<CachedResource>`. When `generation` no longer matches the cache's current
    /// generation (a newer navigation already reset it), the call bypasses the cache
    /// entirely and runs `fetch` directly — a stale producer never pollutes the
    /// current page's entries.
    ///
    /// `fetch` returns the resource on success or an error string; the error is
    /// cached too so waiters share one outcome instead of stampeding the network.
    pub fn fetch(
        &self,
        generation: u64,
        url: &str,
        fetch: impl FnOnce() -> Result<CachedResource, String>,
    ) -> FetchResult {
        match self.claim(generation, url) {
            Claim::Stale => fetch().map(Arc::new),
            // Run the (potentially slow, network-bound) fetch WITHOUT holding any
            // lock, then publish the result and wake waiters.
            Claim::Filler(slot) => Self::fill(&slot, fetch().map(Arc::new)),
            Claim::Waiter(slot) => Self::wait(&slot),
        }
    }

    /// BUG-1116: start filling `url`'s slot in the background, reserving the
    /// slot *before* returning.
    ///
    /// [`Self::fetch`] reserves the slot only once it runs, so a caller that
    /// spawns a thread around it leaves a window in which a consumer probing
    /// with [`Self::lookup_current`] finds no slot and goes to the network
    /// itself — the duplicate request this bug is about. Reserving here, on
    /// the caller's thread, closes that window: anything that looks the URL
    /// up afterwards waits on this fetch. A URL that already has a slot, or a
    /// superseded `generation`, spawns nothing.
    pub(crate) fn warm(
        &self,
        generation: u64,
        url: &str,
        fetch: impl FnOnce() -> Result<CachedResource, String> + Send + 'static,
    ) {
        if let Claim::Filler(slot) = self.claim(generation, url) {
            std::thread::spawn(move || {
                let _ = Self::fill(&slot, fetch().map(Arc::new));
            });
        }
    }

    /// BUG-1116: the bytes of `url` if something this navigation already put
    /// it into the cache (waiting for an in-flight fetch to finish), `None`
    /// when nothing did.
    ///
    /// For consumers that deliberately do NOT fill the cache themselves —
    /// `<img>`/`@font-face`/media bodies (`subresources.rs::
    /// fetch_subresource_bytes`) are held decoded elsewhere (`IMAGE_CACHE`,
    /// the font registry), so keeping their raw bytes here for the whole
    /// navigation would only double the memory. They still read a slot a
    /// `<link rel=preload as=image|font>` hint warmed, which is what stops the
    /// hint and the real consumer from each fetching the same URL.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn lookup_current(&self, url: &str) -> Option<FetchResult> {
        let slot = Arc::clone(self.inner.lock().unwrap().slots.get(url)?);
        Some(Self::wait(&slot))
    }

    /// Reserve `url`'s slot for `generation`: the first caller becomes the
    /// filler, later ones wait on it, a superseded generation bypasses the
    /// cache entirely.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn claim(&self, generation: u64, url: &str) -> Claim {
        let mut inner = self.inner.lock().unwrap();
        if inner.generation != generation {
            return Claim::Stale;
        }
        if let Some(existing) = inner.slots.get(url) {
            return Claim::Waiter(Arc::clone(existing));
        }
        let slot = Arc::new(Slot::new());
        inner.slots.insert(url.to_owned(), Arc::clone(&slot));
        Claim::Filler(slot)
    }

    /// Publish the filler's result and wake every waiter.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn fill(slot: &Slot, result: FetchResult) -> FetchResult {
        let mut state = slot.state.lock().unwrap();
        *state = Some(result.clone());
        slot.cv.notify_all();
        result
    }

    /// Block until the slot's filler has published its result.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn wait(slot: &Slot) -> FetchResult {
        let mut state = slot.state.lock().unwrap();
        while state.is_none() {
            state = slot.cv.wait(state).unwrap();
        }
        state.clone().unwrap()
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
        fetch: impl FnOnce() -> Result<CachedResource, String>,
    ) -> FetchResult {
        let generation = self.current_generation();
        self.fetch(generation, url, fetch)
    }
}

/// Process-global prefetch cache shared between the streaming thread and the
/// UI-thread page pipeline. Reset per navigation via [`PrefetchCache::reset`].
pub static PREFETCH_CACHE: LazyLock<PrefetchCache> = LazyLock::new(PrefetchCache::new);

/// BUG-1116: the `lumen_core::ext::SubresourceCache` glue that hands
/// `lumen-network::HttpClient::fetch_preload_cached` access to
/// [`PREFETCH_CACHE`] without `lumen-network` depending on `lumen-shell`
/// directly. A zero-sized marker — the cache itself is the process-global
/// [`PREFETCH_CACHE`], so there is nothing to store per instance.
pub(crate) struct SharedPrefetchCache;

impl lumen_core::ext::SubresourceCache for SharedPrefetchCache {
    fn generation(&self) -> u64 {
        PREFETCH_CACHE.current_generation()
    }

    fn get_or_fetch<'a>(
        &self,
        generation: u64,
        url: &str,
        fetch: lumen_core::ext::SubresourceFetch<'a>,
    ) -> Result<(Vec<u8>, Option<String>), String> {
        PREFETCH_CACHE
            .fetch(generation, url, move || {
                fetch().map(|(body, content_type)| CachedResource { body, content_type })
            })
            .map(|resource| (resource.body.clone(), resource.content_type.clone()))
    }

    fn lookup(&self, url: &str) -> Option<lumen_core::ext::SubresourceOutcome> {
        let result = PREFETCH_CACHE.lookup_current(url)?;
        Some(result.map(|resource| (resource.body.clone(), resource.content_type.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;

    fn resource(body: &[u8]) -> CachedResource {
        CachedResource { body: body.to_vec(), content_type: None }
    }

    #[test]
    fn first_caller_fills_then_hit_reuses() {
        let cache = PrefetchCache::new();
        cache.reset(1);
        let calls = AtomicUsize::new(0);

        let first = cache.fetch(1, "http://x/a.js", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(resource(b"body"))
        });
        assert_eq!(&first.unwrap().body, b"body");

        // Second call for the same URL must NOT run the closure again.
        let second = cache.fetch(1, "http://x/a.js", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(resource(b"DIFFERENT"))
        });
        assert_eq!(&second.unwrap().body, b"body");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reset_clears_and_changes_generation() {
        let cache = PrefetchCache::new();
        cache.reset(1);
        let _ = cache.fetch(1, "http://x/a.js", || Ok(resource(b"v1")));

        cache.reset(2);
        assert_eq!(cache.current_generation(), 2);
        // Same URL under the new generation re-runs the fetch (old slot cleared).
        let after = cache.fetch(2, "http://x/a.js", || Ok(resource(b"v2")));
        assert_eq!(&after.unwrap().body, b"v2");
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
            Ok(resource(b"stale"))
        });
        assert_eq!(&stale.unwrap().body, b"stale");

        // Current generation 5 sees no entry → runs its own fetch.
        let fresh = cache.fetch(5, "http://x/a.js", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(resource(b"fresh"))
        });
        assert_eq!(&fresh.unwrap().body, b"fresh");
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
            Ok(resource(b"would-succeed"))
        });
        assert_eq!(second.unwrap_err(), "404");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cached_resource_carries_content_type() {
        // BUG-509: producer and consumer share one fetch, so the header must
        // ride along in the cache regardless of who filled the slot.
        let cache = PrefetchCache::new();
        cache.reset(1);
        let hit = cache.fetch(1, "http://x/a.css", || {
            Ok(CachedResource {
                body: b"body".to_vec(),
                content_type: Some("text/css; charset=windows-1251".to_owned()),
            })
        });
        assert_eq!(hit.unwrap().content_type.as_deref(), Some("text/css; charset=windows-1251"));

        // A second consumer reading the already-filled slot sees the same
        // header without running its own closure.
        let second = cache.fetch(1, "http://x/a.css", || Ok(resource(b"unused")));
        assert_eq!(
            second.unwrap().content_type.as_deref(),
            Some("text/css; charset=windows-1251")
        );
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
                        Ok(resource(b"once"))
                    });
                    assert_eq!(&bytes.unwrap().body, b"once");
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lookup_misses_when_nothing_warmed() {
        // BUG-1116: a consumer that only reads the cache (image/font) must
        // not find anything — and must not create a slot — for a URL no hint
        // warmed, so it goes to the network itself.
        let cache = PrefetchCache::new();
        cache.reset(1);
        assert!(cache.lookup_current("http://x/a.png").is_none());
        let calls = AtomicUsize::new(0);
        let _ = cache.fetch(1, "http://x/a.png", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(resource(b"net"))
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1, "lookup must not reserve the slot");
    }

    #[test]
    fn lookup_waits_for_warm_reserved_before_its_thread_runs() {
        // BUG-1116: `warm` reserves the slot on the caller's thread, so a
        // lookup issued right after it — before the warm-up thread has even
        // started its fetch — waits for that fetch instead of missing and
        // sending a second request.
        let cache: &'static PrefetchCache = Box::leak(Box::new(PrefetchCache::new()));
        cache.reset(1);
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        cache.warm(1, "http://x/font.woff2", move || {
            let _ = release_rx.recv();
            Ok(resource(b"font"))
        });
        let reader = std::thread::spawn(move || cache.lookup_current("http://x/font.woff2"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        release_tx.send(()).unwrap();
        let got = reader.join().unwrap().expect("slot reserved by warm").unwrap();
        assert_eq!(&got.body, b"font");
    }

    #[test]
    fn warm_is_noop_for_already_cached_or_stale() {
        let cache: &'static PrefetchCache = Box::leak(Box::new(PrefetchCache::new()));
        cache.reset(2);
        let _ = cache.fetch(2, "http://x/a.js", || Ok(resource(b"first")));
        let calls = Arc::new(AtomicUsize::new(0));
        for generation in [2, 1] {
            let calls = Arc::clone(&calls);
            cache.warm(generation, "http://x/a.js", move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(resource(b"second"))
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(&cache.lookup_current("http://x/a.js").unwrap().unwrap().body, b"first");
    }
}
