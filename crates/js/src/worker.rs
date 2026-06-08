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

// ─── public API ───────────────────────────────────────────────────────────────

/// Spawn a new worker thread that evaluates `script` and waits for messages.
///
/// Returns the unique worker ID assigned to this instance.  The caller stores
/// the ID in the JS `Worker` object and uses it for `postMessage`/`terminate`.
pub fn spawn_worker(
    registry: &WorkerRegistry,
    queue: &WorkerMessageQueue,
    next_id: &Arc<Mutex<u32>>,
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

    let handle = thread::Builder::new()
        .name(format!("lumen-worker-{id}"))
        .spawn(move || run_worker_thread(id, script, rx, reply))
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
/// `_lumen_worker_terminate`) and the `Worker` JS class into `ctx`.
///
/// Must be called after the core DOM shim so that `TextDecoder` and
/// `_object_url_store` are available for blob-URL resolution in the constructor.
pub fn install_worker_bindings(
    ctx: &rquickjs::Ctx<'_>,
    registry: &WorkerRegistry,
    queue: &WorkerMessageQueue,
    next_id: &Arc<Mutex<u32>>,
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
        reg!("_lumen_create_worker", move |script: String| -> u32 {
            spawn_worker(&reg, &q, &nid, script)
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

    // Evaluate the Worker class JS shim.
    ctx.eval::<(), _>(WORKER_SHIM)?;
    Ok(())
}

// ─── worker thread ────────────────────────────────────────────────────────────

fn run_worker_thread(
    id: u32,
    script: String,
    rx: Receiver<WorkerInMsg>,
    reply: Arc<Mutex<Vec<(u32, String)>>>,
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

    if let Err(e) = ctx.with(|ctx| install_worker_globals(&ctx, id, Arc::clone(&reply))) {
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

/// Install the minimal Worker global environment into a QuickJS context.
///
/// Provides: `self`, `postMessage`, `onmessage`, `addEventListener`,
/// `removeEventListener`, `_lumen_worker_dispatch_message`, `console`,
/// `importScripts` (throws), `setTimeout`/`clearTimeout`/`setInterval`/
/// `clearInterval` (minimal stub), `queueMicrotask`.
fn install_worker_globals(
    ctx: &rquickjs::Ctx<'_>,
    worker_id: u32,
    reply: Arc<Mutex<Vec<(u32, String)>>>,
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

    // Install the worker global environment via JS.
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

  // Called by the worker message loop for each incoming postMessage.
  globalThis._lumen_worker_dispatch_message = function(data) {{
    var ev = {{ data: data, type: 'message', target: globalThis,
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

  // importScripts — not supported in Lumen workers.
  globalThis.importScripts = function() {{
    throw new Error('importScripts is not supported');
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
/// - `_object_url_store` (defined in WEB_API_SHIM for blob: URL resolution).
/// - `TextDecoder` (defined in WEB_API_SHIM for UTF-8 decoding of blob bytes).
/// - `atob` (defined in WEB_API_SHIM for data: URLs with base64 encoding).
const WORKER_SHIM: &str = r#"(function() {
  // Registry: worker id (u32) → Worker instance.
  var _workerRegistry = {};

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
  Worker.prototype.postMessage = function(data) {
    _lumen_worker_post(this._id, JSON.stringify(data));
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
