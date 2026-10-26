//! JS-шимы Web Workers (WHATWG Web Workers §4): глобальная область
//! `DedicatedWorkerGlobalScope` ([`worker_global_shim`]), таймеры и rAF воркера,
//! `ErrorEvent`/`MessageEvent` области воркера, разбор `WorkerOptions` и
//! класс `Worker` главного потока ([`WORKER_SHIM`]).
//!
//! Чистый JS-текст; натив и запуск потоков — в `worker.rs`. Выделено из
//! `worker.rs` без изменений поведения (file-size gate, docs/lint-policy.md §5.1).

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

/// `MessageEvent` for a `WorkerGlobalScope` (GAP-WORKERSCOPE, BUG-872) — same
/// rationale and shape as [`WORKER_ERROR_EVENT_SHIM`] just above: a standalone
/// constructor rather than a slice of the page's `Event`-subclassing shim,
/// since that shim is page-only. Follows this codebase's established
/// `new MessageEvent(data, init)` convention (the constructor's first
/// argument is the payload, not a WebIDL event type — see
/// `web_api_shim_mid_b2.js`'s own `MessageEvent`, `broadcast_channel.rs`, and
/// the `v8_ws_sse.rs` tests that pin it) rather than the spec's
/// `new MessageEvent(type, init)`, so a worker-scope message event and a
/// page-scope one stay constructible the same way.
///
/// Evaluated for every worker flavour that goes through
/// [`install_worker_scope_globals_v8`] — dedicated, shared and service —
/// and guarded so a scope that already has the page-side class keeps it.
#[cfg(feature = "v8-backend")]
pub(crate) const WORKER_MESSAGE_EVENT_SHIM: &str = r#"
if (typeof globalThis.MessageEvent !== 'function') {
  var _LumenWorkerMessageEvent = function MessageEvent(data, init) {
    this.type = 'message';
    this.data = data;
    this.origin = (init && init.origin != null) ? String(init.origin) : '';
    this.lastEventId = (init && init.lastEventId != null) ? String(init.lastEventId) : '';
    this.source = (init && init.source !== undefined) ? init.source : null;
    this.ports = (init && Array.isArray(init.ports)) ? init.ports : [];
    // HTML LS §9.3.4 MessageEventInit.userActivation -- spec default is
    // null, not silently dropped (BUG-610, mirrored here for parity with the
    // page-side MessageEvent).
    this.userActivation = (init && init.userActivation !== undefined) ? init.userActivation : null;
    this.bubbles    = !!(init && init.bubbles);
    this.cancelable = !!(init && init.cancelable);
    this.defaultPrevented = false;
    this.target = null;
    this.currentTarget = null;
  };
  _LumenWorkerMessageEvent.prototype.preventDefault = function() {
    if (this.cancelable) this.defaultPrevented = true;
  };
  _LumenWorkerMessageEvent.prototype.stopPropagation = function() {};
  _LumenWorkerMessageEvent.prototype.stopImmediatePropagation = function() {};
  // `assert_class_string(e, 'MessageEvent')` (WPT's testharness) reads
  // `Object.prototype.toString`, which for a plain constructor answers
  // `[object Object]` without this.
  Object.defineProperty(_LumenWorkerMessageEvent.prototype, Symbol.toStringTag, {
    value: 'MessageEvent', configurable: true,
  });
  globalThis.MessageEvent = _LumenWorkerMessageEvent;
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
pub(crate) fn worker_global_shim(worker_id: u32) -> String {
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
  // BUG-1208: `WindowOrWorkerGlobalScope.origin` — same URL as `location`
  // above.
  if (typeof _lumen_make_worker_origin === 'function') {{
    globalThis.origin = _lumen_make_worker_origin(
      typeof _lumen_worker_location_url === 'string' ? _lumen_worker_location_url : '');
  }}

  // `isSecureContext` (BUG-766) — computed from the same
  // `_lumen_worker_location_url` `location` was just built from.
  if (typeof _lumen_worker_secure_context_for === 'function') {{
    Object.defineProperty(globalThis, 'isSecureContext', {{
      value: _lumen_worker_secure_context_for(
        typeof _lumen_worker_location_url === 'string' ? _lumen_worker_location_url : ''),
      enumerable: true, configurable: true,
    }});
  }}
  // BUG-1688: `crypto.subtle`/`CryptoKey` are `[SecureContext]`; the crypto shim leaves
  // them to this point, where `isSecureContext` is known.
  if (typeof _lumen_crypto_expose_secure === 'function') {{
    if (globalThis.isSecureContext !== false) _lumen_crypto_expose_secure();
    delete globalThis._lumen_crypto_expose_secure;
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
    __lumen_C._lumen_worker_error_reported = true;

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
  __lumen_C._lumen_report_worker_exception = _lumen_report_worker_exception;

  // postMessage(data[, transfer]) — send data back to the main thread.
  // BUG-868 GAP-WORKERSCOPE срез 2: when transfer contains MessagePort
  // objects, mirrors `Worker.prototype.postMessage`'s page-side envelope —
  // a worker has exactly one parent, so `_lumen_port_prepare_transfer` binds
  // with `null` (no destination worker id needed for the reply direction).
  globalThis.postMessage = function(data, transfer) {{
    var portIds = (typeof _lumen_port_prepare_transfer === 'function')
      ? _lumen_port_prepare_transfer(transfer, null) : [];
    var toSend = (portIds.length && typeof _lumen_port_walk_serialize === 'function')
      ? _lumen_port_walk_serialize(data) : data;
    var payload = JSON.stringify(toSend);
    if (portIds.length) {{
      payload = JSON.stringify({{ __lumen_msg__: true, data: JSON.parse(payload), ports: portIds }});
    }}
    _lumen_worker_post_reply(payload);
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
  // BUG-868 GAP-WORKERSCOPE срез 2: a message carrying transferred
  // `MessagePort`s arrives wrapped in `{{__lumen_msg__:true, data, ports}}` —
  // see `Worker.prototype.postMessage` (worker.rs, page side). `ports`
  // reifies to `ev.ports` in transfer order; a port reference embedded
  // inside `data` itself resolves to the same objects.
  __lumen_C._lumen_worker_dispatch_message = function(data) {{
    var payload = data, ports = [];
    if (data && typeof data === 'object' && data.__lumen_msg__ === true) {{
      payload = data.data;
      if (typeof _lumen_port_reify_list === 'function') ports = _lumen_port_reify_list(data.ports, null);
      if (typeof _lumen_port_walk_deserialize === 'function') payload = _lumen_port_walk_deserialize(payload, null);
    }}
    // Reconstruct any OffscreenCanvas objects serialized by the main thread.
    var resolved = (typeof _lumen_offscreen_canvas_from_image_data !== 'undefined')
      ? _deserializeTransfers(payload)
      : payload;
    // Lumen's MessageEvent constructor takes (data, init) — see dom.rs shim.
    var ev;
    try {{ ev = new MessageEvent(resolved, {{ bubbles: false, cancelable: false }}); }}
    catch (e) {{ ev = {{ type: 'message', data: resolved, bubbles: false, cancelable: false }}; }}
    ev.target = globalThis;
    ev.ports = ports;
    if (_onmessage) {{ try {{ _onmessage(ev); }} catch(e) {{ _lumen_report_worker_exception(e); }} }}
    for (var i = 0; i < _msgListeners.length; i++) {{
      try {{ _msgListeners[i](ev); }} catch(e) {{ _lumen_report_worker_exception(e); }}
    }}
  }};

  // Called by the worker's own message loop (`run_worker_thread_v8`) for a
  // message addressed to a `MessagePort` transferred into this worker
  // (BUG-868 GAP-WORKERSCOPE срез 2), not to the worker's own `onmessage` —
  // `json` is the raw JSON text of the cloned payload.
  __lumen_C._lumen_worker_deliver_port_message = function(portId, json) {{
    if (typeof _lumenPortRegistry === 'undefined') return;
    var p = _lumenPortRegistry[portId];
    if (!p) return;
    var data;
    try {{ data = JSON.parse(json); }} catch(e) {{ data = null; }}
    if (typeof _lumen_port_walk_deserialize === 'function') data = _lumen_port_walk_deserialize(data, null);
    p._deliverOrQueue({{ type: 'message', data: data, target: p,
                        currentTarget: p, bubbles: false, cancelable: false }});
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
  // `blob:` resolve locally; anything else is resolved against the
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
      _lumen_import_script(script);
    }}
  }};

  // `setTimeout`/`setInterval`/`queueMicrotask` come from the shim shared with
  // the shared-worker scope (`crate::worker::WORKER_TIMERS_SHIM`, evaluated as
  // its own IIFE right after this one), which is what actually drives them
  // (BUG-815).

  // Exposed so the shared net shim (`worker_net::WORKER_NET_SHIM`, evaluated
  // after this one) can route a throwing fetch/XHR
  // listener through the same reporting path (BUG-591 shape, BUG-778 scope).
  __lumen_C._lumen_worker_exception_reporter = _lumen_report_worker_exception;

  // close() — HTML LS §10.2.3 "close a worker" (BUG-778): discard further
  // queued tasks. `_lumen_worker_self_close` flips a shared flag that
  // `run_worker_thread_v8`'s message loop polls after every dispatched task
  // and stops on, without servicing any more.
  // `_lumen_worker_closed` is the JS-visible half of the same flag, read by
  // `WORKER_TIMERS_SHIM`'s task loop so a `close()` from inside a timer stops
  // the remaining due timers of that very flush rather than only the next one.
  globalThis.close = function() {{
    __lumen_C._lumen_worker_closed = true;
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
pub(crate) const WORKER_TIMERS_SHIM: &str = include_str!("shim/worker_timers_shim.js");

/// `requestAnimationFrame`/`cancelAnimationFrame` (BUG-959, HTML LS §8.12
/// `AnimationFrameProvider`) — `DedicatedWorkerGlobalScope` only, per spec;
/// evaluated by [`install_worker_globals_v8`] alone, never by the shared- or
/// service-worker install paths. Must run after [`WORKER_TIMERS_SHIM`]: it
/// schedules through `setTimeout`/`clearTimeout`.
#[cfg(feature = "v8-backend")]
pub(crate) const WORKER_RAF_SHIM: &str = include_str!("shim/worker_raf_shim.js");

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
  if (typeof __lumen_C._lumen_parse_worker_options === 'function') return;

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
  __lumen_C._lumen_parse_worker_options = function(options) {
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
  __lumen_C._lumen_parse_shared_worker_options = function(options) {
    if (options !== undefined && options !== null &&
        typeof options !== 'object' && typeof options !== 'function') {
      return { type: 'classic', credentials: 'same-origin', name: String(options) };
    }
    return __lumen_C._lumen_parse_worker_options(options);
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
/// - `_lumen_blob_url_entry` (defined in WEB_API_SHIM for blob: URL resolution).
/// - `TextDecoder` (defined in WEB_API_SHIM for UTF-8 decoding of blob bytes).
/// - `atob` (defined in WEB_API_SHIM for data: URLs with base64 encoding).
#[cfg(feature = "v8-backend")]
pub(crate) const WORKER_SHIM: &str = r#"(function() {
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

    if (u.startsWith('blob:')) {
      // Blob URL created via URL.createObjectURL(blob).
      var blob = (typeof _lumen_blob_url_entry === 'function') ? _lumen_blob_url_entry(u) : null;
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
      // BUG-984: `location` inside the worker must report the final URL
      // after redirects, not the constructor URL the fetch was issued for.
      scriptUrl = (script !== null && typeof _lumen_worker_fetch_script_url === 'function')
        ? (_lumen_worker_fetch_script_url() || abs) : abs;
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
      //
      // GAP-CSPENF срез 13: `_lumen_worker_fetch_script` returning undefined
      // can mean an ordinary fetch failure or a `worker-src` refusal — read
      // the side channel now, before it is overwritten by the next
      // `new Worker(url)` call.
      var _workerCsp = (typeof _lumen_worker_last_csp_block === 'function')
        ? _lumen_worker_last_csp_block() : null;
      this._id = null;
      var self = this;
      setTimeout(function() {
        if (typeof _lumen_fire_worker_src_violation === 'function') {
          _lumen_fire_worker_src_violation(_workerCsp);
        }
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
  // reconstruct them as OffscreenCanvas instances. BUG-868 GAP-WORKERSCOPE
  // срез 2: when transfer contains MessagePort objects, `_lumen_port_prepare_transfer`
  // (message_channel_shim.js) binds each port's local partner to this worker
  // and assigns it a bridge id; the message goes out wrapped in an envelope
  // carrying those ids so the worker side can rebuild `e.ports`.
  // No-op when the worker never started (`_id === null`, BUG-364 script-fetch failure).
  Worker.prototype.postMessage = function(data, transfer) {
    if (this._id === null) return;
    var portIds = (typeof _lumen_port_prepare_transfer === 'function')
      ? _lumen_port_prepare_transfer(transfer, this._id) : [];
    // Replace embedded MessagePort references with sentinels BEFORE the
    // canvas-aware walk below — once `_lumenSerializeWithTransfers` has
    // JSON.stringify'd the tree, a port's own fields (`_other`, `_queue`, …)
    // would already be baked into garbage output instead of a sentinel.
    var toSend = (portIds.length && typeof _lumen_port_walk_serialize === 'function')
      ? _lumen_port_walk_serialize(data) : data;
    var payload = _lumenSerializeWithTransfers(toSend, transfer);
    if (portIds.length) {
      payload = JSON.stringify({ __lumen_msg__: true, data: JSON.parse(payload), ports: portIds });
    }
    _lumen_worker_post(this._id, payload);
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
    var raw;
    try { raw = JSON.parse(json); } catch(e) { raw = json; }
    // BUG-868 GAP-WORKERSCOPE срез 2: a message carrying transferred
    // `MessagePort`s arrives wrapped in `{__lumen_msg__:true, data, ports}` —
    // see `Worker.prototype.postMessage`. `ports` reifies to `ev.ports` in
    // transfer order; any port reference embedded inside `data` itself is
    // resolved to the SAME objects via `_lumen_port_walk_deserialize`.
    var data = raw, ports = [];
    if (raw && typeof raw === 'object' && raw.__lumen_msg__ === true) {
      data = raw.data;
      if (typeof _lumen_port_reify_list === 'function') ports = _lumen_port_reify_list(raw.ports, this._id);
      if (typeof _lumen_port_walk_deserialize === 'function') data = _lumen_port_walk_deserialize(data, this._id);
    }
    // Lumen's MessageEvent constructor takes (data, init) — see dom.rs shim.
    var ev;
    try { ev = new MessageEvent(data, { bubbles: false, cancelable: false }); }
    catch (e) { ev = { type: 'message', data: data, bubbles: false, cancelable: false }; }
    ev.target = this;
    ev.ports = ports;
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

  // Exposed for `_lumen_bfcache_blocked()` (web_api_shim_mid_b.js, BUG-988): a
  // dedicated Worker runs on its own OS thread with no channel telling it its
  // page went into the back/forward cache — `park_current_page` only stops the
  // *runtime* from being pumped, the Worker keeps ticking regardless. Denying
  // the whole-runtime park for a page with a live Worker mirrors the existing
  // WebSocket/EventSource treatment, so such a page falls back to the
  // fresh-runtime-on-restore path, which does tear the old Worker down.
  __lumen_C._lumen_has_active_worker = function() {
    for (var k in _workerRegistry) {
      if (Object.prototype.hasOwnProperty.call(_workerRegistry, k)) return true;
    }
    return false;
  };

  // Also expose the serialization helper for use in tests and advanced callers.
  __lumen_C._lumenSerializeWithTransfers = _lumenSerializeWithTransfers;

  // Called by QuickJsRuntime::pump_workers() with an array of
  // { id: u32, json: String } objects representing messages from worker threads.
  __lumen_C._lumen_deliver_worker_messages = function(msgs) {
    for (var i = 0; i < msgs.length; i++) {
      var m = msgs[i];
      var w = _workerRegistry[m.id];
      if (w) w._deliver(m.json);
    }
  };

  // Called by V8JsRuntime::pump_workers() with an array of
  // { id: u32, json: String } objects representing messages posted by a
  // `MessagePort` living inside a worker thread to its transferred partner
  // on this page (BUG-868 GAP-WORKERSCOPE срез 2) — `id` here is a port id
  // from `_lumen_port_prepare_transfer`, not a worker id, so it is routed
  // through `_lumenPortRegistry` rather than `_workerRegistry`.
  __lumen_C._lumen_deliver_port_messages = function(msgs) {
    if (typeof _lumenPortRegistry === 'undefined') return;
    for (var i = 0; i < msgs.length; i++) {
      var m = msgs[i];
      var p = _lumenPortRegistry[m.id];
      if (!p) continue;
      var data;
      try { data = JSON.parse(m.json); } catch(e) { data = m.json; }
      if (typeof _lumen_port_walk_deserialize === 'function') data = _lumen_port_walk_deserialize(data, null);
      p._deliverOrQueue({ type: 'message', data: data, target: p,
                          currentTarget: p, bubbles: false, cancelable: false });
    }
  };

  // Called by V8JsRuntime::pump_workers() with an array of
  // { id: u32, json: {message, filename, lineno, colno} } objects representing
  // uncaught-exception reports from worker threads (BUG-591).
  __lumen_C._lumen_deliver_worker_errors = function(errs) {
    for (var i = 0; i < errs.length; i++) {
      var m = errs[i];
      var w = _workerRegistry[m.id];
      if (w) w._deliverError(m.json);
    }
  };
})();
"#;
