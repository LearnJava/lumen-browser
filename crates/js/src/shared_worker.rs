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

use std::sync::{Arc, Mutex};

#[cfg(feature = "v8-backend")]
use std::collections::HashMap;
#[cfg(feature = "v8-backend")]
use std::sync::OnceLock;
#[cfg(feature = "v8-backend")]
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(feature = "v8-backend")]
use std::sync::mpsc::{self, Receiver, Sender};
#[cfg(feature = "v8-backend")]
use std::thread;

#[cfg(feature = "v8-backend")]
use crate::v8_compat::{into_v8_fn1, into_v8_fn2, into_v8_fn3};
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
    /// A new client connected: register `port_id` → its `outbox`, then fire the
    /// `connect` event in the worker with a worker-side port for `port_id`.
    Connect { port_id: u32, outbox: SharedWorkerOutbox },
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

  // Called by the Rust loop when a new client connects.
  globalThis._lumen_sw_dispatch_connect = function(pid) {
    var port = _makePort(pid);
    _ports[pid] = port;
    var ev = { type: 'connect', target: globalThis, source: port,
               ports: [port], bubbles: false, cancelable: false };
    if (_onconnect) { try { _onconnect(ev); } catch(e) {} }
    for (var i = 0; i < _connectListeners.length; i++) {
      try { _connectListeners[i](ev); } catch(e) {}
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

  globalThis.importScripts = function() {
    throw new Error('importScripts is not supported');
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
      try { pending[i].fn(); } catch(e) {}
    }
  };
})();
"#;

/// Main-thread `SharedWorker` class shim (evaluated in the page JS context).
///
/// Depends on the `_lumen_sw_connect` / `_lumen_sw_post` / `_lumen_sw_close`
/// native bindings, plus `_object_url_store` / `TextDecoder` / `atob` from the
/// core DOM shim for blob-/data-URL script resolution.
#[cfg(feature = "v8-backend")]
const SHARED_WORKER_SHIM: &str = r#"(function() {
  var _clientPorts = {};   // port_id → client-side MessagePort

  // Returns the script body as a String, or `null` when an external URL's
  // network fetch failed (BUG-364) — the caller must not spawn a worker
  // thread in that case, only fire `error` on the SharedWorker instance.
  function _resolveScript(url) {
    var u = String(url || '');
    if (u.startsWith('blob:lumen/')) {
      var blob = (typeof _object_url_store !== 'undefined') ? _object_url_store[u] : null;
      if (blob && blob._bytes) {
        try { return new TextDecoder().decode(blob._bytes); } catch(e) { return ''; }
      }
      return '';
    }
    if (u.startsWith('data:')) {
      var comma = u.indexOf(',');
      if (comma === -1) return '';
      var meta = u.slice(5, comma), content = u.slice(comma + 1);
      if (meta.indexOf('base64') !== -1) {
        try { return atob(content); } catch(e) { return ''; }
      }
      try { return decodeURIComponent(content); } catch(e) { return content; }
    }
    // External URL: resolve against the document base and fetch the script
    // body synchronously (previously never hit the network at all).
    var abs = _url_resolve(u, _lumen_document_base_url());
    var fetched = _lumen_sw_fetch_script(abs);
    return (typeof fetched === 'string') ? fetched : null;
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
    this.onerror = null;
    var script = _resolveScript(url);
    if (script === null) {
      // Script fetch failed (BUG-364): never connect, only fire `error`.
      this.port = _makeDeadClientPort();
      var self = this;
      var u = String(url || '');
      setTimeout(function() {
        if (typeof self.onerror !== 'function') return;
        try {
          self.onerror(new ErrorEvent('error', {
            message: 'SharedWorker script failed to load: ' + u,
            filename: u, lineno: 0, colno: 0,
            bubbles: false, cancelable: true,
          }));
        } catch(e) {}
      }, 0);
      return;
    }
    var pid = _lumen_sw_connect(key, script);
    this.port = _makeClientPort(pid, key);
    _clientPorts[pid] = this.port;
  }

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
///
/// Returns the freshly-allocated, process-unique port id.
#[cfg(feature = "v8-backend")]
fn connect_shared_worker_v8(key: String, script: String, outbox: SharedWorkerOutbox) -> u32 {
    let port_id = PORT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut map = hub_v8().lock().unwrap();

    let spawn = |key: String, script: String| -> SharedWorkerThread {
        let (tx, rx) = mpsc::channel::<SwInMsg>();
        let thread = thread::Builder::new()
            .name(format!("lumen-shared-worker-v8-{key}"))
            .spawn(move || run_shared_worker_thread_v8(script, rx))
            .expect("failed to spawn SharedWorker thread (v8)");
        SharedWorkerThread { tx, _thread: thread }
    };

    let entry = map
        .entry(key.clone())
        .or_insert_with(|| spawn(key.clone(), script.clone()));

    let connect = SwInMsg::Connect { port_id, outbox: Arc::clone(&outbox) };
    if entry.tx.send(connect).is_err() {
        let fresh = spawn(key.clone(), script);
        let _ = fresh.tx.send(SwInMsg::Connect { port_id, outbox });
        *entry = fresh;
    }
    port_id
}

/// Forward a client `port.postMessage(data)` to the shared-worker thread.
///
/// No-op if `key` has no live worker (e.g. it already exited).
#[cfg(feature = "v8-backend")]
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
/// resolution in the constructor.  `outbox` is this runtime's outbound queue.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_shared_worker_bindings_v8(
    rt: &V8JsRuntime,
    outbox: &SharedWorkerOutbox,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
) -> JsResult<()> {
    {
        let out = Arc::clone(outbox);
        rt.register_native(
            "_lumen_sw_connect",
            into_v8_fn2(move |key: String, script: String| -> u32 {
                connect_shared_worker_v8(key, script, Arc::clone(&out))
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
fn run_shared_worker_thread_v8(script: String, rx: Receiver<SwInMsg>) {
    let rt = match V8JsRuntime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[shared-worker] v8 runtime init failed: {e}");
            return;
        }
    };

    let ports: Arc<Mutex<HashMap<u32, SharedWorkerOutbox>>> = Arc::new(Mutex::new(HashMap::new()));

    if let Err(e) = install_shared_worker_globals_v8(&rt, Arc::clone(&ports)) {
        eprintln!("[shared-worker] v8 globals install failed: {e:?}");
        return;
    }

    if let Err(e) = rt.eval(&script) {
        eprintln!("[shared-worker] v8 script error: {e:?}");
        // Continue: the worker may still service connections if the error was partial.
    }

    while let Ok(msg) = rx.recv() {
        match msg {
            SwInMsg::Connect { port_id, outbox } => {
                ports.lock().unwrap().insert(port_id, outbox);
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
                let _ = rt.eval(&format!(
                    "if(typeof _lumen_sw_dispatch_port_close==='function')\
                     _lumen_sw_dispatch_port_close({port_id});"
                ));
            }
        }
    }
    // `rt` drops here: `V8JsRuntime::drop` sends `Shutdown` and joins its thread.
}

/// Install the shared-worker global scope (`SharedWorkerGlobalScope`-like).
///
/// Registers `_lumen_sw_port_reply` / `_lumen_sw_console_log` (both plain
/// String/u32 natives — no scoped mechanism needed, unlike `worker.rs`'s
/// throwing `atob`/`btoa`) and evaluates [`SHARED_WORKER_GLOBAL_SHIM`], which
/// provides `self`, `name`, `onconnect`, `addEventListener('connect', …)`, a
/// worker-side `MessagePort` factory, the `_lumen_sw_dispatch_*` hooks the
/// Rust loop calls, `console` (→ stderr), and a minimal `setTimeout` stub.
#[cfg(feature = "v8-backend")]
fn install_shared_worker_globals_v8(
    rt: &V8JsRuntime,
    ports: Arc<Mutex<HashMap<u32, SharedWorkerOutbox>>>,
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

    rt.eval(SHARED_WORKER_GLOBAL_SHIM)?;
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
    use super::*;
    use lumen_core::JsValue;

    fn as_num(v: &JsValue) -> f64 {
        match v {
            JsValue::Number(n) => *n,
            JsValue::Bool(b) => i32::from(*b) as f64,
            _ => f64::NAN,
        }
    }

    fn runtime_with_shared_worker() -> (V8JsRuntime, SharedWorkerOutbox) {
        let rt = V8JsRuntime::new().unwrap();
        let outbox: SharedWorkerOutbox = Arc::new(Mutex::new(Vec::new()));
        install_shared_worker_bindings_v8(&rt, &outbox, None).unwrap();
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
}
