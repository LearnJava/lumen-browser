//! Lightweight navigation tracer that records a single page load as a
//! Chrome Trace Event Format timeline (PERF-1), gated behind an explicit
//! [`enable`] call (the `lumen --trace-nav` CLI mode).
//!
//! Where [`crate::profile`] prints a call *tree* to stderr for ad-hoc
//! investigation, this module collects *timeline* spans (with real
//! wall-clock start + duration and per-thread lanes) and serialises them to
//! JSON that opens directly in Perfetto / `chrome://tracing` / `edge://tracing`
//! — no bespoke UI needed. The two known page-load bottlenecks the 2026-07
//! real-site audit found by hand (CPU rasterisation; sequential subresource
//! fetch on the UI thread) both show up on this timeline at a glance: the paint
//! span's duration, and back-to-back `fetch` spans on a single lane.
//!
//! # Usage
//!
//! ```
//! lumen_core::trace::enable();
//! {
//!     let _nav = lumen_core::trace::span("navigation", "nav");
//!     {
//!         let mut s = lumen_core::trace::span("fetch", "net");
//!         // ... do the fetch ...
//!         s.set_bytes(1234);
//!     }
//! }
//! let json = lumen_core::trace::finish().unwrap();
//! assert!(json.contains("traceEvents"));
//! ```
//!
//! With the tracer disabled (the default), [`span`] is a single relaxed atomic
//! load plus an inert guard, and [`instant`] returns immediately — negligible
//! cost, so instrumentation can live permanently on the load path.

use crate::json::JsonValue;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Fast-path flag checked by every [`span`] / [`instant`] call. Set by
/// [`enable`], cleared by [`finish`]. Relaxed ordering is sufficient: the
/// tracer is single-consumer (one `finish` after the load completes) and a
/// missed span at the exact enable/disable boundary is harmless.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// One completed trace event (Chrome Trace Event Format "X" complete event or
/// "i" instant event).
struct Event {
    /// Event name shown on the timeline lane.
    name: String,
    /// Category string (Chrome groups/colours by this).
    cat: &'static str,
    /// `'X'` for a complete (duration) event, `'i'` for an instant marker.
    phase: char,
    /// Start offset from the trace origin, in microseconds.
    ts_us: f64,
    /// Duration in microseconds (`0.0` and unused for instant events).
    dur_us: f64,
    /// Thread lane id (see [`thread_lane`]).
    tid: u64,
    /// Optional structured args (URL, byte size, HTTP status, …).
    args: BTreeMap<String, JsonValue>,
}

/// The in-progress recording: a fixed time origin plus the events collected so
/// far. Guarded by a mutex because subresource fetches run on worker threads
/// (`parallel_map`), so spans are pushed from multiple threads concurrently.
struct Recorder {
    /// Time origin — every event's `ts` is measured relative to this instant.
    start: Instant,
    /// All events recorded so far, in completion order.
    events: Vec<Event>,
}

/// Global recorder slot. `None` until [`enable`] is called, taken back to
/// `None` by [`finish`].
fn recorder() -> &'static Mutex<Option<Recorder>> {
    static RECORDER: OnceLock<Mutex<Option<Recorder>>> = OnceLock::new();
    RECORDER.get_or_init(|| Mutex::new(None))
}

/// Monotonic source of per-thread lane ids, assigned on first use per thread.
static NEXT_TID: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// This thread's lane id, lazily assigned from [`NEXT_TID`] the first time
    /// the thread records a span.
    static MY_TID: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Returns this thread's stable lane id (the main/UI thread that enables the
/// tracer first gets lane 0), assigning a fresh one on first call.
fn thread_lane() -> u64 {
    MY_TID.with(|c| match c.get() {
        Some(t) => t,
        None => {
            let t = NEXT_TID.fetch_add(1, Ordering::Relaxed);
            c.set(Some(t));
            t
        }
    })
}

/// Starts recording. Installs a fresh time origin and clears any previous
/// events. No-op-safe to call once at the start of a traced navigation.
pub fn enable() {
    *recorder().lock().unwrap() = Some(Recorder {
        start: Instant::now(),
        events: Vec::new(),
    });
    ENABLED.store(true, Ordering::Relaxed);
}

/// Whether the tracer is currently recording. A single relaxed atomic load;
/// cheap enough to guard hot call sites.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Opens a complete ("X") span with the given name and category. The span ends
/// — recording its wall-clock start and elapsed duration — when the returned
/// guard is dropped. Attach structured args (URL, size, status) to the guard
/// before it drops with [`SpanGuard::arg`] / [`SpanGuard::set_bytes`].
///
/// Returns an inert guard (no allocation beyond the empty args map) when the
/// tracer is disabled.
pub fn span(name: impl Into<String>, cat: &'static str) -> SpanGuard {
    if !enabled() {
        return SpanGuard { inner: None };
    }
    SpanGuard {
        inner: Some(SpanInner {
            name: name.into(),
            cat,
            begin: Instant::now(),
            tid: thread_lane(),
            args: BTreeMap::new(),
        }),
    }
}

/// Records a zero-duration instant marker ("i" event) at the current time —
/// e.g. `first-paint`, `dom-content-loaded`. No-op when disabled.
pub fn instant(name: impl Into<String>, cat: &'static str) {
    if !enabled() {
        return;
    }
    let now = Instant::now();
    let tid = thread_lane();
    if let Some(rec) = recorder().lock().unwrap().as_mut() {
        let ts_us = now.saturating_duration_since(rec.start).as_secs_f64() * 1_000_000.0;
        rec.events.push(Event {
            name: name.into(),
            cat,
            phase: 'i',
            ts_us,
            dur_us: 0.0,
            tid,
            args: BTreeMap::new(),
        });
    }
}

/// Stops recording and returns the collected timeline serialised as Chrome
/// Trace Event Format JSON (`{"traceEvents":[…],"displayTimeUnit":"ms"}`), or
/// `None` if the tracer was never enabled. Clears the recorder either way.
pub fn finish() -> Option<String> {
    ENABLED.store(false, Ordering::Relaxed);
    let rec = recorder().lock().unwrap().take()?;
    Some(to_chrome_json(&rec.events))
}

/// Serialises recorded events into the Chrome Trace Event Format object.
fn to_chrome_json(events: &[Event]) -> String {
    let mut trace_events = Vec::with_capacity(events.len());
    for ev in events {
        let mut obj: BTreeMap<String, JsonValue> = BTreeMap::new();
        obj.insert("name".to_owned(), JsonValue::String(ev.name.clone()));
        obj.insert("cat".to_owned(), JsonValue::String(ev.cat.to_owned()));
        obj.insert("ph".to_owned(), JsonValue::String(ev.phase.to_string()));
        obj.insert("ts".to_owned(), JsonValue::Number(ev.ts_us));
        if ev.phase == 'X' {
            obj.insert("dur".to_owned(), JsonValue::Number(ev.dur_us));
        }
        // Single logical process; each OS thread is its own lane.
        obj.insert("pid".to_owned(), JsonValue::Number(1.0));
        obj.insert("tid".to_owned(), JsonValue::Number(ev.tid as f64));
        if !ev.args.is_empty() {
            obj.insert("args".to_owned(), JsonValue::Object(ev.args.clone()));
        }
        trace_events.push(JsonValue::Object(obj));
    }

    let mut root: BTreeMap<String, JsonValue> = BTreeMap::new();
    root.insert("traceEvents".to_owned(), JsonValue::Array(trace_events));
    root.insert(
        "displayTimeUnit".to_owned(),
        JsonValue::String("ms".to_owned()),
    );
    JsonValue::Object(root).to_string()
}

/// One in-progress complete span. Lives inside a [`SpanGuard`]; the event is
/// pushed to the recorder when the guard drops.
struct SpanInner {
    /// Event name.
    name: String,
    /// Event category.
    cat: &'static str,
    /// Wall-clock instant the span opened (start `ts` and duration derive from it).
    begin: Instant,
    /// Thread lane this span belongs to.
    tid: u64,
    /// Structured args accumulated before the span closes.
    args: BTreeMap<String, JsonValue>,
}

/// RAII guard returned by [`span`]. Records the completed span into the
/// timeline when dropped; a no-op when the tracer is disabled.
#[must_use = "the span ends when this guard is dropped — bind it to a name, not `_`"]
pub struct SpanGuard {
    /// `None` when tracing is disabled (inert guard).
    inner: Option<SpanInner>,
}

impl SpanGuard {
    /// Attaches a structured arg to this span (shown under the event in the
    /// trace viewer). No-op on an inert guard.
    pub fn arg(&mut self, key: &str, value: JsonValue) {
        if let Some(inner) = &mut self.inner {
            inner.args.insert(key.to_owned(), value);
        }
    }

    /// Convenience for resource-fetch spans: records the decoded byte size as
    /// the `size` arg.
    pub fn set_bytes(&mut self, len: usize) {
        self.arg("size", JsonValue::Number(len as f64));
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        let Some(inner) = self.inner.take() else {
            return;
        };
        let dur_us = inner.begin.elapsed().as_secs_f64() * 1_000_000.0;
        if let Some(rec) = recorder().lock().unwrap().as_mut() {
            let ts_us =
                inner.begin.saturating_duration_since(rec.start).as_secs_f64() * 1_000_000.0;
            rec.events.push(Event {
                name: inner.name,
                cat: inner.cat,
                phase: 'X',
                ts_us,
                dur_us,
                tid: inner.tid,
                args: inner.args,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests mutate the single global recorder, so they must not run
    // concurrently with each other. Cargo runs tests in one module on separate
    // threads by default; a shared mutex serialises them.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn disabled_span_is_inert() {
        let _guard = TEST_LOCK.lock().unwrap();
        // Ensure a clean disabled state.
        let _ = finish();
        assert!(!enabled());
        {
            let mut s = span("x", "cat");
            s.set_bytes(10);
        }
        instant("marker", "cat");
        // finish() returns None when never enabled since the last finish.
        assert!(finish().is_none());
    }

    #[test]
    fn records_spans_and_instants_as_chrome_json() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable();
        {
            let mut s = span("fetch", "net");
            s.arg("url", JsonValue::String("https://example.com/a.css".to_owned()));
            s.set_bytes(2048);
        }
        instant("first-paint", "paint");
        let json = finish().expect("enabled -> Some");

        // Structural checks (parse it back with the same JSON module).
        let parsed = crate::json::parse(&json).expect("valid JSON");
        let events = parsed
            .get("traceEvents")
            .and_then(JsonValue::as_array)
            .expect("traceEvents array");
        assert_eq!(events.len(), 2);

        let span_ev = &events[0];
        assert_eq!(span_ev.get("name").and_then(JsonValue::as_str), Some("fetch"));
        assert_eq!(span_ev.get("ph").and_then(JsonValue::as_str), Some("X"));
        assert!(span_ev.get("dur").and_then(JsonValue::as_number).is_some());
        assert_eq!(
            span_ev
                .get("args")
                .and_then(|a| a.get("size"))
                .and_then(JsonValue::as_number),
            Some(2048.0)
        );

        let instant_ev = &events[1];
        assert_eq!(instant_ev.get("ph").and_then(JsonValue::as_str), Some("i"));
        assert!(instant_ev.get("dur").is_none());

        // finish() cleared the recorder.
        assert!(!enabled());
        assert!(finish().is_none());
    }
}
