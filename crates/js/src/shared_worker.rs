//! Shared Worker implementation (WHATWG HTML §10.2, stub).
//!
//! `new SharedWorker(url, name)` connects to a single worker that is **shared**
//! between all same-process clients with the same identity key (name, or URL
//! when the name is empty).  Unlike a dedicated [`crate::worker`], a shared
//! worker is **not** spawned per call: the first connection spawns one
//! `std::thread` with its own QuickJS `Runtime`; later connections (from any
//! page / `QuickJsRuntime`) reuse it and only register a fresh `MessagePort`.
//!
//! Identity & lifetime are therefore process-global: a [`HUB`] keyed by the
//! identity string maps to the live worker thread.  Each connection is assigned
//! a globally-unique **port id** ([`PORT_COUNTER`]).
//!
//! **Client → worker:** `port.postMessage(data)` → `_lumen_sw_post(key, pid,
//! json)` → [`SwInMsg::Post`] over the worker's `mpsc` channel.
//!
//! **Worker → client:** inside the worker, `connectEvent.ports[0].postMessage`
//! → `_lumen_sw_port_reply(pid, json)`, which looks up the *connecting client's*
//! outbox (registered at connect time) and pushes `(pid, json)`.  Each
//! `QuickJsRuntime` owns one outbox; it drains its own messages on every
//! event-loop tick via `QuickJsRuntime::pump_shared_workers()`, which calls
//! `_lumen_deliver_shared_worker_messages(msgs)` to route each payload to the
//! matching client `port` by id.
//!
//! GLSL-free, network-free stub: external (`http(s):`) script URLs are not
//! fetched; only `blob:` / `data:` scripts execute (resolution happens in the
//! JS shim, identical to the dedicated-worker resolver).

use rquickjs::{Context, Function, Runtime};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

// ─── shared types ───────────────────────────────────────────────────────────────

/// Outbound queue owned by a single `QuickJsRuntime` (page / context).
///
/// Worker threads push `(port_id, json_string)` pairs destined for that
/// runtime's client ports; the runtime drains it via `drain_messages`.
pub type SharedWorkerOutbox = Arc<Mutex<Vec<(u32, String)>>>;

/// Message sent from a client (main JS thread) to a shared-worker thread.
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
struct SharedWorkerThread {
    /// Channel used to deliver `Connect` / `Post` / `Close` to the worker loop.
    tx: Sender<SwInMsg>,
    /// Join handle, kept so the thread is joined when the hub entry is dropped.
    _thread: thread::JoinHandle<()>,
}

/// Process-global registry of live shared workers, keyed by identity string.
///
/// The identity key is `name` when a non-empty name is given, otherwise the
/// resolved script URL — matching the WHATWG "name or URL" identity rule for a
/// single-origin process.
static HUB: OnceLock<Mutex<HashMap<String, SharedWorkerThread>>> = OnceLock::new();

/// Monotonic source of globally-unique port ids (one per `SharedWorker`).
static PORT_COUNTER: AtomicU32 = AtomicU32::new(1);

fn hub() -> &'static Mutex<HashMap<String, SharedWorkerThread>> {
    HUB.get_or_init(|| Mutex::new(HashMap::new()))
}

// ─── public API ───────────────────────────────────────────────────────────────

/// Connect a new client to the shared worker identified by `key`.
///
/// Spawns the worker thread (evaluating `script`) on first connection; reuses
/// the existing thread on later connections.  `outbox` is the connecting
/// runtime's outbound queue: the worker pushes replies for this port into it.
///
/// Returns the freshly-allocated, process-unique port id.
pub fn connect_shared_worker(key: String, script: String, outbox: SharedWorkerOutbox) -> u32 {
    let port_id = PORT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut map = hub().lock().unwrap();

    // Reuse a live thread, or spawn one. If the stored thread's receiver was
    // dropped (worker exited), respawn transparently.
    let spawn = |key: String, script: String| -> SharedWorkerThread {
        let (tx, rx) = mpsc::channel::<SwInMsg>();
        let thread = thread::Builder::new()
            .name(format!("lumen-shared-worker-{key}"))
            .spawn(move || run_shared_worker_thread(script, rx))
            .expect("failed to spawn SharedWorker thread");
        SharedWorkerThread { tx, _thread: thread }
    };

    let entry = map
        .entry(key.clone())
        .or_insert_with(|| spawn(key.clone(), script.clone()));

    let connect = SwInMsg::Connect { port_id, outbox: Arc::clone(&outbox) };
    if entry.tx.send(connect).is_err() {
        // The worker exited; respawn and retry once.
        let fresh = spawn(key.clone(), script);
        let _ = fresh.tx.send(SwInMsg::Connect { port_id, outbox });
        *entry = fresh;
    }
    port_id
}

/// Forward a client `port.postMessage(data)` to the shared-worker thread.
///
/// No-op if `key` has no live worker (e.g. it already exited).
pub fn post_to_shared_worker(key: &str, port_id: u32, json: String) {
    if let Some(t) = hub().lock().unwrap().get(key) {
        let _ = t.tx.send(SwInMsg::Post { port_id, json });
    }
}

/// Notify the shared worker that a client closed its port.
///
/// The worker-side port mapping is dropped; the worker thread itself stays
/// alive for other clients (shared workers outlive individual connections).
pub fn close_shared_worker_port(key: &str, port_id: u32) {
    if let Some(t) = hub().lock().unwrap().get(key) {
        let _ = t.tx.send(SwInMsg::Close { port_id });
    }
}

/// Drain all messages a runtime's shared-worker ports have received.
///
/// Returns the drained `(port_id, json)` list and clears the queue atomically.
pub fn drain_messages(outbox: &SharedWorkerOutbox) -> Vec<(u32, String)> {
    std::mem::take(&mut outbox.lock().unwrap())
}

/// Install the `_lumen_sw_connect` / `_lumen_sw_post` / `_lumen_sw_close` native
/// bindings and the `SharedWorker` JS class into `ctx`.
///
/// Must be called after the core DOM shim so that `TextDecoder`,
/// `_object_url_store`, and `atob` are available for blob-/data-URL resolution
/// in the constructor.  `outbox` is this runtime's outbound queue.
pub fn install_shared_worker_bindings(
    ctx: &rquickjs::Ctx<'_>,
    outbox: &SharedWorkerOutbox,
) -> rquickjs::Result<()> {
    macro_rules! reg {
        ($name:expr, $f:expr) => {
            ctx.globals().set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }

    // _lumen_sw_connect(key: String, script: String) → u32 (port id)
    {
        let out = Arc::clone(outbox);
        reg!("_lumen_sw_connect", move |key: String, script: String| -> u32 {
            connect_shared_worker(key, script, Arc::clone(&out))
        });
    }

    // _lumen_sw_post(key: String, port_id: u32, json: String)
    reg!("_lumen_sw_post", move |key: String, port_id: u32, json: String| {
        post_to_shared_worker(&key, port_id, json);
    });

    // _lumen_sw_close(key: String, port_id: u32)
    reg!("_lumen_sw_close", move |key: String, port_id: u32| {
        close_shared_worker_port(&key, port_id);
    });

    ctx.eval::<(), _>(SHARED_WORKER_SHIM)?;
    Ok(())
}

// ─── worker thread ──────────────────────────────────────────────────────────────

fn run_shared_worker_thread(script: String, rx: Receiver<SwInMsg>) {
    let rt = match Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[shared-worker] runtime init failed: {e}");
            return;
        }
    };
    let ctx = match Context::full(&rt) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[shared-worker] context init failed: {e}");
            return;
        }
    };

    // port_id → connecting client's outbox. Populated on Connect, read by the
    // `_lumen_sw_port_reply` native binding when worker JS posts to a port.
    let ports: Arc<Mutex<HashMap<u32, SharedWorkerOutbox>>> =
        Arc::new(Mutex::new(HashMap::new()));

    if let Err(e) = ctx.with(|ctx| install_shared_worker_globals(&ctx, Arc::clone(&ports))) {
        eprintln!("[shared-worker] globals install failed: {e:?}");
        return;
    }

    if let Err(e) = ctx.with(|ctx| ctx.eval::<(), _>(script.as_str())) {
        eprintln!("[shared-worker] script error: {e:?}");
        // Continue: the worker may still service connections if the error was partial.
    }

    while let Ok(msg) = rx.recv() {
        match msg {
            SwInMsg::Connect { port_id, outbox } => {
                ports.lock().unwrap().insert(port_id, outbox);
                ctx.with(|ctx| {
                    ctx.eval::<(), _>(
                        format!(
                            "if(typeof _lumen_sw_dispatch_connect==='function')\
                             {{_lumen_sw_dispatch_connect({port_id});\
                              if(typeof _lumen_flush_timers==='function')_lumen_flush_timers();}}"
                        )
                        .as_str(),
                    )
                    .ok();
                });
            }
            SwInMsg::Post { port_id, json } => {
                ctx.with(|ctx| {
                    // Pass JSON via a temporary global to avoid string-literal escaping.
                    let _ = ctx.globals().set("_sw_msg__", json.as_str());
                    ctx.eval::<(), _>(
                        format!(
                            "if(typeof _lumen_sw_dispatch_port_message==='function')\
                             {{_lumen_sw_dispatch_port_message({port_id},JSON.parse(_sw_msg__));\
                              if(typeof _lumen_flush_timers==='function')_lumen_flush_timers();}}"
                        )
                        .as_str(),
                    )
                    .ok();
                });
            }
            SwInMsg::Close { port_id } => {
                ports.lock().unwrap().remove(&port_id);
                ctx.with(|ctx| {
                    ctx.eval::<(), _>(
                        format!(
                            "if(typeof _lumen_sw_dispatch_port_close==='function')\
                             _lumen_sw_dispatch_port_close({port_id});"
                        )
                        .as_str(),
                    )
                    .ok();
                });
            }
        }
    }
}

/// Install the shared-worker global scope (`SharedWorkerGlobalScope`-like).
///
/// Provides `self`, `name`, `onconnect`, `addEventListener('connect', …)`, a
/// worker-side `MessagePort` factory, the `_lumen_sw_dispatch_*` hooks the Rust
/// loop calls, `console` (→ stderr), and a minimal `setTimeout` stub.
fn install_shared_worker_globals(
    ctx: &rquickjs::Ctx<'_>,
    ports: Arc<Mutex<HashMap<u32, SharedWorkerOutbox>>>,
) -> rquickjs::Result<()> {
    macro_rules! reg {
        ($name:expr, $f:expr) => {
            ctx.globals().set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }

    // _lumen_sw_port_reply(port_id, json): push a reply into the connecting
    // client's outbox so its runtime delivers it to the matching client port.
    {
        let p = Arc::clone(&ports);
        reg!("_lumen_sw_port_reply", move |port_id: u32, json: String| {
            if let Some(outbox) = p.lock().unwrap().get(&port_id) {
                outbox.lock().unwrap().push((port_id, json));
            }
        });
    }

    // _lumen_sw_console_log(msg): forward to stderr (no DOM in a worker).
    reg!("_lumen_sw_console_log", move |msg: String| {
        eprintln!("[shared-worker] {msg}");
    });

    ctx.eval::<(), _>(SHARED_WORKER_GLOBAL_SHIM)?;
    Ok(())
}

// ─── JS shims ────────────────────────────────────────────────────────────────

/// Worker-thread global scope shim (evaluated inside each shared-worker context).
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
const SHARED_WORKER_SHIM: &str = r#"(function() {
  var _clientPorts = {};   // port_id → client-side MessagePort

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
    return '/* Lumen: external URL shared worker not supported: ' + u.replace(/\*\//g,'*\\/') + ' */';
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
    var script = _resolveScript(url);
    var pid = _lumen_sw_connect(key, script);
    this.port = _makeClientPort(pid, key);
    this.onerror = null;
    _clientPorts[pid] = this.port;
  }

  globalThis.SharedWorker = SharedWorker;
  if (typeof window !== 'undefined') window.SharedWorker = SharedWorker;

  // Called by QuickJsRuntime::pump_shared_workers() with [{ id, json }, …].
  globalThis._lumen_deliver_shared_worker_messages = function(msgs) {
    for (var i = 0; i < msgs.length; i++) {
      var m = msgs[i];
      var p = _clientPorts[m.id];
      if (p) p._deliver(m.json);
    }
  };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuickJsRuntime;
    use lumen_core::{JsRuntime, JsValue};
    use lumen_dom::Document;

    /// Build a fresh `QuickJsRuntime` with DOM (and thus shared-worker bindings).
    fn runtime() -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "", None, None, None, None, None, None).unwrap();
        rt
    }

    fn as_num(v: &JsValue) -> f64 {
        match v {
            JsValue::Number(n) => *n,
            JsValue::Bool(b) => i32::from(*b) as f64,
            _ => f64::NAN,
        }
    }

    /// Pump the runtime until `count_expr` evaluates to `>= expected`, or the
    /// budget is exhausted (worker threads are async; give them time).
    fn pump_until(rt: &QuickJsRuntime, count_expr: &str, expected: f64) {
        for _ in 0..400 {
            rt.pump_shared_workers();
            let n = rt.eval(count_expr).map(|v| as_num(&v)).unwrap_or(0.0);
            if n >= expected {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn shared_worker_class_exists() {
        let rt = runtime();
        assert_eq!(
            rt.eval("typeof SharedWorker === 'function'").unwrap(),
            JsValue::Bool(true)
        );
    }

    #[test]
    fn port_is_messageport_like() {
        let rt = runtime();
        let v = rt
            .eval(
                "var w = new SharedWorker('data:text/javascript,/*noop*/', 'idle');\
                 (typeof w.port.postMessage==='function' && \
                  typeof w.port.start==='function' && \
                  typeof w.port.close==='function')",
            )
            .unwrap();
        assert_eq!(v, JsValue::Bool(true));
    }

    #[test]
    fn connect_event_and_echo() {
        let rt = runtime();
        // Worker echoes each received message, prefixed with "echo:".
        let script = "onconnect=function(e){var p=e.ports[0];\
            p.onmessage=function(ev){p.postMessage('echo:'+ev.data);};};";
        let data_url = format!("data:text/javascript,{}", urlencode(script));
        rt.eval(&format!(
            "globalThis.__got=null;\
             var w=new SharedWorker('{data_url}','echo-1');\
             w.port.onmessage=function(ev){{globalThis.__got=ev.data;}};\
             w.port.postMessage('hello');"
        ))
        .unwrap();

        pump_until(&rt, "globalThis.__got===null?0:1", 1.0);
        assert_eq!(
            rt.eval("String(globalThis.__got)").unwrap(),
            JsValue::String("echo:hello".into())
        );
    }

    #[test]
    fn two_clients_share_one_worker() {
        let rt = runtime();
        // Worker keeps one counter; every message bumps it and replies with the
        // running total. Two same-name SharedWorker instances must hit the SAME
        // counter, proving the underlying thread is shared.
        let script = "var n=0;onconnect=function(e){var p=e.ports[0];\
            p.onmessage=function(){n+=1;p.postMessage(n);};};";
        let data_url = format!("data:text/javascript,{}", urlencode(script));
        rt.eval(&format!(
            "globalThis.__a=0;globalThis.__b=0;\
             globalThis.__a2=new SharedWorker('{data_url}','shared-counter');\
             globalThis.__b2=new SharedWorker('{data_url}','shared-counter');\
             __a2.port.onmessage=function(ev){{globalThis.__a=ev.data;}};\
             __b2.port.onmessage=function(ev){{globalThis.__b=ev.data;}};\
             __a2.port.postMessage(0);"
        ))
        .unwrap();
        pump_until(&rt, "globalThis.__a", 1.0);
        rt.eval("__b2.port.postMessage(0);").unwrap();
        pump_until(&rt, "globalThis.__b", 2.0);

        assert_eq!(as_num(&rt.eval("globalThis.__a").unwrap()), 1.0);
        assert_eq!(as_num(&rt.eval("globalThis.__b").unwrap()), 2.0);
    }

    #[test]
    fn distinct_names_are_isolated() {
        let rt = runtime();
        let script = "var n=0;onconnect=function(e){var p=e.ports[0];\
            p.onmessage=function(){n+=1;p.postMessage(n);};};";
        let data_url = format!("data:text/javascript,{}", urlencode(script));
        rt.eval(&format!(
            "globalThis.__x=0;globalThis.__y=0;\
             globalThis.__x2=new SharedWorker('{data_url}','iso-x');\
             globalThis.__y2=new SharedWorker('{data_url}','iso-y');\
             __x2.port.onmessage=function(ev){{globalThis.__x=ev.data;}};\
             __y2.port.onmessage=function(ev){{globalThis.__y=ev.data;}};\
             __x2.port.postMessage(0);__x2.port.postMessage(0);__y2.port.postMessage(0);"
        ))
        .unwrap();
        pump_until(&rt, "globalThis.__x", 2.0);
        pump_until(&rt, "globalThis.__y", 1.0);

        // x bumped twice (=2), y bumped once (=1): separate counters.
        assert_eq!(as_num(&rt.eval("globalThis.__x").unwrap()), 2.0);
        assert_eq!(as_num(&rt.eval("globalThis.__y").unwrap()), 1.0);
    }

    #[test]
    fn drain_messages_empties_outbox() {
        let outbox: SharedWorkerOutbox = Arc::new(Mutex::new(vec![(1, "\"a\"".into())]));
        let drained = drain_messages(&outbox);
        assert_eq!(drained.len(), 1);
        assert!(drain_messages(&outbox).is_empty());
    }

    /// Percent-encode the few characters that break a `data:` URL passed inline
    /// in an `eval` string (spaces, `+`, `%`, `&`, `#`).
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
}
