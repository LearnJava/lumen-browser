//! Shared Worker implementation (WHATWG HTML §10.2, stub).
//!
//! `new SharedWorker(url, name)` connects to a single worker that is **shared**
//! between all same-process clients with the same identity key (name, or URL
//! when the name is empty).  Unlike a dedicated [`crate::worker`], a shared
//! worker is **not** spawned per call: the first connection spawns one
//! `std::thread` running its own [`V8JsRuntime`]; later connections (from any
//! page) reuse it and only register a fresh `MessagePort`. The rquickjs
//! backend was removed in S12b-B18 — V8 is now the only backend.
//!
//! Identity & lifetime are therefore process-global: a [`HUB_V8`] keyed by the
//! identity string maps to the live worker thread.  Each connection is assigned
//! a globally-unique **port id** ([`PORT_COUNTER`]).
//!
//! **Client → worker:** `port.postMessage(data)` → `_lumen_sw_post(key, pid,
//! json)` → [`SwInMsg::Post`] over the worker's `mpsc` channel.
//!
//! **Worker → client:** inside the worker, `connectEvent.ports[0].postMessage`
//! → `_lumen_sw_port_reply(pid, json)`, which looks up the *connecting client's*
//! outbox (registered at connect time) and pushes `(pid, json)`.  Each page
//! runtime owns one outbox; it drains its own messages on every event-loop
//! tick via `pump_shared_workers()`, which calls
//! `_lumen_deliver_shared_worker_messages(msgs)` to route each payload to the
//! matching client `port` by id.
//!
//! External (`http(s):`) script URLs are fetched synchronously via the shared
//! `JsFetchProvider` bridge (BUG-364, `crate::worker::fetch_worker_script`),
//! resolved against the document base URL in the JS shim; `blob:` / `data:`
//! scripts still resolve locally (identical to the dedicated-worker
//! resolver). If the fetch fails, the worker never connects and `onerror`
//! fires once instead of running an empty script.
//!
//! **Uncaught-exception reporting (BUG-591 SharedWorker parent-side
//! reporting):** an uncaught exception anywhere in the shared worker's global
//! scope (top-level script body, `onconnect`, a port's `onmessage`, a flushed
//! timer callback) is HTML LS "report the exception" run on a scope that
//! every connected client observes — unlike a dedicated [`crate::worker`],
//! where exactly one client exists. So the error is broadcast to *every*
//! currently-connected port's owning client, not routed by the port that
//! happened to trigger it. `_lumen_sw_report_error` (registered per
//! connecting client in [`install_shared_worker_globals_v8`]) pushes into
//! every live port's [`crate::worker::WorkerErrorQueue`] entry tracked by
//! `error_ports` inside [`run_shared_worker_thread_v8`]; each client's
//! `V8JsRuntime::pump_shared_workers` drains its own queue and fires
//! `ErrorEvent` `'error'` at the matching `SharedWorker` instance via
//! `_lumen_deliver_shared_worker_errors`.

use std::sync::{Arc, Mutex};

#[cfg(feature = "v8-backend")]
use std::collections::HashMap;
#[cfg(feature = "v8-backend")]
use std::sync::OnceLock;
#[cfg(feature = "v8-backend")]
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
#[cfg(feature = "v8-backend")]
use std::sync::mpsc::{self, Receiver, Sender};
#[cfg(feature = "v8-backend")]
use std::thread;

#[cfg(feature = "v8-backend")]
use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn4};
#[cfg(feature = "v8-backend")]
use crate::v8_runtime::V8JsRuntime;
#[cfg(feature = "v8-backend")]
use lumen_core::JsResult;
#[cfg(feature = "v8-backend")]
use lumen_core::ext::JsRuntime as _;

// ─── shared types ───────────────────────────────────────────────────────────────

/// Outbound queue owned by a single page runtime.
///
/// Worker threads push `(port_id, json_string)` pairs destined for that
/// runtime's client ports; the runtime drains it via `drain_messages`.
pub type SharedWorkerOutbox = Arc<Mutex<Vec<(u32, String)>>>;

/// Message sent from a client (main JS thread) to a shared-worker thread.
#[cfg(feature = "v8-backend")]
enum SwInMsg {
    /// A new client connected: register `port_id` → its `outbox`/`errors`,
    /// then fire the `connect` event in the worker with a worker-side port
    /// for `port_id`. `errors` is the connecting client's own
    /// [`crate::worker::WorkerErrorQueue`], registered so a later uncaught
    /// exception in the worker can be broadcast to this client too (BUG-591).
    Connect {
        port_id: u32,
        outbox: SharedWorkerOutbox,
        errors: crate::worker::WorkerErrorQueue,
    },
    /// JSON-serialised data from `port.postMessage(data)` on the client side.
    Post { port_id: u32, json: String },
    /// The client closed its port — drop the worker-side mapping.
    Close { port_id: u32 },
}

/// Live shared-worker thread plus its inbound channel.
#[cfg(feature = "v8-backend")]
struct SharedWorkerThread {
    /// Channel used to deliver `Connect` / `Post` / `Close` to the worker loop.
    tx: Sender<SwInMsg>,
    /// Join handle, kept so the thread is joined when the hub entry is dropped.
    _thread: thread::JoinHandle<()>,
}

/// Monotonic source of globally-unique port ids (one per `SharedWorker`).
#[cfg(feature = "v8-backend")]
static PORT_COUNTER: AtomicU32 = AtomicU32::new(1);

/// Drain all messages a runtime's shared-worker ports have received.
///
/// Returns the drained `(port_id, json)` list and clears the queue atomically.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn drain_messages(outbox: &SharedWorkerOutbox) -> Vec<(u32, String)> {
    std::mem::take(&mut outbox.lock().unwrap())
}

// ─── JS shims ────────────────────────────────────────────────────────────────

/// Worker-thread global scope shim (evaluated inside each shared-worker context).
#[cfg(feature = "v8-backend")]
const SHARED_WORKER_GLOBAL_SHIM: &str = r#"(function() {
  var _connectListeners = [];
  var _onconnect = null;
  var _ports = {};   // port_id → worker-side MessagePort

  globalThis.self = globalThis;

  // Report an uncaught exception from onconnect/onmessage/a flushed timer to
  // every connected client (BUG-591 SharedWorker parent-side reporting; HTML
  // LS "report the exception" broadcasts to all owning `SharedWorker`
  // objects, unlike a dedicated worker's single client). `filename`/`lineno`/
  // `colno` are best-effort-parsed from `.stack`, same technique as the
  // dedicated-worker twin (`worker.rs`'s `_lumen_report_worker_exception`).
  function _lumen_sw_report_exception(err) {
    var message = (err instanceof Error) ? String(err.message) : String(err);
    var filename = '', lineno = 0, colno = 0;
    if (err && typeof err.stack === 'string') {
      var lines = err.stack.split('\n');
      for (var i = 0; i < lines.length; i++) {
        var m = /at (?:.*\()?([^\s()]+):(\d+):(\d+)\)?\s*$/.exec(lines[i]);
        if (m) { filename = m[1]; lineno = +m[2]; colno = +m[3]; break; }
      }
    }
    _lumen_sw_report_error(message, filename, lineno, colno);
  }

  Object.defineProperty(globalThis, 'onconnect', {
    get: function() { return _onconnect; },
    set: function(fn) { _onconnect = typeof fn === 'function' ? fn : null; },
    configurable: true,
  });

  globalThis.addEventListener = function(type, fn, _opts) {
    if (type === 'connect' && typeof fn === 'function') _connectListeners.push(fn);
  };
  globalThis.removeEventListener = function(type, fn) {
    if (type === 'connect') {
      var i = _connectListeners.indexOf(fn);
      if (i !== -1) _connectListeners.splice(i, 1);
    }
  };

  // Worker-side MessagePort for a single client connection.
  function _makePort(pid) {
    var port = {
      _pid: pid,
      _onmessage: null,
      _listeners: [],
      postMessage: function(data) { _lumen_sw_port_reply(pid, JSON.stringify(data)); },
      start: function() {},
      close: function() {},
      addEventListener: function(type, fn) {
        if (type === 'message' && typeof fn === 'function') this._listeners.push(fn);
      },
      removeEventListener: function(type, fn) {
        if (type === 'message') {
          var i = this._listeners.indexOf(fn);
          if (i !== -1) this._listeners.splice(i, 1);
        }
      },
      _deliver: function(data) {
        var ev = { data: data, type: 'message', target: this,
                   bubbles: false, cancelable: false, ports: [] };
        if (this._onmessage) { try { this._onmessage(ev); } catch(e) { _lumen_sw_report_exception(e); } }
        for (var i = 0; i < this._listeners.length; i++) {
          try { this._listeners[i](ev); } catch(e) { _lumen_sw_report_exception(e); }
        }
      },
    };
    Object.defineProperty(port, 'onmessage', {
      get: function() { return this._onmessage; },
      set: function(fn) { this._onmessage = typeof fn === 'function' ? fn : null; },
      configurable: true,
    });
    return port;
  }

  // Called by the Rust loop when a new client connects.
  globalThis._lumen_sw_dispatch_connect = function(pid) {
    var port = _makePort(pid);
    _ports[pid] = port;
    var ev = { type: 'connect', target: globalThis, source: port,
               ports: [port], bubbles: false, cancelable: false };
    if (_onconnect) { try { _onconnect(ev); } catch(e) { _lumen_sw_report_exception(e); } }
    for (var i = 0; i < _connectListeners.length; i++) {
      try { _connectListeners[i](ev); } catch(e) { _lumen_sw_report_exception(e); }
    }
  };

  // Called by the Rust loop for each client port.postMessage.
  globalThis._lumen_sw_dispatch_port_message = function(pid, data) {
    var port = _ports[pid];
    if (port) port._deliver(data);
  };

  // Called by the Rust loop when a client closes its port.
  globalThis._lumen_sw_dispatch_port_close = function(pid) {
    delete _ports[pid];
  };

  globalThis.console = {
    log:   function() { _lumen_sw_console_log(Array.prototype.map.call(arguments, String).join(' ')); },
    info:  function() { _lumen_sw_console_log(Array.prototype.map.call(arguments, String).join(' ')); },
    warn:  function() { _lumen_sw_console_log('[WARN] ' + Array.prototype.map.call(arguments, String).join(' ')); },
    error: function() { _lumen_sw_console_log('[ERR]  ' + Array.prototype.map.call(arguments, String).join(' ')); },
    debug: function() {},
  };

  // importScripts(url1[, url2, …]) — WHATWG Web Workers §4.2.3 (BUG-778:
  // previously an unconditional throw for every URL, including `data:`; now
  // matches the dedicated worker's own resolution — `data:` inline, and
  // anything else resolved against the worker's own script URL
  // (`_lumen_worker_base_url`, empty for a blob:/data: worker) and fetched
  // over the network. `blob:lumen/` still fails — see
  // `install_shared_worker_globals_v8`'s doc comment on why.
  globalThis.importScripts = function() {
    for (var i = 0; i < arguments.length; i++) {
      var u = String(arguments[i]);
      var resolved = u;
      if (u.indexOf('://') === -1 && u.slice(0, 5) !== 'data:' && u.slice(0, 5) !== 'blob:'
          && typeof _lumen_worker_base_url === 'string' && _lumen_worker_base_url) {
        try { resolved = new URL(u, _lumen_worker_base_url).href; } catch (e) { resolved = u; }
      }
      var script = _lumen_import_scripts_resolve(resolved);
      if (script === null || script === undefined) {
        throw new Error('importScripts: cannot load script: ' + resolved);
      }
      (1, eval)(script);
    }
  };

  // Minimal setTimeout stub: callbacks flushed between Rust dispatches.
  var _timerQueue = [];
  var _nextTimerId = 1;
  globalThis.setTimeout = function(fn, _delay) {
    var id = _nextTimerId++;
    _timerQueue.push({ id: id, fn: fn });
    return id;
  };
  globalThis.clearTimeout = function(id) {
    _timerQueue = _timerQueue.filter(function(t) { return t.id !== id; });
  };
  globalThis.setInterval = globalThis.setTimeout;
  globalThis.clearInterval = globalThis.clearTimeout;
  globalThis.queueMicrotask = function(fn) { _timerQueue.unshift({ id: _nextTimerId++, fn: fn }); };
  globalThis._lumen_flush_timers = function() {
    var pending = _timerQueue.splice(0);
    for (var i = 0; i < pending.length; i++) {
      try { pending[i].fn(); } catch(e) { _lumen_sw_report_exception(e); }
    }
  };

  // Exposed so the shared net shim (`crate::worker::WORKER_NET_SHIM`,
  // evaluated as a separate IIFE right after this one) routes a throwing
  // fetch/XHR listener through the same reporting path (BUG-591 shape,
  // BUG-778 scope).
  globalThis._lumen_worker_exception_reporter = _lumen_sw_report_exception;

  // close() — HTML LS §10.2.4 "close a worker" for a shared worker
  // (BUG-778): discard further queued tasks, including a not-yet-delivered
  // `connect`. `_lumen_worker_self_close` flips the shared flag
  // `run_shared_worker_thread_v8`'s message loop polls.
  globalThis.close = function() { _lumen_worker_self_close(); };
})();
"#;

/// Main-thread `SharedWorker` class shim (evaluated in the page JS context).
///
/// Depends on the `_lumen_sw_connect` / `_lumen_sw_post` / `_lumen_sw_close`
/// native bindings, plus `_object_url_store` / `TextDecoder` / `atob` from the
/// core DOM shim for blob-/data-URL script resolution.
#[cfg(feature = "v8-backend")]
const SHARED_WORKER_SHIM: &str = r#"(function() {
  var _clientPorts = {};            // port_id → client-side MessagePort
  var _sharedWorkerInstances = {};  // port_id → owning SharedWorker instance

  // Returns { script, base }: `script` is a String, or `null` when an
  // external URL's network fetch failed (BUG-364) — the caller must not
  // spawn a worker thread in that case, only fire `error` on the
  // SharedWorker instance. `base` is the worker's own resolved script URL
  // (empty for a blob:/data: worker, BUG-778) — threaded down to the worker
  // thread so its `importScripts()`/`fetch()`/`XMLHttpRequest` can resolve a
  // relative or path-absolute target.
  function _resolveScript(url) {
    var u = String(url || '');
    if (u.startsWith('blob:lumen/')) {
      var blob = (typeof _object_url_store !== 'undefined') ? _object_url_store[u] : null;
      if (blob && blob._bytes) {
        try { return { script: new TextDecoder().decode(blob._bytes), base: '' }; }
        catch(e) { return { script: '', base: '' }; }
      }
      return { script: '', base: '' };
    }
    if (u.startsWith('data:')) {
      var comma = u.indexOf(',');
      if (comma === -1) return { script: '', base: '' };
      var meta = u.slice(5, comma), content = u.slice(comma + 1);
      if (meta.indexOf('base64') !== -1) {
        try { return { script: atob(content), base: '' }; } catch(e) { return { script: '', base: '' }; }
      }
      try { return { script: decodeURIComponent(content), base: '' }; }
      catch(e) { return { script: content, base: '' }; }
    }
    // External URL: resolve against the document base and fetch the script
    // body synchronously (previously never hit the network at all).
    var abs = _url_resolve(u, _lumen_document_base_url());
    var fetched = _lumen_sw_fetch_script(abs);
    return { script: (typeof fetched === 'string') ? fetched : null, base: abs };
  }

  // A port for a SharedWorker whose script failed to fetch: never delivers
  // or accepts anything, matching the "worker never started" outcome.
  function _makeDeadClientPort() {
    return {
      onmessage: null,
      postMessage: function() {},
      start: function() {},
      close: function() {},
      addEventListener: function() {},
      removeEventListener: function() {},
    };
  }

  function _makeClientPort(pid, key) {
    var port = {
      _pid: pid,
      _key: key,
      _onmessage: null,
      _listeners: [],
      postMessage: function(data) { _lumen_sw_post(this._key, this._pid, JSON.stringify(data)); },
      start: function() {},   // auto-started: Lumen always delivers
      close: function() { _lumen_sw_close(this._key, this._pid); },
      addEventListener: function(type, fn) {
        if (type === 'message' && typeof fn === 'function') this._listeners.push(fn);
      },
      removeEventListener: function(type, fn) {
        if (type === 'message') {
          var i = this._listeners.indexOf(fn);
          if (i !== -1) this._listeners.splice(i, 1);
        }
      },
      _deliver: function(json) {
        var data;
        try { data = JSON.parse(json); } catch(e) { data = json; }
        var ev = { data: data, type: 'message', target: this,
                   bubbles: false, cancelable: false, ports: [] };
        if (this._onmessage) { try { this._onmessage(ev); } catch(e) {} }
        for (var i = 0; i < this._listeners.length; i++) {
          try { this._listeners[i](ev); } catch(e) {}
        }
      },
    };
    Object.defineProperty(port, 'onmessage', {
      get: function() { return this._onmessage; },
      set: function(fn) { this._onmessage = typeof fn === 'function' ? fn : null; },
      configurable: true,
    });
    return port;
  }

  function SharedWorker(url, name) {
    var nm = (name === undefined || name === null) ? '' : String(name);
    // Identity key: name when present, else the URL (single-origin process).
    var key = nm ? ('name:' + nm) : ('url:' + String(url || ''));
    this._onerror = null;
    this._errorListeners = [];
    var resolved = _resolveScript(url);
    if (resolved.script === null) {
      // Script fetch failed (BUG-364): never connect, only fire `error`.
      this.port = _makeDeadClientPort();
      var self = this;
      var u = String(url || '');
      setTimeout(function() {
        self._deliverError({
          message: 'SharedWorker script failed to load: ' + u,
          filename: u, lineno: 0, colno: 0,
        });
      }, 0);
      return;
    }
    var pid = _lumen_sw_connect(key, resolved.script, resolved.base);
    this.port = _makeClientPort(pid, key);
    _clientPorts[pid] = this.port;
    _sharedWorkerInstances[pid] = this;
  }

  Object.defineProperty(SharedWorker.prototype, 'onerror', {
    get: function() { return this._onerror; },
    set: function(fn) { this._onerror = typeof fn === 'function' ? fn : null; },
    configurable: true,
  });

  SharedWorker.prototype.addEventListener = function(type, fn, _opts) {
    if (type === 'error' && typeof fn === 'function') this._errorListeners.push(fn);
  };

  SharedWorker.prototype.removeEventListener = function(type, fn) {
    if (type === 'error') {
      var i = this._errorListeners.indexOf(fn);
      if (i !== -1) this._errorListeners.splice(i, 1);
    }
  };

  // Internal: fire `error` at this SharedWorker instance — used both for the
  // script-fetch-failure case above and for a genuine uncaught exception in
  // the worker's global scope (BUG-591), delivered via
  // `_lumen_deliver_shared_worker_errors` below. `info` is a plain object
  // ({message, filename, lineno, colno}), not yet an `ErrorEvent`.
  SharedWorker.prototype._deliverError = function(info) {
    var ev = new ErrorEvent('error', {
      message: String((info && info.message) || ''),
      filename: String((info && info.filename) || ''),
      lineno: (info && info.lineno) | 0,
      colno: (info && info.colno) | 0,
      bubbles: false, cancelable: true,
    });
    if (typeof this._onerror === 'function') { try { this._onerror(ev); } catch(e) {} }
    for (var i = 0; i < this._errorListeners.length; i++) {
      try { this._errorListeners[i](ev); } catch(e) {}
    }
  };

  globalThis.SharedWorker = SharedWorker;
  if (typeof window !== 'undefined') window.SharedWorker = SharedWorker;

  // Called by the page runtime's pump_shared_workers() with [{ id, json }, …].
  globalThis._lumen_deliver_shared_worker_messages = function(msgs) {
    for (var i = 0; i < msgs.length; i++) {
      var m = msgs[i];
      var p = _clientPorts[m.id];
      if (p) p._deliver(m.json);
    }
  };

  // Called by the page runtime's pump_shared_workers() with [{ id, json }, …]
  // of uncaught-exception reports (BUG-591 SharedWorker parent-side
  // reporting) — `json` is the `{message, filename, lineno, colno}` object
  // literal `_lumen_sw_report_error` built on the worker side.
  globalThis._lumen_deliver_shared_worker_errors = function(errs) {
    for (var i = 0; i < errs.length; i++) {
      var m = errs[i];
      var w = _sharedWorkerInstances[m.id];
      if (w) w._deliverError(m.json);
    }
  };
})();
"#;

// ─── V8 backend port (Ph3 V8 migration S10) ──────────────────────────────────
//
// Mirrors `worker.rs`'s V8 port: each shared-worker thread owns a full
// `V8JsRuntime` (its own OS thread + isolate). The rquickjs twin (`HUB`,
// `connect_shared_worker`, `run_shared_worker_thread`, …) was removed in
// S12b-B18 — `HUB_V8` below is now the only registry.

/// Process-global registry of live shared workers, keyed by identity string.
///
/// The identity key is `name` when a non-empty name is given, otherwise the
/// resolved script URL — matching the WHATWG "name or URL" identity rule for
/// a single-origin process.
#[cfg(feature = "v8-backend")]
static HUB_V8: OnceLock<Mutex<HashMap<String, SharedWorkerThread>>> = OnceLock::new();

#[cfg(feature = "v8-backend")]
fn hub_v8() -> &'static Mutex<HashMap<String, SharedWorkerThread>> {
    HUB_V8.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Connect a new client to the shared worker identified by `key`.
///
/// Spawns the worker thread (evaluating `script`) on first connection; reuses
/// the existing thread on later connections.  `outbox` is the connecting
/// runtime's outbound queue: the worker pushes replies for this port into it.
/// `errors` is the connecting runtime's uncaught-exception queue (BUG-591):
/// registered the same way so a later worker-scope exception can be
/// broadcast to this client too.
///
/// Returns the freshly-allocated, process-unique port id.
#[cfg(feature = "v8-backend")]
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
#[allow(clippy::too_many_arguments)]  // spawn-time state threaded to a fresh worker thread, same shape as worker.rs's spawn_worker_v8
fn connect_shared_worker_v8(
    key: String,
    script: String,
    base_url: String,
    outbox: SharedWorkerOutbox,
    errors: crate::worker::WorkerErrorQueue,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
) -> u32 {
    let port_id = PORT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut map = hub_v8().lock().unwrap();

    let spawn = |key: String, script: String, base_url: String| -> SharedWorkerThread {
        let (tx, rx) = mpsc::channel::<SwInMsg>();
        let fp = fetch_provider.clone();
        let thread = thread::Builder::new()
            .name(format!("lumen-shared-worker-v8-{key}"))
            .spawn(move || run_shared_worker_thread_v8(script, base_url, rx, fp))
            .expect("failed to spawn SharedWorker thread (v8)");
        SharedWorkerThread { tx, _thread: thread }
    };

    let entry = map
        .entry(key.clone())
        .or_insert_with(|| spawn(key.clone(), script.clone(), base_url.clone()));

    let connect = SwInMsg::Connect { port_id, outbox: Arc::clone(&outbox), errors: Arc::clone(&errors) };
    if entry.tx.send(connect).is_err() {
        let fresh = spawn(key.clone(), script, base_url);
        let _ = fresh.tx.send(SwInMsg::Connect { port_id, outbox, errors });
        *entry = fresh;
    }
    port_id
}

/// Forward a client `port.postMessage(data)` to the shared-worker thread.
///
/// No-op if `key` has no live worker (e.g. it already exited).
#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn post_to_shared_worker_v8(key: &str, port_id: u32, json: String) {
    if let Some(t) = hub_v8().lock().unwrap().get(key) {
        let _ = t.tx.send(SwInMsg::Post { port_id, json });
    }
}

/// Notify the shared worker that a client closed its port.
///
/// The worker-side port mapping is dropped; the worker thread itself stays
/// alive for other clients (shared workers outlive individual connections).
#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn close_shared_worker_port_v8(key: &str, port_id: u32) {
    if let Some(t) = hub_v8().lock().unwrap().get(key) {
        let _ = t.tx.send(SwInMsg::Close { port_id });
    }
}

/// Install the `_lumen_sw_connect` / `_lumen_sw_post` / `_lumen_sw_close`
/// native bindings and the `SharedWorker` JS class into a V8 context.
///
/// Must be called after the core DOM shim so that `TextDecoder`,
/// `_object_url_store`, and `atob` are available for blob-/data-URL
/// resolution in the constructor.  `outbox` is this runtime's outbound queue;
/// `errors` is this runtime's shared-worker uncaught-exception queue
/// (BUG-591), drained by [`crate::v8_runtime::V8JsRuntime::pump_shared_workers`].
#[cfg(feature = "v8-backend")]
pub(crate) fn install_shared_worker_bindings_v8(
    rt: &V8JsRuntime,
    outbox: &SharedWorkerOutbox,
    errors: &crate::worker::WorkerErrorQueue,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
) -> JsResult<()> {
    // _lumen_sw_connect(key, script, base_url) → u32
    //
    // `base_url` (BUG-778) is the resolved worker script URL (empty for a
    // blob:/data: worker) — only used for the connection that actually
    // spawns the thread; later reconnects to an already-live worker ignore
    // it, matching the pre-existing `script`-is-ignored-on-reuse contract.
    {
        let out = Arc::clone(outbox);
        let errs = Arc::clone(errors);
        let fp = fetch_provider.clone();
        rt.register_native(
            "_lumen_sw_connect",
            into_v8_fn3(move |key: String, script: String, base_url: String| -> u32 {
                connect_shared_worker_v8(key, script, base_url, Arc::clone(&out), Arc::clone(&errs), fp.clone())
            }),
        )?;
    }

    // _lumen_sw_fetch_script(url: String) → String | undefined
    //
    // Synchronous GET for an external SharedWorker script (BUG-364), mirroring
    // `worker.rs::fetch_worker_script`. `undefined` on network error / non-2xx
    // status tells the JS shim to fire `error` instead of connecting.
    {
        let fp = fetch_provider.clone();
        rt.register_native(
            "_lumen_sw_fetch_script",
            into_v8_fn1(move |url: String| -> Option<String> {
                crate::worker::fetch_worker_script(fp.as_deref(), &url)
            }),
        )?;
    }

    rt.register_native(
        "_lumen_sw_post",
        into_v8_fn3(move |key: String, port_id: u32, json: String| {
            post_to_shared_worker_v8(&key, port_id, json);
        }),
    )?;

    rt.register_native(
        "_lumen_sw_close",
        into_v8_fn2(move |key: String, port_id: u32| {
            close_shared_worker_port_v8(&key, port_id);
        }),
    )?;

    rt.eval(SHARED_WORKER_SHIM)?;
    Ok(())
}

/// Worker-thread event loop: installs the worker global scope, evaluates the
/// worker script, then services `Connect`/`Post`/`Close` messages until the
/// channel closes.
#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn run_shared_worker_thread_v8(
    script: String,
    base_url: String,
    rx: Receiver<SwInMsg>,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
) {
    let rt = match V8JsRuntime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[shared-worker] v8 runtime init failed: {e}");
            return;
        }
    };

    let ports: Arc<Mutex<HashMap<u32, SharedWorkerOutbox>>> = Arc::new(Mutex::new(HashMap::new()));
    // Broadcast target for uncaught exceptions (BUG-591): every currently
    // connected client's own error queue, keyed the same way as `ports`.
    let error_ports: Arc<Mutex<HashMap<u32, crate::worker::WorkerErrorQueue>>> =
        Arc::new(Mutex::new(HashMap::new()));
    // BUG-778 `self.close()`: flipped from inside the worker script; polled
    // below to stop servicing further Connect/Post tasks (HTML LS §10.2.4).
    let close_flag: crate::worker::WorkerCloseFlag = Arc::new(AtomicBool::new(false));

    if let Err(e) = install_shared_worker_globals_v8(
        &rt,
        Arc::clone(&ports),
        Arc::clone(&error_ports),
        fetch_provider,
        &base_url,
        Arc::clone(&close_flag),
    ) {
        eprintln!("[shared-worker] v8 globals install failed: {e:?}");
        return;
    }

    if let Err(e) = rt.eval(&script) {
        eprintln!("[shared-worker] v8 script error: {e:?}");
        // BUG-591 SharedWorker parent-side reporting: broadcast the top-level
        // failure to every already-connected client (there may be none yet —
        // the first client's own Connect below still races this eval).
        let message = match &e {
            lumen_core::JsError::Parse(m) | lumen_core::JsError::Runtime(m) => m.clone(),
            lumen_core::JsError::NotImplemented => "not implemented".to_string(),
        };
        broadcast_shared_worker_error(&error_ports, &message, "", 0, 0);
        // Continue: the worker may still service connections if the error was partial.
    }

    // BUG-778 "close a worker": discard further queued tasks (including a
    // connect from a fresh client) once `self.close()` ran, same shape as
    // `worker.rs::run_worker_thread_v8`'s dedicated-worker loop.
    while !close_flag.load(Ordering::Relaxed) {
        let Ok(msg) = rx.recv() else { break };
        match msg {
            SwInMsg::Connect { port_id, outbox, errors } => {
                ports.lock().unwrap().insert(port_id, outbox);
                error_ports.lock().unwrap().insert(port_id, errors);
                let _ = rt.eval(&format!(
                    "if(typeof _lumen_sw_dispatch_connect==='function')\
                     {{_lumen_sw_dispatch_connect({port_id});\
                      if(typeof _lumen_flush_timers==='function')_lumen_flush_timers();}}"
                ));
            }
            SwInMsg::Post { port_id, json } => {
                if rt
                    .set_global("_sw_msg__", lumen_core::JsValue::String(json))
                    .is_ok()
                {
                    let _ = rt.eval(&format!(
                        "if(typeof _lumen_sw_dispatch_port_message==='function')\
                         {{_lumen_sw_dispatch_port_message({port_id},JSON.parse(_sw_msg__));\
                          if(typeof _lumen_flush_timers==='function')_lumen_flush_timers();}}"
                    ));
                }
            }
            SwInMsg::Close { port_id } => {
                ports.lock().unwrap().remove(&port_id);
                error_ports.lock().unwrap().remove(&port_id);
                let _ = rt.eval(&format!(
                    "if(typeof _lumen_sw_dispatch_port_close==='function')\
                     _lumen_sw_dispatch_port_close({port_id});"
                ));
            }
        }
    }
    // `rt` drops here: `V8JsRuntime::drop` sends `Shutdown` and joins its thread.
}

/// Push an uncaught-exception report onto every currently-connected client's
/// error queue (BUG-591 SharedWorker parent-side reporting — HTML LS "report
/// the exception" fires `error` at *every* `SharedWorker` object owning a
/// port into this worker, not just the one that triggered it).
#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn broadcast_shared_worker_error(
    error_ports: &Mutex<HashMap<u32, crate::worker::WorkerErrorQueue>>,
    message: &str,
    filename: &str,
    lineno: i32,
    colno: i32,
) {
    let info = crate::worker::error_info_json(message, filename, lineno, colno);
    for (port_id, errors) in error_ports.lock().unwrap().iter() {
        errors.lock().unwrap().push((*port_id, info.clone()));
    }
}

/// Install the shared-worker global scope (`SharedWorkerGlobalScope`-like).
///
/// Registers `_lumen_sw_port_reply` / `_lumen_sw_console_log` (both plain
/// String/u32 natives — no scoped mechanism needed, unlike `worker.rs`'s
/// throwing `atob`/`btoa`), `_lumen_sw_report_error` (BUG-591 — broadcasts an
/// uncaught-exception report to every connected client via `error_ports`),
/// `_lumen_worker_self_close`/`_lumen_worker_net_fetch`/
/// `_lumen_import_scripts_resolve` (BUG-778 — the same three natives the
/// dedicated worker registers, reusing [`crate::worker::worker_net_fetch_json`]/
/// [`crate::worker::resolve_import_url`] rather than a second implementation),
/// sets the `_lumen_worker_base_url` global, and evaluates
/// [`SHARED_WORKER_GLOBAL_SHIM`] followed by [`crate::worker::WORKER_NET_SHIM`].
///
/// `importScripts('blob:lumen/…')` is not supported here (`None` is passed as
/// the blob store — always empty): unlike a dedicated [`crate::worker`], a
/// shared worker has no per-instance blob mirroring from the connecting
/// page's `_object_url_store`. `data:`/`http(s):` targets — the shape that
/// matters for the WPT-RUN-6 `.any.sharedworker.html` wrapper's own
/// `importScripts("/resources/testharness.js")` — work either way.
#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn install_shared_worker_globals_v8(
    rt: &V8JsRuntime,
    ports: Arc<Mutex<HashMap<u32, SharedWorkerOutbox>>>,
    error_ports: Arc<Mutex<HashMap<u32, crate::worker::WorkerErrorQueue>>>,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    base_url: &str,
    close_flag: crate::worker::WorkerCloseFlag,
) -> JsResult<()> {
    rt.register_native(
        "_lumen_sw_port_reply",
        into_v8_fn2(move |port_id: u32, json: String| {
            if let Some(outbox) = ports.lock().unwrap().get(&port_id) {
                outbox.lock().unwrap().push((port_id, json));
            }
        }),
    )?;

    rt.register_native(
        "_lumen_sw_console_log",
        into_v8_fn1(move |msg: String| {
            eprintln!("[shared-worker] {msg}");
        }),
    )?;

    // _lumen_sw_report_error(message, filename, lineno, colno) — called by
    // `SHARED_WORKER_GLOBAL_SHIM`'s `_lumen_sw_report_exception` for an
    // uncaught exception from `onconnect`, a port's `onmessage`, or a
    // flushed timer callback (BUG-591 SharedWorker parent-side reporting).
    rt.register_native(
        "_lumen_sw_report_error",
        into_v8_fn4(
            move |message: String, filename: String, lineno: i32, colno: i32| {
                broadcast_shared_worker_error(&error_ports, &message, &filename, lineno, colno);
            },
        ),
    )?;

    // _lumen_worker_self_close() — BUG-778 `self.close()`.
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
    // BUG-778: backs `fetch()`/`XMLHttpRequest` (`crate::worker::WORKER_NET_SHIM`).
    {
        let fp = fetch_provider.clone();
        rt.register_native(
            "_lumen_worker_net_fetch",
            into_v8_fn4(
                move |url: String, method: String, headers_flat: Vec<String>, body_b64: Option<String>| -> Option<String> {
                    crate::worker::worker_net_fetch_json(fp.as_deref(), &url, &method, &headers_flat, body_b64.as_deref())
                },
            ),
        )?;
    }

    // _lumen_import_scripts_resolve(url) → String | undefined — BUG-778, see
    // this function's own doc comment on the `blob:lumen/` limitation.
    {
        let no_blobs: crate::worker::WorkerBlobStore = Arc::new(Mutex::new(HashMap::new()));
        let fp = fetch_provider;
        rt.register_native(
            "_lumen_import_scripts_resolve",
            into_v8_fn1(move |url: String| -> Option<String> {
                crate::worker::resolve_import_url(&url, &no_blobs, fp.as_deref())
            }),
        )?;
    }

    // `SharedWorkerGlobalScope` is a `WorkerGlobalScope` too, so it gets the
    // same `EventTarget`/`performance` surface as the dedicated worker (BUG-401).
    crate::worker::install_worker_scope_globals_v8(rt)?;

    // BUG-778: read by both `SHARED_WORKER_GLOBAL_SHIM`'s `importScripts` and
    // `WORKER_NET_SHIM`'s `fetch`/`XMLHttpRequest` — set before either evaluates.
    rt.set_global("_lumen_worker_base_url", lumen_core::JsValue::String(base_url.to_string()))?;

    rt.eval(SHARED_WORKER_GLOBAL_SHIM)?;
    rt.eval(crate::worker::WORKER_NET_SHIM)?;
    Ok(())
}

/// Percent-encode the few characters that break a `data:` URL passed inline
/// in an `eval` string (spaces, `+`, `%`, `&`, `#`). Used by [`tests_v8`].
#[cfg(all(test, feature = "v8-backend"))]
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b' ' => out.push_str("%20"),
            b'+' => out.push_str("%2B"),
            b'%' => out.push_str("%25"),
            b'&' => out.push_str("%26"),
            b'#' => out.push_str("%23"),
            b'\'' => out.push_str("%27"),
            _ => out.push(b as char),
        }
    }
    out
}

/// V8 test coverage for shared workers (the rquickjs twin was removed in
/// S12b-B18; this module ports its 6 tests to V8). Covers the shim install
/// and the behaviors specific to shared workers: identity-keyed thread reuse
/// (two clients share one worker) and isolation across distinct names — both
/// exercised end-to-end through a real `V8JsRuntime` per client and per
/// worker thread.
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use lumen_core::JsValue;

    fn as_num(v: &JsValue) -> f64 {
        match v {
            JsValue::Number(n) => *n,
            JsValue::Bool(b) => i32::from(*b) as f64,
            _ => f64::NAN,
        }
    }

    /// BUG-401: `SharedWorkerGlobalScope` is a `WorkerGlobalScope`, so it gets
    /// the same `performance` the dedicated worker and the page do — the same
    /// shim source, hence the same prototype chain, not a look-alike.
    #[test]
    fn shared_worker_global_scope_has_performance() {
        let rt = V8JsRuntime::new().unwrap();
        let ports = Arc::new(Mutex::new(HashMap::new()));
        let error_ports = Arc::new(Mutex::new(HashMap::new()));
        install_shared_worker_globals_v8(&rt, ports, error_ports, None, "", Arc::new(AtomicBool::new(false))).unwrap();
        for expr in [
            "typeof performance.now === 'function'",
            "performance instanceof Performance",
            "Object.getPrototypeOf(Performance.prototype) === EventTarget.prototype",
        ] {
            assert_eq!(rt.eval(expr).unwrap(), JsValue::Bool(true), "{expr}");
        }
    }

    fn runtime_with_shared_worker() -> (V8JsRuntime, SharedWorkerOutbox) {
        let rt = V8JsRuntime::new().unwrap();
        let outbox: SharedWorkerOutbox = Arc::new(Mutex::new(Vec::new()));
        let errors: crate::worker::WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_shared_worker_bindings_v8(&rt, &outbox, &errors, None).unwrap();
        (rt, outbox)
    }

    /// Pump `rt`'s outbox until `count_expr` evaluates to `>= expected`, or the
    /// budget is exhausted (worker threads are async; give them time).
    fn pump_until(rt: &V8JsRuntime, outbox: &SharedWorkerOutbox, count_expr: &str, expected: f64) {
        for _ in 0..400 {
            let msgs = drain_messages(outbox);
            if !msgs.is_empty() {
                let json = crate::build_worker_messages_json(&msgs);
                let _ = rt.eval(&format!(
                    "if(typeof _lumen_deliver_shared_worker_messages==='function')\
                     _lumen_deliver_shared_worker_messages({json})"
                ));
            }
            let n = rt.eval(count_expr).map(|v| as_num(&v)).unwrap_or(0.0);
            if n >= expected {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn v8_shared_worker_class_exists() {
        let (rt, _outbox) = runtime_with_shared_worker();
        assert_eq!(
            rt.eval("typeof SharedWorker === 'function'").unwrap(),
            JsValue::Bool(true)
        );
    }

    #[test]
    fn v8_port_is_messageport_like() {
        let (rt, _outbox) = runtime_with_shared_worker();
        let v = rt
            .eval(
                "var w = new SharedWorker('data:text/javascript,/*noop*/', 'v8-idle');\
                 (typeof w.port.postMessage==='function' && \
                  typeof w.port.start==='function' && \
                  typeof w.port.close==='function')",
            )
            .unwrap();
        assert_eq!(v, JsValue::Bool(true));
    }

    #[test]
    fn v8_connect_event_and_echo() {
        let (rt, outbox) = runtime_with_shared_worker();
        let script = "onconnect=function(e){var p=e.ports[0];\
            p.onmessage=function(ev){p.postMessage('echo:'+ev.data);};};";
        let data_url = format!("data:text/javascript,{}", urlencode(script));
        rt.eval(&format!(
            "globalThis.__got=null;\
             var w=new SharedWorker('{data_url}','v8-echo-1');\
             w.port.onmessage=function(ev){{globalThis.__got=ev.data;}};\
             w.port.postMessage('hello');"
        ))
        .unwrap();

        pump_until(&rt, &outbox, "globalThis.__got===null?0:1", 1.0);
        assert_eq!(
            rt.eval("String(globalThis.__got)").unwrap(),
            JsValue::String("echo:hello".into())
        );
    }

    #[test]
    fn v8_two_clients_share_one_worker() {
        let (rt, outbox) = runtime_with_shared_worker();
        let script = "var n=0;onconnect=function(e){var p=e.ports[0];\
            p.onmessage=function(){n+=1;p.postMessage(n);};};";
        let data_url = format!("data:text/javascript,{}", urlencode(script));
        rt.eval(&format!(
            "globalThis.__a=0;globalThis.__b=0;\
             globalThis.__a2=new SharedWorker('{data_url}','v8-shared-counter');\
             globalThis.__b2=new SharedWorker('{data_url}','v8-shared-counter');\
             __a2.port.onmessage=function(ev){{globalThis.__a=ev.data;}};\
             __b2.port.onmessage=function(ev){{globalThis.__b=ev.data;}};\
             __a2.port.postMessage(0);"
        ))
        .unwrap();
        pump_until(&rt, &outbox, "globalThis.__a", 1.0);
        rt.eval("__b2.port.postMessage(0);").unwrap();
        pump_until(&rt, &outbox, "globalThis.__b", 2.0);

        assert_eq!(as_num(&rt.eval("globalThis.__a").unwrap()), 1.0);
        assert_eq!(as_num(&rt.eval("globalThis.__b").unwrap()), 2.0);
    }

    #[test]
    fn v8_distinct_names_are_isolated() {
        let (rt, outbox) = runtime_with_shared_worker();
        let script = "var n=0;onconnect=function(e){var p=e.ports[0];\
            p.onmessage=function(){n+=1;p.postMessage(n);};};";
        let data_url = format!("data:text/javascript,{}", urlencode(script));
        rt.eval(&format!(
            "globalThis.__x=0;globalThis.__y=0;\
             globalThis.__x2=new SharedWorker('{data_url}','v8-iso-x');\
             globalThis.__y2=new SharedWorker('{data_url}','v8-iso-y');\
             __x2.port.onmessage=function(ev){{globalThis.__x=ev.data;}};\
             __y2.port.onmessage=function(ev){{globalThis.__y=ev.data;}};\
             __x2.port.postMessage(0);__x2.port.postMessage(0);__y2.port.postMessage(0);"
        ))
        .unwrap();
        pump_until(&rt, &outbox, "globalThis.__x", 2.0);
        pump_until(&rt, &outbox, "globalThis.__y", 1.0);

        // x bumped twice (=2), y bumped once (=1): separate counters.
        assert_eq!(as_num(&rt.eval("globalThis.__x").unwrap()), 2.0);
        assert_eq!(as_num(&rt.eval("globalThis.__y").unwrap()), 1.0);
    }

    #[test]
    fn v8_drain_messages_empties_outbox() {
        let outbox: SharedWorkerOutbox = Arc::new(Mutex::new(vec![(1, "\"a\"".into())]));
        let drained = drain_messages(&outbox);
        assert_eq!(drained.len(), 1);
        assert!(drain_messages(&outbox).is_empty());
    }

    // ── BUG-778: close() / fetch() / XMLHttpRequest / importScripts(http) ──────

    /// Minimal `JsFetchProvider` double, mirroring `worker::tests_v8::TestNet`
    /// (kept local rather than shared — small, and the two test modules have
    /// no other coupling).
    struct SwTestNet {
        bodies: HashMap<String, String>,
    }
    impl SwTestNet {
        fn new(bodies: &[(&str, &str)]) -> Arc<Self> {
            Arc::new(Self {
                bodies: bodies.iter().map(|(u, b)| ((*u).to_string(), (*b).to_string())).collect(),
            })
        }
    }
    impl lumen_core::ext::JsFetchProvider for SwTestNet {
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

    fn runtime_with_shared_worker_net(fp: Arc<SwTestNet>) -> (V8JsRuntime, SharedWorkerOutbox) {
        let rt = V8JsRuntime::new().unwrap();
        let outbox: SharedWorkerOutbox = Arc::new(Mutex::new(Vec::new()));
        let errors: crate::worker::WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
        install_shared_worker_bindings_v8(&rt, &outbox, &errors, Some(fp)).unwrap();
        (rt, outbox)
    }

    /// `self.close()` — HTML LS §10.2.4 "close a worker" for a shared
    /// worker: a message sent to an already-connected port after the worker
    /// closes itself gets no further reply.
    #[test]
    fn v8_shared_worker_close_stops_further_messages() {
        use std::time::Duration;
        let (rt, outbox) = runtime_with_shared_worker();
        let script = "onconnect=function(e){var p=e.ports[0];\
            p.onmessage=function(ev){p.postMessage('got:'+ev.data);self.close();};};";
        let data_url = format!("data:text/javascript,{}", urlencode(script));
        rt.eval(&format!(
            "globalThis.__got=[];\
             var wc=new SharedWorker('{data_url}','v8-close-1');\
             wc.port.onmessage=function(ev){{globalThis.__got.push(ev.data);}};\
             wc.port.postMessage('1');"
        ))
        .unwrap();
        pump_until(&rt, &outbox, "globalThis.__got.length", 1.0);

        rt.eval("wc.port.postMessage('2');").unwrap();
        std::thread::sleep(Duration::from_millis(200));

        assert!(drain_messages(&outbox).is_empty(), "no reply expected after close");
        assert_eq!(as_num(&rt.eval("globalThis.__got.length").unwrap()), 1.0);
    }

    /// `typeof fetch/XMLHttpRequest/close === 'function'` inside a
    /// `SharedWorkerGlobalScope` (BUG-778: previously `importScripts` was the
    /// only one of the four even attempted, and it unconditionally threw).
    #[test]
    fn v8_shared_worker_global_scope_has_fetch_xhr_close() {
        let rt = V8JsRuntime::new().unwrap();
        let ports = Arc::new(Mutex::new(HashMap::new()));
        let error_ports = Arc::new(Mutex::new(HashMap::new()));
        install_shared_worker_globals_v8(&rt, ports, error_ports, None, "", Arc::new(AtomicBool::new(false))).unwrap();
        for expr in ["typeof fetch", "typeof XMLHttpRequest", "typeof close", "typeof Headers", "typeof Response"] {
            assert_eq!(rt.eval(expr).unwrap(), JsValue::String("function".into()), "{expr}");
        }
    }

    /// End-to-end `fetch()` from inside a connected client's `onmessage`
    /// handler, real network bridge via [`SwTestNet`].
    #[test]
    fn v8_shared_worker_fetch_reaches_provider() {
        let net = SwTestNet::new(&[("https://example.test/shared-fetch", "shared fetch body")]);
        let (rt, outbox) = runtime_with_shared_worker_net(net);
        let script = "onconnect=function(e){var p=e.ports[0];\
            p.onmessage=function(ev){\
              fetch('https://example.test/shared-fetch').then(function(r){return r.text();})\
                .then(function(t){p.postMessage(t);});\
            };};";
        let data_url = format!("data:text/javascript,{}", urlencode(script));
        rt.eval(&format!(
            "globalThis.__got=null;\
             var wf=new SharedWorker('{data_url}','v8-net-fetch-1');\
             wf.port.onmessage=function(ev){{globalThis.__got=ev.data;}};\
             wf.port.postMessage('go');"
        ))
        .unwrap();
        pump_until(&rt, &outbox, "globalThis.__got===null?0:1", 1.0);
        assert_eq!(
            rt.eval("String(globalThis.__got)").unwrap(),
            JsValue::String("shared fetch body".into())
        );
    }

    /// Synchronous `XMLHttpRequest` from inside a connected client's
    /// `onmessage` handler, over the same bridge.
    #[test]
    fn v8_shared_worker_xhr_reaches_provider() {
        let net = SwTestNet::new(&[("https://example.test/shared-xhr", "shared xhr body")]);
        let (rt, outbox) = runtime_with_shared_worker_net(net);
        let script = "onconnect=function(e){var p=e.ports[0];\
            p.onmessage=function(ev){\
              var x=new XMLHttpRequest();\
              x.open('GET','https://example.test/shared-xhr');\
              x.onload=function(){p.postMessage(x.status+':'+x.responseText);};\
              x.send();\
            };};";
        let data_url = format!("data:text/javascript,{}", urlencode(script));
        rt.eval(&format!(
            "globalThis.__got=null;\
             var wx=new SharedWorker('{data_url}','v8-net-xhr-1');\
             wx.port.onmessage=function(ev){{globalThis.__got=ev.data;}};\
             wx.port.postMessage('go');"
        ))
        .unwrap();
        pump_until(&rt, &outbox, "globalThis.__got===null?0:1", 1.0);
        assert_eq!(
            rt.eval("String(globalThis.__got)").unwrap(),
            JsValue::String("200:shared xhr body".into())
        );
    }

    /// BUG-778's WPT-RUN-6 extension for `.any.sharedworker.html`:
    /// `importScripts()` with a path-absolute URL, resolved against the
    /// worker's own `base_url` and fetched over the network.
    #[test]
    fn v8_shared_worker_import_scripts_path_absolute_resolves_against_base_url() {
        let rt = V8JsRuntime::new().unwrap();
        let ports = Arc::new(Mutex::new(HashMap::new()));
        let error_ports = Arc::new(Mutex::new(HashMap::new()));
        let net = SwTestNet::new(&[("https://example.test/resources/testharness.js", "globalThis._sms1 = 40;")]);
        install_shared_worker_globals_v8(
            &rt, ports, error_ports, Some(net), "https://example.test/worker.js", Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        rt.eval("importScripts('/resources/testharness.js')").unwrap();
        assert_eq!(rt.eval("_sms1").unwrap(), JsValue::Number(40.0));
    }
}
