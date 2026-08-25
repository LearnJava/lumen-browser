//! Resource Timing entries for the subresources the **engine** loads (BUG-839).
//!
//! W3C Resource Timing L2 wants one `PerformanceResourceTiming` per fetched
//! resource in the page's `performance` buffer. Everything the page starts
//! itself — `fetch()`, `XMLHttpRequest`, and the `<script src>`/`<link>` paths
//! that go through `fetch()` since BUG-703/BUG-826 — records itself inside the
//! JS shim, where the real `performance.now()` of the call is available.
//!
//! What is left is everything the engine fetches on the document's behalf:
//! images, cascade stylesheets, `@font-face` bodies, parser-collected scripts.
//! Those run on worker threads with no JS context at all, often before the
//! runtime for the document even exists, so they cannot record themselves.
//! `lumen-network` publishes an [`Event::ResourceTimed`] for each of them and
//! [`ResourceTimingSink`] parks it here until the shell's event loop hands the
//! batch to the page.
//!
//! # Why a process-global queue
//!
//! Same reasoning as `lumen_js::network_log_bindings`: the sink is built in
//! `main()` and handed to network clients spawned on arbitrary threads, long
//! before (and independently of) the `Lumen` that owns the JS runtime. A
//! `static` the shell drains keeps that wiring out of the already-large
//! construction path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use lumen_core::event::Event;
use lumen_core::ext::EventSink;

/// Hard cap on queued rows.
///
/// The queue is drained by whoever owns the current document's JS runtime, and
/// a page that never gets one (no scripts at all) never drains — its images
/// would otherwise accumulate until the next navigation. 4096 is far past any
/// real page's subresource count and past the 250-entry Resource Timing buffer
/// they feed, so reaching it means nobody is draining.
const MAX_QUEUED_ROWS: usize = 4096;

/// While true, [`take_rows`] answers empty and leaves the queue alone.
///
/// Raised by [`clear`] at the start of a navigation and lowered by [`resume`]
/// once the new document is committed. Without it the shell's once-per-step
/// drain — which runs against the *outgoing* document's runtime while the new
/// one is still loading — would hand the new page's stylesheets and scripts to
/// the page being replaced.
static SUSPENDED: AtomicBool = AtomicBool::new(false);

/// One completed subresource load, in the shape
/// `_lumen_deliver_resource_timings` reads.
///
/// Field names are the JSON keys the shim expects, so the row is serialised
/// by hand rather than through `serde` — the shell has no derive on
/// `lumen-core`'s event enum and this is the only consumer.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceTimingRow {
    /// Absolute URL of the request.
    pub url: String,
    /// Resource Timing `initiatorType` (`img` / `link` / `script` / `css` / …).
    pub initiator: &'static str,
    /// Start of the load, unix-epoch milliseconds.
    pub start_ms: f64,
    /// Duration of the load, milliseconds.
    pub duration_ms: f64,
    /// HTTP status, or 0 when unknown (a fresh HTTP-cache hit).
    pub status: u16,
    /// Encoded body size in bytes.
    pub encoded_body_size: u64,
    /// Decoded body size in bytes.
    pub decoded_body_size: u64,
    /// `Content-Type` of the response, empty when absent.
    pub content_type: String,
    /// `deliveryType`: `"cache"` for an HTTP-cache hit, otherwise empty.
    pub delivery_type: &'static str,
}

/// Process-global queue of loads awaiting the shell's drain.
static QUEUE: OnceLock<Mutex<Vec<ResourceTimingRow>>> = OnceLock::new();

fn queue() -> &'static Mutex<Vec<ResourceTimingRow>> {
    QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Drain every pending row for the shell's per-step delivery. Answers empty
/// while a navigation is in flight (see [`SUSPENDED`]) and when nothing arrived
/// since the last call — the common case, which the caller checks before
/// touching the JS runtime.
pub fn take_rows() -> Vec<ResourceTimingRow> {
    if SUSPENDED.load(Ordering::Relaxed) {
        return Vec::new();
    }
    take_rows_unconditionally()
}

/// Drain for the runtime of the document being loaded, ignoring the suspend
/// flag: this caller *is* the new document, and it runs before the page's first
/// script — which is the only moment a synchronous
/// `getEntriesByType('resource')` at the top of that script can see the
/// stylesheets and scripts the parser pulled in.
pub fn take_rows_unconditionally() -> Vec<ResourceTimingRow> {
    match queue().lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        // A poisoned queue means a panic while pushing; timing entries are not
        // worth propagating that, and the next drain recovers on its own.
        Err(_) => Vec::new(),
    }
}

/// Drop everything queued and suspend delivery until [`resume`]. Called at the
/// start of a navigation, *before* the new document's own subresources are
/// fetched: entries belong to the document that asked for them, and a row left
/// over from the previous page would land in the new page's buffer with a start
/// time before its own time origin.
pub fn clear() {
    SUSPENDED.store(true, Ordering::Relaxed);
    if let Ok(mut q) = queue().lock() {
        q.clear();
    }
}

/// Re-enable per-step delivery. Called once the new document is committed, so
/// whatever is still queued — and everything that arrives from here on —
/// belongs to the runtime the shell now holds.
pub fn resume() {
    SUSPENDED.store(false, Ordering::Relaxed);
}

/// Serialise a batch into the JSON array `_lumen_deliver_resource_timings`
/// parses. Returns `None` for an empty batch so the caller can skip the JS hop.
pub fn rows_to_json(rows: &[ResourceTimingRow]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let arr: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "url": r.url,
                "initiatorType": r.initiator,
                "startMs": r.start_ms,
                "durationMs": r.duration_ms,
                "status": r.status,
                "encodedBodySize": r.encoded_body_size,
                "decodedBodySize": r.decoded_body_size,
                "contentType": r.content_type,
                "deliveryType": r.delivery_type,
            })
        })
        .collect();
    serde_json::to_string(&arr).ok()
}

/// Sink wrapper that captures [`Event::ResourceTimed`] and forwards every
/// event, including that one, to `inner`.
///
/// Sits in the same chain as `NetworkLogSink`/`ShieldCountSink`: capturing
/// rather than consuming keeps the stderr network log and the DevTools panel
/// seeing exactly what they saw before.
pub struct ResourceTimingSink {
    /// The next sink in the chain.
    pub inner: std::sync::Arc<dyn EventSink>,
}

impl EventSink for ResourceTimingSink {
    fn emit(&self, event: &Event) {
        if let Event::ResourceTimed {
            url,
            initiator,
            start_ms,
            duration_ms,
            status,
            encoded_body_size,
            decoded_body_size,
            content_type,
            delivery_type,
            ..
        } = event
            && let Ok(mut q) = queue().lock()
            && q.len() < MAX_QUEUED_ROWS
        {
            q.push(ResourceTimingRow {
                url: url.clone(),
                initiator,
                start_ms: *start_ms,
                duration_ms: *duration_ms,
                status: *status,
                encoded_body_size: *encoded_body_size,
                decoded_body_size: *decoded_body_size,
                content_type: content_type.clone(),
                delivery_type,
            });
        }
        self.inner.emit(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::event::TabId;
    use std::sync::Arc;

    /// The queue under test is process-global, so these tests are not
    /// independent of each other: `cargo test` runs them on parallel threads
    /// and a bare `take_rows()` would occasionally return a neighbour's row.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    struct NullSink;
    impl EventSink for NullSink {
        fn emit(&self, _event: &Event) {}
    }

    fn timed(url: &str) -> Event {
        Event::ResourceTimed {
            tab_id: TabId(0),
            url: url.to_string(),
            initiator: "img",
            start_ms: 1_700_000_000_000.0,
            duration_ms: 12.5,
            status: 200,
            encoded_body_size: 64,
            decoded_body_size: 64,
            content_type: "image/png".to_string(),
            delivery_type: "",
        }
    }

    #[test]
    fn sink_captures_and_forwards() {
        let _guard = exclusive();
        clear();
        resume();
        let sink = ResourceTimingSink { inner: Arc::new(NullSink) };
        sink.emit(&timed("https://example.com/a.png"));
        let rows = take_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "https://example.com/a.png");
        assert_eq!(rows[0].initiator, "img");
        // Drained: a second call must not replay the same row into the next
        // document's buffer.
        assert!(take_rows().is_empty());
    }

    #[test]
    fn empty_batch_has_no_json() {
        assert_eq!(rows_to_json(&[]), None);
    }

    #[test]
    fn json_uses_the_shim_key_names() {
        let _guard = exclusive();
        clear();
        resume();
        let sink = ResourceTimingSink { inner: Arc::new(NullSink) };
        sink.emit(&timed("https://example.com/b.png"));
        let json = rows_to_json(&take_rows()).expect("non-empty batch");
        for key in [
            "\"url\"",
            "\"initiatorType\"",
            "\"startMs\"",
            "\"durationMs\"",
            "\"status\"",
            "\"encodedBodySize\"",
            "\"decodedBodySize\"",
            "\"contentType\"",
            "\"deliveryType\"",
        ] {
            assert!(json.contains(key), "{key} missing from {json}");
        }
    }

    #[test]
    fn clear_drops_pending_rows() {
        let _guard = exclusive();
        clear();
        resume();
        let sink = ResourceTimingSink { inner: Arc::new(NullSink) };
        sink.emit(&timed("https://example.com/c.png"));
        clear();
        resume();
        assert!(take_rows().is_empty());
    }

    #[test]
    fn suspended_queue_holds_rows_for_the_loading_document() {
        // The per-step drain must not hand a row to the outgoing document
        // while the new one is still fetching its subresources; the loading
        // document's own runtime takes them unconditionally.
        let _guard = exclusive();
        clear();
        let sink = ResourceTimingSink { inner: Arc::new(NullSink) };
        sink.emit(&timed("https://example.com/d.png"));
        assert!(take_rows().is_empty(), "suspended drain must answer empty");
        let rows = take_rows_unconditionally();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "https://example.com/d.png");
        resume();
    }

    #[test]
    fn resume_reopens_the_per_step_drain() {
        let _guard = exclusive();
        clear();
        let sink = ResourceTimingSink { inner: Arc::new(NullSink) };
        sink.emit(&timed("https://example.com/e.png"));
        resume();
        assert_eq!(take_rows().len(), 1);
    }
}
