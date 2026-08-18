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
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

#[cfg(feature = "v8-backend")]
use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2};
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

// ─── public API ───────────────────────────────────────────────────────────────

/// Send a JSON-serialized message to a live worker thread.
///
/// No-op if `id` is not registered (e.g. worker already terminated).
pub fn post_to_worker(registry: &WorkerRegistry, id: u32, json: String) {
    if let Some(h) = registry.lock().unwrap().get(&id) {
        let _ = h.tx.send(WorkerInMsg::Post(json));
    }
}

/// Terminate a worker and remove it from the registry.
///
/// Sends a `Terminate` message so the worker thread exits its event loop and
/// the associated `JoinHandle` can be dropped.
pub fn terminate_worker(registry: &WorkerRegistry, id: u32) {
    if let Some(h) = registry.lock().unwrap().remove(&id) {
        let _ = h.tx.send(WorkerInMsg::Terminate);
    }
}

/// Drain all pending messages sent from worker threads to the main thread.
///
/// Returns the drained list; clears the internal queue atomically.
pub fn drain_messages(queue: &WorkerMessageQueue) -> Vec<(u32, String)> {
    std::mem::take(&mut queue.lock().unwrap())
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
///
/// Returns `None` for any other scheme (external HTTP/HTTPS URLs require async
/// network access which is not available inside a synchronous worker thread).
fn resolve_import_url(url: &str, blob_store: &WorkerBlobStore) -> Option<String> {
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
        None
    }
}

// ─── shared WorkerGlobalScope surface ─────────────────────────────────────────

/// Install the parts of the global scope that WHATWG exposes in **every**
/// `WorkerGlobalScope`, not just the dedicated-worker one: the `_lumen_now_ms`
/// clock native plus [`crate::dom::worker_exposed_shim`] (`EventTarget` and the
/// `Performance` interface with its `performance` singleton).
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
    rt.eval(&crate::dom::worker_exposed_shim())?;
    Ok(())
}

// ─── worker thread ────────────────────────────────────────────────────────────

/// Build the worker-thread global-scope shim source for a given worker id.
///
/// Pure JS (no engine-specific bits), used by [`install_worker_globals_v8`].
/// Provides `self`, `postMessage`, `onmessage`, `addEventListener`,
/// `removeEventListener`, `_lumen_worker_dispatch_message`, `console`,
/// `importScripts` (data: + blob: URLs), `setTimeout`/`clearTimeout`/
/// `setInterval`/`clearInterval` (minimal stubs), `queueMicrotask`.
/// `atob`/`btoa` are installed separately as natives since they need
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
    if (type === 'message' && typeof fn === 'function') _msgListeners.push(fn);
  }};

  globalThis.removeEventListener = function(type, fn) {{
    if (type === 'message') {{
      var i = _msgListeners.indexOf(fn);
      if (i !== -1) _msgListeners.splice(i, 1);
    }}
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
    if (_onmessage) {{ try {{ _onmessage(ev); }} catch(e) {{}} }}
    for (var i = 0; i < _msgListeners.length; i++) {{
      try {{ _msgListeners[i](ev); }} catch(e) {{}}
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
  // Synchronously loads and evaluates one or more scripts.
  // Supported: data: URLs (base64 or percent-encoded) and blob:lumen/ URLs.
  // External http(s): URLs throw NetworkError (no sync fetch in worker threads).
  globalThis.importScripts = function() {{
    for (var i = 0; i < arguments.length; i++) {{
      var u = String(arguments[i]);
      var script = _lumen_import_scripts_resolve(u);
      if (script === null || script === undefined) {{
        throw new Error('importScripts: cannot load script: ' + u);
      }}
      (1, eval)(script);
    }}
  }};

  // Minimal setTimeout stub: enqueues callbacks, flushed between messages
  // (see _lumen_flush_timers called by the Rust message loop).
  var _timerQueue = [];
  var _nextTimerId = 1;
  globalThis.setTimeout = function(fn, _delay) {{
    var id = _nextTimerId++;
    _timerQueue.push({{ id: id, fn: fn }});
    return id;
  }};
  globalThis.clearTimeout = function(id) {{
    _timerQueue = _timerQueue.filter(function(t) {{ return t.id !== id; }});
  }};
  // setInterval: single-shot stub (no repeating in Phase 0).
  globalThis.setInterval = globalThis.setTimeout;
  globalThis.clearInterval = globalThis.clearTimeout;

  // queueMicrotask: front-queue so microtasks fire before regular timers.
  globalThis.queueMicrotask = function(fn) {{
    _timerQueue.unshift({{ id: _nextTimerId++, fn: fn }});
  }};

  // Flush all pending timer callbacks (called by Rust between message dispatches).
  globalThis._lumen_flush_timers = function() {{
    var pending = _timerQueue.splice(0);
    for (var i = 0; i < pending.length; i++) {{
      try {{ pending[i].fn(); }} catch(e) {{}}
    }}
  }};

}})({worker_id});"#
    )
}

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

  function Worker(url) {
    var script;
    var u = String(url || '');

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
        if (typeof self._onerror === 'function') { try { self._onerror(ev); } catch(e) {} }
        for (var i = 0; i < self._errorListeners.length; i++) {
          try { self._errorListeners[i](ev); } catch(e) {}
        }
      }, 0);
      return;
    }

    this._id = _lumen_create_worker(script);
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
    if (this._onmessage) { try { this._onmessage(ev); } catch(e) {} }
    for (var i = 0; i < this._listeners.length; i++) {
      try { this._listeners[i](ev); } catch(e) {}
    }
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
pub(crate) fn install_worker_bindings_v8(
    rt: &V8JsRuntime,
    registry: &WorkerRegistry,
    queue: &WorkerMessageQueue,
    next_id: &Arc<Mutex<u32>>,
    blob_store: &WorkerBlobStore,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
) -> JsResult<()> {
    // _lumen_create_worker(script: String) → u32
    {
        let reg = Arc::clone(registry);
        let q = Arc::clone(queue);
        let nid = Arc::clone(next_id);
        let bs = Arc::clone(blob_store);
        rt.register_native(
            "_lumen_create_worker",
            into_v8_fn1(move |script: String| -> u32 {
                spawn_worker_v8(&reg, &q, &nid, &bs, script)
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

    rt.eval(WORKER_SHIM)?;
    Ok(())
}

/// Fetch a classic worker script body over the network via `provider`.
///
/// Returns `None` when there is no provider, the request fails, or the
/// response status is not 2xx — the caller (`_lumen_worker_fetch_script`)
/// surfaces that as `undefined` to JS, which fires `error` on the `Worker`
/// instead of running an empty script.
#[cfg(feature = "v8-backend")]
pub(crate) fn fetch_worker_script(provider: Option<&dyn lumen_core::ext::JsFetchProvider>, url: &str) -> Option<String> {
    let resp = provider?.fetch_sync(url, "GET").ok()?;
    if !(200..300).contains(&resp.status) {
        return None;
    }
    Some(String::from_utf8_lossy(&resp.body).into_owned())
}

/// Spawn a new worker thread backed by its own [`V8JsRuntime`] that evaluates
/// `script` and waits for messages.
///
/// Returns the unique worker ID assigned to this instance. The caller stores
/// the ID in the JS `Worker` object and uses it for `postMessage`/`terminate`.
#[cfg(feature = "v8-backend")]
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
fn spawn_worker_v8(
    registry: &WorkerRegistry,
    queue: &WorkerMessageQueue,
    next_id: &Arc<Mutex<u32>>,
    blob_store: &WorkerBlobStore,
    script: String,
) -> u32 {
    let id = {
        let mut n = next_id.lock().unwrap();
        let id = *n;
        *n += 1;
        id
    };

    let (tx, rx) = mpsc::channel::<WorkerInMsg>();
    let reply = Arc::clone(queue);
    let store = Arc::clone(blob_store);

    let handle = thread::Builder::new()
        .name(format!("lumen-worker-v8-{id}"))
        .spawn(move || run_worker_thread_v8(id, script, rx, reply, store))
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
fn run_worker_thread_v8(
    id: u32,
    script: String,
    rx: Receiver<WorkerInMsg>,
    reply: Arc<Mutex<Vec<(u32, String)>>>,
    blob_store: WorkerBlobStore,
) {
    let rt = match V8JsRuntime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[worker-{id}] v8 runtime init failed: {e}");
            return;
        }
    };

    if let Err(e) = install_worker_globals_v8(&rt, id, Arc::clone(&reply), Arc::clone(&blob_store)) {
        eprintln!("[worker-{id}] v8 globals install failed: {e:?}");
        return;
    }

    if let Err(e) = rt.eval(&script) {
        eprintln!("[worker-{id}] v8 script error: {e:?}");
        // Continue: worker may still receive messages if the error was partial.
    }

    // Message loop: continue for Post; Terminate or channel-close exits.
    while let Ok(WorkerInMsg::Post(json)) = rx.recv() {
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

/// Install the Worker global environment into a V8 runtime. Registers the
/// natives `_lumen_worker_post_reply`, `_lumen_worker_console_log`,
/// `_lumen_import_scripts_resolve`, `atob`, `btoa` and evaluates
/// [`worker_global_shim`].
///
/// `atob`/`btoa` go through [`crate::v8_compat::V8NativeFnScoped`] (raw scope
/// access) rather than the plain `into_v8_fnN` path, because they must throw
/// a JS exception on invalid input (WHATWG Infra §forgiving-base64) — the
/// generic compat layer's `IntoJsReturn` has no error/throw variant (same
/// reasoning as `wasm_compile_native_v8` in S9).
#[cfg(feature = "v8-backend")]
fn install_worker_globals_v8(
    rt: &V8JsRuntime,
    worker_id: u32,
    reply: Arc<Mutex<Vec<(u32, String)>>>,
    blob_store: WorkerBlobStore,
) -> JsResult<()> {
    rt.register_native(
        "_lumen_worker_post_reply",
        into_v8_fn1(move |json: String| {
            reply.lock().unwrap().push((worker_id, json));
        }),
    )?;

    rt.register_native(
        "_lumen_worker_console_log",
        into_v8_fn1(move |msg: String| {
            eprintln!("[worker-{worker_id}] {msg}");
        }),
    )?;

    rt.register_native(
        "_lumen_import_scripts_resolve",
        into_v8_fn1(move |url: String| -> Option<String> { resolve_import_url(&url, &blob_store) }),
    )?;

    rt.register_native_scoped("atob", Box::new(atob_native_v8))?;
    rt.register_native_scoped("btoa", Box::new(btoa_native_v8))?;

    // Before the dedicated-worker shim: it is what gives the scope `performance`
    // (BUG-401) and `EventTarget`, and the `performance` time origin is taken at
    // this point — the creation of this global scope, per HR Time L3 §4.2.
    install_worker_scope_globals_v8(rt)?;

    rt.eval(&worker_global_shim(worker_id))?;
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
        assert_eq!(resolve_import_url(&url, &store).unwrap(), script);
    }

    #[test]
    fn resolve_data_url_base64() {
        let store = make_store();
        // base64("postMessage('hi');") = cG9zdE1lc3NhZ2UoJ2hpJyk7
        let url = "data:text/javascript;base64,cG9zdE1lc3NhZ2UoJ2hpJyk7";
        assert_eq!(resolve_import_url(url, &store).unwrap(), "postMessage('hi');");
    }

    #[test]
    fn resolve_blob_url_from_store() {
        let store = make_store();
        store.lock().unwrap().insert("blob:lumen/42".to_string(), "var x = 1;".to_string());
        assert_eq!(resolve_import_url("blob:lumen/42", &store).unwrap(), "var x = 1;");
    }

    #[test]
    fn resolve_external_url_returns_none() {
        let store = make_store();
        assert!(resolve_import_url("https://example.com/lib.js", &store).is_none());
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
    use super::*;

    fn make_store() -> WorkerBlobStore {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[test]
    fn v8_worker_shim_installs_without_error() {
        let rt = V8JsRuntime::new().unwrap();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let nid = Arc::new(Mutex::new(0u32));
        install_worker_bindings_v8(&rt, &reg, &queue, &nid, &make_store(), None).unwrap();
        let result = rt.eval("typeof Worker === 'function'").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn v8_worker_globals_have_atob_btoa() {
        let rt = V8JsRuntime::new().unwrap();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), make_store()).unwrap();

        let decoded = rt.eval("atob('aGVsbG8=')").unwrap();
        assert_eq!(decoded, lumen_core::JsValue::String("hello".into()));
        let encoded = rt.eval("btoa('hello')").unwrap();
        assert_eq!(encoded, lumen_core::JsValue::String("aGVsbG8=".into()));
    }

    #[test]
    fn v8_atob_throws_on_invalid_input() {
        let rt = V8JsRuntime::new().unwrap();
        let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), make_store()).unwrap();

        let ok = rt
            .eval("(function(){try{atob('!!!');return false;}catch(e){return e instanceof TypeError;}})()")
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
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
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), make_store()).unwrap();

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
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), make_store()).unwrap();
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
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), make_store()).unwrap();

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
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        let script = "var t0 = performance.now();\
                      onmessage = function(e) {\
                        postMessage(performance.now() >= t0 && performance.timeOrigin > 0);\
                      };"
            .to_string();
        let worker_id = spawn_worker_v8(&reg, &queue, &nid, &store, script);

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
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        // Worker echoes its received message doubled.
        let script = "onmessage = function(e) { postMessage(e.data * 2); };".to_string();
        let worker_id = spawn_worker_v8(&reg, &queue, &nid, &store, script);

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
        let worker_id = spawn_worker_v8(&reg, &queue, &nid, &store, script);

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

        let worker_id = spawn_worker_v8(&reg, &queue, &nid, &store, script);
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
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        // Worker posts a reply to every message.
        let script = "onmessage = function(e) { postMessage('got:' + e.data); };".to_string();
        let worker_id = spawn_worker_v8(&reg, &queue, &nid, &store, script);

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
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), store).unwrap();

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
        install_worker_globals_v8(&rt, 0, Arc::clone(&queue), store).unwrap();

        let result = rt.eval("importScripts('https://external.example/lib.js')");
        assert!(result.is_err(), "importScripts with http URL should throw");
    }

    #[test]
    fn v8_serialize_with_no_transfers_is_standard_json() {
        let rt = V8JsRuntime::new().unwrap();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let nid = Arc::new(Mutex::new(0u32));
        install_worker_bindings_v8(&rt, &reg, &queue, &nid, &make_store(), None).unwrap();

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
        let nid = Arc::new(Mutex::new(0u32));
        install_worker_bindings_v8(&rt, &reg, &queue, &nid, &make_store(), None).unwrap();
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
}
