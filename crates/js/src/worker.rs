//! Web Worker implementation (WHATWG Web Workers §4).
//!
//! Each `new Worker(script_url)` call spawns a dedicated `std::thread` with its
//! own [`crate::v8_runtime::V8JsRuntime`] (own OS thread + `v8::OwnedIsolate`,
//! Ph3 V8 migration S10; the rquickjs twin was removed in S12b-B27). Messages
//! are JSON-serialized strings passed through `mpsc` channels in both directions.
//!
//! **Main → worker:** via `Sender<WorkerInMsg>` stored in `WorkerRegistry`.
//! **Worker → main:** via `Arc<Mutex<Vec<(u32,String)>>>` (`WorkerMessageQueue`).
//! The shell drains the queue each event-loop tick by calling
//! `V8JsRuntime::pump_workers()`, which delivers messages to the matching
//! `Worker` instance in JS via `_lumen_deliver_worker_messages(msgs)`.
//!
//! **importScripts():** supported for `data:` and `blob:lumen/` URLs via
//! `WorkerBlobStore` — a Rust-side `Arc<Mutex<HashMap<String, String>>>` that
//! mirrors text blobs registered by `URL.createObjectURL()` on the main thread.
//! The WORKER_SHIM wraps `URL.createObjectURL` to populate this store for any
//! Blob whose MIME type starts with "text/" or is "application/javascript".

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

#[cfg(feature = "v8-backend")]
use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn4};
#[cfg(feature = "v8-backend")]
use crate::v8_runtime::V8JsRuntime;
#[cfg(feature = "v8-backend")]
use lumen_core::JsResult;
#[cfg(feature = "v8-backend")]
use lumen_core::ext::JsRuntime as _;

// ─── message types ────────────────────────────────────────────────────────────

/// Message sent from the main JS thread to a worker thread.
pub enum WorkerInMsg {
    /// JSON-serialized data from `worker.postMessage(data)`.
    Post(String),
    /// Terminate the worker event loop cleanly.
    Terminate,
}

// ─── public registry types ────────────────────────────────────────────────────

/// Live handle to a spawned worker thread.
pub struct WorkerHandle {
    /// Channel used to send messages and terminate signals to the worker.
    pub tx: Sender<WorkerInMsg>,
    /// Join handle — kept so the thread is joined on drop (daemon thread would
    /// silently discard queued output on process exit).
    _thread: thread::JoinHandle<()>,
}

/// All live Worker instances for the current page, keyed by worker ID.
///
/// Shared between the main JS thread (via `Arc` clone in native bindings) and
/// `QuickJsRuntime::pump_workers` which reads it to route terminate calls.
pub type WorkerRegistry = Arc<Mutex<HashMap<u32, WorkerHandle>>>;

/// Outbound message queue: messages posted by worker threads to the main thread.
///
/// Worker threads push `(worker_id, json_string)` pairs; the shell drains the
/// queue on each event-loop tick via `QuickJsRuntime::pump_workers`.
pub type WorkerMessageQueue = Arc<Mutex<Vec<(u32, String)>>>;

/// Shared blob store: blob URL → decoded script text.
///
/// Populated on the main thread via `_lumen_register_worker_blob(url, text)`
/// whenever `URL.createObjectURL` is called with a text/javascript Blob.
/// Worker threads read this store to implement `importScripts('blob:lumen/…')`.
pub type WorkerBlobStore = Arc<Mutex<HashMap<String, String>>>;

/// Outbound error-report queue: uncaught exceptions from an already-started
/// worker (top-level script failure, or an exception from a message/timer
/// callback), parallel to [`WorkerMessageQueue`] (BUG-591 worker parent-side
/// reporting — see `run_worker_thread_v8`/`install_worker_globals_v8`).
///
/// Each entry is `(worker_id, error_info_json)` where `error_info_json` is a
/// JS object literal text `{"message":…,"filename":…,"lineno":…,"colno":…}`,
/// embedded directly (not re-stringified) the same way [`WorkerMessageQueue`]
/// entries are — see [`crate::build_worker_messages_json`].
pub type WorkerErrorQueue = Arc<Mutex<Vec<(u32, String)>>>;

/// Set by `self.close()` inside a running worker thread (BUG-778, dedicated
/// and shared alike). `run_worker_thread_v8`/`run_shared_worker_thread_v8`'s
/// message loop polls this after every dispatched task and stops without
/// servicing any further one — HTML LS §10.2.3/§10.2.4 "close a worker":
/// discard any tasks already queued for the worker, and prevent any further
/// tasks from being queued. Not removed from the parent's registry — a
/// `postMessage` sent after close is simply dropped once the channel's
/// receiver is gone, same as any other dead worker.
pub(crate) type WorkerCloseFlag = Arc<AtomicBool>;

// ─── public API ───────────────────────────────────────────────────────────────

/// Send a JSON-serialized message to a live worker thread.
///
/// No-op if `id` is not registered (e.g. worker already terminated).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn post_to_worker(registry: &WorkerRegistry, id: u32, json: String) {
    if let Some(h) = registry.lock().unwrap().get(&id) {
        let _ = h.tx.send(WorkerInMsg::Post(json));
    }
}

/// Terminate a worker and remove it from the registry.
///
/// Sends a `Terminate` message so the worker thread exits its event loop and
/// the associated `JoinHandle` can be dropped.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn terminate_worker(registry: &WorkerRegistry, id: u32) {
    if let Some(h) = registry.lock().unwrap().remove(&id) {
        let _ = h.tx.send(WorkerInMsg::Terminate);
    }
}

/// Drain all pending messages sent from worker threads to the main thread.
///
/// Returns the drained list; clears the internal queue atomically.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn drain_messages(queue: &WorkerMessageQueue) -> Vec<(u32, String)> {
    std::mem::take(&mut queue.lock().unwrap())
}

/// Drain all pending worker error reports, analogous to [`drain_messages`].
pub fn drain_errors(queue: &WorkerErrorQueue) -> Vec<(u32, String)> {
    queue.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default()
}

/// Escape a Rust string for embedding as a JSON string literal body (between
/// the surrounding `"` quotes). Mirrors `filesystem_access.rs::json_escape`
/// (kept local rather than shared — small, and the two crates' JSON needs
/// have historically diverged in scope).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Build the `{"message":…,"filename":…,"lineno":…,"colno":…}` object-literal
/// text pushed into a [`WorkerErrorQueue`] — the parent's `Worker.prototype`
/// `error` handler reads these four fields directly (`WORKER_SHIM`'s
/// `_deliverError`). `pub(crate)` so `shared_worker.rs` can build the same
/// shape for its own (broadcast) error queue instead of duplicating this.
pub(crate) fn error_info_json(message: &str, filename: &str, lineno: i32, colno: i32) -> String {
    format!(
        "{{\"message\":\"{}\",\"filename\":\"{}\",\"lineno\":{lineno},\"colno\":{colno}}}",
        json_escape(message),
        json_escape(filename),
    )
}

// ─── base64 helpers ───────────────────────────────────────────────────────────

/// Decode standard base64 (RFC 4648 §4) to bytes.
///
/// Returns `None` on any invalid character or bad padding. Whitespace is skipped
/// so that multi-line base64 (as produced by some tools) is accepted.
fn b64_decode(encoded: &str) -> Option<Vec<u8>> {
    const INVALID: u8 = 0xFF;
    let table: [u8; 256] = {
        let mut t = [INVALID; 256];
        for (i, &c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
            .iter()
            .enumerate()
        {
            t[c as usize] = i as u8;
        }
        t
    };

    let mut out = Vec::with_capacity(encoded.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;

    for b in encoded.bytes() {
        if b == b'=' || b == b'\n' || b == b'\r' || b == b' ' {
            continue;
        }
        let v = table[b as usize];
        if v == INVALID {
            return None;
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Encode bytes as standard base64 (RFC 4648 §4). Used by the V8 `btoa`
/// native in [`btoa_native_v8`].
#[cfg(feature = "v8-backend")]
fn b64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[(n >> 18) as usize] as char);
        out.push(CHARS[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 { CHARS[((n >> 6) & 0x3F) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[(n & 0x3F) as usize] as char } else { '=' });
    }
    out
}

/// Minimal percent-decoder for `data:` URL content fields.
///
/// Decodes `%XX` sequences; passes everything else through as-is.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolve a URL to its script text for `importScripts()` use.
///
/// Supported schemes:
/// - `data:[type][;base64],<content>` — decoded inline; no network required.
/// - `blob:lumen/<id>` — looked up in `blob_store`.
/// - anything else — a synchronous GET via `fetch_provider` (BUG-778's
///   WPT-RUN-6 extension: `importScripts()` previously only worked for
///   `data:`/`blob:lumen/`, but the wrapper wptrunner builds for every
///   `.worker.html`/`.any.worker.html`/`.any.sharedworker.html` test opens
///   with `importScripts("/resources/testharness.js")`). `url` is expected
///   pre-resolved to absolute by the calling JS shim (`_lumen_worker_base_url`
///   — this function has no worker/document base URL of its own); reuses the
///   same bridge [`fetch_worker_script`] uses for the worker's own classic
///   script, not new machinery.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn resolve_import_url(
    url: &str,
    blob_store: &WorkerBlobStore,
    fetch_provider: Option<&dyn lumen_core::ext::JsFetchProvider>,
) -> Option<String> {
    if let Some(rest) = url.strip_prefix("data:") {
        let comma = rest.find(',').unwrap_or(rest.len());
        let meta = &rest[..comma];
        let content = if comma < rest.len() { &rest[comma + 1..] } else { "" };

        if meta.contains("base64") {
            b64_decode(content)
                .and_then(|b| String::from_utf8(b).ok())
        } else {
            Some(percent_decode(content))
        }
    } else if url.starts_with("blob:lumen/") {
        blob_store.lock().unwrap().get(url).cloned()
    } else {
        fetch_worker_script(fetch_provider, url)
    }
}

// ─── shared WorkerGlobalScope surface ─────────────────────────────────────────

/// The base URL a worker scope resolves a relative `importScripts()`/`fetch()`/
/// `XMLHttpRequest` target against (BUG-778): its own script URL, or the empty
/// string when that URL is opaque — nothing can be resolved against a `blob:`
/// or `data:` URL, and the shims' `_lumen_worker_base_url` guard is a
/// truthiness check for exactly that case.
///
/// Derived here rather than passed in alongside the script URL (BUG-776 needed
/// the *unreduced* URL for `location`) so the two can never be swapped at a
/// call site: every worker flavour hands its own script URL down and gets both
/// values from it.
pub(crate) fn worker_base_url(script_url: &str) -> &str {
    if script_url.starts_with("blob:") || script_url.starts_with("data:") {
        ""
    } else {
        script_url
    }
}

/// Install the parts of the global scope that WHATWG exposes in **every**
/// `WorkerGlobalScope`, not just the dedicated-worker one: the `_lumen_now_ms`
/// clock native, the `_lumen_navigator_id` values (BUG-776) and
/// [`crate::dom::worker_exposed_shim`] (`EventTarget`, the `Performance`
/// interface with its `performance` singleton, and `WorkerLocation`/
/// `WorkerNavigator` with the `navigator` singleton).
///
/// Called by all three worker flavours — dedicated ([`install_worker_globals_v8`]),
/// shared (`shared_worker.rs`) and service (`sw_worker.rs`) — before each
/// evaluates its own flavour-specific scope shim. Sharing the page's shim source
/// rather than re-writing it here is the point: a second copy of `Performance`
/// would silently drift from the page one (BUG-401 was filed right after BUG-400
/// had rebuilt the page copy as a real `EventTarget` subclass).
///
/// `_lumen_now_ms` is registered per worker runtime and reports wall-clock
/// milliseconds since the Unix epoch, the same contract as the main-context
/// native of that name. It deliberately does **not** honour `--deterministic`:
/// the deterministic clock/RNG patch is evaluated in the page context only, so
/// `Date.now()` and `Math.random()` are already live inside every worker —
/// freezing `performance.now()` alone would fake a determinism the scope does
/// not have ([BUG-768](bugs/BUG-768-OPEN.md)).
#[cfg(feature = "v8-backend")]
pub(crate) fn install_worker_scope_globals_v8(rt: &V8JsRuntime) -> JsResult<()> {
    rt.register_native(
        "_lumen_now_ms",
        into_v8_fn0(move || -> f64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0)
        }),
    )?;
    // BUG-776: the `NavigatorID` values `WORKER_LOCATION_NAVIGATOR_SHIM` (part
    // of `worker_exposed_shim`) reads — built here rather than baked into that
    // shim so `platform`/`language`/`languages` come from the same
    // `NavigatorProfile` the page's own antidetect layer uses.
    rt.eval(&crate::navigator_bindings::worker_navigator_id_shim())?;
    rt.eval(&crate::dom::worker_exposed_shim())?;
    rt.eval(WORKER_ERROR_EVENT_SHIM)?;
    Ok(())
}

/// `ErrorEvent` for a `WorkerGlobalScope` (BUG-813) — the object handed to an
/// `addEventListener('error', …)` listener inside a worker, and the only event
/// class such a scope needs.
///
/// A local definition rather than a slice of the page shim because the page's
/// `Event` hierarchy lives in `WEB_API_SHIM_MID`, which is page-only (it pulls
/// in `document`, `window` and the whole UI-event family); splitting `Event`
/// out the way `EVENT_TARGET_SHIM` was split is a larger change than this bug.
/// The consequence to know: inside a worker `errorEvent instanceof Event` is
/// false, because there is no `Event` in that scope to inherit from.
///
/// Evaluated for every worker flavour that goes through
/// [`install_worker_scope_globals_v8`] — dedicated, shared and service —
/// and guarded so a scope that already has the page-side class keeps it.
#[cfg(feature = "v8-backend")]
pub(crate) const WORKER_ERROR_EVENT_SHIM: &str = r#"
if (typeof globalThis.ErrorEvent !== 'function') {
  var _LumenWorkerErrorEvent = function ErrorEvent(type, init) {
    this.type = String(type);
    this.message  = (init && init.message  != null) ? String(init.message)  : '';
    this.filename = (init && init.filename != null) ? String(init.filename) : '';
    this.lineno   = (init && init.lineno   != null) ? (init.lineno | 0) : 0;
    this.colno    = (init && init.colno    != null) ? (init.colno  | 0) : 0;
    this.error    = (init && init.error    !== undefined) ? init.error : null;
    this.bubbles    = !!(init && init.bubbles);
    this.cancelable = !!(init && init.cancelable);
    this.defaultPrevented = false;
    this.target = null;
    this.currentTarget = null;
  };
  _LumenWorkerErrorEvent.prototype.preventDefault = function() {
    if (this.cancelable) this.defaultPrevented = true;
  };
  _LumenWorkerErrorEvent.prototype.stopPropagation = function() {};
  _LumenWorkerErrorEvent.prototype.stopImmediatePropagation = function() {};
  // `assert_class_string(e, 'ErrorEvent')` (WPT's testharness) reads
  // `Object.prototype.toString`, which for a plain constructor answers
  // `[object Object]` without this.
  Object.defineProperty(_LumenWorkerErrorEvent.prototype, Symbol.toStringTag, {
    value: 'ErrorEvent', configurable: true,
  });
  globalThis.ErrorEvent = _LumenWorkerErrorEvent;
}
undefined;
"#;

// ─── worker thread ────────────────────────────────────────────────────────────

/// Build the worker-thread global-scope shim source for a given worker id.
///
/// Pure JS (no engine-specific bits), used by [`install_worker_globals_v8`].
/// Provides `self`, `postMessage`, `onmessage`, `addEventListener`,
/// `removeEventListener`, `_lumen_worker_dispatch_message`, `console`,
/// `importScripts` (data: + blob: URLs). Timers (`setTimeout`/`setInterval`/
/// `queueMicrotask` and friends) come from [`WORKER_TIMERS_SHIM`], shared with
/// the shared-worker scope. `atob`/`btoa` are installed separately as natives since they need
/// Rust-side base64 codecs; `EventTarget` and `performance` come from the
/// shim shared with the page scope ([`install_worker_scope_globals_v8`]).
#[cfg(feature = "v8-backend")]
fn worker_global_shim(worker_id: u32) -> String {
    format!(
        r#"(function(wid) {{
  var _msgListeners = [];
  var _onmessage = null;

  globalThis.self = globalThis;
  globalThis.name = 'worker-' + wid;

  // `DedicatedWorkerGlobalScope` (BUG-777) — see the factory's own comment in
  // `crate::dom::WORKER_LOCATION_NAVIGATOR_SHIM`.
  if (typeof _lumen_define_worker_scope === 'function') {{
    _lumen_define_worker_scope('DedicatedWorkerGlobalScope');
  }}

  // `location` — HTML LS §10.2.4: the scope's own script URL, set by Rust as
  // `_lumen_worker_location_url` before this shim runs (BUG-776). The class and
  // the `navigator` singleton come from the shim shared with the other two
  // worker flavours (`crate::dom::WORKER_LOCATION_NAVIGATOR_SHIM`, evaluated by
  // `install_worker_scope_globals_v8` just before this one).
  if (typeof _lumen_make_worker_location === 'function') {{
    globalThis.location = _lumen_make_worker_location(
      typeof _lumen_worker_location_url === 'string' ? _lumen_worker_location_url : '');
  }}

  // ── "report the exception" inside a WorkerGlobalScope ──────────────────────
  // HTML LS §8.1.3.6 then §10.2.6 "runtime script errors": an uncaught
  // exception fires `error` at the worker's *own* global scope first, and only
  // if nothing there cancelled it does it reach the owning `Worker` object on
  // the page (BUG-813; the page half alone was BUG-591). Sources: the top-level
  // script (Rust calls this through `eval_and_report_via`, so `err` is the real
  // thrown value and the location comes from `v8::Message`), a message
  // callback, a flushed timer, and the net shim's listeners.
  var _onerror = null;
  var _errListeners = [];
  // Re-entrancy guard: an exception thrown *by* an error handler must not fire
  // `error` again (that recurses), but must not be swallowed either — it goes
  // straight to the parent instead.
  var _reportingError = false;

  Object.defineProperty(globalThis, 'onerror', {{
    get: function() {{ return _onerror; }},
    set: function(fn) {{ _onerror = typeof fn === 'function' ? fn : null; }},
    configurable: true,
  }});

  // `filename`/`lineno`/`colno` are best-effort-parsed from `.stack` the same
  // way the page-side `_lumen_report_exception` (`crate::dom::WEB_API_SHIM`)
  // does, since V8's `Error` has no structured location API from script.
  function _lumen_worker_error_location(err) {{
    if (err && typeof err.stack === 'string') {{
      var lines = err.stack.split('\n');
      for (var i = 0; i < lines.length; i++) {{
        var m = /at (?:.*\()?([^\s()]+):(\d+):(\d+)\)?\s*$/.exec(lines[i]);
        if (m) return {{ filename: m[1], lineno: +m[2], colno: +m[3] }};
      }}
    }}
    return {{ filename: '', lineno: 0, colno: 0 }};
  }}

  function _lumen_report_worker_exception(err, filename, lineno, colno) {{
    var message = (err instanceof Error) ? String(err.message) : String(err);
    var loc = _lumen_worker_error_location(err);
    var file = (typeof filename === 'string' && filename) ? filename : loc.filename;
    var line = (typeof lineno === 'number' && lineno) ? lineno : loc.lineno;
    var col  = (typeof colno === 'number' && colno) ? colno : loc.colno;
    // A worker's classic script is compiled with no resource name, so neither
    // `v8::Message` nor `Error.stack` carries the URL HTML LS requires here —
    // and every frame of such a stack belongs to the worker's own script.
    if (!file || file === '<anonymous>') {{
      file = (globalThis.location && globalThis.location.href) || '';
    }}
    // Read by `run_worker_thread_v8` to tell "the scope reported this" from a
    // module *load* failure, which never reaches this function at all.
    globalThis._lumen_worker_error_reported = true;

    if (_reportingError) {{ _lumen_worker_report_error(message, file, line, col); return; }}
    _reportingError = true;
    var cancelled = false;
    try {{
      // `onerror` on a global scope is an OnErrorEventHandler (WebIDL): it
      // takes the five legacy arguments rather than the event object, and
      // cancels by returning true — `support/ErrorEvent.js` returns false
      // precisely so the page still gets the error.
      if (typeof _onerror === 'function') {{
        try {{
          if (_onerror.call(globalThis, message, file, line, col, err) === true) cancelled = true;
        }} catch (e) {{ _lumen_report_worker_exception(e); }}
      }}
      if (_errListeners.length) {{
        var ev = new ErrorEvent('error', {{
          message: message, filename: file, lineno: line, colno: col,
          error: err, bubbles: false, cancelable: true,
        }});
        ev.target = globalThis;
        for (var i = 0; i < _errListeners.length; i++) {{
          try {{ _errListeners[i].call(globalThis, ev); }}
          catch (e) {{ _lumen_report_worker_exception(e); }}
        }}
        if (ev.defaultPrevented) cancelled = true;
      }}
    }} finally {{ _reportingError = false; }}
    if (!cancelled) _lumen_worker_report_error(message, file, line, col);
  }}
  // The top-level script is evaluated by Rust (`eval_and_report_via`), which
  // can only reach a global.
  globalThis._lumen_report_worker_exception = _lumen_report_worker_exception;

  // postMessage(data) — send data back to the main thread.
  globalThis.postMessage = function(data) {{
    _lumen_worker_post_reply(JSON.stringify(data));
  }};

  Object.defineProperty(globalThis, 'onmessage', {{
    get: function() {{ return _onmessage; }},
    set: function(fn) {{ _onmessage = typeof fn === 'function' ? fn : null; }},
    configurable: true,
  }});

  globalThis.addEventListener = function(type, fn, _opts) {{
    if (typeof fn !== 'function') return;
    if (type === 'message') _msgListeners.push(fn);
    else if (type === 'error') _errListeners.push(fn);   // BUG-813
  }};

  globalThis.removeEventListener = function(type, fn) {{
    var list = type === 'message' ? _msgListeners : (type === 'error' ? _errListeners : null);
    if (!list) return;
    var i = list.indexOf(fn);
    if (i !== -1) list.splice(i, 1);
  }};

  // Reconstruct transferred OffscreenCanvas sentinels inside received data.
  // Called recursively on the parsed data object before delivering to handlers.
  function _deserializeTransfers(obj) {{
    if (!obj || typeof obj !== 'object') return obj;
    if (obj.__lumen_sentinel__ === '__lumen_offscreen_transfer__') {{
      // Restore OffscreenCanvas from pixel data using the existing native binding.
      var cid = _lumen_offscreen_canvas_from_image_data(obj.w >>> 0, obj.h >>> 0, obj.p || '');
      if (cid === 0) return null;
      var oc = Object.create(OffscreenCanvas.prototype);
      oc.__canvas_id__ = cid;
      oc.width = obj.w >>> 0;
      oc.height = obj.h >>> 0;
      oc._2d_context = null;
      return oc;
    }}
    if (Array.isArray(obj)) {{
      return obj.map(_deserializeTransfers);
    }}
    var out = {{}};
    for (var k in obj) {{
      if (Object.prototype.hasOwnProperty.call(obj, k)) {{
        out[k] = _deserializeTransfers(obj[k]);
      }}
    }}
    return out;
  }}

  // Called by the worker message loop for each incoming postMessage.
  globalThis._lumen_worker_dispatch_message = function(data) {{
    // Reconstruct any OffscreenCanvas objects serialized by the main thread.
    var resolved = (typeof _lumen_offscreen_canvas_from_image_data !== 'undefined')
      ? _deserializeTransfers(data)
      : data;
    var ev = {{ data: resolved, type: 'message', target: globalThis,
                bubbles: false, cancelable: false }};
    if (_onmessage) {{ try {{ _onmessage(ev); }} catch(e) {{ _lumen_report_worker_exception(e); }} }}
    for (var i = 0; i < _msgListeners.length; i++) {{
      try {{ _msgListeners[i](ev); }} catch(e) {{ _lumen_report_worker_exception(e); }}
    }}
  }};

  // Minimal console (no DOM — write to stderr via native binding).
  globalThis.console = {{
    log:   function() {{ _lumen_worker_console_log(Array.prototype.map.call(arguments, String).join(' ')); }},
    info:  function() {{ _lumen_worker_console_log(Array.prototype.map.call(arguments, String).join(' ')); }},
    warn:  function() {{ _lumen_worker_console_log('[WARN] ' + Array.prototype.map.call(arguments, String).join(' ')); }},
    error: function() {{ _lumen_worker_console_log('[ERR]  ' + Array.prototype.map.call(arguments, String).join(' ')); }},
    debug: function() {{}},
  }};

  // importScripts(url1[, url2, …]) — WHATWG Web Workers §4.2.3.
  // Synchronously loads and evaluates one or more scripts. `data:`/
  // `blob:lumen/` resolve locally; anything else is resolved against the
  // worker's own script URL (`_lumen_worker_base_url`, set at worker
  // creation — empty for a blob:/data: worker) and fetched over the network
  // (BUG-778 — previously only data:/blob: worked at all, and an http(s) URL
  // always threw NetworkError even when absolute).
  globalThis.importScripts = function() {{
    // HTML LS §10.2.3: a module worker may only pull code in through its
    // module graph, so `importScripts()` throws unconditionally there — even
    // for zero arguments, and before any URL is looked at (BUG-777).
    if (typeof _lumen_worker_is_module !== 'undefined' && _lumen_worker_is_module) {{
      throw new TypeError('importScripts() is not available in a module worker');
    }}
    for (var i = 0; i < arguments.length; i++) {{
      var u = String(arguments[i]);
      var resolved = u;
      if (u.indexOf('://') === -1 && u.slice(0, 5) !== 'data:' && u.slice(0, 5) !== 'blob:'
          && typeof _lumen_worker_base_url === 'string' && _lumen_worker_base_url) {{
        try {{ resolved = new URL(u, _lumen_worker_base_url).href; }} catch (e) {{ resolved = u; }}
      }}
      var script = _lumen_import_scripts_resolve(resolved);
      if (script === null || script === undefined) {{
        throw new Error('importScripts: cannot load script: ' + resolved);
      }}
      (1, eval)(script);
    }}
  }};

  // `setTimeout`/`setInterval`/`queueMicrotask` come from the shim shared with
  // the shared-worker scope (`crate::worker::WORKER_TIMERS_SHIM`, evaluated as
  // its own IIFE right after this one), which is what actually drives them
  // (BUG-815).

  // Exposed so the shared net shim (WORKER_NET_SHIM, evaluated as a
  // separate IIFE right after this one) can route a throwing fetch/XHR
  // listener through the same reporting path (BUG-591 shape, BUG-778 scope).
  globalThis._lumen_worker_exception_reporter = _lumen_report_worker_exception;

  // close() — HTML LS §10.2.3 "close a worker" (BUG-778): discard further
  // queued tasks. `_lumen_worker_self_close` flips a shared flag that
  // `run_worker_thread_v8`'s message loop polls after every dispatched task
  // and stops on, without servicing any more.
  // `_lumen_worker_closed` is the JS-visible half of the same flag, read by
  // `WORKER_TIMERS_SHIM`'s task loop so a `close()` from inside a timer stops
  // the remaining due timers of that very flush rather than only the next one.
  globalThis.close = function() {{
    globalThis._lumen_worker_closed = true;
    _lumen_worker_self_close();
  }};

}})({worker_id});"#
    )
}

/// Timers for a `WorkerGlobalScope` — `setTimeout`, `setInterval`,
/// `clearTimeout`, `clearInterval`, `queueMicrotask` (BUG-815).
///
/// Shared verbatim by the dedicated ([`install_worker_globals_v8`]) and shared
/// (`shared_worker.rs::install_shared_worker_globals_v8`) scopes, evaluated as
/// its own IIFE right after the flavour-specific globals shim. It depends on
/// nothing but `globalThis._lumen_worker_exception_reporter`, which that shim
/// has already set, and it is read lazily so the two flavours can keep their
/// own reporting functions.
///
/// The queue this replaces stored callbacks with no deadline at all, was
/// drained only from the Rust message loop, and aliased `setInterval` to
/// `setTimeout` — so a worker nobody wrote to never ran a timer, a delay was
/// ignored, and an interval fired exactly once (BUG-815). What makes the queue
/// *run* is on the Rust side: [`run_worker_tasks`] calls
/// `_lumen_worker_run_tasks` below and sleeps on `recv_timeout` for exactly the
/// interval it returns, so the thread wakes on whichever comes first — the next
/// deadline or an incoming message.
///
/// Two spec details this carries that the page shim also has, and for the same
/// reasons: a delay goes through the WebIDL `long` conversion (`| 0`, which is
/// ToInt32) before being clamped to a non-negative value, and the §8.6 nesting
/// clamp raises any delay below 4 ms to 4 once the nesting level passes 5.
/// The clamp is what keeps `setInterval(fn, 0)` from spinning this thread —
/// a repeat re-runs the timer initialization steps with the nesting level
/// incremented, so the third cycle onward is 4 ms apart.
#[cfg(feature = "v8-backend")]
pub(crate) const WORKER_TIMERS_SHIM: &str = r#"(function() {
  // {id, fn, args, due (epoch ms), delay, interval (bool), nesting, seq}
  var _timers = [];
  var _micro = [];
  var _nextId = 1;
  // HTML LS §8.6 "timer nesting level" of the task currently running, so a
  // timer armed from inside a callback inherits its parent's depth.
  var _nesting = 0;

  function _report(e) {
    var r = globalThis._lumen_worker_exception_reporter;
    if (typeof r === 'function') { try { r(e); } catch (_e) {} }
  }

  // WebIDL `long`: ToInt32 (so 2^32 becomes 0, matching every other engine),
  // then HTML LS §8.6 step 5 — a negative timeout becomes 0.
  function _toDelay(v) {
    var n = Number(v) | 0;
    return n < 0 ? 0 : n;
  }

  function _clamp(delay, nesting) {
    return (nesting > 5 && delay < 4) ? 4 : delay;
  }

  function _schedule(fn, delay, args, repeating) {
    if (typeof fn !== 'function') return 0;
    var nesting = _nesting + 1;
    var d = _toDelay(delay);
    var id = _nextId++;
    _timers.push({
      id: id, fn: fn, args: args, delay: d, interval: repeating,
      nesting: nesting, due: Date.now() + _clamp(d, nesting), seq: id,
    });
    return id;
  }

  globalThis.setTimeout = function(fn, delay) {
    return _schedule(fn, delay, Array.prototype.slice.call(arguments, 2), false);
  };
  globalThis.setInterval = function(fn, delay) {
    return _schedule(fn, delay, Array.prototype.slice.call(arguments, 2), true);
  };
  globalThis.clearTimeout = function(id) {
    for (var i = 0; i < _timers.length; i++) {
      if (_timers[i].id === id) { _timers.splice(i, 1); return; }
    }
  };
  // One list, one id space — as the spec's single "map of active timers" has.
  globalThis.clearInterval = globalThis.clearTimeout;

  globalThis.queueMicrotask = function(fn) {
    if (typeof fn !== 'function') {
      throw new TypeError('queueMicrotask: callback is not a function');
    }
    _micro.push(fn);
  };

  function _drainMicrotasks() {
    while (_micro.length) {
      var fn = _micro.shift();
      try { fn(); } catch (e) { _report(e); }
    }
  }

  // Index of the due timer that should run next: earliest deadline, ties
  // broken by insertion order (`seq`, re-stamped on every repeat so a
  // reloaded interval queues behind whatever was already waiting).
  function _nextDue(now) {
    var best = -1;
    for (var i = 0; i < _timers.length; i++) {
      var t = _timers[i];
      if (t.due > now) continue;
      if (best === -1 || t.due < _timers[best].due
          || (t.due === _timers[best].due && t.seq < _timers[best].seq)) {
        best = i;
      }
    }
    return best;
  }

  // Run everything already due, then report how long the thread may sleep:
  // milliseconds until the next deadline, or -1 when nothing is pending.
  // Called from Rust once per turn of the worker's task loop.
  globalThis._lumen_worker_run_tasks = function() {
    _drainMicrotasks();
    for (;;) {
      var now = Date.now();
      var i = _nextDue(now);
      if (i === -1) break;
      var task = _timers[i];
      if (task.interval) {
        // HTML LS §8.6 step 12: a repeating timer re-runs the initialization
        // steps with the nesting level incremented — which is what puts the
        // 4 ms floor under a zero-delay interval after a few cycles.
        task.nesting += 1;
        task.due = now + _clamp(task.delay, task.nesting);
        task.seq = _nextId++;
      } else {
        _timers.splice(i, 1);
      }
      var outer = _nesting;
      _nesting = task.nesting;
      try { task.fn.apply(globalThis, task.args); } catch (e) { _report(e); }
      _nesting = outer;
      _drainMicrotasks();
      // A callback may have called close(); the Rust loop checks the flag
      // right after this returns, so just stop handing out more work.
      if (globalThis._lumen_worker_closed === true) break;
    }
    if (_micro.length) return 0;
    if (!_timers.length) return -1;
    var soonest = Infinity;
    for (var j = 0; j < _timers.length; j++) {
      if (_timers[j].due < soonest) soonest = _timers[j].due;
    }
    var wait = soonest - Date.now();
    return wait > 0 ? wait : 0;
  };

  // The name the message-loop eval strings have called since before the queue
  // had deadlines; kept so the dispatch path still flushes what is due.
  globalThis._lumen_flush_timers = function() { globalThis._lumen_worker_run_tasks(); };
})();
"#;

/// Shared `fetch()`/`XMLHttpRequest`/`Headers`/`Response` surface for a
/// `WorkerGlobalScope` (BUG-778): minimal but spec-shaped, synchronous over
/// the same `_lumen_worker_net_fetch` bridge both dedicated
/// ([`install_worker_globals_v8`]) and shared
/// (`shared_worker.rs::install_shared_worker_globals_v8`) worker scopes
/// register. Evaluated as its own IIFE right after the flavour-specific
/// globals shim, so it only depends on globals that shim already defined:
/// `_lumen_worker_net_fetch` (native), `_lumen_worker_base_url` (native-set
/// global, used for relative-URL resolution), and — if present —
/// `globalThis._lumen_worker_exception_reporter` for routing a throwing
/// listener the same way BUG-591 does elsewhere.
///
/// Base64 codecs are self-contained (`_lumenB64ToBin`/`_lumenUtf8ToBin`/
/// `_lumenBinToB64`) rather than relying on `atob`/`btoa`: the dedicated
/// worker scope has them (`install_worker_globals_v8`'s scoped natives), but
/// the shared-worker scope does not — see the module doc.
#[cfg(feature = "v8-backend")]
pub(crate) const WORKER_NET_SHIM: &str = r#"(function() {
  function _lumenReportException(e) {
    var r = globalThis._lumen_worker_exception_reporter;
    if (typeof r === 'function') { try { r(e); } catch (_e) {} }
  }

  function _lumenB64ToBin(s) {
    var CHARS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
    var out = '', buf = 0, bits = 0;
    for (var i = 0; i < s.length; i++) {
      var v = CHARS.indexOf(s.charAt(i));
      if (v < 0) continue;
      buf = (buf << 6) | v; bits += 6;
      if (bits >= 8) { bits -= 8; out += String.fromCharCode((buf >> bits) & 0xFF); }
    }
    return out;
  }
  function _lumenBinToUtf8(b) {
    var out = '', i = 0;
    while (i < b.length) {
      var c = b.charCodeAt(i++) & 0xFF, cp;
      if (c < 0x80) cp = c;
      else if (c < 0xE0) cp = ((c & 0x1F) << 6) | (b.charCodeAt(i++) & 0x3F);
      else if (c < 0xF0) cp = ((c & 0x0F) << 12) | ((b.charCodeAt(i++) & 0x3F) << 6)
                            | (b.charCodeAt(i++) & 0x3F);
      else cp = ((c & 0x07) << 18) | ((b.charCodeAt(i++) & 0x3F) << 12)
              | ((b.charCodeAt(i++) & 0x3F) << 6) | (b.charCodeAt(i++) & 0x3F);
      out += String.fromCodePoint(cp);
    }
    return out;
  }
  function _lumenUtf8ToBin(str) {
    var out = '';
    for (var i = 0; i < str.length; i++) {
      var cp = str.codePointAt(i);
      if (cp > 0xFFFF) i++;
      if (cp < 0x80) { out += String.fromCharCode(cp); }
      else if (cp < 0x800) {
        out += String.fromCharCode(0xC0 | (cp >> 6), 0x80 | (cp & 0x3F));
      } else if (cp < 0x10000) {
        out += String.fromCharCode(0xE0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3F), 0x80 | (cp & 0x3F));
      } else {
        out += String.fromCharCode(
          0xF0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3F),
          0x80 | ((cp >> 6) & 0x3F), 0x80 | (cp & 0x3F));
      }
    }
    return out;
  }
  function _lumenBinToB64(bin) {
    var CHARS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
    var out = '';
    for (var i = 0; i < bin.length; i += 3) {
      var b0 = bin.charCodeAt(i) & 0xFF;
      var b1 = i + 1 < bin.length ? bin.charCodeAt(i + 1) & 0xFF : 0;
      var b2 = i + 2 < bin.length ? bin.charCodeAt(i + 2) & 0xFF : 0;
      var n = (b0 << 16) | (b1 << 8) | b2;
      out += CHARS.charAt((n >> 18) & 0x3F);
      out += CHARS.charAt((n >> 12) & 0x3F);
      out += (i + 1 < bin.length) ? CHARS.charAt((n >> 6) & 0x3F) : '=';
      out += (i + 2 < bin.length) ? CHARS.charAt(n & 0x3F) : '=';
    }
    return out;
  }

  function _lumenResolve(url) {
    if (url.indexOf('://') !== -1) return url;
    var base = (typeof _lumen_worker_base_url === 'string' && _lumen_worker_base_url)
      ? _lumen_worker_base_url
      : (typeof location !== 'undefined' ? location.href : '');
    if (!base) return url;
    try { return new URL(url, base).href; } catch (e) { return url; }
  }

  // Minimal Headers (Fetch §3).
  function Headers(init) {
    this._h = {};
    if (init) {
      if (init instanceof Headers) { for (var k in init._h) this._h[k] = init._h[k]; }
      else { for (var k2 in init) this._h[String(k2).toLowerCase()] = String(init[k2]); }
    }
  }
  Headers.prototype.get = function(n) { var v = this._h[String(n).toLowerCase()]; return v === undefined ? null : v; };
  Headers.prototype.set = function(n, v) { this._h[String(n).toLowerCase()] = String(v); };
  Headers.prototype.has = function(n) { return String(n).toLowerCase() in this._h; };
  Headers.prototype.append = function(n, v) {
    var k = String(n).toLowerCase();
    this._h[k] = (k in this._h) ? this._h[k] + ', ' + String(v) : String(v);
  };
  Headers.prototype['delete'] = function(n) { delete this._h[String(n).toLowerCase()]; };
  Headers.prototype.forEach = function(fn, thisArg) {
    for (var k in this._h) fn.call(thisArg, this._h[k], k, this);
  };
  globalThis.Headers = Headers;

  // Minimal Response (Fetch §5.7) — body arrives as a "binary string"
  // (one char = one byte) already decoded from the wire's base64 transport.
  function Response(bodyBin, init) {
    this._bin = bodyBin || '';
    init = init || {};
    this.status = init.status || 200;
    this.statusText = init.statusText || '';
    this.ok = this.status >= 200 && this.status < 300;
    this.headers = (init.headers instanceof Headers) ? init.headers : new Headers(init.headers);
    this.url = init.url || '';
    this.bodyUsed = false;
  }
  Response.prototype.text = function() {
    this.bodyUsed = true;
    return Promise.resolve(_lumenBinToUtf8(this._bin));
  };
  Response.prototype.json = function() {
    return this.text().then(function(t) { return JSON.parse(t); });
  };
  Response.prototype.arrayBuffer = function() {
    this.bodyUsed = true;
    var b = this._bin;
    var buf = new ArrayBuffer(b.length);
    var view = new Uint8Array(buf);
    for (var i = 0; i < b.length; i++) view[i] = b.charCodeAt(i) & 0xFF;
    return Promise.resolve(buf);
  };
  globalThis.Response = Response;

  // fetch(resource[, init]) — WHATWG Fetch, synchronous network via the
  // `_lumen_worker_net_fetch` bridge (BUG-778: previously undefined inside
  // any worker, so a worker script could only receive/post messages).
  globalThis.fetch = function(resource, init) {
    var url = (typeof resource === 'string') ? resource : (resource && resource.url) || '';
    init = init || {};
    var method = String(init.method || (resource && resource.method) || 'GET').toUpperCase();
    var abs = _lumenResolve(url);
    var reqHeaders = new Headers(init.headers);
    var flat = [];
    reqHeaders.forEach(function(v, k) { flat.push(k); flat.push(v); });
    var bodyB64 = (init.body !== undefined && init.body !== null)
      ? _lumenBinToB64(_lumenUtf8ToBin(String(init.body))) : null;
    var raw;
    try { raw = _lumen_worker_net_fetch(abs, method, flat, bodyB64); }
    catch (e) { return Promise.reject(new TypeError('fetch: ' + e)); }
    if (!raw) return Promise.reject(new TypeError('fetch: network error for ' + abs));
    var res = JSON.parse(raw);
    return Promise.resolve(new Response(_lumenB64ToBin(res.body), {
      status: res.status, statusText: res.statusText, headers: res.headers, url: abs,
    }));
  };

  // XMLHttpRequest — minimal synchronous port over the same bridge
  // (BUG-778). `send()` performs the request immediately and fires all
  // readystatechange/load/error transitions on the same turn: a worker
  // thread here has no real async event loop to defer them onto.
  function XMLHttpRequest() {
    this.readyState = 0;
    this.status = 0;
    this.statusText = '';
    this.response = '';
    this.responseText = '';
    this._listeners = {};
    this._method = 'GET';
    this._url = '';
    this._headers = [];
  }
  XMLHttpRequest.UNSENT = 0;
  XMLHttpRequest.OPENED = 1;
  XMLHttpRequest.HEADERS_RECEIVED = 2;
  XMLHttpRequest.LOADING = 3;
  XMLHttpRequest.DONE = 4;
  XMLHttpRequest.prototype.open = function(method, url) {
    this._method = String(method || 'GET').toUpperCase();
    this._url = String(url || '');
    this.readyState = 1;
  };
  XMLHttpRequest.prototype.setRequestHeader = function(name, value) {
    this._headers.push(String(name)); this._headers.push(String(value));
  };
  XMLHttpRequest.prototype.addEventListener = function(type, fn) {
    if (typeof fn !== 'function') return;
    (this._listeners[type] || (this._listeners[type] = [])).push(fn);
  };
  XMLHttpRequest.prototype.removeEventListener = function(type, fn) {
    var l = this._listeners[type];
    if (!l) return;
    var i = l.indexOf(fn);
    if (i !== -1) l.splice(i, 1);
  };
  XMLHttpRequest.prototype._fire = function(type) {
    var ev = { type: type, target: this };
    var onProp = this['on' + type];
    if (typeof onProp === 'function') { try { onProp.call(this, ev); } catch (e) { _lumenReportException(e); } }
    var l = this._listeners[type] || [];
    for (var i = 0; i < l.length; i++) { try { l[i].call(this, ev); } catch (e) { _lumenReportException(e); } }
  };
  XMLHttpRequest.prototype.send = function(body) {
    var abs = _lumenResolve(this._url);
    var bodyB64 = (body !== undefined && body !== null)
      ? _lumenBinToB64(_lumenUtf8ToBin(String(body))) : null;
    var raw;
    try { raw = _lumen_worker_net_fetch(abs, this._method, this._headers.slice(), bodyB64); }
    catch (e) { raw = null; }
    if (!raw) {
      this.readyState = 4;
      this._fire('readystatechange');
      this._fire('error');
      this._fire('loadend');
      return;
    }
    var res = JSON.parse(raw);
    this.status = res.status;
    this.statusText = res.statusText;
    this.responseText = _lumenBinToUtf8(_lumenB64ToBin(res.body));
    this.response = this.responseText;
    this.readyState = 2;
    this._fire('readystatechange');
    this.readyState = 3;
    this._fire('readystatechange');
    this.readyState = 4;
    this._fire('readystatechange');
    this._fire('load');
    this._fire('loadend');
  };
  XMLHttpRequest.prototype.getAllResponseHeaders = function() { return ''; };
  XMLHttpRequest.prototype.getResponseHeader = function() { return null; };
  globalThis.XMLHttpRequest = XMLHttpRequest;
})();
"#;

// ─── WorkerOptions (shared by Worker and SharedWorker) ───────────────────────

/// IIFE defining `_lumen_parse_worker_options` / `_lumen_parse_shared_worker_options`
/// — the WebIDL conversion of the second constructor argument (BUG-777).
///
/// One source of truth for both flavours, evaluated by *both*
/// [`install_worker_bindings_v8`] and
/// `shared_worker.rs::install_shared_worker_bindings_v8` (it guards on its own
/// global, so the second evaluation is a no-op). Two separate copies would be
/// the BUG-780 trap one more time: `SHARED_WORKER_SHIM` is its own `rt.eval`,
/// so a fix made in `WORKER_SHIM` would never reach it — and the shared-worker
/// unit tests install their bindings *without* the dedicated-worker ones, so
/// referencing a helper the other shim happens to define would break there.
///
/// Both conversions must throw **before** the constructor touches the URL:
/// the binding layer converts arguments first, so
/// `new Worker(url, {type: 'unknown'})` is a `TypeError` even when the script
/// would have failed to fetch (`workers/modules/dedicated-worker-options-type.html`
/// asserts exactly that, twice).
#[cfg(feature = "v8-backend")]
pub(crate) const WORKER_OPTIONS_SHIM: &str = r#"(function() {
  if (typeof globalThis._lumen_parse_worker_options === 'function') return;

  // `enum WorkerType` (HTML LS §10.2.6.1) and `enum RequestCredentials`
  // (Fetch §5.4) — an out-of-list value is a TypeError, not a fallback to the
  // default, which is what makes the two negative subtests of
  // `dedicated-worker-options-type.html` observable at all.
  var _TYPES = ['classic', 'module'];
  var _CREDENTIALS = ['omit', 'same-origin', 'include'];

  function _enumOr(value, allowed, enumName) {
    var s = String(value);
    if (allowed.indexOf(s) === -1) {
      throw new TypeError(
        "Failed to construct 'Worker': The provided value '" + s +
        "' is not a valid enum value of type " + enumName + '.');
    }
    return s;
  }

  // WebIDL "convert an ECMAScript value to a dictionary": undefined/null give
  // the all-defaults dictionary, a non-object is a TypeError, and a member
  // explicitly set to `undefined` counts as absent (so it takes the default
  // rather than failing the enum check).
  globalThis._lumen_parse_worker_options = function(options) {
    var out = { type: 'classic', credentials: 'same-origin', name: '' };
    if (options === undefined || options === null) return out;
    if (typeof options !== 'object' && typeof options !== 'function') {
      throw new TypeError('WorkerOptions: the provided value is not an object.');
    }
    if (options.type !== undefined) out.type = _enumOr(options.type, _TYPES, 'WorkerType');
    if (options.credentials !== undefined) {
      out.credentials = _enumOr(options.credentials, _CREDENTIALS, 'RequestCredentials');
    }
    if (options.name !== undefined) out.name = String(options.name);
    return out;
  };

  // `SharedWorker(scriptURL, optional (DOMString or WorkerOptions) options)`:
  // the union sends null/undefined and every object to the dictionary, and
  // anything else (a string, a number, a boolean) to DOMString — i.e. the
  // legacy `new SharedWorker(url, 'my name')` spelling stays a name.
  globalThis._lumen_parse_shared_worker_options = function(options) {
    if (options !== undefined && options !== null &&
        typeof options !== 'object' && typeof options !== 'function') {
      return { type: 'classic', credentials: 'same-origin', name: String(options) };
    }
    return globalThis._lumen_parse_worker_options(options);
  };
})();
"#;

// ─── Worker JS class (evaluated in the main-thread JS context) ───────────────

/// IIFE that defines `globalThis.Worker` and `_lumen_deliver_worker_messages`.
///
/// Depends on:
/// - `_lumen_create_worker` / `_lumen_worker_post` / `_lumen_worker_terminate`
///   (native bindings installed by `install_worker_bindings_v8` above).
/// - `_lumen_register_worker_blob` (native binding installed above — mirrors
///   text blobs into `WorkerBlobStore` so `importScripts` can load them).
/// - `_object_url_store` (defined in WEB_API_SHIM for blob: URL resolution).
/// - `TextDecoder` (defined in WEB_API_SHIM for UTF-8 decoding of blob bytes).
/// - `atob` (defined in WEB_API_SHIM for data: URLs with base64 encoding).
#[cfg(feature = "v8-backend")]
const WORKER_SHIM: &str = r#"(function() {
  // Registry: worker id (u32) → Worker instance.
  var _workerRegistry = {};

  // ── importScripts blob mirroring ─────────────────────────────────────────────

  // Wrap URL.createObjectURL so that text/javascript and text/* blobs are also
  // registered in the Rust WorkerBlobStore.  Workers can then importScripts()
  // with the blob URL even though they run in a separate thread with no access
  // to the JS-side _object_url_store.
  if (typeof URL !== 'undefined' && typeof URL.createObjectURL === 'function') {
    var _origCreateObjectURL = URL.createObjectURL;
    URL.createObjectURL = function(blob) {
      var url = _origCreateObjectURL.call(URL, blob);
      if (blob && blob._bytes && blob.type) {
        var t = String(blob.type).toLowerCase().split(';')[0].trim();
        if (t === 'text/javascript' || t === 'application/javascript' ||
            t.startsWith('text/')) {
          try {
            var text = new TextDecoder().decode(blob._bytes);
            _lumen_register_worker_blob(url, text);
          } catch(e) {}
        }
      }
      return url;
    };
  }

  // ── Structured transfer helpers (Phase 1: OffscreenCanvas only) ─────────────

  // Sentinel marker embedded in JSON for transferred OffscreenCanvas objects.
  var _OFFSCREEN_SENTINEL = '__lumen_offscreen_transfer__';

  // Deep-walk `obj`, replacing any OffscreenCanvas found in `transferSet` with
  // a JSON-serializable sentinel that includes pixel data.
  function _serializeObj(obj, transferSet) {
    if (!obj || typeof obj !== 'object') return obj;
    if (typeof obj.__canvas_id__ === 'number' && transferSet[obj.__canvas_id__]) {
      // Read the pixel buffer, then neuter the source canvas — matches the
      // structured-clone transfer contract (the sender loses access).
      var raw = _lumen_offscreen_canvas2d_get_image_data(obj.__canvas_id__);
      if (!raw) return null;
      var comma1 = raw.indexOf(',');
      var comma2 = raw.indexOf(',', comma1 + 1);
      var w = parseInt(raw.slice(0, comma1), 10);
      var h = parseInt(raw.slice(comma1 + 1, comma2), 10);
      var p = raw.slice(comma2 + 1);
      _lumen_offscreen_canvas_bitmap_close(obj.__canvas_id__);
      return { __lumen_sentinel__: _OFFSCREEN_SENTINEL, w: w, h: h, p: p };
    }
    if (Array.isArray(obj)) {
      var arr = [];
      for (var i = 0; i < obj.length; i++) arr.push(_serializeObj(obj[i], transferSet));
      return arr;
    }
    var out = {};
    for (var k in obj) {
      if (Object.prototype.hasOwnProperty.call(obj, k)) {
        out[k] = _serializeObj(obj[k], transferSet);
      }
    }
    return out;
  }

  // Serialize `data` to JSON, replacing transferred OffscreenCanvas objects
  // with sentinels containing pixel data.
  function _lumenSerializeWithTransfers(data, transfer) {
    if (!transfer || !transfer.length) return JSON.stringify(data);
    var transferSet = {};
    for (var i = 0; i < transfer.length; i++) {
      var t = transfer[i];
      if (t && typeof t.__canvas_id__ === 'number') transferSet[t.__canvas_id__] = true;
    }
    if (!Object.keys(transferSet).length) return JSON.stringify(data);
    return JSON.stringify(_serializeObj(data, transferSet));
  }

  function Worker(url, options) {
    // WebIDL argument conversion runs before anything else the constructor
    // does (BUG-777) — an invalid `type`/`credentials` throws TypeError even
    // though the script URL below would have failed on its own.
    var opts = _lumen_parse_worker_options(options);
    var script;
    var u = String(url || '');
    // The worker scope's own script URL: `location.href` inside it (BUG-776),
    // and — via `worker_base_url`, Rust-side — the base a relative
    // importScripts()/fetch()/XHR target resolves against (BUG-778). A
    // blob:/data: worker keeps its opaque URL here; the base derived from it
    // is empty, as before.
    var scriptUrl = u;

    if (u.startsWith('blob:lumen/')) {
      // Blob URL created via URL.createObjectURL(blob).
      var blob = (typeof _object_url_store !== 'undefined') ? _object_url_store[u] : null;
      if (blob && blob._bytes) {
        // Decode UTF-8 bytes stored in the Blob.
        try {
          script = new TextDecoder().decode(blob._bytes);
        } catch(e) {
          script = '';
        }
      } else {
        script = '';
      }
    } else if (u.startsWith('data:')) {
      // data:[<mediatype>][;base64],<data>
      var comma = u.indexOf(',');
      if (comma !== -1) {
        var meta    = u.slice(5, comma);
        var content = u.slice(comma + 1);
        if (meta.indexOf('base64') !== -1) {
          try { script = atob(content); } catch(e) { script = ''; }
        } else {
          try { script = decodeURIComponent(content); } catch(e) { script = content; }
        }
      } else {
        script = '';
      }
    } else {
      // External URL: resolve against the document base and fetch the
      // script body synchronously (BUG-364 — previously never hit the
      // network at all). `null` here means the fetch failed (network error
      // or non-2xx status); `script` stays a String on success.
      var abs = _url_resolve(u, _lumen_document_base_url());
      var fetched = _lumen_worker_fetch_script(abs);
      script = (typeof fetched === 'string') ? fetched : null;
      scriptUrl = abs;
    }

    this._onmessage = null;
    this._onerror = null;
    this._listeners = [];
    this._errorListeners = [];

    if (script === null) {
      // HTML LS §10.2.6.1 "run a worker": when the classic script fetch
      // fails, queue a task to fire `error` at the worker and never start
      // it — `_id` stays null so postMessage/terminate become no-ops and
      // the worker is not registered for message delivery.
      this._id = null;
      var self = this;
      setTimeout(function() {
        var ev = new ErrorEvent('error', {
          message: 'Worker script failed to load: ' + u,
          filename: u, lineno: 0, colno: 0,
          bubbles: false, cancelable: true,
        });
        if (typeof self._onerror === 'function') { try { self._onerror(ev); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); } }
        for (var i = 0; i < self._errorListeners.length; i++) {
          try { self._errorListeners[i](ev); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); }
        }
      }, 0);
      return;
    }

    this._id = _lumen_create_worker(script, scriptUrl, opts.type === 'module');
    _workerRegistry[this._id] = this;
  }

  // postMessage(data[, transfer]) — send structured data to the worker thread.
  // When transfer contains OffscreenCanvas objects (identified by __canvas_id__),
  // their pixel buffers are serialized into the payload so the worker can
  // reconstruct them as OffscreenCanvas instances.
  // No-op when the worker never started (`_id === null`, BUG-364 script-fetch failure).
  Worker.prototype.postMessage = function(data, transfer) {
    if (this._id === null) return;
    _lumen_worker_post(this._id, _lumenSerializeWithTransfers(data, transfer));
  };

  // terminate() — immediately stop the worker; no more messages delivered.
  // No-op when the worker never started (`_id === null`).
  Worker.prototype.terminate = function() {
    if (this._id === null) return;
    _lumen_worker_terminate(this._id);
    delete _workerRegistry[this._id];
  };

  Object.defineProperty(Worker.prototype, 'onmessage', {
    get: function() { return this._onmessage; },
    set: function(fn) {
      this._onmessage = typeof fn === 'function' ? fn : null;
    },
    configurable: true,
  });

  Object.defineProperty(Worker.prototype, 'onerror', {
    get: function() { return this._onerror; },
    set: function(fn) {
      this._onerror = typeof fn === 'function' ? fn : null;
    },
    configurable: true,
  });

  Worker.prototype.addEventListener = function(type, fn, _opts) {
    if (type === 'message' && typeof fn === 'function') {
      this._listeners.push(fn);
    } else if (type === 'error' && typeof fn === 'function') {
      this._errorListeners.push(fn);
    }
  };

  Worker.prototype.removeEventListener = function(type, fn) {
    if (type === 'message') {
      var i = this._listeners.indexOf(fn);
      if (i !== -1) this._listeners.splice(i, 1);
    } else if (type === 'error') {
      var j = this._errorListeners.indexOf(fn);
      if (j !== -1) this._errorListeners.splice(j, 1);
    }
  };

  // Internal: deliver a message from the worker thread to this Worker instance.
  Worker.prototype._deliver = function(json) {
    var data;
    try { data = JSON.parse(json); } catch(e) { data = json; }
    var ev = { data: data, type: 'message', target: this,
               bubbles: false, cancelable: false };
    if (this._onmessage) { try { this._onmessage(ev); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); } }
    for (var i = 0; i < this._listeners.length; i++) {
      try { this._listeners[i](ev); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); }
    }
  };

  // Internal: deliver an uncaught-exception report from the worker thread
  // (BUG-591 worker parent-side reporting) — `info` is the
  // `{message, filename, lineno, colno}` object literal embedded by
  // `_lumen_deliver_worker_errors`. HTML LS "runtime script errors": fires
  // an `ErrorEvent` named `error` at the owning `Worker` object.
  Worker.prototype._deliverError = function(info) {
    var ev = new ErrorEvent('error', {
      message: String((info && info.message) || ''),
      filename: String((info && info.filename) || ''),
      lineno: (info && info.lineno) | 0,
      colno: (info && info.colno) | 0,
      bubbles: false, cancelable: true,
    });
    this._dispatchError(ev);
  };

  // Internal: run the `error` handlers of this `Worker` over an already-built
  // event. Split out of `_deliverError` for `dispatchEvent`, which hands in an
  // event the page constructed itself rather than one made from an `info`.
  Worker.prototype._dispatchError = function(ev) {
    ev.target = this; ev.currentTarget = this;
    if (typeof this._onerror === 'function') { try { this._onerror(ev); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); } }
    var listeners = this._errorListeners.slice();
    for (var i = 0; i < listeners.length; i++) {
      try { listeners[i].call(this, ev); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); }
    }
    return !ev.defaultPrevented;
  };

  // EventTarget.dispatchEvent on the `Worker` object itself (BUG-813).
  // `Worker` is an `AbstractWorker`, i.e. an `EventTarget`, so a page may
  // dispatch its own `error`/`message` at it — `Worker_dispatchEvent_
  // ErrorEvent.htm` does exactly that, with an event carrying a real `error`
  // object (which a report coming *from* the worker thread can never have,
  // since it crosses an agent boundary and arrives as JSON).
  Worker.prototype.dispatchEvent = function(ev) {
    if (!ev || typeof ev.type !== 'string') {
      throw new TypeError('dispatchEvent: argument is not an Event');
    }
    if (ev.type === 'error') return this._dispatchError(ev);
    if (ev.type === 'message') {
      ev.target = this; ev.currentTarget = this;
      if (typeof this._onmessage === 'function') { try { this._onmessage(ev); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); } }
      var listeners = this._listeners.slice();
      for (var i = 0; i < listeners.length; i++) {
        try { listeners[i].call(this, ev); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); }
      }
    }
    return !ev.defaultPrevented;
  };

  globalThis.Worker = Worker;
  // Also expose on the window snapshot created by WEB_API_SHIM.
  if (typeof window !== 'undefined') window.Worker = Worker;

  // Also expose the serialization helper for use in tests and advanced callers.
  globalThis._lumenSerializeWithTransfers = _lumenSerializeWithTransfers;

  // Called by QuickJsRuntime::pump_workers() with an array of
  // { id: u32, json: String } objects representing messages from worker threads.
  globalThis._lumen_deliver_worker_messages = function(msgs) {
    for (var i = 0; i < msgs.length; i++) {
      var m = msgs[i];
      var w = _workerRegistry[m.id];
      if (w) w._deliver(m.json);
    }
  };

  // Called by V8JsRuntime::pump_workers() with an array of
  // { id: u32, json: {message, filename, lineno, colno} } objects representing
  // uncaught-exception reports from worker threads (BUG-591).
  globalThis._lumen_deliver_worker_errors = function(errs) {
    for (var i = 0; i < errs.length; i++) {
      var m = errs[i];
      var w = _workerRegistry[m.id];
      if (w) w._deliverError(m.json);
    }
  };
})();
"#;

// ─── V8 backend port (Ph3 V8 migration S10; QuickJS twin removed S12b-B27) ───
//
// Each Worker thread gets its own dedicated `V8JsRuntime` (own OS thread +
// `v8::OwnedIsolate`, per the S1 threading model). `WorkerHandle`/
// `WorkerRegistry`/`WorkerMessageQueue`/`WorkerBlobStore`/`WorkerInMsg` and
// the `post_to_worker`/`terminate_worker`/`drain_messages` free functions
// above are engine-agnostic (plain channel/JSON plumbing) and are reused
// as-is. `WORKER_SHIM` (the main-thread `Worker` class) and
// `worker_global_shim` (the worker-thread global scope) are pure JS.

/// Install native bindings (`_lumen_create_worker`, `_lumen_worker_post`,
/// `_lumen_worker_terminate`, `_lumen_register_worker_blob`,
/// `_lumen_worker_fetch_script`) and the `Worker` JS class into `rt`.
///
/// Must be called after the core DOM shim so that `TextDecoder` and
/// `_object_url_store` are available for blob-URL resolution in the constructor.
#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_worker_bindings_v8(
    rt: &V8JsRuntime,
    registry: &WorkerRegistry,
    queue: &WorkerMessageQueue,
    errors: &WorkerErrorQueue,
    next_id: &Arc<Mutex<u32>>,
    blob_store: &WorkerBlobStore,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
) -> JsResult<()> {
    // _lumen_create_worker(script: String, script_url: String, is_module: bool) → u32
    //
    // `script_url` is the worker's own resolved script URL (the opaque URL
    // itself for a blob:/data: worker) — threaded down to the worker thread,
    // where it becomes both `location` (BUG-776) and, reduced by
    // [`worker_base_url`], the base its `importScripts()`/`fetch()`/
    // `XMLHttpRequest` resolve a relative or path-absolute target against the
    // way `.worker.html`'s wptrunner-built wrapper needs
    // (`importScripts("/resources/testharness.js")`, BUG-778).
    //
    // `is_module` is `WorkerOptions.type === 'module'` (BUG-777): it decides
    // whether the worker thread evaluates the script as a classic script or as
    // an ES module, and there is no way to derive it on this side — the option
    // exists nowhere but the constructor call.
    {
        let reg = Arc::clone(registry);
        let q = Arc::clone(queue);
        let errs = Arc::clone(errors);
        let nid = Arc::clone(next_id);
        let bs = Arc::clone(blob_store);
        let fp = fetch_provider.clone();
        rt.register_native(
            "_lumen_create_worker",
            into_v8_fn3(move |script: String, script_url: String, is_module: bool| -> u32 {
                spawn_worker_v8(
                    &reg, &q, &errs, &nid, &bs, script, script_url, is_module, fp.clone(),
                )
            }),
        )?;
    }

    // _lumen_worker_fetch_script(url: String) → String | undefined
    //
    // Synchronous GET for a classic (non-blob/data) worker script, backed by
    // the same `JsFetchProvider` bridge `fetch()`/`<script src>` use (BUG-364:
    // previously an external `new Worker(url)` never touched the network at
    // all and silently ran an empty comment). Returns `undefined` on any
    // network error or non-2xx status so the JS shim can fire `error` instead
    // of pretending the worker started.
    {
        let fp = fetch_provider.clone();
        rt.register_native(
            "_lumen_worker_fetch_script",
            into_v8_fn1(move |url: String| -> Option<String> {
                fetch_worker_script(fp.as_deref(), &url)
            }),
        )?;
    }

    // _lumen_worker_post(id: u32, json: String)
    {
        let reg = Arc::clone(registry);
        rt.register_native(
            "_lumen_worker_post",
            into_v8_fn2(move |id: u32, json: String| {
                post_to_worker(&reg, id, json);
            }),
        )?;
    }

    // _lumen_worker_terminate(id: u32)
    {
        let reg = Arc::clone(registry);
        rt.register_native(
            "_lumen_worker_terminate",
            into_v8_fn1(move |id: u32| {
                terminate_worker(&reg, id);
            }),
        )?;
    }

    // _lumen_register_worker_blob(url: String, text: String)
    {
        let bs = Arc::clone(blob_store);
        rt.register_native(
            "_lumen_register_worker_blob",
            into_v8_fn2(move |url: String, text: String| {
                bs.lock().unwrap().insert(url, text);
            }),
        )?;
    }

    rt.eval(WORKER_OPTIONS_SHIM)?;
    rt.eval(WORKER_SHIM)?;
    Ok(())
}

/// Fetch a classic worker script body over the network via `provider`.
///
/// Returns `None` when there is no provider, the request fails, or the
/// response status is not 2xx — the caller (`_lumen_worker_fetch_script`)
/// surfaces that as `undefined` to JS, which fires `error` on the `Worker`
/// instead of running an empty script.
pub(crate) fn fetch_worker_script(provider: Option<&dyn lumen_core::ext::JsFetchProvider>, url: &str) -> Option<String> {
    let resp = provider?.fetch_sync(url, "GET").ok()?;
    if !(200..300).contains(&resp.status) {
        return None;
    }
    Some(String::from_utf8_lossy(&resp.body).into_owned())
}

/// Perform one synchronous network request for a worker's `fetch()`/
/// `XMLHttpRequest` (BUG-778), returning
/// `{"status":…,"statusText":…,"headers":{…},"body":"<base64>"}` on success,
/// `None` on any network error or when `provider` is absent.
///
/// Shared by dedicated ([`install_worker_globals_v8`]) and shared
/// (`shared_worker.rs::install_shared_worker_globals_v8`) worker scopes —
/// mirrors `sw_worker.rs`'s `_lumen_sw_net_fetch`, but through the plain
/// `fetch_request`/`fetch_sync` path rather than `fetch_bypassing_sw`: a
/// dedicated/shared worker has no `FetchInterceptor` routing requests back
/// to itself the way a service worker's own scope does, so there is nothing
/// to bypass.
#[cfg(feature = "v8-backend")]
pub(crate) fn worker_net_fetch_json(
    provider: Option<&dyn lumen_core::ext::JsFetchProvider>,
    url: &str,
    method: &str,
    headers_flat: &[String],
    body_b64: Option<&str>,
) -> Option<String> {
    let provider = provider?;
    let headers: Vec<(String, String)> = headers_flat
        .chunks_exact(2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect();
    let body_bytes = body_b64.and_then(b64_decode);
    let req = lumen_core::ext::JsFetchRequest {
        url,
        method,
        headers: &headers,
        body: body_bytes.as_ref().map(|bytes| lumen_core::ext::JsFetchBody {
            content_type: "text/plain;charset=UTF-8",
            bytes,
        }),
        token: None,
    };
    let resp = match provider.fetch_request(&req) {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("[worker] fetch: {url}: {e}");
            return None;
        }
    };
    let headers: serde_json::Map<String, serde_json::Value> = resp
        .headers
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    serde_json::to_string(&serde_json::json!({
        "status": resp.status,
        "statusText": resp.status_text,
        "headers": headers,
        "body": b64_encode(&resp.body),
    }))
    .ok()
}

/// Spawn a new worker thread backed by its own [`V8JsRuntime`] that evaluates
/// `script` and waits for messages.
///
/// Returns the unique worker ID assigned to this instance. The caller stores
/// the ID in the JS `Worker` object and uses it for `postMessage`/`terminate`.
#[cfg(feature = "v8-backend")]
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
#[allow(clippy::too_many_arguments)]  // spawn-time state for the worker thread; BUG-778 added base_url/fetch_provider, BUG-777 is_module
fn spawn_worker_v8(
    registry: &WorkerRegistry,
    queue: &WorkerMessageQueue,
    errors: &WorkerErrorQueue,
    next_id: &Arc<Mutex<u32>>,
    blob_store: &WorkerBlobStore,
    script: String,
    script_url: String,
    is_module: bool,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
) -> u32 {
    let id = {
        let mut n = next_id.lock().unwrap();
        let id = *n;
        *n += 1;
        id
    };

    let (tx, rx) = mpsc::channel::<WorkerInMsg>();
    let reply = Arc::clone(queue);
    let err_reply = Arc::clone(errors);
    let store = Arc::clone(blob_store);

    let handle = thread::Builder::new()
        .name(format!("lumen-worker-v8-{id}"))
        .spawn(move || {
            run_worker_thread_v8(
                id, script, script_url, is_module, rx, reply, err_reply, store, fetch_provider,
            )
        })
        .expect("failed to spawn Web Worker thread (v8)");

    registry
        .lock()
        .unwrap()
        .insert(id, WorkerHandle { tx, _thread: handle });
    id
}

/// Worker thread body. Each worker owns a full [`V8JsRuntime`] (dedicated OS
/// thread + isolate) — there is no additional cross-thread dispatch needed,
/// so this outer thread just owns the runtime handle and pumps `WorkerInMsg`.
///
/// `OffscreenCanvas` is NOT installed here: this thread only calls
/// [`install_worker_globals_v8`], not the full `install_dom` install list
/// that wires `offscreen_canvas`'s V8 port
/// (`offscreen_canvas::install_offscreen_canvas_bindings_v8`, P1-imagebitmap)
/// for the main page context. A worker script that references
/// `OffscreenCanvas` sees `undefined`; `worker_global_shim`'s
/// `_deserializeTransfers` already guards on `typeof
/// _lumen_offscreen_canvas_from_image_data !== 'undefined'` and degrades to
/// passing the raw (un-deserialized) data through.
#[cfg(feature = "v8-backend")]
#[allow(clippy::too_many_arguments)]  // worker-thread setup, BUG-778 added base_url/fetch_provider
fn run_worker_thread_v8(
    id: u32,
    script: String,
    script_url: String,
    is_module: bool,
    rx: Receiver<WorkerInMsg>,
    reply: Arc<Mutex<Vec<(u32, String)>>>,
    errors: WorkerErrorQueue,
    blob_store: WorkerBlobStore,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
) {
    let rt = match V8JsRuntime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[worker-{id}] v8 runtime init failed: {e}");
            return;
        }
    };

    // BUG-778: flipped by `self.close()` — polled below after the initial
    // script eval and after every dispatched message, so the worker stops
    // servicing further tasks (HTML LS §10.2.3 "close a worker").
    let close_flag: WorkerCloseFlag = Arc::new(AtomicBool::new(false));

    // BUG-777: a module worker's own static/dynamic imports are fetched by the
    // ESM loader *on this thread*, so the same bridge the classic path hands to
    // `fetch()`/`importScripts()` has to reach `v8_esm`'s thread-local state too.
    let fp_esm = fetch_provider.clone();

    if let Err(e) = install_worker_globals_v8(
        &rt,
        id,
        Arc::clone(&reply),
        Arc::clone(&errors),
        Arc::clone(&blob_store),
        fetch_provider,
        &script_url,
        is_module,
        Arc::clone(&close_flag),
    ) {
        eprintln!("[worker-{id}] v8 globals install failed: {e:?}");
        return;
    }

    // A module worker evaluates its script under its own URL (BUG-777), so a
    // relative `import './dep.js'` resolves against the worker script rather
    // than against the page — the same reason `eval_module_at` exists for an
    // external `<script type=module src=…>`. The globals shims above are
    // classic scripts evaluated first, so everything they define on
    // `globalThis` is visible to the module body.
    // BUG-813: the top-level script goes through the reporting variants so an
    // uncaught exception fires `error` at the worker's own global scope (which
    // may have installed `onerror` earlier in the very same script) before it
    // is forwarded to the page — the forwarding itself is done from JS by
    // `_lumen_report_worker_exception`, not from here. Routing it through Rust
    // rather than wrapping the script in `try`/`catch` is what keeps the real
    // thrown value and V8's structured location, and leaves top-level `let`/
    // `const`/function declarations in the global scope where they belong.
    let outcome = if is_module {
        rt.set_module_context(&script_url, fp_esm);
        rt.eval_module_at_and_report_via(&script_url, &script, "_lumen_report_worker_exception")
    } else {
        rt.eval_and_report_via(&script, "_lumen_report_worker_exception")
            .map(|_| ())
    };
    if let Err(e) = outcome {
        eprintln!("[worker-{id}] v8 script error: {e:?}");
        // A module *load* failure (parse/link/import-not-found) never starts
        // the body, so it does not reach the JS reporter — HTML LS still wants
        // `error` at the `Worker` object for it, so post it from here. The flag
        // is the only thing that separates the two cases (BUG-591 worker
        // parent-side reporting: before it, this was `eprintln!` only, so an
        // uncaught top-level worker exception looked to the parent exactly like
        // a worker that never posts anything back).
        let reported = matches!(
            rt.eval("!!globalThis._lumen_worker_error_reported"),
            Ok(lumen_core::JsValue::Bool(true))
        );
        if !reported {
            let message = match &e {
                lumen_core::JsError::Parse(m) | lumen_core::JsError::Runtime(m) => m.clone(),
                lumen_core::JsError::NotImplemented => "not implemented".to_string(),
            };
            if let Ok(mut errs) = errors.lock() {
                errs.push((id, error_info_json(&message, "", 0, 0)));
            }
        }
        // Continue: worker may still receive messages if the error was partial.
    }

    // Task loop: continue for Post; Terminate, channel-close, or a
    // BUG-778 `self.close()` (checked both before the top-level script had a
    // chance to call it synchronously, and after every dispatched message —
    // "discard any tasks queued for the worker, and prevent any further
    // tasks from being queued", HTML LS §10.2.3) exits.
    //
    // BUG-815: the wait is bounded by the worker's own next timer deadline
    // instead of being an unconditional `recv()`, which is what makes a worker
    // nobody writes to run its timers at all. `run_worker_tasks` flushes
    // whatever is already due — including anything the top-level script armed,
    // before the first message ever arrives — and answers with how long the
    // thread may sleep.
    loop {
        if close_flag.load(Ordering::Relaxed) {
            break;
        }
        let wait = run_worker_tasks(&rt);
        if close_flag.load(Ordering::Relaxed) {
            break;
        }
        let msg = match wait {
            Some(d) => match rx.recv_timeout(d) {
                Ok(m) => m,
                // Deadline reached with nothing incoming — go round and let
                // `run_worker_tasks` fire the timer that woke us.
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(m) => m,
                Err(_) => break,
            },
        };
        let WorkerInMsg::Post(json) = msg else { break };
        if rt.set_global("_lw_msg__", lumen_core::JsValue::String(json)).is_ok() {
            let _ = rt.eval(
                "if(typeof _lumen_worker_dispatch_message==='function')\
                 {_lumen_worker_dispatch_message(JSON.parse(_lw_msg__));\
                  if(typeof _lumen_flush_timers==='function')_lumen_flush_timers();}",
            );
        }
    }
    // `rt` drops here: `V8JsRuntime::drop` sends `Shutdown` to its own JS
    // thread and joins it.
}

/// Run the worker scope's already-due timers and microtasks, and report how
/// long its thread may block before the next one (BUG-815).
///
/// `None` means "nothing is pending" — the caller may wait for an incoming
/// message indefinitely. Shared by both worker flavours' task loops
/// ([`run_worker_thread_v8`] and `shared_worker.rs::run_shared_worker_thread_v8`),
/// which is why it lives here rather than inside either of them.
///
/// A failed eval also answers `None`: the JS side catches every callback
/// exception itself ([`WORKER_TIMERS_SHIM`]), so a throw here would mean the
/// loop itself is broken, and blocking is a better answer than spinning on it.
/// The wait is capped at an hour so a timer armed 49 days out (which a page can
/// ask for) cannot turn into an absurd `Duration`.
#[cfg(feature = "v8-backend")]
pub(crate) fn run_worker_tasks(rt: &V8JsRuntime) -> Option<std::time::Duration> {
    // A negative answer (the shim's own "nothing pending", and the `-1` the
    // guard falls back to when the shim is absent) and a NaN alike fall
    // through to `None`.
    match rt.eval("(typeof _lumen_worker_run_tasks==='function')?_lumen_worker_run_tasks():-1") {
        Ok(lumen_core::JsValue::Number(ms)) if ms >= 0.0 => {
            Some(std::time::Duration::from_millis(ms.min(3_600_000.0) as u64))
        }
        _ => None,
    }
}

/// Install the Worker global environment into a V8 runtime. Registers the
/// natives `_lumen_worker_post_reply`, `_lumen_worker_console_log`,
/// `_lumen_import_scripts_resolve`, `_lumen_worker_self_close`,
/// `_lumen_worker_net_fetch` (BUG-778), `atob`, `btoa`, sets the
/// `_lumen_worker_base_url` (BUG-778) and `_lumen_worker_location_url`
/// (BUG-776) globals — both derived from `script_url`, the worker's own
/// resolved script URL — plus `_lumen_worker_is_module` (BUG-777, gates
/// `importScripts`) and evaluates [`worker_global_shim`] followed by
/// [`WORKER_TIMERS_SHIM`] and [`WORKER_NET_SHIM`].
///
/// `atob`/`btoa` go through [`crate::v8_compat::V8NativeFnScoped`] (raw scope
/// access) rather than the plain `into_v8_fnN` path, because they must throw
/// a JS exception on invalid input (WHATWG Infra §forgiving-base64) — the
/// generic compat layer's `IntoJsReturn` has no error/throw variant (same
/// reasoning as `wasm_compile_native_v8` in S9).
#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
#[allow(clippy::too_many_arguments)]  // per-flavour worker install, same shape as sw_worker.rs's twin
fn install_worker_globals_v8(
    rt: &V8JsRuntime,
    worker_id: u32,
    reply: Arc<Mutex<Vec<(u32, String)>>>,
    errors: WorkerErrorQueue,
    blob_store: WorkerBlobStore,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    script_url: &str,
    is_module: bool,
    close_flag: WorkerCloseFlag,
) -> JsResult<()> {
    rt.register_native(
        "_lumen_worker_post_reply",
        into_v8_fn1(move |json: String| {
            reply.lock().unwrap().push((worker_id, json));
        }),
    )?;

    // _lumen_worker_report_error(message, filename, lineno, colno) — called by
    // `worker_global_shim`'s `_lumen_report_worker_exception` for an uncaught
    // exception from a message/timer callback (BUG-591 worker parent-side
    // reporting: these used to be swallowed by a bare `catch(e){}`).
    {
        let errs = Arc::clone(&errors);
        rt.register_native(
            "_lumen_worker_report_error",
            into_v8_fn4(
                move |message: String, filename: String, lineno: i32, colno: i32| {
                    errs.lock()
                        .unwrap()
                        .push((worker_id, error_info_json(&message, &filename, lineno, colno)));
                },
            ),
        )?;
    }

    rt.register_native(
        "_lumen_worker_console_log",
        into_v8_fn1(move |msg: String| {
            eprintln!("[worker-{worker_id}] {msg}");
        }),
    )?;

    // _lumen_worker_self_close() — BUG-778 `self.close()`: flips the flag
    // `run_worker_thread_v8`'s message loop polls to stop servicing tasks.
    {
        let flag = Arc::clone(&close_flag);
        rt.register_native(
            "_lumen_worker_self_close",
            into_v8_fn0(move || {
                flag.store(true, Ordering::Relaxed);
            }),
        )?;
    }

    // _lumen_worker_net_fetch(url, method, headers_flat, body_b64) → String | undefined
    // BUG-778: backs `fetch()`/`XMLHttpRequest` inside the worker scope
    // (WORKER_NET_SHIM) over the same synchronous `JsFetchProvider` bridge
    // the classic-script fetch already uses — see [`worker_net_fetch_json`].
    {
        let fp = fetch_provider.clone();
        rt.register_native(
            "_lumen_worker_net_fetch",
            into_v8_fn4(
                move |url: String, method: String, headers_flat: Vec<String>, body_b64: Option<String>| -> Option<String> {
                    worker_net_fetch_json(fp.as_deref(), &url, &method, &headers_flat, body_b64.as_deref())
                },
            ),
        )?;
    }

    {
        let fp = fetch_provider;
        rt.register_native(
            "_lumen_import_scripts_resolve",
            into_v8_fn1(move |url: String| -> Option<String> {
                resolve_import_url(&url, &blob_store, fp.as_deref())
            }),
        )?;
    }

    rt.register_native_scoped("atob", Box::new(atob_native_v8))?;
    rt.register_native_scoped("btoa", Box::new(btoa_native_v8))?;

    // Before the dedicated-worker shim: it is what gives the scope `performance`
    // (BUG-401) and `EventTarget`, and the `performance` time origin is taken at
    // this point — the creation of this global scope, per HR Time L3 §4.2.
    install_worker_scope_globals_v8(rt)?;

    // BUG-778: read by both `worker_global_shim`'s `importScripts` and
    // `WORKER_NET_SHIM`'s `fetch`/`XMLHttpRequest` to resolve a relative
    // target — set before either is evaluated so it is never read as `undefined`.
    rt.set_global(
        "_lumen_worker_base_url",
        lumen_core::JsValue::String(worker_base_url(script_url).to_string()),
    )?;
    // BUG-776: read by `worker_global_shim` to build `location`. Unlike the
    // base URL this keeps the opaque form (`blob:`/`data:`), which is what
    // `location.href` must report for such a worker.
    rt.set_global(
        "_lumen_worker_location_url",
        lumen_core::JsValue::String(script_url.to_string()),
    )?;
    // BUG-777: read by `worker_global_shim`'s `importScripts`, which HTML LS
    // §10.2.3 requires to throw `TypeError` in a module worker (the module
    // graph is the only way such a scope may pull code in).
    rt.set_global(
        "_lumen_worker_is_module",
        lumen_core::JsValue::Bool(is_module),
    )?;

    rt.eval(&worker_global_shim(worker_id))?;
    rt.eval(WORKER_TIMERS_SHIM)?;   // BUG-815
    rt.eval(WORKER_NET_SHIM)?;
    Ok(())
}

/// `atob(str)` — V8 scoped native; throws a `TypeError` on invalid base64
/// input, matching the QuickJS `atob` native's `Err(rquickjs::Error::Exception)`.
#[cfg(feature = "v8-backend")]
fn atob_native_v8(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    rv: &mut v8::ReturnValue,
) {
    let encoded = args
        .get(0)
        .to_string(scope)
        .map(|s| s.to_rust_string_lossy(scope))
        .unwrap_or_default();
    match b64_decode(&encoded).and_then(|b| String::from_utf8(b).ok()) {
        Some(s) => {
            if let Some(v) = v8::String::new(scope, &s) {
                rv.set(v.into());
            }
        }
        None => throw_type_error(scope, "atob: invalid base64 input"),
    }
}

/// `btoa(str)` — V8 scoped native; throws a `TypeError` for characters
/// outside Latin-1 (U+00FF), matching the QuickJS `btoa` native.
#[cfg(feature = "v8-backend")]
fn btoa_native_v8(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    rv: &mut v8::ReturnValue,
) {
    let s = args
        .get(0)
        .to_string(scope)
        .map(|s| s.to_rust_string_lossy(scope))
        .unwrap_or_default();
    if s.chars().any(|c| c as u32 > 255) {
        throw_type_error(scope, "btoa: string contains characters outside Latin-1");
        return;
    }
    let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
    if let Some(v) = v8::String::new(scope, &b64_encode(&bytes)) {
        rv.set(v.into());
    }
}

/// Schedule a JS `TypeError` on `scope`. Mirrors `webassembly.rs`'s
/// same-named helper for the S9 scoped natives.
#[cfg(feature = "v8-backend")]
fn throw_type_error(scope: &mut v8::PinScope, msg: &str) {
    if let Some(s) = v8::String::new(scope, msg) {
        let exc = v8::Exception::type_error(scope, s);
        scope.throw_exception(exc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> WorkerBlobStore {
        Arc::new(Mutex::new(HashMap::new()))
    }

    // ── b64_decode ─────────────────────────────────────────────────────────────

    #[test]
    fn b64_decode_hello() {
        // base64("hello") = "aGVsbG8="
        assert_eq!(b64_decode("aGVsbG8=").unwrap(), b"hello");
    }

    #[test]
    fn b64_decode_roundtrip_via_btoa_atob() {
        // Verify our encoder and decoder agree.
        let input = "postMessage('hello');";
        // encode with btoa algorithm inline
        let encoded: String = {
            const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let bytes = input.as_bytes();
            let mut out = String::new();
            for chunk in bytes.chunks(3) {
                let b0 = chunk[0] as u32;
                let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
                let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
                let n = (b0 << 16) | (b1 << 8) | b2;
                out.push(CHARS[(n >> 18) as usize] as char);
                out.push(CHARS[((n >> 12) & 0x3F) as usize] as char);
                out.push(if chunk.len() > 1 { CHARS[((n >> 6) & 0x3F) as usize] as char } else { '=' });
                out.push(if chunk.len() > 2 { CHARS[(n & 0x3F) as usize] as char } else { '=' });
            }
            out
        };
        let decoded = b64_decode(&encoded).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), input);
    }

    #[test]
    fn b64_decode_invalid_returns_none() {
        assert!(b64_decode("!!!").is_none());
    }

    // ── percent_decode ─────────────────────────────────────────────────────────

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("postMessage%281%29"), "postMessage(1)");
    }

    // ── resolve_import_url ─────────────────────────────────────────────────────

    #[test]
    fn resolve_data_url_plain() {
        let store = make_store();
        let script = "postMessage(42);";
        let url = format!("data:text/javascript,{}", script);
        assert_eq!(resolve_import_url(&url, &store, None).unwrap(), script);
    }

    #[test]
    fn resolve_data_url_base64() {
        let store = make_store();
        // base64("postMessage('hi');") = cG9zdE1lc3NhZ2UoJ2hpJyk7
        let url = "data:text/javascript;base64,cG9zdE1lc3NhZ2UoJ2hpJyk7";
        assert_eq!(resolve_import_url(url, &store, None).unwrap(), "postMessage('hi');");
    }

    #[test]
    fn resolve_blob_url_from_store() {
        let store = make_store();
        store.lock().unwrap().insert("blob:lumen/42".to_string(), "var x = 1;".to_string());
        assert_eq!(resolve_import_url("blob:lumen/42", &store, None).unwrap(), "var x = 1;");
    }

    #[test]
    fn resolve_external_url_returns_none() {
        let store = make_store();
        assert!(resolve_import_url("https://example.com/lib.js", &store, None).is_none());
    }
}

/// V8-backend counterpart of the pure-Rust [`tests`] module above (Ph3 V8
/// migration S10; the rquickjs suite was removed in S12b-B27). Covers shim
/// install, `atob`/`btoa` (the only natives needing the scoped/throwing
/// mechanism), `importScripts` (data:/blob: URLs, multiple URLs, unknown
/// scheme throws), structured-clone transfer serialization, and end-to-end
/// `spawn_worker_v8` → postMessage round trips (including termination)
/// proving the whole per-worker `V8JsRuntime` thread actually runs.
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn make_store() -> WorkerBlobStore {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[test]
    fn v8_worker_shim_installs_without_error() {
        let rt = V8JsRuntime::new().unwrap();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let nid = Arc::new(Mutex::new(0u32));
        install_worker_bindings_v8(&rt, &reg, &queue, &errors, &nid, &make_store(), None).unwrap();
        let result = rt.eval("typeof Worker === 'function'").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn v8_worker_globals_have_atob_btoa() {
        let rt = V8JsRuntime::new().unwrap();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), Arc::clone(&errors), make_store(), None, "", false, Arc::new(AtomicBool::new(false))).unwrap();

        let decoded = rt.eval("atob('aGVsbG8=')").unwrap();
        assert_eq!(decoded, lumen_core::JsValue::String("hello".into()));
        let encoded = rt.eval("btoa('hello')").unwrap();
        assert_eq!(encoded, lumen_core::JsValue::String("aGVsbG8=".into()));
    }

    /// [BUG-591] `EventTarget.prototype.dispatchEvent`'s catch arms now call
    /// `_lumen_report_exception` for a throwing listener, but that native is
    /// only installed in the *page* shim (`WEB_API_SHIM_MID`), not in
    /// `worker_exposed_shim`. Guards that the `typeof` check added alongside
    /// it (`_lumen_et_report`) keeps a worker-scope `EventTarget` instance
    /// swallowing the exception instead of throwing a `ReferenceError` for
    /// the missing native — the pre-fix behaviour, just without the new gap.
    #[test]
    fn v8_worker_event_target_listener_exception_does_not_reference_error() {
        let rt = V8JsRuntime::new().unwrap();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), Arc::clone(&errors), make_store(), None, "", false, Arc::new(AtomicBool::new(false))).unwrap();
        let result = rt
            .eval(
                "var et = new EventTarget(); \
                 et.addEventListener('ping', function() { throw new Error('boom'); }); \
                 et.dispatchEvent({ type: 'ping' }); \
                 'survived'",
            )
            .unwrap();
        assert_eq!(
            result,
            lumen_core::JsValue::String("survived".to_string())
        );
    }

    #[test]
    fn v8_atob_throws_on_invalid_input() {
        let rt = V8JsRuntime::new().unwrap();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), Arc::clone(&errors), make_store(), None, "", false, Arc::new(AtomicBool::new(false))).unwrap();

        let ok = rt
            .eval("(function(){try{atob('!!!');return false;}catch(e){return e instanceof TypeError;}})()")
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    // ── BUG-813: "report the exception" inside a WorkerGlobalScope ───────────

    /// Install a dedicated-worker scope over a fresh runtime and hand back the
    /// error queue the parent-side `Worker` object would drain.
    fn scope_with_errors(script_url: &str) -> (V8JsRuntime, WorkerErrorQueue) {
        let rt = V8JsRuntime::new().unwrap();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(
            &rt,
            7,
            queue,
            Arc::clone(&errors),
            make_store(),
            None,
            script_url,
            false,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        (rt, errors)
    }

    /// [BUG-813] `WorkerGlobalScope.onerror` is an OnErrorEventHandler: it takes
    /// the five legacy arguments (message, filename, lineno, colno, error), not
    /// the event object. `workers/support/ErrorEvent.js` — the script behind
    /// nine vendored tests — reads all four of the first ones and posts them
    /// back, so passing an event here would report four `undefined`s and the
    /// family would fail rather than hang.
    #[test]
    fn v8_worker_scope_onerror_gets_the_legacy_five_arguments() {
        let (rt, errors) = scope_with_errors("http://example.test/w.js");
        let seen = rt
            .eval(
                "var seen = null; \
                 onerror = function(message, filename, lineno, colno, error) { \
                   seen = [message, filename, lineno, colno, error === _thrown, arguments.length]; \
                   return false; \
                 }; \
                 var _thrown = new Error('boom'); \
                 _lumen_report_worker_exception(_thrown, 'http://example.test/w.js', 3, 11); \
                 JSON.stringify(seen.slice(0, 4)) + '|' + seen[4] + '|' + seen[5]",
            )
            .unwrap();
        assert_eq!(
            seen,
            lumen_core::JsValue::String(
                "[\"boom\",\"http://example.test/w.js\",3,11]|true|5".to_string()
            )
        );
        // `return false` is "not handled", so the page still gets the report.
        let reported = drain_errors(&errors);
        assert_eq!(reported.len(), 1);
        assert!(reported[0].1.contains("\"message\":\"boom\""));
        assert!(reported[0].1.contains("\"lineno\":3"));
    }

    /// [BUG-813] The cancellation half of the same handler: returning `true`
    /// from an OnErrorEventHandler cancels the event, and a cancelled error
    /// must not be forwarded to the owning `Worker` object at all. Asserted
    /// separately from the argument shape because a handler that is called but
    /// whose return value is ignored passes the test above unchanged.
    #[test]
    fn v8_worker_scope_onerror_returning_true_stops_the_report_to_the_page() {
        let (rt, errors) = scope_with_errors("http://example.test/w.js");
        rt.eval(
            "onerror = function() { return true; }; \
             _lumen_report_worker_exception(new Error('boom'));",
        )
        .unwrap();
        assert!(drain_errors(&errors).is_empty());
    }

    /// [BUG-813] `addEventListener('error', …)` inside a worker — the second
    /// half of `workers/support/ErrorEvent-error.js`, which registers both
    /// forms. Unlike `onerror` a listener receives the `ErrorEvent` itself,
    /// including `error` (the thrown value, which crosses no agent boundary
    /// here and so is the real object, unlike the page-side `e.error === null`).
    #[test]
    fn v8_worker_scope_error_listener_receives_the_event_object() {
        let (rt, errors) = scope_with_errors("http://example.test/w.js");
        let seen = rt
            .eval(
                "var seen = ''; var thrown = new Error('boom'); \
                 addEventListener('error', function(e) { \
                   seen = [e.type, e.message, e.filename, e.lineno, e.error === thrown, \
                           e.bubbles, e.cancelable, \
                           Object.prototype.toString.call(e)].join('|'); \
                 }); \
                 _lumen_report_worker_exception(thrown, 'http://example.test/w.js', 3, 11); \
                 seen",
            )
            .unwrap();
        assert_eq!(
            seen,
            lumen_core::JsValue::String(
                "error|boom|http://example.test/w.js|3|true|false|true|[object ErrorEvent]"
                    .to_string()
            )
        );
        assert_eq!(drain_errors(&errors).len(), 1);
    }

    /// [BUG-813] A listener may cancel too — `preventDefault()` is what
    /// `WorkerGlobalScope_ErrorEvent_message.htm` relies on one level up, and
    /// the event has to be `cancelable` for it to take effect.
    #[test]
    fn v8_worker_scope_error_listener_prevent_default_stops_the_report() {
        let (rt, errors) = scope_with_errors("http://example.test/w.js");
        rt.eval(
            "var f = function(e) { e.preventDefault(); }; \
             addEventListener('error', f); \
             _lumen_report_worker_exception(new Error('boom'));",
        )
        .unwrap();
        assert!(drain_errors(&errors).is_empty());
        // …and removing that same listener puts the report back, so the list is
        // real rather than a one-shot "somebody once cancelled" flag.
        rt.eval(
            "removeEventListener('error', f); \
             _lumen_report_worker_exception(new Error('again'));",
        )
        .unwrap();
        assert_eq!(drain_errors(&errors).len(), 1);
    }

    /// [BUG-813] An exception thrown *by* an error handler must neither recurse
    /// (firing `error` again re-enters the same handler) nor be swallowed — the
    /// bare `catch(e) {}` shape BUG-591 swept out of the rest of the engine. It
    /// goes straight to the page instead, so the page sees two reports: the
    /// original (the handler did not cancel it) and the handler's own.
    #[test]
    fn v8_worker_scope_throwing_error_handler_does_not_recurse() {
        let (rt, errors) = scope_with_errors("http://example.test/w.js");
        let calls = rt
            .eval(
                "var calls = 0; \
                 onerror = function() { calls++; throw new Error('nested'); }; \
                 _lumen_report_worker_exception(new Error('boom')); \
                 calls",
            )
            .unwrap();
        assert_eq!(calls, lumen_core::JsValue::Number(1.0));
        let reported = drain_errors(&errors);
        assert_eq!(reported.len(), 2, "nested + original, neither swallowed");
        assert!(reported.iter().any(|(_, j)| j.contains("\"message\":\"nested\"")));
        assert!(reported.iter().any(|(_, j)| j.contains("\"message\":\"boom\"")));
    }

    /// [BUG-813] V8 compiles a worker's classic script with no resource name,
    /// so both `v8::Message` and `Error.stack` report `<anonymous>` — but
    /// `Worker_ErrorEvent_filename.htm` asserts the worker's absolute script
    /// URL. Every frame of such a stack belongs to that script, so the scope's
    /// own `location.href` is the substitution.
    #[test]
    fn v8_worker_scope_error_filename_falls_back_to_the_script_url() {
        let (rt, errors) = scope_with_errors("http://example.test/support/w.js");
        rt.eval("_lumen_report_worker_exception(new Error('boom'));")
            .unwrap();
        let reported = drain_errors(&errors);
        assert_eq!(reported.len(), 1);
        assert!(
            reported[0]
                .1
                .contains("\"filename\":\"http://example.test/support/w.js\""),
            "got {}",
            reported[0].1
        );
    }

    /// [BUG-813] End to end over a real worker thread: `support/ErrorEvent.js`
    /// throws from `onmessage`, its own `onerror` posts the four legacy
    /// arguments back and returns false, and the page's error queue gets the
    /// report anyway. This is the exact shape of the nine vendored
    /// `Worker_ErrorEvent_*`/`WorkerGlobalScope_ErrorEvent_*` tests.
    #[test]
    fn v8_worker_end_to_end_message_exception_runs_scope_onerror_then_reports() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        let script = "onmessage = function(e) { throw new Error(e.data); };\n\
                      onerror = function(message, location, line, col) {\n\
                        postMessage(message + '@' + location + ':' + line);\n\
                        return false;\n\
                      };\n"
            .to_string();
        let worker_id = spawn_worker_v8(
            &reg,
            &queue,
            &errors,
            &nid,
            &store,
            script,
            "http://example.test/support/ErrorEvent.js".to_string(),
            false,
            None,
        );

        post_to_worker(&reg, worker_id, "\"boom\"".to_string());
        std::thread::sleep(Duration::from_millis(400));

        let msgs = drain_messages(&queue);
        assert_eq!(
            msgs.iter().map(|(_, j)| j.as_str()).collect::<Vec<_>>(),
            vec!["\"boom@http://example.test/support/ErrorEvent.js:1\""]
        );
        let reported = drain_errors(&errors);
        assert_eq!(reported.len(), 1);
        assert!(reported[0].1.contains("\"message\":\"boom\""));

        terminate_worker(&reg, worker_id);
    }

    /// [BUG-813] The top-level script is evaluated by Rust, not by the shim, so
    /// its exception reaches the scope's own handlers only because
    /// `run_worker_thread_v8` routes it through `eval_and_report_via`
    /// (`support/ErrorEvent-error.js` registers both handler forms and then
    /// throws a bare string at top level). One report reaches the page too,
    /// exactly one — the fallback push in `run_worker_thread_v8` is for a
    /// module *load* failure and must stay quiet here.
    #[test]
    fn v8_worker_end_to_end_top_level_throw_runs_scope_handlers_once() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        let script = "onerror = function(message, location, line, col, error) {\n\
                        postMessage('onerror:' + error);\n\
                      };\n\
                      addEventListener('error', function(e) {\n\
                        postMessage('listener:' + e.error);\n\
                      });\n\
                      throw 'hello';\n"
            .to_string();
        let worker_id = spawn_worker_v8(
            &reg,
            &queue,
            &errors,
            &nid,
            &store,
            script,
            "http://example.test/support/ErrorEvent-error.js".to_string(),
            false,
            None,
        );
        std::thread::sleep(Duration::from_millis(400));

        let mut msgs: Vec<String> = drain_messages(&queue).into_iter().map(|(_, j)| j).collect();
        msgs.sort();
        assert_eq!(msgs, vec!["\"listener:hello\"", "\"onerror:hello\""]);
        assert_eq!(drain_errors(&errors).len(), 1);

        terminate_worker(&reg, worker_id);
    }

    /// BUG-401: `performance` was absent from the worker global scope entirely,
    /// so a worker script reading `performance.now()` aborted with a
    /// `ReferenceError` on its first line. What is asserted here is the
    /// prototype chain, not the presence of three method names: a hand-written
    /// literal with `now`/`timeOrigin` on it would satisfy the names and still
    /// be a different object from the page's `Performance` — exactly the shape
    /// defect BUG-400 had just removed from the page copy.
    #[test]
    fn v8_worker_globals_have_performance() {
        let rt = V8JsRuntime::new().unwrap();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), Arc::clone(&errors), make_store(), None, "", false, Arc::new(AtomicBool::new(false))).unwrap();

        for expr in [
            "typeof performance === 'object'",
            "self.performance === performance",
            "typeof performance.now === 'function'",
            "typeof performance.timeOrigin === 'number'",
            "performance instanceof Performance",
            "Object.getPrototypeOf(Performance.prototype) === EventTarget.prototype",
            "typeof performance.addEventListener === 'function'",
            // The operations live on the prototype, so the singleton has no own
            // enumerable properties and the WebIDL default toJSON stays honest.
            "Object.keys(performance).length === 0",
            "JSON.stringify(performance) === JSON.stringify({timeOrigin: performance.timeOrigin})",
        ] {
            assert_eq!(
                rt.eval(expr).unwrap(),
                lumen_core::JsValue::Bool(true),
                "{expr}"
            );
        }
    }

    /// The worker's time origin is its own scope-creation instant (HR Time L3
    /// §4.2), not zero and not the page's. Bracketing it with two wall-clock
    /// reads taken around the install is the only way to tell those apart from
    /// inside a single runtime.
    #[test]
    fn v8_worker_performance_time_origin_is_scope_creation() {
        let epoch_ms = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0)
        };
        let before = epoch_ms();
        let rt = V8JsRuntime::new().unwrap();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), Arc::clone(&errors), make_store(), None, "", false, Arc::new(AtomicBool::new(false))).unwrap();
        let after = epoch_ms();

        let origin = match rt.eval("performance.timeOrigin").unwrap() {
            lumen_core::JsValue::Number(n) => n,
            other => panic!("timeOrigin is not a number: {other:?}"),
        };
        assert!(
            origin >= before && origin <= after,
            "timeOrigin {origin} is outside the install window [{before}, {after}]"
        );

        // now() is measured from that origin, so it is a small offset, not an
        // epoch timestamp.
        let now = match rt.eval("performance.now()").unwrap() {
            lumen_core::JsValue::Number(n) => n,
            other => panic!("now() is not a number: {other:?}"),
        };
        assert!((0.0..60_000.0).contains(&now), "now() = {now}");
    }

    /// `PerformanceObserver` is part of the page shim only, so in a worker
    /// `_perf_observer_notify` does not exist. User Timing must still work —
    /// this is the test for the `typeof` guard the shared shim calls it
    /// through; without it `mark()` throws `ReferenceError` in a worker.
    #[test]
    fn v8_worker_user_timing_works_without_performance_observer() {
        let rt = V8JsRuntime::new().unwrap();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), Arc::clone(&errors), make_store(), None, "", false, Arc::new(AtomicBool::new(false))).unwrap();

        assert_eq!(
            rt.eval("typeof _perf_observer_notify").unwrap(),
            lumen_core::JsValue::String("undefined".into()),
        );
        let ok = rt
            .eval(
                "(function(){\
                   performance.mark('a'); performance.mark('b');\
                   var m = performance.measure('m', 'a', 'b');\
                   return m.entryType === 'measure' &&\
                          performance.getEntriesByType('mark').length === 2;\
                 })()",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    /// The reported failure end-to-end: a real spawned worker whose very first
    /// statement reads `performance.now()`. Before the fix the script died
    /// before `onmessage` was installed, so no reply ever came back — the
    /// TIMEOUT the three `hr-time` WPT files hit.
    #[test]
    fn v8_worker_end_to_end_performance_now() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        let script = "var t0 = performance.now();\
                      onmessage = function(e) {\
                        postMessage(performance.now() >= t0 && performance.timeOrigin > 0);\
                      };"
            .to_string();
        let worker_id = spawn_worker_v8(&reg, &queue, &errors, &nid, &store, script, String::new(), false, None);

        post_to_worker(&reg, worker_id, "0".to_string());
        std::thread::sleep(Duration::from_millis(300));

        let msgs = drain_messages(&queue);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].1, "true");

        terminate_worker(&reg, worker_id);
    }

    #[test]
    fn v8_worker_end_to_end_postmessage() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        // Worker echoes its received message doubled.
        let script = "onmessage = function(e) { postMessage(e.data * 2); };".to_string();
        let worker_id = spawn_worker_v8(&reg, &queue, &errors, &nid, &store, script, String::new(), false, None);

        post_to_worker(&reg, worker_id, "21".to_string());
        std::thread::sleep(Duration::from_millis(300));

        let msgs = drain_messages(&queue);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, worker_id);
        assert_eq!(msgs[0].1, "42");

        terminate_worker(&reg, worker_id);
    }

    #[test]
    fn v8_worker_import_scripts_via_data_url() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        // base64 of "function add(a,b){return a+b;}" =
        // ZnVuY3Rpb24gYWRkKGEsYil7cmV0dXJuIGErYjt9
        let script = concat!(
            "importScripts('data:text/javascript;base64,",
            "ZnVuY3Rpb24gYWRkKGEsYil7cmV0dXJuIGErYjt9",
            "');onmessage = function(e) { postMessage(add(e.data, 8)); };",
        )
        .to_string();
        let worker_id = spawn_worker_v8(&reg, &queue, &errors, &nid, &store, script, String::new(), false, None);

        post_to_worker(&reg, worker_id, "34".to_string());
        std::thread::sleep(Duration::from_millis(300));

        let msgs = drain_messages(&queue);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].1, "42");

        terminate_worker(&reg, worker_id);
    }

    #[test]
    fn v8_worker_import_scripts_via_blob_url() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        // Pre-populate the blob store as the main thread would via createObjectURL.
        let store = make_store();
        store.lock().unwrap().insert(
            "blob:lumen/helper".to_string(),
            "function mul(a,b){return a*b;}".to_string(),
        );

        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        let script =
            "importScripts('blob:lumen/helper');\
             onmessage = function(e) { postMessage(mul(e.data, 3)); };"
                .to_string();

        let worker_id = spawn_worker_v8(&reg, &queue, &errors, &nid, &store, script, String::new(), false, None);
        post_to_worker(&reg, worker_id, "7".to_string());
        std::thread::sleep(Duration::from_millis(300));

        let msgs = drain_messages(&queue);
        assert_eq!(msgs.len(), 1, "expected one reply");
        assert_eq!(msgs[0].1, "21");

        terminate_worker(&reg, worker_id);
    }

    #[test]
    fn v8_worker_terminate_stops_message_delivery() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        // Worker posts a reply to every message.
        let script = "onmessage = function(e) { postMessage('got:' + e.data); };".to_string();
        let worker_id = spawn_worker_v8(&reg, &queue, &errors, &nid, &store, script, String::new(), false, None);

        // Terminate immediately before any postMessage.
        terminate_worker(&reg, worker_id);
        std::thread::sleep(Duration::from_millis(50));

        // Any message sent after terminate is silently dropped (no handle in registry).
        post_to_worker(&reg, worker_id, "\"ping\"".to_string());
        std::thread::sleep(Duration::from_millis(50));

        let msgs = drain_messages(&queue);
        assert!(msgs.is_empty(), "terminated worker should produce no replies");
    }

    #[test]
    fn v8_import_scripts_multiple_urls() {
        let rt = V8JsRuntime::new().unwrap();
        let store = make_store();
        store.lock().unwrap().insert(
            "blob:lumen/1".to_string(),
            "globalThis._ms1 = 10;".to_string(),
        );
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), Arc::clone(&errors), store, None, "", false, Arc::new(AtomicBool::new(false))).unwrap();

        rt.eval(
            "importScripts(\
               'blob:lumen/1',\
               'data:text/javascript,globalThis._ms2 = 20;'\
             )"
        ).unwrap();
        let v1 = rt.eval("_ms1").unwrap();
        let v2 = rt.eval("_ms2").unwrap();
        assert_eq!(v1, lumen_core::JsValue::Number(10.0));
        assert_eq!(v2, lumen_core::JsValue::Number(20.0));
    }

    #[test]
    fn v8_import_scripts_unknown_url_throws() {
        let rt = V8JsRuntime::new().unwrap();
        let store = make_store();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), Arc::clone(&errors), store, None, "", false, Arc::new(AtomicBool::new(false))).unwrap();

        let result = rt.eval("importScripts('https://external.example/lib.js')");
        assert!(result.is_err(), "importScripts with http URL should throw");
    }

    #[test]
    fn v8_serialize_with_no_transfers_is_standard_json() {
        let rt = V8JsRuntime::new().unwrap();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let nid = Arc::new(Mutex::new(0u32));
        install_worker_bindings_v8(&rt, &reg, &queue, &errors, &nid, &make_store(), None).unwrap();

        let result = rt
            .eval(r#"_lumenSerializeWithTransfers({x: 1, y: "hello"}, [])"#)
            .unwrap();
        assert_eq!(
            result,
            lumen_core::JsValue::String(r#"{"x":1,"y":"hello"}"#.to_string())
        );
    }

    #[test]
    fn v8_serialize_with_offscreen_canvas_transfer_embeds_sentinel() {
        let rt = V8JsRuntime::new().unwrap();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let nid = Arc::new(Mutex::new(0u32));
        install_worker_bindings_v8(&rt, &reg, &queue, &errors, &nid, &make_store(), None).unwrap();
        crate::offscreen_canvas::install_offscreen_canvas_bindings_v8(&rt).unwrap();

        let result = rt
            .eval(
                r#"
                var oc = new OffscreenCanvas(2, 2);
                var ctx2d = oc.getContext('2d');
                ctx2d.fillStyle = '#ff0000';
                ctx2d.fillRect(0, 0, 2, 2);
                _lumenSerializeWithTransfers({canvas: oc}, [oc])
            "#,
            )
            .unwrap();
        let json_str = match result {
            lumen_core::JsValue::String(s) => s,
            other => panic!("expected string, got {other:?}"),
        };
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let sentinel = &v["canvas"]["__lumen_sentinel__"];
        assert_eq!(sentinel.as_str().unwrap(), "__lumen_offscreen_transfer__");
        assert_eq!(v["canvas"]["w"].as_u64().unwrap(), 2);
        assert_eq!(v["canvas"]["h"].as_u64().unwrap(), 2);
        assert!(!v["canvas"]["p"].as_str().unwrap().is_empty(), "pixel data should be present");
    }

    // ── BUG-778: close() / fetch() / XMLHttpRequest / importScripts(http) ──────

    /// Minimal `JsFetchProvider` double: answers by exact URL, 404 otherwise.
    /// Mirrors `sw_worker.rs::tests_v8::SwNet` but only needs `fetch_sync` —
    /// `worker_net_fetch_json` calls `fetch_request`, whose default impl
    /// dispatches a body-less, token-less request to `fetch_sync`.
    struct TestNet {
        bodies: HashMap<String, String>,
    }
    impl TestNet {
        fn new(bodies: &[(&str, &str)]) -> Arc<Self> {
            Arc::new(Self {
                bodies: bodies.iter().map(|(u, b)| ((*u).to_string(), (*b).to_string())).collect(),
            })
        }
    }
    impl lumen_core::ext::JsFetchProvider for TestNet {
        fn fetch_sync(&self, url: &str, _method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
            match self.bodies.get(url) {
                Some(body) => Ok(lumen_core::ext::JsFetchResult {
                    status: 200,
                    status_text: "OK".into(),
                    headers: vec![],
                    body: body.clone().into_bytes(),
                }),
                None => Ok(lumen_core::ext::JsFetchResult {
                    status: 404,
                    status_text: "Not Found".into(),
                    headers: vec![],
                    body: Vec::new(),
                }),
            }
        }
    }

    /// `self.close()` — HTML LS §10.2.3 "close a worker": a message sent
    /// after the worker closes itself gets no reply, even though the
    /// registry entry (and hence `postMessage`) is still nominally live.
    #[test]
    fn v8_worker_self_close_stops_further_messages() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        // First message replies then closes; a second message must produce
        // no further reply.
        let script = "onmessage = function(e) { postMessage('got:' + e.data); self.close(); };".to_string();
        let worker_id = spawn_worker_v8(&reg, &queue, &errors, &nid, &store, script, String::new(), false, None);

        post_to_worker(&reg, worker_id, "1".to_string());
        std::thread::sleep(Duration::from_millis(200));
        post_to_worker(&reg, worker_id, "2".to_string());
        std::thread::sleep(Duration::from_millis(200));

        let msgs = drain_messages(&queue);
        assert_eq!(msgs.len(), 1, "expected exactly one reply before close: {msgs:?}");
        assert_eq!(msgs[0].1, "\"got:1\"");
    }

    /// `typeof fetch/XMLHttpRequest === 'function'` inside a plain (no
    /// network provider) worker scope — the surface must exist even when the
    /// browser has nothing to fetch with (headless dump modes, BUG-778
    /// mirrors the page-level `fetch_provider = None` gotcha).
    #[test]
    fn v8_worker_globals_have_fetch_and_xhr() {
        let rt = V8JsRuntime::new().unwrap();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), Arc::clone(&errors), make_store(), None, "", false, Arc::new(AtomicBool::new(false))).unwrap();
        for expr in ["typeof fetch", "typeof XMLHttpRequest", "typeof close", "typeof Headers", "typeof Response"] {
            assert_eq!(rt.eval(expr).unwrap(), lumen_core::JsValue::String("function".into()), "{expr}");
        }
    }

    /// End-to-end `fetch()` inside a worker thread, real network bridge via
    /// [`TestNet`] — proves `_lumen_worker_net_fetch` reaches the provider
    /// and the JS `Response` wrapper decodes the body back to text.
    #[test]
    fn v8_worker_fetch_reaches_provider_and_decodes_body() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));
        let net = TestNet::new(&[("https://example.test/data", "hello from network")]);

        let script = "onmessage = function(e) {\
            fetch('https://example.test/data').then(function(r) { return r.text(); })\
              .then(function(t) { postMessage(t); });\
        };"
            .to_string();
        let worker_id = spawn_worker_v8(&reg, &queue, &errors, &nid, &store, script, String::new(), false, Some(net));

        post_to_worker(&reg, worker_id, "0".to_string());
        std::thread::sleep(Duration::from_millis(300));

        let msgs = drain_messages(&queue);
        assert_eq!(msgs.len(), 1, "expected one fetch()-derived reply: {msgs:?}");
        assert_eq!(msgs[0].1, "\"hello from network\"");

        terminate_worker(&reg, worker_id);
    }

    /// Synchronous `XMLHttpRequest` inside a worker thread over the same
    /// bridge, including the `readyState`/`status` surface a WPT
    /// `semantics/xhr/*` test reads.
    #[test]
    fn v8_worker_xhr_send_reaches_provider() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));
        let net = TestNet::new(&[("https://example.test/xhr", "xhr body")]);

        let script = "onmessage = function(e) {\
            var x = new XMLHttpRequest();\
            x.open('GET', 'https://example.test/xhr');\
            x.onload = function() { postMessage(x.status + ':' + x.responseText); };\
            x.send();\
        };"
            .to_string();
        let worker_id = spawn_worker_v8(&reg, &queue, &errors, &nid, &store, script, String::new(), false, Some(net));

        post_to_worker(&reg, worker_id, "0".to_string());
        std::thread::sleep(Duration::from_millis(300));

        let msgs = drain_messages(&queue);
        assert_eq!(msgs.len(), 1, "expected one XHR-derived reply: {msgs:?}");
        assert_eq!(msgs[0].1, "\"200:xhr body\"");

        terminate_worker(&reg, worker_id);
    }

    /// BUG-778's WPT-RUN-6 extension: `importScripts()` with a path-absolute
    /// URL, resolved against the worker's own `base_url` and fetched over
    /// the network — the exact shape wptrunner's `.worker.html` wrapper
    /// opens with (`importScripts("/resources/testharness.js")`).
    #[test]
    fn v8_import_scripts_path_absolute_resolves_against_base_url_and_fetches() {
        let rt = V8JsRuntime::new().unwrap();
        let store = make_store();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let net = TestNet::new(&[("https://example.test/resources/testharness.js", "globalThis._ms3 = 30;")]);
        install_worker_globals_v8(
            &rt, 0, Arc::clone(&queue), Arc::clone(&errors), store,
            Some(net), "https://example.test/worker.js", false, Arc::new(AtomicBool::new(false)),
        ).unwrap();

        rt.eval("importScripts('/resources/testharness.js')").unwrap();
        assert_eq!(rt.eval("_ms3").unwrap(), lumen_core::JsValue::Number(30.0));
    }

    /// BUG-776: the scope's own script URL, split into `WorkerLocation`
    /// members. The URL carries both a query and a fragment because
    /// `interfaces/WorkerGlobalScope/location/worker-separate-file.html`
    /// starts a worker at `post-location-members.js?a#b?c` and asserts both
    /// survive.
    #[test]
    fn v8_worker_location_reports_its_own_script_url() {
        let rt = V8JsRuntime::new().unwrap();
        install_worker_globals_v8(
            &rt, 0, Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())),
            make_store(), None, "https://example.test:8443/a/w.js?q=1#h?c", false,
            Arc::new(AtomicBool::new(false)),
        ).unwrap();

        for (expr, want) in [
            ("location.href", "https://example.test:8443/a/w.js?q=1#h?c"),
            ("location.origin", "https://example.test:8443"),
            ("location.protocol", "https:"),
            ("location.host", "example.test:8443"),
            ("location.hostname", "example.test"),
            ("location.port", "8443"),
            ("location.pathname", "/a/w.js"),
            ("location.search", "?q=1"),
            ("location.hash", "#h?c"),
            ("location.toString()", "https://example.test:8443/a/w.js?q=1#h?c"),
        ] {
            assert_eq!(
                rt.eval(expr).unwrap(),
                lumen_core::JsValue::String(want.to_string()),
                "{expr}"
            );
        }
        // `returns-same-object.any.js` — one object, not a fresh one per read.
        assert_eq!(rt.eval("location === location").unwrap(), lumen_core::JsValue::Bool(true));
        assert_eq!(
            rt.eval("location instanceof WorkerLocation").unwrap(),
            lumen_core::JsValue::Bool(true)
        );
    }

    /// BUG-776: `[LegacyUnforgeable] readonly attribute` in a non-strict
    /// script — `setting-members.html` asserts an empty exception list *and*
    /// unchanged values, so the assignment must be silently dropped rather
    /// than either throwing or sticking.
    #[test]
    fn v8_worker_location_members_are_silently_read_only() {
        let rt = V8JsRuntime::new().unwrap();
        install_worker_globals_v8(
            &rt, 0, Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())),
            make_store(), None, "https://example.test/w.js", false, Arc::new(AtomicBool::new(false)),
        ).unwrap();

        let thrown = rt
            .eval(
                "(function() { var n = 0;\
                   ['href','protocol','host','hostname','port','pathname','search','hash']\
                     .forEach(function(k) { try { location[k] = 1; } catch (e) { n++; } });\
                   return n; })()",
            )
            .unwrap();
        assert_eq!(thrown, lumen_core::JsValue::Number(0.0), "assignment must not throw");
        assert_eq!(
            rt.eval("location.href").unwrap(),
            lumen_core::JsValue::String("https://example.test/w.js".to_string())
        );
    }

    /// BUG-776: `navigator` exists and answers the values the page's
    /// `Navigator` answers — the whole `NavigatorID` mixin point, and what
    /// `interfaces/WorkerUtils/navigator/*` compares against `window`.
    #[test]
    fn v8_worker_navigator_answers_the_shared_navigator_id_values() {
        let rt = V8JsRuntime::new().unwrap();
        install_worker_globals_v8(
            &rt, 0, Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())),
            make_store(), None, "https://example.test/w.js", false, Arc::new(AtomicBool::new(false)),
        ).unwrap();

        assert_eq!(
            rt.eval("navigator instanceof WorkerNavigator").unwrap(),
            lumen_core::JsValue::Bool(true)
        );
        assert_eq!(
            rt.eval("navigator.appName").unwrap(),
            lumen_core::JsValue::String("Netscape".to_string())
        );
        assert_eq!(
            rt.eval("navigator.appCodeName").unwrap(),
            lumen_core::JsValue::String("Mozilla".to_string())
        );
        assert_eq!(
            rt.eval("navigator.product").unwrap(),
            lumen_core::JsValue::String("Gecko".to_string())
        );
        // Every member must come from the one shared source, so a bump of any
        // of them cannot leave the worker behind.
        assert_eq!(
            rt.eval(
                "Object.keys(_lumen_navigator_id)\
                   .every(function(k) { return navigator[k] === _lumen_navigator_id[k]; })"
            )
            .unwrap(),
            lumen_core::JsValue::Bool(true)
        );
    }

    /// BUG-776/778: an opaque worker URL is `location` verbatim, while the
    /// relative-resolution base derived from it stays empty — nothing can be
    /// resolved against a `data:` URL, and `importScripts` guards on exactly
    /// that emptiness.
    #[test]
    fn v8_data_url_worker_keeps_its_href_but_has_no_base() {
        assert_eq!(worker_base_url("data:text/javascript,1"), "");
        assert_eq!(worker_base_url("blob:lumen/7"), "");
        assert_eq!(worker_base_url("https://example.test/w.js"), "https://example.test/w.js");

        let rt = V8JsRuntime::new().unwrap();
        install_worker_globals_v8(
            &rt, 0, Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())),
            make_store(), None, "data:text/javascript,1", false, Arc::new(AtomicBool::new(false)),
        ).unwrap();

        assert_eq!(
            rt.eval("location.href").unwrap(),
            lumen_core::JsValue::String("data:text/javascript,1".to_string())
        );
        assert_eq!(
            rt.eval("location.protocol").unwrap(),
            lumen_core::JsValue::String("data:".to_string())
        );
        assert_eq!(
            rt.eval("_lumen_worker_base_url").unwrap(),
            lumen_core::JsValue::String(String::new())
        );
    }

    // ── BUG-777: WorkerOptions + module workers ──────────────────────────────

    /// The `WorkerType` enum is a hard gate, not a fallback: both negative
    /// subtests of `workers/modules/dedicated-worker-options-type.html` assert
    /// a *synchronous* `TypeError`, and before BUG-777 the whole second
    /// argument was dropped, so both constructions quietly succeeded.
    #[test]
    fn v8_worker_options_reject_an_invalid_type() {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(WORKER_OPTIONS_SHIM).unwrap();

        for bad in ["''", "'unknown'", "'Module'"] {
            let expr = format!(
                "(function(){{try{{_lumen_parse_worker_options({{type:{bad}}});return 'no-throw';}}\
                 catch(e){{return e instanceof TypeError ? 'TypeError' : String(e);}}}})()"
            );
            assert_eq!(
                rt.eval(&expr).unwrap(),
                lumen_core::JsValue::String("TypeError".to_string()),
                "type {bad} must be rejected"
            );
        }
        // A non-object second argument is a dictionary-conversion TypeError too.
        assert_eq!(
            rt.eval(
                "(function(){try{_lumen_parse_worker_options(7);return 'no-throw';}\
                 catch(e){return e instanceof TypeError ? 'TypeError' : String(e);}})()"
            )
            .unwrap(),
            lumen_core::JsValue::String("TypeError".to_string())
        );
    }

    /// The positive half of the same dictionary conversion, including the
    /// WebIDL rule that an explicitly-`undefined` member counts as absent
    /// (otherwise `{type: undefined}` would fail the enum check instead of
    /// taking the default).
    #[test]
    fn v8_worker_options_defaults_and_accepted_values() {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(WORKER_OPTIONS_SHIM).unwrap();

        for (arg, want) in [
            ("undefined", "classic|same-origin|"),
            ("null", "classic|same-origin|"),
            ("{}", "classic|same-origin|"),
            ("{type: undefined}", "classic|same-origin|"),
            ("{type: 'module'}", "module|same-origin|"),
            ("{type: 'classic', credentials: 'omit', name: 'n'}", "classic|omit|n"),
        ] {
            let expr = format!(
                "(function(){{var o=_lumen_parse_worker_options({arg});\
                 return o.type+'|'+o.credentials+'|'+o.name;}})()"
            );
            assert_eq!(
                rt.eval(&expr).unwrap(),
                lumen_core::JsValue::String(want.to_string()),
                "options {arg}"
            );
        }
    }

    /// `SharedWorker`'s union argument: a string stays the legacy name, an
    /// object is a dictionary. The dictionary spelling is what used to collapse
    /// to the literal name `[object Object]`.
    #[test]
    fn v8_shared_worker_options_union_splits_string_from_dictionary() {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(WORKER_OPTIONS_SHIM).unwrap();

        for (arg, want) in [
            ("'my name'", "classic|my name"),
            ("{name: 'my name'}", "classic|my name"),
            ("{name: 'my name', type: 'module'}", "module|my name"),
            ("undefined", "classic|"),
        ] {
            let expr = format!(
                "(function(){{var o=_lumen_parse_shared_worker_options({arg});\
                 return o.type+'|'+o.name;}})()"
            );
            assert_eq!(
                rt.eval(&expr).unwrap(),
                lumen_core::JsValue::String(want.to_string()),
                "options {arg}"
            );
        }
    }

    /// End-to-end module worker: the script body uses `import`, which a classic
    /// script cannot parse at all, and the imported binding must reach the
    /// message handler. The dependency is served by the fetch double, i.e. the
    /// ESM loader really runs on the worker thread over the same bridge.
    #[test]
    fn v8_module_worker_evaluates_static_import() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));
        let net = TestNet::new(&[("https://example.test/dep.js", "export const answer = 42;")]);

        let script = "import { answer } from './dep.js';\n\
             self.onmessage = function(e) { postMessage(answer); };"
            .to_string();
        let worker_id = spawn_worker_v8(
            &reg,
            &queue,
            &errors,
            &nid,
            &store,
            script,
            "https://example.test/w.js".to_string(),
            true,
            Some(net),
        );

        post_to_worker(&reg, worker_id, "0".to_string());
        std::thread::sleep(Duration::from_millis(500));

        let msgs = drain_messages(&queue);
        assert_eq!(
            msgs.len(),
            1,
            "module worker never replied: errors={:?}",
            drain_errors(&errors)
        );
        assert_eq!(msgs[0].1, "42");

        terminate_worker(&reg, worker_id);
    }

    /// The same script run as a *classic* worker must fail — otherwise the
    /// test above would pass with the option ignored, which is exactly the
    /// state BUG-777 describes (a green test masking a missing feature).
    #[test]
    fn v8_classic_worker_still_rejects_an_import_statement() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        let script = "import { answer } from './dep.js';".to_string();
        let worker_id = spawn_worker_v8(
            &reg,
            &queue,
            &errors,
            &nid,
            &store,
            script,
            "https://example.test/w.js".to_string(),
            false,
            None,
        );
        std::thread::sleep(Duration::from_millis(300));

        let errs = drain_errors(&errors);
        assert_eq!(errs.len(), 1, "classic worker should report a parse error: {errs:?}");
        assert!(
            errs[0].1.contains("import statement"),
            "unexpected error text: {}",
            errs[0].1
        );

        terminate_worker(&reg, worker_id);
    }

    /// `importScripts()` is unavailable in a module worker (HTML LS §10.2.3),
    /// and `workers/modules/dedicated-worker-import-failure.html` asserts the
    /// exception is specifically a `TypeError`.
    #[test]
    fn v8_module_worker_import_scripts_throws_type_error() {
        let rt = V8JsRuntime::new().unwrap();
        install_worker_globals_v8(
            &rt,
            0,
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
            make_store(),
            None,
            "https://example.test/w.js",
            true,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();

        assert_eq!(
            rt.eval(
                "(function(){try{importScripts('data:text/javascript,1');return 'no-throw';}\
                 catch(e){return e.constructor.name;}})()"
            )
            .unwrap(),
            lumen_core::JsValue::String("TypeError".to_string())
        );
    }

    /// The scope's own interface object: WPT worker resources branch on
    /// `'DedicatedWorkerGlobalScope' in self && self instanceof
    /// DedicatedWorkerGlobalScope`, so both halves have to answer, and the
    /// scope must not claim to be one of the other two flavours.
    #[test]
    fn v8_worker_scope_exposes_its_own_global_scope_interface() {
        let rt = V8JsRuntime::new().unwrap();
        install_worker_globals_v8(
            &rt,
            0,
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
            make_store(),
            None,
            "https://example.test/w.js",
            false,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();

        for (expr, want) in [
            ("'DedicatedWorkerGlobalScope' in self", true),
            ("self instanceof DedicatedWorkerGlobalScope", true),
            ("self instanceof WorkerGlobalScope", true),
            ("'SharedWorkerGlobalScope' in self", false),
        ] {
            assert_eq!(rt.eval(expr).unwrap(), lumen_core::JsValue::Bool(want), "{expr}");
        }
        // Own globals still shadow the fresh prototype chain.
        assert_eq!(
            rt.eval("typeof postMessage").unwrap(),
            lumen_core::JsValue::String("function".to_string())
        );
    }

    // ── BUG-815: worker timers ────────────────────────────────────────────────

    /// Turn the worker's task loop exactly the way [`run_worker_thread_v8`]
    /// does — flush what is due, then sleep for as long as the scope asked —
    /// until `done` evaluates true. Returns false if the scope ran out of
    /// pending work, or the budget ran out, without that happening.
    ///
    /// Nothing here delivers a message: that is the whole point of the bug —
    /// before the fix these timers only ran when the page wrote to the worker.
    fn pump_until(rt: &V8JsRuntime, done: &str, budget: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + budget;
        loop {
            let wait = run_worker_tasks(rt);
            if matches!(rt.eval(done), Ok(lumen_core::JsValue::Bool(true))) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            match wait {
                // Capped so a mis-scheduled timer fails the test on the budget
                // rather than parking the whole suite on one `sleep`.
                Some(d) => std::thread::sleep(d.min(std::time::Duration::from_millis(20))),
                None => return false,
            }
        }
    }

    /// [BUG-815] The headline symptom: a worker nobody writes to never ran a
    /// timer at all, because the only `_lumen_flush_timers` call site was the
    /// message-delivery loop. `workers/WorkerGlobalScope_setTimeout.htm` is
    /// exactly this — the page arms nothing and waits for the worker to post.
    #[test]
    fn v8_worker_timer_fires_with_no_incoming_message() {
        let (rt, _errors) = scope_with_errors("http://example.test/w.js");
        rt.eval("var fired = false; setTimeout(function() { fired = true; }, 10);")
            .unwrap();
        assert!(pump_until(&rt, "fired", std::time::Duration::from_secs(5)));
    }

    /// [BUG-815] The delay was ignored entirely (the old stub's parameter was
    /// literally named `_delay`), so callbacks ran in arm order. The order
    /// asserted here is what `Worker-timeout-increasing-order.html` checks.
    #[test]
    fn v8_worker_timers_run_in_deadline_order_not_arm_order() {
        let (rt, _errors) = scope_with_errors("http://example.test/w.js");
        rt.eval(
            "var log = []; \
             setTimeout(function() { log.push('a'); }, 60); \
             setTimeout(function() { log.push('b'); }, 10); \
             setTimeout(function() { log.push('c'); }, 35);",
        )
        .unwrap();
        assert!(pump_until(
            &rt,
            "log.length === 3",
            std::time::Duration::from_secs(5)
        ));
        assert_eq!(
            rt.eval("log.join('')").unwrap(),
            lumen_core::JsValue::String("bca".to_string())
        );
    }

    /// [BUG-815] `setInterval` was a bare alias of `setTimeout`, so it fired
    /// once and stopped — the measured `interval:1` with no `interval:2`.
    #[test]
    fn v8_worker_set_interval_repeats() {
        let (rt, _errors) = scope_with_errors("http://example.test/w.js");
        rt.eval("var n = 0; var h = setInterval(function() { n++; }, 5);")
            .unwrap();
        assert!(pump_until(&rt, "n >= 3", std::time::Duration::from_secs(5)));
    }

    /// [BUG-815] …and `clearInterval` has to reach the same timer: with the
    /// two functions aliased there was only ever one shot to cancel.
    #[test]
    fn v8_worker_clear_interval_stops_the_repeat() {
        let (rt, _errors) = scope_with_errors("http://example.test/w.js");
        rt.eval(
            "var n = 0; \
             var h = setInterval(function() { n++; if (n === 2) clearInterval(h); }, 5);",
        )
        .unwrap();
        assert!(pump_until(&rt, "n === 2", std::time::Duration::from_secs(5)));
        // Nothing is left pending, so the thread would go back to a plain
        // blocking `recv()` — which is the observable form of "it stopped".
        assert!(run_worker_tasks(&rt).is_none());
    }

    /// [BUG-815] The value the Rust loop actually steers by: `None` is "block
    /// on the channel", `Some(d)` is "wake in `d`". Getting this wrong either
    /// spins the thread or loses every timer, and neither shows up in the
    /// callback-level tests above.
    #[test]
    fn v8_worker_run_tasks_reports_the_wait_the_loop_should_use() {
        let (rt, _errors) = scope_with_errors("http://example.test/w.js");
        assert!(run_worker_tasks(&rt).is_none());
        rt.eval("setTimeout(function() {}, 10000);").unwrap();
        let wait = run_worker_tasks(&rt).expect("a pending timer must bound the wait");
        assert!(
            wait > std::time::Duration::from_millis(5_000),
            "expected ≈10 s, got {wait:?}"
        );
    }

    /// [BUG-815] `setTimeout(fn, delay, …args)` — the trailing arguments were
    /// dropped by the old stub, which stored `{id, fn}` and nothing else.
    #[test]
    fn v8_worker_set_timeout_passes_extra_arguments() {
        let (rt, _errors) = scope_with_errors("http://example.test/w.js");
        rt.eval("var got = null; setTimeout(function(a, b) { got = a + b; }, 0, 'x', 'y');")
            .unwrap();
        assert!(pump_until(
            &rt,
            "got !== null",
            std::time::Duration::from_secs(5)
        ));
        assert_eq!(
            rt.eval("got").unwrap(),
            lumen_core::JsValue::String("xy".to_string())
        );
    }

    /// [BUG-815] The old stub put a microtask at the *front of the timer
    /// queue*, which happens to order these two right but makes a microtask
    /// queued from a timer callback wait for the next flush. Both halves are
    /// asserted here: the ordering, and that a microtask queued from inside a
    /// callback still runs within the same turn.
    #[test]
    fn v8_worker_microtasks_run_before_and_between_due_timers() {
        let (rt, _errors) = scope_with_errors("http://example.test/w.js");
        rt.eval(
            "var log = []; \
             setTimeout(function() { \
               log.push('t'); queueMicrotask(function() { log.push('m2'); }); \
             }, 0); \
             queueMicrotask(function() { log.push('m1'); });",
        )
        .unwrap();
        assert!(pump_until(
            &rt,
            "log.length === 3",
            std::time::Duration::from_secs(5)
        ));
        assert_eq!(
            rt.eval("log.join('')").unwrap(),
            lumen_core::JsValue::String("m1tm2".to_string())
        );
    }

    /// [BUG-815] The §8.6 nesting clamp is not decoration here — it is the
    /// only thing keeping `setInterval(fn, 0)` from re-arming itself as
    /// already-due forever inside one flush. Asserting the *wait* rather than
    /// a tick count is what proves the loop yields: an unclamped build never
    /// returns from `run_worker_tasks` at all.
    #[test]
    fn v8_worker_zero_delay_interval_is_clamped_and_yields() {
        let (rt, _errors) = scope_with_errors("http://example.test/w.js");
        rt.eval("var n = 0; setInterval(function() { n++; }, 0);").unwrap();
        let wait = run_worker_tasks(&rt).expect("a live interval must bound the wait");
        assert_eq!(wait, std::time::Duration::from_millis(4), "§8.6 clamp");
        // The five pre-clamp cycles ran inside that one flush.
        assert_eq!(rt.eval("n").unwrap(), lumen_core::JsValue::Number(5.0));
    }

    /// [BUG-815] The whole thing through a real worker thread: the page sends
    /// **nothing**, and the worker's own `setTimeout`/`setInterval` still post
    /// back. Every other timer test here drives [`run_worker_tasks`] by hand,
    /// which cannot catch the half of the bug that lived in the thread's own
    /// `rx.recv()` — an unconditional block that no deadline could interrupt.
    /// This is `verify_csp_url_worker_gaps.py --variant worker-timers` in the
    /// small: `interval:2` is the entry that never used to arrive.
    #[test]
    fn v8_worker_end_to_end_timers_run_without_the_page_writing() {
        use std::time::{Duration, Instant};
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        let script = "queueMicrotask(function() { postMessage('microtask'); }); \
                      setTimeout(function() { postMessage('timeout'); }, 10); \
                      var n = 0; \
                      setInterval(function() { n++; postMessage('interval:' + n); }, 20);"
            .to_string();
        let worker_id =
            spawn_worker_v8(&reg, &queue, &errors, &nid, &store, script, String::new(), false, None);

        let mut got: Vec<String> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && !got.iter().any(|m| m == "\"interval:2\"") {
            std::thread::sleep(Duration::from_millis(20));
            got.extend(drain_messages(&queue).into_iter().map(|(_, m)| m));
        }
        terminate_worker(&reg, worker_id);

        // Order matters as much as arrival: the microtask before the 10 ms
        // timeout, and that before the first 20 ms interval tick.
        let head: Vec<&str> = got.iter().take(4).map(String::as_str).collect();
        assert_eq!(
            head,
            ["\"microtask\"", "\"timeout\"", "\"interval:1\"", "\"interval:2\""],
            "got {got:?}"
        );
    }

    /// [BUG-815] An exception from a timer callback still reaches the page —
    /// the queue rewrite must not undo BUG-813's reporting path, and the loop
    /// must carry on with the remaining timers.
    #[test]
    fn v8_worker_timer_exception_is_reported_and_the_loop_continues() {
        let (rt, errors) = scope_with_errors("http://example.test/w.js");
        rt.eval(
            "var after = false; \
             setTimeout(function() { throw new Error('boom'); }, 0); \
             setTimeout(function() { after = true; }, 0);",
        )
        .unwrap();
        assert!(pump_until(&rt, "after", std::time::Duration::from_secs(5)));
        let reported = drain_errors(&errors);
        assert_eq!(reported.len(), 1);
        assert!(reported[0].1.contains("\"message\":\"boom\""));
    }
}
