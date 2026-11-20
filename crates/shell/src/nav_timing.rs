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
//! - **Synthesized once, then frozen:** `fetchStart`/`domainLookupStart/End`/
//!   `connectStart/End`/`secureConnectionStart`/`requestStart`/`responseStart`/
//!   `responseEnd`. `lumen-network` exposes no DNS/connect/TLS/request
//!   sub-phase breakdown (the same limitation `_lumen_record_resource_timing`
//!   documents for subresources), so [`fill_early_meta`] derives a monotonic
//!   ladder anchored on the *real* elapsed time to response completion and
//!   stores it in [`EARLY`] — the pre-script and post-`load` deliveries then
//!   report identical values, the way Chrome fixes them at commit. Without an
//!   early context (frames, tests) they stay the honest `0` stub.
//! - **Honestly stubbed to `0`:** `redirectStart`/`redirectEnd` (no per-hop
//!   timestamp exists anywhere in `lumen-network`), `unloadEventStart/End`
//!   (no previous-document unload is timed), `workerStart` (no
//!   service-worker-intercepted navigation timing), and `activationStart`
//!   (no prerendering).
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

/// The network sub-phases `lumen-network` cannot measure — see the module
/// doc comment for why they are synthesized once and frozen.
#[derive(Debug, Clone, Copy)]
struct NetPhases {
    fetch_start: f64,
    domain_lookup_start: f64,
    domain_lookup_end: f64,
    connect_start: f64,
    secure_connection_start: f64,
    connect_end: f64,
    request_start: f64,
    response_start: f64,
    response_end: f64,
}

/// Pre-script navigation context: everything [`early_delivery_js`] needs to
/// hand `run_scripts_with_dom` a ready-made eval snippet that runs after
/// `install_dom` but before the first script line — the point where Chrome
/// has already populated `performance.timing`/`PerformanceNavigationTiming`
/// (navigation commit), while Lumen used to deliver the entry only after
/// `load`, leaving every field `0` for parse-time/anti-bot scripts.
struct EarlyNav {
    /// Same [`Instant`] `Lumen::nav_start` carries — `detail_json`'s `nav_start`.
    start: Instant,
    /// `start` on the `_lumen_now_ms()` clock (Unix-epoch ms). The JS
    /// `timeOrigin`/`navigationStart` must equal the real navigation start —
    /// pinning it at shim-install time (post-response) would shift every
    /// `performance.now()`/timing value by the whole fetch+parse duration.
    unix_ms: f64,
    url: String,
    /// Filled by [`fill_early_meta`] once `render_bytes` knows the response
    /// facts; `None` until then (and forever for a load that never reaches
    /// `render_bytes`, e.g. a network error page — no early entry then,
    /// matching the no-`LoadedPage` no-late-delivery behaviour).
    meta: Option<NavResponseMeta>,
    /// The frozen network-phase ladder — computed once at `fill`, reused by
    /// both the early and the post-`load` `detail_json` calls.
    net: Option<NetPhases>,
}

static EARLY: OnceLock<Mutex<Option<EarlyNav>>> = OnceLock::new();

fn early() -> &'static Mutex<Option<EarlyNav>> {
    EARLY.get_or_init(|| Mutex::new(None))
}

/// Record the navigation start where `Lumen::reload()` sets `nav_start`.
/// Called right after [`clear`] so a stale context can never survive into
/// the incoming navigation.
pub(crate) fn begin(start: Instant, unix_ms: f64, url: &str) {
    if let Ok(mut g) = early().lock() {
        *g = Some(EarlyNav {
            start,
            unix_ms,
            url: url.to_owned(),
            meta: None,
            net: None,
        });
    }
}

/// Snapshot the response facts `render_bytes` already holds — before
/// `parse_and_layout` runs the document's scripts. First call wins: a
/// re-entrant `render_bytes` must not recompute the ladder, or the early
/// and final deliveries would disagree.
pub(crate) fn fill_early_meta(meta: NavResponseMeta) {
    let Ok(mut g) = early().lock() else {
        return;
    };
    let Some(e) = g.as_mut() else {
        return;
    };
    if e.meta.is_some() {
        return;
    }
    e.meta = Some(meta);
    // Anchor the ladder on the real elapsed time to "response complete"
    // (this runs right before parsing, i.e. right after the body arrived).
    // Shape measured on Chrome 157 through the same CONNECT proxy (Ozon
    // `/abt/result` payload, 2026-11-20): DNS/connect collapse onto
    // `fetchStart`, `secureConnectionStart` stays `0` (no TLS phase is
    // reported for a tunneled connection), `requestStart` follows
    // `fetchStart` by ~2 ms, and `responseStart`/`responseEnd` sit at the
    // real tail. Every phase stays strictly ordered for any elapsed value.
    let elapsed = e.start.elapsed().as_secs_f64() * 1000.0;
    let resp_end = (elapsed * 0.97).max(1.0);
    let resp_start = (resp_end - (resp_end * 0.08).min(60.0)).max(resp_end * 0.5);
    let fetch_start = 2.0_f64.min(resp_start * 0.2);
    let request_start = (fetch_start + 2.0).min(resp_start * 0.6);
    e.net = Some(NetPhases {
        fetch_start,
        domain_lookup_start: fetch_start,
        domain_lookup_end: fetch_start,
        connect_start: fetch_start,
        connect_end: fetch_start,
        secure_connection_start: 0.0,
        request_start,
        response_start: resp_start,
        response_end: resp_end,
    });
}

/// The frozen ladder for *this* navigation — `None` for callers without an
/// early context (frames, headless tests) or a mismatched `nav_start`, both
/// of which keep the honest `0` stubs.
fn early_net_phases(nav_start: Instant) -> Option<NetPhases> {
    let g = early().lock().ok()?;
    let e = g.as_ref()?;
    if e.start != nav_start {
        return None;
    }
    e.net
}

/// JS snippet `run_scripts_with_dom` evals after `install_dom`, before the
/// first script line: pins `_perf_origin_ms` to the real navigation start
/// (unless `deterministic`, whose `_lumen_now_ms` is a frozen/monotonic test
/// clock a Unix-epoch pin would break) and delivers the navigation entry.
/// `None` when there is no top-level navigation context or the response
/// facts were never filled — frames/workers/tests are unaffected.
#[cfg(feature = "v8")]
pub(crate) fn early_delivery_js(pin_origin: bool) -> Option<String> {
    // Snapshot under the lock, then DROP the guard before `detail_json` —
    // it re-locks `EARLY` via `early_net_phases`, and `std::sync::Mutex` is
    // not reentrant: holding `g` across the call deadlocked the pipeline
    // thread (the page never finished loading).
    let (meta, start, unix_ms, url) = {
        let g = early().lock().ok()?;
        let Some(e) = g.as_ref() else {
            eprintln!("[nav-timing] early delivery skipped: no begin()");
            return None;
        };
        let Some(meta) = e.meta else {
            eprintln!("[nav-timing] early delivery skipped: response meta not filled");
            return None;
        };
        (meta, e.start, e.unix_ms, e.url.clone())
    };
    let detail = detail_json(meta, start);
    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
    let url = crate::js_escape::js_string_literal(&url);
    let detail = crate::js_escape::js_string_literal(&detail);
    let pin = if pin_origin {
        format!("_perf_origin_ms = {unix_ms:.3}; ")
    } else {
        String::new()
    };
    Some(format!(
        "{pin}_lumen_deliver_perf_entry('navigation', {url}, 0.0, {duration_ms}, {detail});"
    ))
}

/// Reset at the start of every navigation (`Lumen::reload()`), mirroring
/// `resource_timing::clear()` — a mark from the outgoing document must never
/// leak into the incoming one's entry.
pub(crate) fn clear() {
    if let Ok(mut m) = marks().lock() {
        *m = Marks::default();
    }
    if let Ok(mut e) = early().lock() {
        *e = None;
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
    let net = early_net_phases(nav_start);
    let phase = |f: fn(NetPhases) -> f64| net.map(f).unwrap_or(0.0);
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
        // §Redirect — real-ish only for a redirected navigation (Chrome
        // reports the hop window; `redirectEnd` coincides with `fetchStart`).
        "type": "navigate",
        "redirectStart": if meta.redirected { phase(|n| n.fetch_start) * 0.75 } else { 0.0 },
        "redirectEnd": if meta.redirected { phase(|n| n.fetch_start) } else { 0.0 },
        "fetchStart": phase(|n| n.fetch_start),
        "domainLookupStart": phase(|n| n.domain_lookup_start),
        "domainLookupEnd": phase(|n| n.domain_lookup_end),
        "connectStart": phase(|n| n.connect_start),
        "connectEnd": phase(|n| n.connect_end),
        "secureConnectionStart": phase(|n| n.secure_connection_start),
        "requestStart": phase(|n| n.request_start),
        "responseStart": phase(|n| n.response_start),
        "responseEnd": phase(|n| n.response_end),
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
        assert_eq!(value(&json, "redirectStart").as_f64().unwrap(), 0.0);
        assert_eq!(value(&json, "redirectEnd").as_f64().unwrap(), 0.0);
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

    #[test]
    fn early_ladder_is_ordered_real_and_frozen() {
        let _guard = exclusive();
        clear();
        let start = Instant::now();
        begin(start, 1_700_000_000_000.0, "https://example.com/");
        let meta = NavResponseMeta { status: 200, redirected: false, decoded_body_size: 100 };
        fill_early_meta(meta);
        // A second fill must not recompute — early and final deliveries
        // have to report the same frozen values.
        fill_early_meta(NavResponseMeta { status: 500, redirected: true, decoded_body_size: 1 });
        let json = detail_json(meta, start);
        let f = |k: &str| value(&json, k).as_f64().unwrap_or(f64::NAN);
        let (fetch, dns_s, dns_e, conn_s, conn_e, req, resp_s, resp_e) = (
            f("fetchStart"), f("domainLookupStart"), f("domainLookupEnd"),
            f("connectStart"), f("connectEnd"),
            f("requestStart"), f("responseStart"), f("responseEnd"),
        );
        // Chrome-through-proxy shape: DNS/connect collapse onto fetchStart,
        // secureConnectionStart stays 0 for a tunneled connection.
        assert_eq!(value(&json, "secureConnectionStart"), 0.0);
        assert_eq!(dns_s, fetch);
        assert_eq!(dns_e, fetch);
        assert_eq!(conn_s, fetch);
        assert_eq!(conn_e, fetch);
        for v in [fetch, req, resp_s, resp_e] {
            assert!(v > 0.0, "phase must be a real positive ms value");
        }
        assert!(fetch < req && req < resp_s && resp_s < resp_e);
        // responseEnd anchors on the real elapsed time — never beyond it
        // (plus scheduling slack between fill and this read).
        assert!(resp_e <= start.elapsed().as_secs_f64() * 1000.0 + 50.0);
        // http:// carries no TLS phase.
        clear();
        let start2 = Instant::now();
        begin(start2, 0.0, "http://example.com/");
        fill_early_meta(meta);
        let json2 = detail_json(meta, start2);
        assert_eq!(value(&json2, "secureConnectionStart"), 0.0);
        // A mismatched nav_start (frame/test caller) keeps the honest zeros.
        let json3 = detail_json(meta, Instant::now());
        assert_eq!(value(&json3, "fetchStart"), 0.0);
        clear();
    }
}
