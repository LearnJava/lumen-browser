//! Programmatic network-request logging (`_lumen_log_network_request`).
//!
//! Lets page scripts (and the `fetch`/`XMLHttpRequest` shims) record a completed
//! request in the DevTools Network panel. The native function
//! `_lumen_log_network_request(method, url, status, duration_ms)` pushes a
//! [`NetworkLogRecord`] onto a process-global queue; the shell drains it each
//! event-loop tick via [`take_network_log_records`] and folds each entry into the
//! shared `NetworkLog` so it shows up alongside engine-issued requests.
//!
//! # Why a process-global queue
//!
//! Mirrors [`crate::download_bindings`] / `clipboard::PROVIDER`: the binding has
//! no access to the shell's `NetworkLog`, so it records intent in a `static` that
//! the shell owns the draining of. This avoids threading another `Arc` through
//! `install_primitives`' already-large signature.
//!
//! Records are *completed* requests (method, URL, optional status and duration);
//! JS gets no callback. Pending/lifecycle tracking for engine requests stays in
//! the `EventSink` path — this binding is only for JS-initiated traffic that the
//! engine's network thread never sees (e.g. `fetch` served from cache, or a
//! synthetic XHR in a test page).

use rquickjs::{Ctx, Function};
use std::sync::{Mutex, OnceLock};

/// A single network request logged by JS, awaiting the shell's drain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkLogRecord {
    /// HTTP method (`"GET"`, `"POST"`, …). Upper-cased by the binding; falls back
    /// to `"GET"` when the caller passes an empty string.
    pub method: String,
    /// Full request URL.
    pub url: String,
    /// Response status code, or `None` when the caller passed a non-positive
    /// value (unknown / not yet received).
    pub status: Option<u16>,
    /// Request duration in milliseconds, or `None` when the caller passed a
    /// negative value (unknown).
    pub duration_ms: Option<u64>,
}

/// Process-global queue of JS-logged requests awaiting the shell's drain.
static QUEUE: OnceLock<Mutex<Vec<NetworkLogRecord>>> = OnceLock::new();

fn queue() -> &'static Mutex<Vec<NetworkLogRecord>> {
    QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Enqueue a network-log record. Public so non-JS engine paths can reuse the
/// same channel if needed.
pub fn enqueue(method: String, url: String, status: Option<u16>, duration_ms: Option<u64>) {
    queue().lock().unwrap().push(NetworkLogRecord {
        method,
        url,
        status,
        duration_ms,
    });
}

/// Drain and return all pending network-log records.
///
/// Called by the shell each event-loop tick; the queue is left empty.
pub fn take_network_log_records() -> Vec<NetworkLogRecord> {
    std::mem::take(&mut *queue().lock().unwrap())
}

/// Install the `_lumen_log_network_request(method, url, status, duration_ms)`
/// native binding.
///
/// `status` ≤ 0 → unknown (`None`); `duration_ms` < 0 → unknown (`None`). A
/// blank URL is ignored so a stray call cannot enqueue a junk row.
pub fn install_network_log_bindings(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let f = Function::new(
        ctx.clone(),
        move |method: String, url: String, status: f64, duration_ms: f64| {
            let url = url.trim();
            if url.is_empty() {
                return;
            }
            let method = method.trim();
            let method = if method.is_empty() {
                "GET".to_string()
            } else {
                method.to_uppercase()
            };
            let status = if status >= 1.0 && status <= f64::from(u16::MAX) {
                Some(status as u16)
            } else {
                None
            };
            let duration_ms = if duration_ms >= 0.0 {
                Some(duration_ms as u64)
            } else {
                None
            };
            enqueue(method, url.to_string(), status, duration_ms);
        },
    )?;
    ctx.globals().set("_lumen_log_network_request", f)?;
    // Convenience shim: tolerate missing status / duration arguments.
    ctx.eval::<(), _>(
        "globalThis._lumen_net_log = function(method, url, status, ms) { \
           _lumen_log_network_request(String(method == null ? 'GET' : method), String(url), \
             Number(status == null ? 0 : status), Number(ms == null ? -1 : ms)); \
         };",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuickJsRuntime;
    use lumen_core::JsRuntime;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex, MutexGuard};

    /// Serializes tests: the record queue is process-global, so parallel tests
    /// would otherwise observe each other's enqueues.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn guard() -> MutexGuard<'static, ()> {
        let g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = take_network_log_records();
        g
    }

    fn runtime() -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "", None, None, None, None, None, None, None)
            .unwrap();
        rt
    }

    #[test]
    fn enqueue_and_take_roundtrips() {
        let _g = guard();
        enqueue("GET".into(), "https://example.com/a".into(), Some(200), Some(12));
        let recs = take_network_log_records();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].url, "https://example.com/a");
        assert_eq!(recs[0].status, Some(200));
        assert_eq!(recs[0].duration_ms, Some(12));
    }

    #[test]
    fn take_clears_queue() {
        let _g = guard();
        enqueue("GET".into(), "https://example.com/x".into(), None, None);
        assert_eq!(take_network_log_records().len(), 1);
        assert_eq!(take_network_log_records().len(), 0);
    }

    #[test]
    fn js_call_enqueues() {
        let _g = guard();
        let rt = runtime();
        rt.eval("_lumen_log_network_request('POST', 'https://h/api', 201, 34)")
            .unwrap();
        let recs = take_network_log_records();
        let r = recs.iter().find(|r| r.url == "https://h/api").unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.status, Some(201));
        assert_eq!(r.duration_ms, Some(34));
    }

    #[test]
    fn js_lowercase_method_uppercased() {
        let _g = guard();
        let rt = runtime();
        rt.eval("_lumen_log_network_request('get', 'https://h/m', 200, 1)")
            .unwrap();
        let recs = take_network_log_records();
        assert_eq!(recs.iter().find(|r| r.url == "https://h/m").unwrap().method, "GET");
    }

    #[test]
    fn js_zero_status_becomes_none() {
        let _g = guard();
        let rt = runtime();
        rt.eval("_lumen_log_network_request('GET', 'https://h/pending', 0, -1)")
            .unwrap();
        let recs = take_network_log_records();
        let r = recs.iter().find(|r| r.url == "https://h/pending").unwrap();
        assert_eq!(r.status, None);
        assert_eq!(r.duration_ms, None);
    }

    #[test]
    fn js_blank_url_ignored() {
        let _g = guard();
        let rt = runtime();
        rt.eval("_lumen_log_network_request('GET', '   ', 200, 1)").unwrap();
        assert!(take_network_log_records().is_empty());
    }

    #[test]
    fn js_empty_method_defaults_get() {
        let _g = guard();
        let rt = runtime();
        rt.eval("_lumen_log_network_request('', 'https://h/d', 200, 1)").unwrap();
        let recs = take_network_log_records();
        assert_eq!(recs.iter().find(|r| r.url == "https://h/d").unwrap().method, "GET");
    }

    #[test]
    fn shim_tolerates_missing_args() {
        let _g = guard();
        let rt = runtime();
        rt.eval("_lumen_net_log('GET', 'https://h/shim')").unwrap();
        let recs = take_network_log_records();
        let r = recs.iter().find(|r| r.url == "https://h/shim").unwrap();
        assert_eq!(r.status, None);
        assert_eq!(r.duration_ms, None);
    }
}
