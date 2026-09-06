//! `PerformanceNavigationTiming` detail payload for [`crate::persistent_js::PersistentJs::deliver_nav_timing`]
//! (BUG-640).
//!
//! # Why a process-global mark table
//!
//! `notify_dom_content_loaded`/`notify_window_loaded` (`persistent_js.rs`)
//! fire deep inside `page_pipeline.rs::render_bytes` (off the UI thread — that
//! whole function runs inside the `std::thread::spawn` in
//! `app/user_event.rs`) and `page_load.rs::apply_loaded_page` (UI thread,
//! itself sometimes routed through the engine-thread task queue) respectively
//! — long before either of `deliver_nav_timing`'s two call sites has any way
//! to see them, and both run inside functions whose signatures this
//! codebase's own docs already warn against growing further. A process-global
//! slot mirrors `resource_timing.rs`'s own "why a process-global queue"
//! reasoning for the identical cross-thread shape (see that module's doc
//! comment).
//!
//! # What is real and what is stubbed
//!
//! [`detail_json`] fills every `PerformanceNavigationTiming`-specific field
//! the W3C Navigation Timing L2 attributes-exist test enumerates, but not
//! every value is measured:
//!
//! - **Real:** `responseStatus`/`encodedBodySize`/`decodedBodySize`/
//!   `transferSize` (from [`NavResponseMeta`], threaded from
//!   `lumen_network::PageResponse`/`RawPage`), `redirectCount` (`0`/`1` —
//!   whether the final URL differs from the requested one; `lumen-network`'s
//!   `fetch_with_redirect` never surfaces an exact hop count, so this is an
//!   honest lower bound, not a fabricated exact count), and the DOM lifecycle
//!   milestones (`domInteractive`/`domContentLoadedEventStart/End`/
//!   `domComplete`/`loadEventStart/End`), captured as real [`Instant`]s
//!   relative to the same `nav_start` `deliver_nav_timing`'s `duration_ms`
//!   already used — deliberately **not** JS `performance.now()`, which would
//!   sit on a different zero point (`timeOrigin` is captured at `install_dom`,
//!   strictly after `nav_start`; mixing the two clocks would make the entry's
//!   own timeline non-monotonic).
//! - **Honestly stubbed to `0`:** `redirectStart`/`redirectEnd` (no per-hop
//!   timestamp exists anywhere in `lumen-network`), `domainLookupStart/End`/
//!   `connectStart/End`/`secureConnectionStart`/`requestStart`/`fetchStart`/
//!   `responseStart`/`responseEnd` (no DNS/connect/TLS/request sub-phase
//!   breakdown is exposed by `lumen-network`'s HTTP client at all — the exact
//!   same limitation `_lumen_record_resource_timing` already documents for
//!   subresources), `unloadEventStart/End` (no previous-document unload is
//!   timed), `workerStart` (no service-worker-intercepted navigation timing),
//!   and `activationStart` (no prerendering).
//! - **`type`:** always `"navigate"`. Every navigation kind (`navigate_to`/
//!   `navigate_replace`/`navigate_back`/`navigate_forward`/an actual reload)
//!   converges on the single `Lumen::reload()` entry point with no tag
//!   distinguishing which one asked for it — correct for the common case (a
//!   fresh navigation) and for the WPT subtest this bug tracks
//!   (`nav2-test-navigation-type-navigate.html`), wrong for an actual
//!   reload/back-forward. Left as a documented remainder rather than adding a
//!   `pending_nav_type` field threaded through eleven `self.reload()` call
//!   sites (several with early returns for intercepted navigations) for one
//!   untested WPT subtest.
//! - **`serverTiming`:** always `[]` — no `Server-Timing` response-header
//!   parsing exists anywhere in the workspace.
//! - **`nextHopProtocol`:** always `""` — `lumen-network` doesn't surface the
//!   negotiated protocol (h2/http1.1) to `PageResponse`, same gap
//!   `_lumen_record_resource_timing` already lives with.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Real facts about the top-level document's HTTP response, threaded from
/// `lumen_network::PageResponse` through `RawPage` into [`crate::page_pipeline::LoadedPage`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NavResponseMeta {
    /// HTTP status of the response that supplied the document body, or `0`
    /// for a non-network source (`File`/`Snapshot`/`Static`/`AboutBlank`) or a
    /// fresh HTTP-cache hit — same convention as
    /// `resource_timing::ResourceTimingRow::status`.
    pub(crate) status: u16,
    /// Whether the final URL differs from the originally-requested one — see
    /// this module's doc comment for why this can't be an exact hop count.
    pub(crate) redirected: bool,
    /// Decoded body length in bytes. Also stands in for the encoded size
    /// (`lumen-network` decodes `Content-Encoding` transparently and never
    /// keeps the wire length around) — the same simplification
    /// `fetch_subresource_inner` already makes for `ResourceTimingRow`.
    pub(crate) decoded_body_size: u64,
}

/// One navigation's DOM milestones, filled in as they occur.
#[derive(Debug, Clone, Copy, Default)]
struct Marks {
    dom_content_loaded_start: Option<Instant>,
    dom_content_loaded_end: Option<Instant>,
    load_start: Option<Instant>,
    load_end: Option<Instant>,
}

static MARKS: OnceLock<Mutex<Marks>> = OnceLock::new();

fn marks() -> &'static Mutex<Marks> {
    MARKS.get_or_init(|| Mutex::new(Marks::default()))
}

/// Reset at the start of every navigation (`Lumen::reload()`), mirroring
/// `resource_timing::clear()` — a mark from the outgoing document must never
/// leak into the incoming one's entry.
pub(crate) fn clear() {
    if let Ok(mut m) = marks().lock() {
        *m = Marks::default();
    }
}

/// HTML LS §8.2.3: `domContentLoadedEventStart`/`domInteractive`, right
/// before `DOMContentLoaded` dispatches. Called from `page_pipeline.rs`,
/// around `notify_dom_content_loaded()`.
pub(crate) fn record_dom_content_loaded_start() {
    if let Ok(mut m) = marks().lock() {
        m.dom_content_loaded_start = Some(Instant::now());
    }
}

/// `domContentLoadedEventEnd`, right after `DOMContentLoaded` listeners ran.
pub(crate) fn record_dom_content_loaded_end() {
    if let Ok(mut m) = marks().lock() {
        m.dom_content_loaded_end = Some(Instant::now());
    }
}

/// HTML LS §8.2.3: `domComplete`/`loadEventStart`, right before the `load`
/// event dispatches. Called from `page_load.rs::apply_loaded_page`, around
/// `notify_window_loaded()`.
pub(crate) fn record_load_start() {
    if let Ok(mut m) = marks().lock() {
        m.load_start = Some(Instant::now());
    }
}

/// `loadEventEnd`, right after `load` listeners ran.
pub(crate) fn record_load_end() {
    if let Ok(mut m) = marks().lock() {
        m.load_end = Some(Instant::now());
    }
}

/// Milliseconds from `nav_start` to `mark`, or `0` when the milestone never
/// fired (e.g. the rare no-window sync `reload()` fallback never calls
/// `apply_loaded_page`, so `load_start`/`load_end` stay `None`) — `0` is the
/// spec's own fallback value for an unmeasured timestamp, not an invented one.
fn relative_ms(mark: Option<Instant>, nav_start: Instant) -> f64 {
    mark.map(|t| t.saturating_duration_since(nav_start).as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// Build the JSON object `_lumen_deliver_perf_entry`'s `detail_json` parses
/// and merges onto the `PerformanceNavigationTiming` entry — every field the
/// W3C Navigation Timing L2 attributes-exist test enumerates beyond the four
/// base `PerformanceEntry` ones `deliver_nav_timing` already sets directly.
/// See this module's doc comment for which values are real.
pub(crate) fn detail_json(meta: NavResponseMeta, nav_start: Instant) -> String {
    let m = marks().lock().map(|g| *g).unwrap_or_default();
    let dom_content_loaded_start_ms = relative_ms(m.dom_content_loaded_start, nav_start);
    let load_start_ms = relative_ms(m.load_start, nav_start);
    let transfer_size = if meta.status == 0 { 0 } else { meta.decoded_body_size + 300 };
    serde_json::json!({
        // §Real (redirect / response).
        "redirectCount": u32::from(meta.redirected),
        "responseStatus": meta.status,
        "encodedBodySize": meta.decoded_body_size,
        "decodedBodySize": meta.decoded_body_size,
        "transferSize": transfer_size,
        // §Real (DOM lifecycle) — domInteractive/domComplete share an instant
        // with domContentLoadedEventStart/loadEventStart respectively, per
        // HTML LS §8.2.3 (readyState flips at the same tick the event fires).
        "domInteractive": dom_content_loaded_start_ms,
        "domContentLoadedEventStart": dom_content_loaded_start_ms,
        "domContentLoadedEventEnd": relative_ms(m.dom_content_loaded_end, nav_start),
        "domComplete": load_start_ms,
        "loadEventStart": load_start_ms,
        "loadEventEnd": relative_ms(m.load_end, nav_start),
        // §Honestly stubbed — see module doc comment.
        "type": "navigate",
        "redirectStart": 0,
        "redirectEnd": 0,
        "fetchStart": 0,
        "domainLookupStart": 0,
        "domainLookupEnd": 0,
        "connectStart": 0,
        "connectEnd": 0,
        "secureConnectionStart": 0,
        "requestStart": 0,
        "responseStart": 0,
        "responseEnd": 0,
        "unloadEventStart": 0,
        "unloadEventEnd": 0,
        "workerStart": 0,
        "activationStart": 0,
        "nextHopProtocol": "",
        "initiatorType": "navigation",
        "serverTiming": [],
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `MARKS` is process-global, so these tests are not independent of each
    /// other — same reasoning (and pattern) as `resource_timing.rs`'s own
    /// `TEST_LOCK`.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn value(json: &str, key: &str) -> serde_json::Value {
        let parsed: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        parsed.get(key).cloned().unwrap_or(serde_json::Value::Null)
    }

    #[test]
    fn clear_resets_every_mark_to_the_stub_zero() {
        let _guard = exclusive();
        record_dom_content_loaded_start();
        record_dom_content_loaded_end();
        record_load_start();
        record_load_end();
        clear();
        let json = detail_json(NavResponseMeta::default(), Instant::now());
        for key in [
            "domInteractive",
            "domContentLoadedEventStart",
            "domContentLoadedEventEnd",
            "domComplete",
            "loadEventStart",
            "loadEventEnd",
        ] {
            assert_eq!(value(&json, key), 0.0, "{key} should be 0 right after clear()");
        }
    }

    #[test]
    fn dom_lifecycle_marks_are_real_and_ordered() {
        let _guard = exclusive();
        clear();
        let nav_start = Instant::now();
        record_dom_content_loaded_start();
        record_dom_content_loaded_end();
        record_load_start();
        record_load_end();
        let json = detail_json(NavResponseMeta::default(), nav_start);
        let dcl_start = value(&json, "domContentLoadedEventStart").as_f64().unwrap();
        let dcl_end = value(&json, "domContentLoadedEventEnd").as_f64().unwrap();
        let load_start = value(&json, "loadEventStart").as_f64().unwrap();
        let load_end = value(&json, "loadEventEnd").as_f64().unwrap();
        // domInteractive/domComplete share an instant with
        // domContentLoadedEventStart/loadEventStart respectively (HTML LS
        // §8.2.3 — readyState flips at the same tick the event fires).
        assert_eq!(value(&json, "domInteractive").as_f64().unwrap(), dcl_start);
        assert_eq!(value(&json, "domComplete").as_f64().unwrap(), load_start);
        // All four are real elapsed-since-nav_start milliseconds, in the
        // order they were recorded — never negative, never out of order.
        assert!(dcl_start >= 0.0);
        assert!(dcl_end >= dcl_start);
        assert!(load_start >= dcl_end);
        assert!(load_end >= load_start);
    }

    #[test]
    fn detail_json_carries_real_response_meta() {
        let _guard = exclusive();
        clear();
        let meta = NavResponseMeta { status: 200, redirected: true, decoded_body_size: 12_345 };
        let json = detail_json(meta, Instant::now());
        assert_eq!(value(&json, "responseStatus"), 200);
        assert_eq!(value(&json, "redirectCount"), 1);
        assert_eq!(value(&json, "encodedBodySize"), 12_345);
        assert_eq!(value(&json, "decodedBodySize"), 12_345);
        // §4.3 transferSize: encoded body + the spec's fixed 300-byte overhead.
        assert_eq!(value(&json, "transferSize"), 12_345 + 300);
    }

    #[test]
    fn detail_json_redirect_count_is_zero_when_not_redirected() {
        let _guard = exclusive();
        clear();
        let meta = NavResponseMeta { status: 200, redirected: false, decoded_body_size: 0 };
        let json = detail_json(meta, Instant::now());
        assert_eq!(value(&json, "redirectCount"), 0);
        assert_eq!(value(&json, "redirectStart"), 0);
        assert_eq!(value(&json, "redirectEnd"), 0);
    }

    #[test]
    fn detail_json_transfer_size_is_zero_for_a_cache_hit() {
        let _guard = exclusive();
        clear();
        // `status: 0` is this module's "fresh HTTP-cache hit" convention.
        let meta = NavResponseMeta { status: 0, redirected: false, decoded_body_size: 999 };
        let json = detail_json(meta, Instant::now());
        assert_eq!(value(&json, "transferSize"), 0);
    }

    #[test]
    fn detail_json_has_every_navtiming2_attribute_the_wpt_test_enumerates() {
        // Mirrors `tests/wpt/navigation-timing/nav2-test-attributes-exist.html`'s
        // own list, minus the four base `PerformanceEntry` attributes
        // (`duration`/`entryType`/`name`/`startTime`) `deliver_nav_timing` sets
        // directly rather than through this JSON.
        let _guard = exclusive();
        clear();
        let json = detail_json(NavResponseMeta::default(), Instant::now());
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let obj = parsed.as_object().expect("a JSON object");
        for key in [
            "connectEnd", "connectStart", "decodedBodySize", "domComplete",
            "domContentLoadedEventEnd", "domContentLoadedEventStart", "domInteractive",
            "domainLookupEnd", "domainLookupStart", "encodedBodySize", "fetchStart",
            "initiatorType", "loadEventEnd", "loadEventStart", "nextHopProtocol",
            "redirectCount", "redirectEnd", "redirectStart", "requestStart",
            "responseEnd", "responseStart", "secureConnectionStart", "transferSize",
            "type", "unloadEventEnd", "unloadEventStart", "workerStart",
        ] {
            assert!(obj.contains_key(key), "missing attribute: {key}");
        }
    }

    #[test]
    fn detail_json_is_valid_json_text_not_a_bare_object_literal() {
        // `_lumen_deliver_perf_entry` runs `JSON.parse` on this string — it
        // must be JSON text, not something that merely looks like one
        // (BUG-829's failure mode for a different call site).
        let json = detail_json(NavResponseMeta::default(), Instant::now());
        serde_json::from_str::<serde_json::Value>(&json).expect("must parse as JSON");
    }
}
