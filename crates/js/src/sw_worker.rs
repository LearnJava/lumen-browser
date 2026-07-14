//! Service Worker execution thread (PH3-20: SW fetch interception).
//!
//! Each activated SW gets a persistent QuickJS Runtime + Context running in a
//! dedicated `std::thread`. The shell calls `spawn_sw_worker` when a SW
//! activates; `ServiceWorkerInterceptor` (lumen-storage) sends `SwFetchRequest`
//! messages to the thread, which dispatches a `FetchEvent` and returns the
//! response body.

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use lumen_core::ext::{CacheBackend, SwFetchRequest, SwWorkerHandle};
use rquickjs::{Context, Function, Runtime};

#[cfg(feature = "v8-backend")]
use crate::v8_compat::{into_v8_fn1, into_v8_fn4};
#[cfg(feature = "v8-backend")]
use crate::v8_runtime::V8JsRuntime;
#[cfg(feature = "v8-backend")]
use lumen_core::JsResult;
#[cfg(feature = "v8-backend")]
use lumen_core::ext::JsRuntime as _;

/// Timeout for a SW to call `event.respondWith()`.
const FETCH_TIMEOUT: Duration = Duration::from_millis(5_000);

/// Spawn a Service Worker execution thread.
///
/// Evaluates `script` in a new QuickJS context with `ServiceWorkerGlobalScope`
/// globals and a `caches` API backed by `cache_backend`. Returns a handle used
/// to send `SwFetchRequest` messages to the thread.
pub fn spawn_sw_worker(
    origin: String,
    scope: String,
    script: String,
    cache_backend: Arc<dyn CacheBackend>,
) -> SwWorkerHandle {
    let (tx, rx) = std::sync::mpsc::channel::<SwFetchRequest>();
    let thread_name = format!("lumen-sw-{origin}{scope}");
    let handle = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || run_sw_thread(origin, scope, script, rx, cache_backend))
        .expect("failed to spawn SW thread");
    SwWorkerHandle { tx, _thread: handle }
}

fn run_sw_thread(
    origin: String,
    scope: String,
    script: String,
    rx: Receiver<SwFetchRequest>,
    cache_backend: Arc<dyn CacheBackend>,
) {
    let rt = match Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[sw {origin}{scope}] RT init failed: {e:?}");
            return;
        }
    };
    let ctx = match Context::full(&rt) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[sw {origin}{scope}] ctx init failed: {e:?}");
            return;
        }
    };

    // Install ServiceWorkerGlobalScope + caches API.
    if let Err(e) = ctx.with(|ctx| install_sw_globals(&ctx, &origin, &scope, Arc::clone(&cache_backend))) {
        eprintln!("[sw {origin}{scope}] globals failed: {e:?}");
        return;
    }

    // Evaluate SW script — installs fetch/install/activate handlers.
    if let Err(e) = ctx.with(|ctx| ctx.eval::<(), _>(script.as_str())) {
        eprintln!("[sw {origin}{scope}] script eval error: {e:?}");
        // Continue — partial install may still handle some fetches.
    }

    // Fire install event, then drain microtasks OUTSIDE `ctx.with` (calling a
    // `Runtime` method inside `ctx.with` re-enters the runtime borrow → panic).
    ctx.with(|ctx| {
        let _ = ctx.eval::<(), _>(
            "if(typeof _sw_fire_event==='function'){_sw_fire_event('install');}",
        );
    });
    flush_jobs(&rt);

    // Fire activate event, then drain microtasks (same re-entrancy rule).
    ctx.with(|ctx| {
        let _ = ctx.eval::<(), _>(
            "if(typeof _sw_fire_event==='function'){_sw_fire_event('activate');}",
        );
    });
    flush_jobs(&rt);

    // Message loop: handle fetch requests from the network layer.
    while let Ok(req) = rx.recv() {
        let body = dispatch_fetch(&ctx, &rt, &req.url, &req.method);
        let _ = req.response_tx.send(body);
    }
}

/// Dispatch a `FetchEvent` into the SW's QuickJS context and return the response body.
///
/// `flush_jobs` (a `Runtime` method) must run OUTSIDE `ctx.with` — calling it
/// while a context borrow is held re-enters the runtime and panics in rquickjs.
fn dispatch_fetch(ctx: &Context, rt: &Runtime, url: &str, method: &str) -> Option<Vec<u8>> {
    // Clear previous response, set request params, and dispatch the fetch event.
    ctx.with(|ctx| {
        let _ = ctx.globals().set("_sw_resp_body__", rquickjs::Undefined);
        let _ = ctx.globals().set("_sw_req_url__", url);
        let _ = ctx.globals().set("_sw_req_method__", method);
        let _ = ctx.eval::<(), _>(
            "if(typeof _sw_fire_fetch==='function'){_sw_fire_fetch(_sw_req_url__,_sw_req_method__);}",
        );
    });

    // Run microtasks/promises until respondWith resolves (outside ctx.with).
    flush_jobs(rt);

    // Read response body set by respondWith().
    ctx.with(|ctx| {
        let body_opt: Option<String> = ctx
            .globals()
            .get("_sw_resp_body__")
            .ok()
            .and_then(|v: rquickjs::Value| {
                if v.is_null() || v.is_undefined() {
                    None
                } else {
                    v.into_string().and_then(|s| s.to_string().ok())
                }
            });
        body_opt.map(|s| s.into_bytes())
    })
}

/// Run all pending QuickJS jobs (Promise callbacks, microtasks) until the queue empties.
fn flush_jobs(rt: &Runtime) {
    for _ in 0..1000 {
        match rt.execute_pending_job() {
            Ok(true) => continue,
            _ => break,
        }
    }
}

/// Install `ServiceWorkerGlobalScope` globals into the QuickJS context.
fn install_sw_globals(
    ctx: &rquickjs::Ctx<'_>,
    origin: &str,
    scope: &str,
    cache_backend: Arc<dyn CacheBackend>,
) -> rquickjs::Result<()> {
    macro_rules! reg {
        ($name:expr, $f:expr) => {
            ctx.globals().set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }

    // _lumen_sw_cache_match(url) -> Option<String (base64 body)>
    {
        let be = Arc::clone(&cache_backend);
        let orig = origin.to_string();
        reg!("_lumen_sw_cache_match", move |url: String| -> Option<String> {
            let names = be.cache_names(&orig);
            for name in &names {
                if let Some((_meta, body)) = be.cache_match(&orig, name, &url) {
                    return Some(base64_encode(&body));
                }
            }
            None
        });
    }

    // _lumen_sw_cache_put(name, url, meta_json, body_b64)
    {
        let be = Arc::clone(&cache_backend);
        let orig = origin.to_string();
        reg!("_lumen_sw_cache_put", move |name: String, url: String, meta: String, body_b64: String| {
            let body = base64_decode(&body_b64).unwrap_or_default();
            be.cache_put(&orig, &name, &url, &meta, &body);
        });
    }

    // _lumen_sw_cache_names() -> Vec<String>
    {
        let be = Arc::clone(&cache_backend);
        let orig = origin.to_string();
        reg!("_lumen_sw_cache_names", move || -> Vec<String> {
            be.cache_names(&orig)
        });
    }

    // Real base64 atob/btoa — the bare QuickJS context has none, and the JS shim
    // would otherwise install identity stubs that break cache body round-trips
    // (`_lumen_sw_cache_match` returns base64 → JS `atob` must actually decode it).
    reg!("atob", move |s: String| -> Option<String> {
        base64_decode(&s).and_then(|b| String::from_utf8(b).ok())
    });
    reg!("btoa", move |s: String| -> String { base64_encode(s.as_bytes()) });

    let scope_js = scope.replace('\'', "\\'");
    let scope_str = format!("'{scope_js}'");
    ctx.eval::<(), _>(sw_globals_shim(&scope_str).as_str())
}

/// Build the `ServiceWorkerGlobalScope` shim source for a given (already
/// JS-string-literal-quoted) `scope_str`.
///
/// Pure JS (no engine-specific bits) — shared by [`install_sw_globals`]
/// (QuickJS) and [`install_sw_globals_v8`] (V8). Provides `self`, `location`,
/// `registration`, `skipWaiting`, `clients`, `addEventListener`/
/// `removeEventListener`, minimal `Headers`/`Response` classes, the `caches`
/// API (backed by the Rust `CacheBackend` via `_lumen_sw_cache_*` natives),
/// a cache-first `fetch` stub, `_sw_fire_event`/`_sw_fire_fetch` dispatch
/// hooks called by the Rust message loop, `console`, and
/// `setTimeout`/`clearTimeout`/`setInterval`/`clearInterval` stubs.
fn sw_globals_shim(scope_str: &str) -> String {
    format!(r#"
(function(scope) {{
  globalThis.self = globalThis;
  globalThis.location = {{ href: scope, origin: scope.slice(0, scope.lastIndexOf('/')) }};
  globalThis.registration = {{
    scope: scope,
    active: {{ state: 'activated', scriptURL: '' }},
    installing: null, waiting: null,
  }};
  globalThis.skipWaiting = function() {{ return Promise.resolve(); }};
  globalThis.clients = {{
    claim: function() {{ return Promise.resolve(); }},
    get:   function() {{ return Promise.resolve(undefined); }},
    matchAll: function() {{ return Promise.resolve([]); }},
  }};

  var _handlers = {{}};
  globalThis.addEventListener = function(type, fn) {{
    if (!_handlers[type]) _handlers[type] = [];
    _handlers[type].push(fn);
  }};
  globalThis.removeEventListener = function(type, fn) {{
    if (_handlers[type]) {{
      var i = _handlers[type].indexOf(fn);
      if (i !== -1) _handlers[type].splice(i, 1);
    }}
  }};

  // Minimal Headers class.
  function Headers(init) {{
    this._h = {{}};
    if (init) {{ for (var k in init) this._h[k.toLowerCase()] = String(init[k]); }}
  }}
  Headers.prototype.get = function(n) {{ return this._h[n.toLowerCase()] || null; }};
  Headers.prototype.set = function(n, v) {{ this._h[n.toLowerCase()] = String(v); }};
  Headers.prototype.has = function(n) {{ return n.toLowerCase() in this._h; }};
  globalThis.Headers = Headers;

  // Minimal Response class.
  function Response(body, init) {{
    this._body = body || '';
    init = init || {{}};
    this.status = init.status || 200;
    this.statusText = init.statusText || 'OK';
    this.ok = (this.status >= 200 && this.status < 300);
    this.headers = new Headers(init.headers);
    this.url = '';
  }}
  Response.prototype.text = function() {{
    var b = this._body;
    return Promise.resolve(typeof b === 'string' ? b : String(b));
  }};
  Response.prototype.json = function() {{
    var b = this._body;
    return Promise.resolve(JSON.parse(typeof b === 'string' ? b : String(b)));
  }};
  Response.prototype.arrayBuffer = function() {{
    return Promise.resolve(new ArrayBuffer(0));
  }};
  Response.prototype.clone = function() {{ return new Response(this._body, {{ status: this.status, headers: this.headers._h }}); }};
  globalThis.Response = Response;

  // caches API — backed by Rust CacheStorage via _lumen_sw_cache_* bindings.
  var _cache_obj = {{
    match: function(req, _opts) {{
      var url = (typeof req === 'string') ? req : req.url;
      var b64 = _lumen_sw_cache_match(url);
      if (!b64) return Promise.resolve(undefined);
      var body = atob(b64);
      return Promise.resolve(new Response(body, {{ status: 200 }}));
    }},
    put: function(req, res) {{
      var url = (typeof req === 'string') ? req : req.url;
      var self_cache = this;
      res.text().then(function(text) {{
        _lumen_sw_cache_put(self_cache._name || 'default', url,
          JSON.stringify({{method:'GET',status:res.status,statusText:res.statusText,headers:{{}}}}),
          btoa(text));
      }});
      return Promise.resolve();
    }},
    keys: function() {{ return Promise.resolve([]); }},
    delete: function() {{ return Promise.resolve(false); }},
    addAll: function(urls) {{
      return Promise.all(urls.map(function(u) {{
        return fetch(u).then(function(r) {{ return _cache_obj.put(u, r); }});
      }}));
    }},
  }};
  globalThis.caches = {{
    match: function(req, opts) {{ return _cache_obj.match(req, opts); }},
    open: function(name) {{
      return Promise.resolve(Object.assign(Object.create(_cache_obj), {{ _name: name }}));
    }},
    delete: function() {{ return Promise.resolve(false); }},
    keys: function() {{
      return Promise.resolve(_lumen_sw_cache_names().map(function(n) {{ return n; }}));
    }},
    has: function(name) {{ return Promise.resolve(_lumen_sw_cache_names().indexOf(name) !== -1); }},
  }};

  // atob/btoa stubs (needed by cache operations).
  if (typeof atob === 'undefined') {{
    globalThis.atob = function(s) {{ return s; }};
    globalThis.btoa = function(s) {{ return s; }};
  }}

  // Minimal fetch stub — cache-first only (Phase 1: no real network access from SW).
  globalThis.fetch = function(resource) {{
    var url = (typeof resource === 'string') ? resource : resource.url;
    var b64 = _lumen_sw_cache_match(url);
    if (b64) {{
      return Promise.resolve(new Response(atob(b64), {{ status: 200 }}));
    }}
    return Promise.reject(new TypeError('fetch not available in SW worker (Phase 1)'));
  }};

  // _sw_fire_event: fire install/activate handlers.
  globalThis._sw_fire_event = function(type) {{
    var fns = _handlers[type] || [];
    var evt = {{ type: type, waitUntil: function(p) {{}} }};
    for (var i = 0; i < fns.length; i++) {{
      try {{ fns[i](evt); }} catch(e) {{ }}
    }}
  }};

  // _sw_fire_fetch: dispatch FetchEvent, collect respondWith body.
  globalThis._sw_fire_fetch = function(url, method) {{
    var fns = _handlers['fetch'] || [];
    if (!fns.length) return;
    var request = {{
      url: url, method: method,
      headers: new Headers(),
      clone: function() {{ return request; }},
      mode: 'navigate', destination: '',
      referrer: '', credentials: 'include',
    }};
    var responded = false;
    var evt = {{
      type: 'fetch',
      request: request,
      respondWith: function(promise) {{
        if (responded) return;
        responded = true;
        Promise.resolve(promise).then(function(resp) {{
          if (!resp) return;
          resp.text().then(function(text) {{
            globalThis._sw_resp_body__ = text;
          }});
        }});
      }},
      waitUntil: function(p) {{}},
      preventDefault: function() {{}},
    }};
    for (var i = 0; i < fns.length; i++) {{
      try {{ fns[i](evt); }} catch(e) {{ }}
    }}
  }};

  // Minimal console stub.
  globalThis.console = {{
    log: function() {{}}, warn: function() {{}}, error: function() {{}},
    debug: function() {{}}, info: function() {{}},
  }};

  // queueMicrotask stub.
  globalThis.queueMicrotask = function(fn) {{ Promise.resolve().then(fn); }};

  // setTimeout / clearTimeout stubs (fire synchronously for Phase 1).
  globalThis.setTimeout = function(fn, _delay) {{ fn(); return 0; }};
  globalThis.clearTimeout = function() {{}};
  globalThis.setInterval = function() {{ return 0; }};
  globalThis.clearInterval = function() {{}};

}})({scope_str});
"#
    )
}

/// Encode bytes as standard base64.
fn base64_encode(data: &[u8]) -> String {
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

fn base64_decode(encoded: &str) -> Option<Vec<u8>> {
    const INVALID: u8 = 0xFF;
    let mut table = [INVALID; 256];
    for (i, &c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .iter()
        .enumerate()
    {
        table[c as usize] = i as u8;
    }
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

// Suppress unused-import warning for FETCH_TIMEOUT (currently not used at runtime
// since recv() is blocking; kept as documentation for the intended timeout).
const _: Duration = FETCH_TIMEOUT;

// ─── V8 backend port (Ph3 V8 migration S10) ──────────────────────────────────
//
// Mirrors `worker.rs`/`shared_worker.rs`'s V8 port: the SW thread owns a full
// `V8JsRuntime` instead of a bare `rquickjs::Runtime`/`Context`. Unlike the
// QuickJS version, no `flush_jobs`/`execute_pending_job` equivalent is
// needed here — V8's microtask queue auto-runs (`MicrotasksPolicy::kAuto`,
// confirmed by the S3 slice notes in `docs/tasks/ph3-v8-migration.md`), so a
// `Promise` chain started by `_sw_fire_fetch`/`_sw_fire_event` fully drains
// by the time `V8JsRuntime::eval` returns — verified empirically by
// `tests_v8::v8_sw_responds_from_cache` below (the QuickJS version's
// `caches.match(...).then(...)` chain needs the manual pump; the V8 version
// does not).
//
// `SwWorkerHandle`/`SwFetchRequest` (from `lumen_core::ext`) are reused
// unchanged — plain channel plumbing, no engine-specific types.

/// V8 port of [`spawn_sw_worker`].
#[cfg(feature = "v8-backend")]
pub(crate) fn spawn_sw_worker_v8(
    origin: String,
    scope: String,
    script: String,
    cache_backend: Arc<dyn CacheBackend>,
) -> SwWorkerHandle {
    let (tx, rx) = std::sync::mpsc::channel::<SwFetchRequest>();
    let thread_name = format!("lumen-sw-v8-{origin}{scope}");
    let handle = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || run_sw_thread_v8(origin, scope, script, rx, cache_backend))
        .expect("failed to spawn SW thread (v8)");
    SwWorkerHandle { tx, _thread: handle }
}

#[cfg(feature = "v8-backend")]
fn run_sw_thread_v8(
    origin: String,
    scope: String,
    script: String,
    rx: Receiver<SwFetchRequest>,
    cache_backend: Arc<dyn CacheBackend>,
) {
    let rt = match V8JsRuntime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[sw {origin}{scope}] v8 RT init failed: {e:?}");
            return;
        }
    };

    if let Err(e) = install_sw_globals_v8(&rt, &origin, &scope, Arc::clone(&cache_backend)) {
        eprintln!("[sw {origin}{scope}] v8 globals failed: {e:?}");
        return;
    }

    if let Err(e) = rt.eval(&script) {
        eprintln!("[sw {origin}{scope}] v8 script eval error: {e:?}");
        // Continue — partial install may still handle some fetches.
    }

    // Fire install then activate. No manual microtask pump needed (V8 runs
    // its own queue to completion inside `eval`) — see module doc comment.
    let _ = rt.eval("if(typeof _sw_fire_event==='function'){_sw_fire_event('install');}");
    let _ = rt.eval("if(typeof _sw_fire_event==='function'){_sw_fire_event('activate');}");

    while let Ok(req) = rx.recv() {
        let body = dispatch_fetch_v8(&rt, &req.url, &req.method);
        let _ = req.response_tx.send(body);
    }
}

/// V8 twin of [`dispatch_fetch`]. No separate flush step — see module doc
/// comment on why V8 doesn't need one.
#[cfg(feature = "v8-backend")]
fn dispatch_fetch_v8(rt: &V8JsRuntime, url: &str, method: &str) -> Option<Vec<u8>> {
    let _ = rt.set_global("_sw_resp_body__", lumen_core::JsValue::Null);
    let _ = rt.set_global("_sw_req_url__", lumen_core::JsValue::String(url.to_string()));
    let _ = rt.set_global("_sw_req_method__", lumen_core::JsValue::String(method.to_string()));
    let _ = rt.eval(
        "if(typeof _sw_fire_fetch==='function'){_sw_fire_fetch(_sw_req_url__,_sw_req_method__);}",
    );

    match rt.eval("_sw_resp_body__") {
        Ok(lumen_core::JsValue::String(s)) => Some(s.into_bytes()),
        _ => None,
    }
}

/// V8 port of [`install_sw_globals`]. Registers the same three cache natives
/// (`_lumen_sw_cache_match`, `_lumen_sw_cache_put`, `_lumen_sw_cache_names`)
/// plus `atob`/`btoa` (non-throwing here, unlike `worker.rs`'s — matching
/// [`install_sw_globals`]'s own `Option<String>`/plain-`String` signatures,
/// so the plain `into_v8_fnN` path is sufficient; no scoped native needed)
/// and evaluates the same globals shim JS used by the QuickJS SW thread.
#[cfg(feature = "v8-backend")]
fn install_sw_globals_v8(
    rt: &V8JsRuntime,
    origin: &str,
    scope: &str,
    cache_backend: Arc<dyn CacheBackend>,
) -> JsResult<()> {
    {
        let be = Arc::clone(&cache_backend);
        let orig = origin.to_string();
        rt.register_native(
            "_lumen_sw_cache_match",
            into_v8_fn1(move |url: String| -> Option<String> {
                let names = be.cache_names(&orig);
                for name in &names {
                    if let Some((_meta, body)) = be.cache_match(&orig, name, &url) {
                        return Some(base64_encode(&body));
                    }
                }
                None
            }),
        )?;
    }

    {
        let be = Arc::clone(&cache_backend);
        let orig = origin.to_string();
        rt.register_native(
            "_lumen_sw_cache_put",
            into_v8_fn4(move |name: String, url: String, meta: String, body_b64: String| {
                let body = base64_decode(&body_b64).unwrap_or_default();
                be.cache_put(&orig, &name, &url, &meta, &body);
            }),
        )?;
    }

    {
        let be = Arc::clone(&cache_backend);
        let orig = origin.to_string();
        rt.register_native(
            "_lumen_sw_cache_names",
            crate::v8_compat::into_v8_fn0(move || -> Vec<String> { be.cache_names(&orig) }),
        )?;
    }

    rt.register_native(
        "atob",
        into_v8_fn1(move |s: String| -> Option<String> {
            base64_decode(&s).and_then(|b| String::from_utf8(b).ok())
        }),
    )?;
    rt.register_native(
        "btoa",
        into_v8_fn1(move |s: String| -> String { base64_encode(s.as_bytes()) }),
    )?;

    let scope_js = scope.replace('\'', "\\'");
    let scope_str = format!("'{scope_js}'");
    rt.eval(&sw_globals_shim(&scope_str))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::CacheBackend;
    use std::sync::Mutex;

    struct MockCache {
        entries: Mutex<std::collections::HashMap<String, Vec<u8>>>,
    }
    impl MockCache {
        fn new() -> Arc<Self> {
            Arc::new(Self { entries: Mutex::new(Default::default()) })
        }
        fn insert(&self, url: &str, body: &[u8]) {
            self.entries.lock().unwrap().insert(url.to_string(), body.to_vec());
        }
    }
    impl CacheBackend for MockCache {
        fn cache_put(&self, _o: &str, _n: &str, url: &str, _meta: &str, body: &[u8]) {
            self.entries.lock().unwrap().insert(url.to_string(), body.to_vec());
        }
        fn cache_match(&self, _o: &str, _n: &str, url: &str) -> Option<(String, Vec<u8>)> {
            self.entries.lock().unwrap().get(url).map(|b| (String::new(), b.clone()))
        }
        fn cache_match_any(&self, _o: &str, url: &str) -> Option<(String, Vec<u8>)> {
            self.entries.lock().unwrap().get(url).map(|b| (String::new(), b.clone()))
        }
        fn cache_keys(&self, _o: &str, _n: &str) -> Vec<(String, String)> {
            vec![]
        }
        fn cache_delete(&self, _o: &str, _n: &str, _u: &str) -> bool {
            false
        }
        fn cache_has(&self, _o: &str, _n: &str) -> bool {
            false
        }
        fn cache_delete_cache(&self, _o: &str, _n: &str) -> bool {
            false
        }
        fn cache_names(&self, _o: &str) -> Vec<String> {
            vec!["default".to_string()]
        }
    }

    #[test]
    fn sw_responds_from_cache() {
        let cache = MockCache::new();
        cache.insert("https://example.com/api/data", b"cached data");

        let handle = spawn_sw_worker(
            "https://example.com".to_string(),
            "/".to_string(),
            r#"
self.addEventListener('fetch', function(event) {
    event.respondWith(caches.match(event.request));
});
"#
            .to_string(),
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/api/data".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            })
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(result, Some(b"cached data".to_vec()));
    }

    #[test]
    fn sw_returns_none_for_uncached_url() {
        let cache = MockCache::new();

        let handle = spawn_sw_worker(
            "https://example.com".to_string(),
            "/".to_string(),
            r#"
self.addEventListener('fetch', function(event) {
    event.respondWith(caches.match(event.request));
});
"#
            .to_string(),
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/missing.js".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            })
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn sw_no_fetch_handler_returns_none() {
        let cache = MockCache::new();

        let handle = spawn_sw_worker(
            "https://example.com".to_string(),
            "/".to_string(),
            "// no fetch handler".to_string(),
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/page".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            })
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(result, None);
    }
}

/// V8-backend counterpart of the [`tests`] module above (Ph3 V8 migration
/// S10). Same three scenarios (cache hit, cache miss, no fetch handler) —
/// the cache-hit case is the load-bearing proof that `_sw_fire_fetch`'s
/// `respondWith(caches.match(...))` promise chain fully resolves by the time
/// `dispatch_fetch_v8` reads `_sw_resp_body__`, with no manual microtask
/// pump (see the V8-port module doc comment above `spawn_sw_worker_v8`).
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    use super::*;
    use lumen_core::ext::CacheBackend;
    use std::sync::Mutex;

    /// Mirrors `tests::MockCache` (private to the sibling `tests` module, so
    /// duplicated here rather than reached into).
    struct MockCache {
        entries: Mutex<std::collections::HashMap<String, Vec<u8>>>,
    }
    impl MockCache {
        fn new() -> Arc<Self> {
            Arc::new(Self { entries: Mutex::new(Default::default()) })
        }
        fn insert(&self, url: &str, body: &[u8]) {
            self.entries.lock().unwrap().insert(url.to_string(), body.to_vec());
        }
    }
    impl CacheBackend for MockCache {
        fn cache_put(&self, _o: &str, _n: &str, url: &str, _meta: &str, body: &[u8]) {
            self.entries.lock().unwrap().insert(url.to_string(), body.to_vec());
        }
        fn cache_match(&self, _o: &str, _n: &str, url: &str) -> Option<(String, Vec<u8>)> {
            self.entries.lock().unwrap().get(url).map(|b| (String::new(), b.clone()))
        }
        fn cache_match_any(&self, _o: &str, url: &str) -> Option<(String, Vec<u8>)> {
            self.entries.lock().unwrap().get(url).map(|b| (String::new(), b.clone()))
        }
        fn cache_keys(&self, _o: &str, _n: &str) -> Vec<(String, String)> {
            vec![]
        }
        fn cache_delete(&self, _o: &str, _n: &str, _u: &str) -> bool {
            false
        }
        fn cache_has(&self, _o: &str, _n: &str) -> bool {
            false
        }
        fn cache_delete_cache(&self, _o: &str, _n: &str) -> bool {
            false
        }
        fn cache_names(&self, _o: &str) -> Vec<String> {
            vec!["default".to_string()]
        }
    }

    #[test]
    fn v8_sw_responds_from_cache() {
        let cache = MockCache::new();
        cache.insert("https://example.com/api/data", b"cached data");

        let handle = spawn_sw_worker_v8(
            "https://example.com".to_string(),
            "/".to_string(),
            r#"
self.addEventListener('fetch', function(event) {
    event.respondWith(caches.match(event.request));
});
"#
            .to_string(),
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/api/data".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            })
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(result, Some(b"cached data".to_vec()));
    }

    #[test]
    fn v8_sw_returns_none_for_uncached_url() {
        let cache = MockCache::new();

        let handle = spawn_sw_worker_v8(
            "https://example.com".to_string(),
            "/".to_string(),
            r#"
self.addEventListener('fetch', function(event) {
    event.respondWith(caches.match(event.request));
});
"#
            .to_string(),
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/missing.js".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            })
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn v8_sw_no_fetch_handler_returns_none() {
        let cache = MockCache::new();

        let handle = spawn_sw_worker_v8(
            "https://example.com".to_string(),
            "/".to_string(),
            "// no fetch handler".to_string(),
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/page".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            })
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(result, None);
    }
}
