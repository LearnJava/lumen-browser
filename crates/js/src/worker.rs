//! Web Worker implementation (WHATWG Web Workers §4).
//!
//! Each `new Worker(script_url)` call spawns a dedicated `std::thread` with its
//! own QuickJS `Runtime` + `Context`.  Messages are JSON-serialized strings
//! passed through `mpsc` channels in both directions.
//!
//! **Main → worker:** via `Sender<WorkerInMsg>` stored in `WorkerRegistry`.
//! **Worker → main:** via `Arc<Mutex<Vec<(u32,String)>>>` (`WorkerMessageQueue`).
//! The shell drains the queue each event-loop tick by calling
//! `QuickJsRuntime::pump_workers()`, which delivers messages to the matching
//! `Worker` instance in JS via `_lumen_deliver_worker_messages(msgs)`.
//!
//! **importScripts():** supported for `data:` and `blob:lumen/` URLs via
//! `WorkerBlobStore` — a Rust-side `Arc<Mutex<HashMap<String, String>>>` that
//! mirrors text blobs registered by `URL.createObjectURL()` on the main thread.
//! The WORKER_SHIM wraps `URL.createObjectURL` to populate this store for any
//! Blob whose MIME type starts with "text/" or is "application/javascript".

use crate::offscreen_canvas::install_offscreen_canvas_bindings;
use rquickjs::{Context, Function, Runtime};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

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

/// Spawn a new worker thread that evaluates `script` and waits for messages.
///
/// Returns the unique worker ID assigned to this instance.  The caller stores
/// the ID in the JS `Worker` object and uses it for `postMessage`/`terminate`.
pub fn spawn_worker(
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
        .name(format!("lumen-worker-{id}"))
        .spawn(move || run_worker_thread(id, script, rx, reply, store))
        .expect("failed to spawn Web Worker thread");

    registry
        .lock()
        .unwrap()
        .insert(id, WorkerHandle { tx, _thread: handle });
    id
}

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

/// Install native bindings (`_lumen_create_worker`, `_lumen_worker_post`,
/// `_lumen_worker_terminate`, `_lumen_register_worker_blob`) and the `Worker`
/// JS class into `ctx`.
///
/// Must be called after the core DOM shim so that `TextDecoder` and
/// `_object_url_store` are available for blob-URL resolution in the constructor.
pub fn install_worker_bindings(
    ctx: &rquickjs::Ctx<'_>,
    registry: &WorkerRegistry,
    queue: &WorkerMessageQueue,
    next_id: &Arc<Mutex<u32>>,
    blob_store: &WorkerBlobStore,
) -> rquickjs::Result<()> {
    macro_rules! reg {
        ($name:expr, $f:expr) => {
            ctx.globals()
                .set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }

    // _lumen_create_worker(script: String) → u32
    {
        let reg = Arc::clone(registry);
        let q = Arc::clone(queue);
        let nid = Arc::clone(next_id);
        let bs = Arc::clone(blob_store);
        reg!("_lumen_create_worker", move |script: String| -> u32 {
            spawn_worker(&reg, &q, &nid, &bs, script)
        });
    }

    // _lumen_worker_post(id: u32, json: String)
    {
        let reg = Arc::clone(registry);
        reg!("_lumen_worker_post", move |id: u32, json: String| {
            post_to_worker(&reg, id, json);
        });
    }

    // _lumen_worker_terminate(id: u32)
    {
        let reg = Arc::clone(registry);
        reg!("_lumen_worker_terminate", move |id: u32| {
            terminate_worker(&reg, id);
        });
    }

    // _lumen_register_worker_blob(url: String, text: String) — called from the
    // WORKER_SHIM URL.createObjectURL wrapper for text/* / application/javascript
    // blobs so that importScripts('blob:lumen/…') can find the script text.
    {
        let bs = Arc::clone(blob_store);
        reg!("_lumen_register_worker_blob", move |url: String, text: String| {
            bs.lock().unwrap().insert(url, text);
        });
    }

    // Evaluate the Worker class JS shim.
    ctx.eval::<(), _>(WORKER_SHIM)?;
    Ok(())
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

// ─── worker thread ────────────────────────────────────────────────────────────

fn run_worker_thread(
    id: u32,
    script: String,
    rx: Receiver<WorkerInMsg>,
    reply: Arc<Mutex<Vec<(u32, String)>>>,
    blob_store: WorkerBlobStore,
) {
    let rt = match Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[worker-{id}] runtime init failed: {e}");
            return;
        }
    };
    let ctx = match Context::full(&rt) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[worker-{id}] context init failed: {e}");
            return;
        }
    };

    if let Err(e) = ctx.with(|ctx| install_worker_globals(&ctx, id, Arc::clone(&reply), Arc::clone(&blob_store))) {
        eprintln!("[worker-{id}] globals install failed: {e:?}");
        return;
    }

    // OffscreenCanvas is available in dedicated workers (HTML LS §4.12.14).
    // Each worker thread gets its own thread-local canvas registry.
    if let Err(e) = ctx.with(|ctx| install_offscreen_canvas_bindings(&ctx)) {
        eprintln!("[worker-{id}] OffscreenCanvas install failed: {e:?}");
    }

    if let Err(e) = ctx.with(|ctx| ctx.eval::<(), _>(script.as_str())) {
        eprintln!("[worker-{id}] script error: {e:?}");
        // Continue: worker may still receive messages if the error was partial.
    }

    // Message loop: continue for Post; Terminate or channel-close exits.
    while let Ok(WorkerInMsg::Post(json)) = rx.recv() {
        ctx.with(|ctx| {
            // Pass JSON via a temporary global to avoid embedding raw JSON
            // in a JS string literal (avoids escaping issues).
            let _ = ctx.globals().set("_lw_msg__", json.as_str());
            ctx.eval::<(), _>(
                "if(typeof _lumen_worker_dispatch_message==='function')\
                 {_lumen_worker_dispatch_message(JSON.parse(_lw_msg__));\
                  if(typeof _lumen_flush_timers==='function')_lumen_flush_timers();}"
            )
            .ok();
        });
    }
}

/// Install the Worker global environment into a QuickJS context.
///
/// Provides: `self`, `postMessage`, `onmessage`, `addEventListener`,
/// `removeEventListener`, `_lumen_worker_dispatch_message`, `console`,
/// `importScripts` (data: + blob: URLs), `atob`, `btoa`,
/// `setTimeout`/`clearTimeout`/`setInterval`/`clearInterval` (minimal stubs),
/// `queueMicrotask`.
fn install_worker_globals(
    ctx: &rquickjs::Ctx<'_>,
    worker_id: u32,
    reply: Arc<Mutex<Vec<(u32, String)>>>,
    blob_store: WorkerBlobStore,
) -> rquickjs::Result<()> {
    macro_rules! reg {
        ($name:expr, $f:expr) => {
            ctx.globals()
                .set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }

    // _lumen_worker_post_reply(json): push reply to the shared outbox.
    {
        let r = Arc::clone(&reply);
        reg!("_lumen_worker_post_reply", move |json: String| {
            r.lock().unwrap().push((worker_id, json));
        });
    }

    // _lumen_worker_console_log(msg): forward to stderr.
    reg!("_lumen_worker_console_log", move |msg: String| {
        eprintln!("[worker-{worker_id}] {msg}");
    });

    // _lumen_import_scripts_resolve(url) → Option<String>
    // Resolves data: or blob:lumen/ URLs to script text for importScripts().
    {
        let bs = Arc::clone(&blob_store);
        reg!("_lumen_import_scripts_resolve", move |url: String| -> Option<String> {
            resolve_import_url(&url, &bs)
        });
    }

    // atob(str) → base64-decoded string (WHATWG Infra §forgiving-base64).
    reg!("atob", move |encoded: String| -> rquickjs::Result<String> {
        b64_decode(&encoded)
            .and_then(|b| String::from_utf8(b).ok())
            .ok_or(rquickjs::Error::Exception)
    });

    // btoa(str) → base64-encoded string (WHATWG Infra §forgiving-base64 encode).
    reg!("btoa", move |s: String| -> rquickjs::Result<String> {
        // btoa only accepts Latin-1; characters > U+00FF throw.
        if s.chars().any(|c| c as u32 > 255) {
            return Err(rquickjs::Error::Exception);
        }
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
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
        Ok(out)
    });

    // Install the remaining worker global environment via JS.
    let init = format!(
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
    );

    ctx.eval::<(), _>(init.as_str())
}

// ─── Worker JS class (evaluated in the main-thread JS context) ───────────────

/// IIFE that defines `globalThis.Worker` and `_lumen_deliver_worker_messages`.
///
/// Depends on:
/// - `_lumen_create_worker` / `_lumen_worker_post` / `_lumen_worker_terminate`
///   (native bindings installed by `install_worker_bindings` above).
/// - `_lumen_register_worker_blob` (native binding installed above — mirrors
///   text blobs into `WorkerBlobStore` so `importScripts` can load them).
/// - `_object_url_store` (defined in WEB_API_SHIM for blob: URL resolution).
/// - `TextDecoder` (defined in WEB_API_SHIM for UTF-8 decoding of blob bytes).
/// - `atob` (defined in WEB_API_SHIM for data: URLs with base64 encoding).
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
      // Serialize the pixel buffer via the existing native transfer binding.
      var raw = _lumen_offscreen_canvas_transfer_to_image_bitmap(obj.__canvas_id__);
      if (!raw) return null;
      var comma1 = raw.indexOf(',');
      var comma2 = raw.indexOf(',', comma1 + 1);
      var w = parseInt(raw.slice(0, comma1), 10);
      var h = parseInt(raw.slice(comma1 + 1, comma2), 10);
      var p = raw.slice(comma2 + 1);
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
      // External URL workers are not yet supported (requires async fetch).
      script = '/* Lumen: external URL worker not supported: ' + u.replace(/\*\//g,'*\\/') + ' */';
    }

    this._id = _lumen_create_worker(script);
    this._onmessage = null;
    this._listeners = [];
    _workerRegistry[this._id] = this;
  }

  // postMessage(data[, transfer]) — send structured data to the worker thread.
  // When transfer contains OffscreenCanvas objects (identified by __canvas_id__),
  // their pixel buffers are serialized into the payload so the worker can
  // reconstruct them as OffscreenCanvas instances.
  Worker.prototype.postMessage = function(data, transfer) {
    _lumen_worker_post(this._id, _lumenSerializeWithTransfers(data, transfer));
  };

  // terminate() — immediately stop the worker; no more messages delivered.
  Worker.prototype.terminate = function() {
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

  Worker.prototype.addEventListener = function(type, fn, _opts) {
    if (type === 'message' && typeof fn === 'function') {
      this._listeners.push(fn);
    }
  };

  Worker.prototype.removeEventListener = function(type, fn) {
    if (type === 'message') {
      var i = this._listeners.indexOf(fn);
      if (i !== -1) this._listeners.splice(i, 1);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offscreen_canvas::install_offscreen_canvas_bindings;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn make_store() -> WorkerBlobStore {
        Arc::new(Mutex::new(HashMap::new()))
    }

    fn setup_ctx(ctx: &rquickjs::Ctx<'_>, store: &WorkerBlobStore) {
        install_offscreen_canvas_bindings(ctx).unwrap();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let nid = Arc::new(Mutex::new(0u32));
        install_worker_bindings(ctx, &reg, &queue, &nid, store).unwrap();
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

    // ── JS shim installs ───────────────────────────────────────────────────────

    #[test]
    fn worker_shim_installs_without_error() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup_ctx(&ctx, &make_store());
            let result: bool = ctx.eval("typeof Worker === 'function'").unwrap();
            assert!(result, "Worker class should be defined");
        });
    }

    #[test]
    fn worker_globals_have_atob_btoa() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        let store = make_store();
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install_worker_globals(&ctx, 0, Arc::clone(&queue), Arc::clone(&store)).unwrap();
            // atob should decode base64
            let decoded: String = ctx.eval("atob('aGVsbG8=')").unwrap();
            assert_eq!(decoded, "hello");
            // btoa should encode to base64
            let encoded: String = ctx.eval("btoa('hello')").unwrap();
            assert_eq!(encoded, "aGVsbG8=");
        });
    }

    #[test]
    fn import_scripts_data_url_plain() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        let store = make_store();
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install_worker_globals(&ctx, 0, Arc::clone(&queue), Arc::clone(&store)).unwrap();
            // importScripts with a plain data: URL should evaluate the script
            ctx.eval::<(), _>(
                "importScripts('data:text/javascript,globalThis._imported_x = 99;')"
            ).unwrap();
            let v: i32 = ctx.eval("_imported_x").unwrap();
            assert_eq!(v, 99);
        });
    }

    #[test]
    fn import_scripts_data_url_base64() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        let store = make_store();
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install_worker_globals(&ctx, 0, Arc::clone(&queue), Arc::clone(&store)).unwrap();
            // base64("globalThis._b64_val = 77;") =
            // Z2xvYmFsVGhpcy5fYjY0X3ZhbCA9IDc3Ow==
            ctx.eval::<(), _>(
                "importScripts('data:text/javascript;base64,Z2xvYmFsVGhpcy5fYjY0X3ZhbCA9IDc3Ow==')"
            ).unwrap();
            let v: i32 = ctx.eval("_b64_val").unwrap();
            assert_eq!(v, 77);
        });
    }

    #[test]
    fn import_scripts_blob_url() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        let store = make_store();
        store.lock().unwrap().insert(
            "blob:lumen/99".to_string(),
            "globalThis._blob_loaded = 'yes';".to_string(),
        );
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install_worker_globals(&ctx, 0, Arc::clone(&queue), Arc::clone(&store)).unwrap();
            ctx.eval::<(), _>("importScripts('blob:lumen/99')").unwrap();
            let v: String = ctx.eval("_blob_loaded").unwrap();
            assert_eq!(v, "yes");
        });
    }

    #[test]
    fn import_scripts_multiple_urls() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        let store = make_store();
        store.lock().unwrap().insert(
            "blob:lumen/1".to_string(),
            "globalThis._ms1 = 10;".to_string(),
        );
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install_worker_globals(&ctx, 0, Arc::clone(&queue), Arc::clone(&store)).unwrap();
            ctx.eval::<(), _>(
                "importScripts(\
                   'blob:lumen/1',\
                   'data:text/javascript,globalThis._ms2 = 20;'\
                 )"
            ).unwrap();
            let v1: i32 = ctx.eval("_ms1").unwrap();
            let v2: i32 = ctx.eval("_ms2").unwrap();
            assert_eq!(v1, 10);
            assert_eq!(v2, 20);
        });
    }

    #[test]
    fn import_scripts_unknown_url_throws() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        let store = make_store();
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install_worker_globals(&ctx, 0, Arc::clone(&queue), Arc::clone(&store)).unwrap();
            let result: rquickjs::Result<()> = ctx.eval(
                "importScripts('https://external.example/lib.js')"
            );
            assert!(result.is_err(), "importScripts with http URL should throw");
        });
    }

    // ── serialize helpers ──────────────────────────────────────────────────────

    #[test]
    fn serialize_with_no_transfers_is_standard_json() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup_ctx(&ctx, &make_store());
            let result: String = ctx.eval(
                r#"_lumenSerializeWithTransfers({x: 1, y: "hello"}, [])"#,
            ).unwrap();
            assert_eq!(result, r#"{"x":1,"y":"hello"}"#);
        });
    }

    #[test]
    fn serialize_with_offscreen_canvas_transfer_embeds_sentinel() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup_ctx(&ctx, &make_store());
            let result: String = ctx.eval(r#"
                var oc = new OffscreenCanvas(2, 2);
                var ctx2d = oc.getContext('2d');
                ctx2d.fillStyle = '#ff0000';
                ctx2d.fillRect(0, 0, 2, 2);
                _lumenSerializeWithTransfers({canvas: oc}, [oc])
            "#).unwrap();
            let v: serde_json::Value = serde_json::from_str(&result).unwrap();
            let sentinel = &v["canvas"]["__lumen_sentinel__"];
            assert_eq!(sentinel.as_str().unwrap(), "__lumen_offscreen_transfer__");
            assert_eq!(v["canvas"]["w"].as_u64().unwrap(), 2);
            assert_eq!(v["canvas"]["h"].as_u64().unwrap(), 2);
            assert!(!v["canvas"]["p"].as_str().unwrap().is_empty(), "pixel data should be present");
        });
    }

    // ── end-to-end worker message passing ──────────────────────────────────────

    #[test]
    fn worker_end_to_end_postmessage() {
        use std::time::Duration;
        let rt = Runtime::new().unwrap();
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();

        // Spawn a worker that echoes its received message back doubled.
        let script = "onmessage = function(e) { postMessage(e.data * 2); };".to_string();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));
        let worker_id = spawn_worker(&reg, &queue, &nid, &store, script);

        // Send a message to the worker.
        post_to_worker(&reg, worker_id, "21".to_string());

        // Give the worker thread time to process.
        std::thread::sleep(Duration::from_millis(150));

        // Drain outbound messages.
        let msgs = drain_messages(&queue);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, worker_id);
        assert_eq!(msgs[0].1, "42");

        terminate_worker(&reg, worker_id);
        let _ = rt; // keep rt alive (not used, just makes the intent clear)
    }

    #[test]
    fn worker_terminate_stops_message_delivery() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        // Worker posts a reply to every message.
        let script = "onmessage = function(e) { postMessage('got:' + e.data); };".to_string();
        let worker_id = spawn_worker(&reg, &queue, &nid, &store, script);

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
    fn worker_import_scripts_via_data_url() {
        use std::time::Duration;
        let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
        let store = make_store();
        let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
        let nid = Arc::new(Mutex::new(0u32));

        // Worker uses importScripts to load a helper via data: URL then calls it.
        // The helper defines add(a, b) = a + b.
        // base64 of "function add(a,b){return a+b;}" = ZnVuY3Rpb24gYWRkKGEsYil7cmV0dXJuIGErYjt9
        let script = concat!(
            "importScripts('data:text/javascript;base64,",
            "ZnVuY3Rpb24gYWRkKGEsYil7cmV0dXJuIGErYjt9",
            "');",
            "onmessage = function(e) { postMessage(add(e.data, 1)); };"
        ).to_string();

        let worker_id = spawn_worker(&reg, &queue, &nid, &store, script);
        post_to_worker(&reg, worker_id, "9".to_string());
        std::thread::sleep(Duration::from_millis(200));

        let msgs = drain_messages(&queue);
        assert_eq!(msgs.len(), 1, "expected one reply");
        assert_eq!(msgs[0].1, "10");

        terminate_worker(&reg, worker_id);
    }

    #[test]
    fn worker_import_scripts_via_blob_url() {
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

        let worker_id = spawn_worker(&reg, &queue, &nid, &store, script);
        post_to_worker(&reg, worker_id, "7".to_string());
        std::thread::sleep(Duration::from_millis(200));

        let msgs = drain_messages(&queue);
        assert_eq!(msgs.len(), 1, "expected one reply");
        assert_eq!(msgs[0].1, "21");

        terminate_worker(&reg, worker_id);
    }
}
