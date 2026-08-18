//! Service Worker execution thread (PH3-20: SW fetch interception).
//!
//! Each activated SW gets a persistent V8 runtime running in a dedicated
//! `std::thread`. The shell calls `spawn_sw_worker_v8` when a SW activates;
//! `ServiceWorkerInterceptor` (lumen-storage) sends `SwFetchRequest` messages
//! to the thread, which dispatches a `FetchEvent` and returns the response
//! body. The rquickjs-backed `spawn_sw_worker` was removed in S12b-B17.

use std::time::Duration;

#[cfg(feature = "v8-backend")]
use std::sync::Arc;
#[cfg(feature = "v8-backend")]
use std::sync::mpsc::Receiver;

#[cfg(feature = "v8-backend")]
use lumen_core::ext::{CacheBackend, SwFetchRequest, SwWorkerHandle};

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

/// Build the `ServiceWorkerGlobalScope` shim source for a given (already
/// JS-string-literal-quoted) `scope_str`.
///
/// Pure JS (no engine-specific bits) — used by [`install_sw_globals_v8`].
/// Provides `self`, `location`, `registration`, `skipWaiting`, `clients`,
/// `addEventListener`/
/// `removeEventListener`, minimal `Headers`/`Response` classes, the `caches`
/// API (backed by the Rust `CacheBackend` via `_lumen_sw_cache_*` natives),
/// a cache-first `fetch` stub, `_sw_fire_event`/`_sw_fire_fetch` dispatch
/// hooks called by the Rust message loop, `console`, and
/// `setTimeout`/`clearTimeout`/`setInterval`/`clearInterval` stubs.
#[cfg(feature = "v8-backend")]
fn sw_globals_shim(scope_str: &str, origin_str: &str) -> String {
    format!(r#"
(function(scope, origin) {{
  globalThis.self = globalThis;
  // `WorkerLocation` (HTML LS §8.1.5.4) целиком, а не два поля: сервис-воркеры
  // ветвятся по `location.host`/`location.search` прямо на верхнем уровне, и
  // `undefined.includes(...)` роняет установку всего воркера ещё до первого
  // слушателя (живой пример — `sw.js` t-банка). Разбор берём у той же функции,
  // что и страница (`_lumen_parse_url` приехал с worker-шимом), поэтому
  // `origin` — настоящий, а не `scope` без последнего сегмента пути.
  globalThis.location = (function() {{
    var abs = (scope.indexOf('://') !== -1) ? scope
            : (origin + (scope.charAt(0) === '/' ? scope : '/' + scope));
    var p = _lumen_parse_url(abs);
    return {{
      href: p.href, origin: p.origin, protocol: p.protocol,
      host: p.host, hostname: p.hostname, port: p.port,
      pathname: p.pathname, search: p.search, hash: p.hash,
      toString: function() {{ return p.href; }},
    }};
  }})();
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

  // base64 → «байтовая» строка (по символу на байт) и она же → текст UTF-8.
  //
  // Нативный `atob` здесь отдаёт `undefined` на любом теле, которое не
  // является корректным UTF-8 (он декодирует через `String::from_utf8`), а
  // сетевой ответ может быть каким угодно. Свой разбор держит байты целыми:
  // `Response.arrayBuffer` читает их через `charCodeAt`, а `text()` уже
  // собирает из них UTF-8.
  var _B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  function _sw_b64_to_bin(s) {{
    var out = '', buf = 0, bits = 0;
    for (var i = 0; i < s.length; i++) {{
      var v = _B64.indexOf(s.charAt(i));
      if (v < 0) continue;
      buf = (buf << 6) | v; bits += 6;
      if (bits >= 8) {{ bits -= 8; out += String.fromCharCode((buf >> bits) & 0xFF); }}
    }}
    return out;
  }}
  function _sw_bin_to_utf8(b) {{
    var out = '', i = 0;
    while (i < b.length) {{
      var c = b.charCodeAt(i++) & 0xFF, cp;
      if (c < 0x80) cp = c;
      else if (c < 0xE0) cp = ((c & 0x1F) << 6) | (b.charCodeAt(i++) & 0x3F);
      else if (c < 0xF0) cp = ((c & 0x0F) << 12) | ((b.charCodeAt(i++) & 0x3F) << 6)
                            | (b.charCodeAt(i++) & 0x3F);
      else cp = ((c & 0x07) << 18) | ((b.charCodeAt(i++) & 0x3F) << 12)
              | ((b.charCodeAt(i++) & 0x3F) << 6) | (b.charCodeAt(i++) & 0x3F);
      out += String.fromCodePoint(cp);
    }}
    return out;
  }}

  // Minimal Headers class.
  function Headers(init) {{
    this._h = {{}};
    if (init) {{ for (var k in init) this._h[k.toLowerCase()] = String(init[k]); }}
  }}
  Headers.prototype.get = function(n) {{ return this._h[n.toLowerCase()] || null; }};
  Headers.prototype.set = function(n, v) {{ this._h[n.toLowerCase()] = String(v); }};
  Headers.prototype.has = function(n) {{ return n.toLowerCase() in this._h; }};
  Headers.prototype.forEach = function(fn, thisArg) {{
    for (var k in this._h) fn.call(thisArg, this._h[k], k, this);
  }};
  globalThis.Headers = Headers;

  // Request (Fetch §5.1) — минимально, но с теми полями, по которым воркер
  // ветвится в обработчике `fetch`: без класса `new Request(url)` в теле
  // воркера падало на первой же строке, и обработчик не регистрировался.
  function Request(input, init) {{
    init = init || {{}};
    this.url = (typeof input === 'string') ? input : (input && input.url) || '';
    this.method = (init.method || (input && input.method) || 'GET').toUpperCase();
    this.headers = (init.headers instanceof Headers) ? init.headers : new Headers(init.headers);
    this.mode = init.mode || 'cors';
    this.credentials = init.credentials || 'same-origin';
    this.destination = (input && input.destination) || '';
    this.referrer = init.referrer || '';
    this._body = init.body;
  }}
  Request.prototype.clone = function() {{
    return new Request(this.url, {{
      method: this.method, headers: this.headers, mode: this.mode,
      credentials: this.credentials, referrer: this.referrer, body: this._body,
    }});
  }};
  Request.prototype.text = function() {{ return Promise.resolve(String(this._body || '')); }};
  Request.prototype.json = function() {{ return Promise.resolve(JSON.parse(String(this._body || 'null'))); }};
  globalThis.Request = Request;

  // Minimal Response class.
  function Response(body, init) {{
    this._body = body || '';
    init = init || {{}};
    this.status = init.status || 200;
    this.statusText = init.statusText || 'OK';
    this.ok = (this.status >= 200 && this.status < 300);
    this.headers = new Headers(init.headers);
    this.url = init.url || '';
    this.type = init.type || 'basic';
    this.redirected = false;
    this.bodyUsed = false;
    // Тело из сети/кэша приходит «байтовой» строкой (символ = байт), и в
    // текст его превращает разбор UTF-8. Тело, собранное самим воркером
    // (`new Response('привет')`), уже текст — второй разбор его бы испортил,
    // поэтому происхождение помечается, а не угадывается по содержимому.
    this._binary = !!init._binary;
  }}
  Response.prototype._text = function() {{
    var b = String(this._body || '');
    return this._binary ? _sw_bin_to_utf8(b) : b;
  }};
  Response.prototype.text = function() {{
    return Promise.resolve(this._text());
  }};
  Response.prototype.json = function() {{
    return Promise.resolve(JSON.parse(this._text()));
  }};
  Response.prototype.arrayBuffer = function() {{
    var b = String(this._body || '');
    var buf = new ArrayBuffer(b.length);
    var view = new Uint8Array(buf);
    for (var i = 0; i < b.length; i++) view[i] = b.charCodeAt(i) & 0xFF;
    return Promise.resolve(buf);
  }};
  Response.prototype.clone = function() {{
    return new Response(this._body, {{
      status: this.status, statusText: this.statusText,
      headers: this.headers._h, url: this.url, _binary: this._binary,
    }});
  }};
  globalThis.Response = Response;

  // importScripts(...) (HTML LS §8.1.5.1) — синхронно по сети.
  //
  // До этого в области сервис-воркера функции не было вовсе, и воркер,
  // подключающий библиотеку (push-SDK, workbox), падал на первой строке с
  // `importScripts is not defined` — то есть не регистрировал НИ ОДНОГО
  // обработчика. Исполняем непрямым `eval`, чтобы объявления легли в
  // глобальную область, как требует спецификация.
  globalThis.importScripts = function() {{
    for (var i = 0; i < arguments.length; i++) {{
      var u = String(arguments[i]);
      var abs = (u.indexOf('://') !== -1) ? u : new URL(u, location.href).href;
      var raw = _lumen_sw_net_fetch(abs, 'GET');
      if (!raw) throw new Error('importScripts: cannot load script: ' + abs);
      var res = JSON.parse(raw);
      if (res.status < 200 || res.status >= 300) {{
        throw new Error('importScripts: HTTP ' + res.status + ' for ' + abs);
      }}
      (0, eval)(_sw_bin_to_utf8(_sw_b64_to_bin(res.body)));
    }}
  }};

  // caches API — backed by Rust CacheStorage via _lumen_sw_cache_* bindings.
  var _cache_obj = {{
    match: function(req, _opts) {{
      var url = (typeof req === 'string') ? req : req.url;
      var b64 = _lumen_sw_cache_match(url);
      if (!b64) return Promise.resolve(undefined);
      return Promise.resolve(new Response(_sw_b64_to_bin(b64), {{ status: 200, _binary: true }}));
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

  // fetch() — настоящая сеть, в обход перехвата этим же воркером.
  //
  // Прежняя версия отвечала только из CacheStorage и отклоняла всё остальное,
  // поэтому воркер, который обновляет кэш («достань из сети — положи в кэш»),
  // не мог наполнить его ни разу: его первый же `fetch` отклонялся, и цепочка
  // `install` обрывалась. Сеть идёт мимо `FetchInterceptor` — иначе запрос
  // вернулся бы в этот же воркер, а он стоит внутри своего же `fetch`.
  globalThis.fetch = function(resource, init) {{
    var url = (typeof resource === 'string') ? resource : (resource && resource.url) || '';
    var method = (init && init.method) || (resource && resource.method) || 'GET';
    var abs = (url.indexOf('://') !== -1) ? url : new URL(url, location.href).href;
    var raw = _lumen_sw_net_fetch(abs, String(method).toUpperCase());
    if (!raw) {{
      // Сеть не ответила — последний шанс отдать из кэша, как раньше.
      var b64 = _lumen_sw_cache_match(abs);
      if (b64) {{
        return Promise.resolve(new Response(_sw_b64_to_bin(b64), {{ status: 200, url: abs, _binary: true }}));
      }}
      return Promise.reject(new TypeError('fetch: network error for ' + abs));
    }}
    var res = JSON.parse(raw);
    return Promise.resolve(new Response(_sw_b64_to_bin(res.body), {{
      status: res.status, statusText: res.statusText,
      headers: res.headers, url: abs, _binary: true,
    }}));
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

}})({scope_str}, {origin_str});
"#
    )
}

/// Encode bytes as standard base64.
///
/// Also used by [`crate::filesystem_access`] for the same reason as
/// [`base64_decode`]: file bytes cannot cross into JS as a string.
#[cfg(feature = "v8-backend")]
pub(crate) fn base64_encode(data: &[u8]) -> String {
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

/// Decode standard base64, ignoring padding and ASCII whitespace.
///
/// `None` on any character outside the alphabet. Also used by
/// [`crate::filesystem_access`], which moves file bytes across the JS boundary
/// as base64 — a JS string cannot carry arbitrary bytes intact.
#[cfg(feature = "v8-backend")]
pub(crate) fn base64_decode(encoded: &str) -> Option<Vec<u8>> {
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
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
) -> SwWorkerHandle {
    let (tx, rx) = std::sync::mpsc::channel::<SwFetchRequest>();
    let thread_name = format!("lumen-sw-v8-{origin}{scope}");
    let handle = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            run_sw_thread_v8(origin, scope, script, rx, cache_backend, fetch_provider, idb_backend)
        })
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
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
) {
    let rt = match V8JsRuntime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[sw {origin}{scope}] v8 RT init failed: {e:?}");
            return;
        }
    };

    if let Err(e) =
        install_sw_globals_v8(
            &rt,
            &origin,
            &scope,
            Arc::clone(&cache_backend),
            fetch_provider,
            idb_backend,
        )
    {
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
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
) -> JsResult<()> {
    // Сеть области воркера: `importScripts` и `fetch` внутри него. Ответ едет
    // одной JSON-строкой, тело — base64: сетевой ответ не обязан быть текстом,
    // а строка JS не переносит произвольные байты.
    //
    // `fetch_bypassing_sw`, а не `fetch_sync`: обычный путь отдал бы запрос
    // перехватчику, тот выбрал бы по scope ЭТОТ ЖЕ воркер и послал бы ему
    // сообщение, которого воркер не разберёт — он стоит внутри своего же
    // запроса. Поток ждал бы сам себя.
    {
        let provider = fetch_provider.clone();
        rt.register_native(
            "_lumen_sw_net_fetch",
            crate::v8_compat::into_v8_fn2(move |url: String, method: String| -> Option<String> {
                let provider = provider.as_ref()?;
                let res = match provider.fetch_bypassing_sw(&url, &method) {
                    Ok(res) => res,
                    Err(e) => {
                        eprintln!("[sw] сеть: {url}: {e}");
                        return None;
                    }
                };
                let headers: serde_json::Map<String, serde_json::Value> = res
                    .headers
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect();
                serde_json::to_string(&serde_json::json!({
                    "status": res.status,
                    "statusText": res.status_text,
                    "headers": headers,
                    "body": base64_encode(&res.body),
                }))
                .ok()
            }),
        )?;
    }

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

    // `ServiceWorkerGlobalScope` is a `WorkerGlobalScope` too, so it gets the
    // same `EventTarget`/`performance` surface as the dedicated worker (BUG-401).
    crate::worker::install_worker_scope_globals_v8(rt)?;

    // IndexedDB — тот же блок шима, что у страницы (`[Exposed=(Window,Worker)]`),
    // и та же база: воркер, который пишет свою очередь в `indexedDB`, иначе
    // умирал на первой строке обращения к ней. Нативы ставятся только при
    // наличии backend-а — без него шим держит базу в куче (его собственные
    // охраны `typeof … === 'function'`).
    if let Some(idb) = idb_backend {
        let b = Arc::clone(&idb);
        rt.register_native(
            "_lumen_idb_load",
            crate::v8_compat::into_v8_fn0(move || -> Option<String> { b.load() }),
        )?;
        let b = Arc::clone(&idb);
        rt.register_native(
            "_lumen_idb_persist",
            into_v8_fn1(move |snapshot: String| {
                b.save(&snapshot);
            }),
        )?;
        let b = Arc::clone(&idb);
        rt.register_native(
            "_lumen_idb_schema_op",
            into_v8_fn1(move |json: String| -> bool {
                match serde_json::from_str::<lumen_core::ext::IdbSchemaOp>(&json) {
                    Ok(op) => b.apply_schema(&op).is_ok(),
                    Err(_) => false,
                }
            }),
        )?;
        let b = Arc::clone(&idb);
        rt.register_native(
            "_lumen_idb_commit_txn",
            into_v8_fn1(move |json: String| -> bool {
                match serde_json::from_str::<Vec<lumen_core::ext::IdbRecordOp>>(&json) {
                    Ok(ops) => b.commit_txn(&ops).is_ok(),
                    Err(_) => false,
                }
            }),
        )?;
        let b = Arc::clone(&idb);
        rt.register_native(
            "_lumen_idb_exec_op",
            into_v8_fn1(move |json: String| -> Option<String> {
                serde_json::from_str::<lumen_core::ext::IdbRecordOp>(&json)
                    .ok()
                    .and_then(|op| b.exec_op(&op).ok())
                    .and_then(|result| serde_json::to_string(&result).ok())
            }),
        )?;
        let b = Arc::clone(&idb);
        rt.register_native(
            "_lumen_idb_db_version",
            into_v8_fn1(move |db_name: String| -> i32 { b.db_version(&db_name) as i32 }),
        )?;
        let b = Arc::clone(&idb);
        rt.register_native(
            "_lumen_idb_databases",
            crate::v8_compat::into_v8_fn0(move || -> String {
                let dbs = b.list_databases();
                serde_json::to_string(
                    &dbs.iter()
                        .map(|(name, version)| serde_json::json!({ "name": name, "version": version }))
                        .collect::<Vec<_>>(),
                )
                .unwrap_or_else(|_| "[]".to_string())
            }),
        )?;
    }

    let scope_js = scope.replace('\'', "\\'");
    let scope_str = format!("'{scope_js}'");
    // `scope` приходит путём (`/invest/`), а `WorkerLocation` обязан быть
    // абсолютным адресом — иначе `location.host` пуст, и ветвление воркера по
    // хосту молча выбирает не ту ветку.
    let origin_js = origin.trim_end_matches('/').replace('\'', "\\'");
    let origin_str = format!("'{origin_js}'");
    rt.eval(&sw_globals_shim(&scope_str, &origin_str))?;
    // После шима области: блоки опираются на `_lumen_console_error`,
    // `queueMicrotask` и `setTimeout`, которые шим только что определил.
    rt.eval(crate::dom::MESSAGE_CHANNEL_SHIM)?;
    rt.eval(crate::dom::IDB_SHIM)?;
    Ok(())
}

/// Test coverage for the SW execution thread (Ph3 V8 migration S10, extended
/// to full parity in S12b-B17 when the rquickjs-backed `spawn_sw_worker` was
/// removed). Same three scenarios (cache hit, cache miss, no fetch handler) —
/// the cache-hit case is the load-bearing proof that `_sw_fire_fetch`'s
/// `respondWith(caches.match(...))` promise chain fully resolves by the time
/// `dispatch_fetch_v8` reads `_sw_resp_body__`, with no manual microtask
/// pump (see the V8-port module doc comment above `spawn_sw_worker_v8`).
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    use super::*;
    use lumen_core::ext::CacheBackend;
    use std::sync::Mutex;

    /// BUG-401: `ServiceWorkerGlobalScope` is a `WorkerGlobalScope` too, so it
    /// gets the same `performance` as the page and the other worker flavours —
    /// same shim source, hence the same prototype chain.
    #[test]
    fn sw_global_scope_has_performance() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None).unwrap();
        for expr in [
            "typeof performance.now === 'function'",
            "performance instanceof Performance",
            "Object.getPrototypeOf(Performance.prototype) === EventTarget.prototype",
        ] {
            assert_eq!(
                rt.eval(expr).unwrap(),
                lumen_core::JsValue::Bool(true),
                "{expr}"
            );
        }
    }

    /// `WorkerLocation` целиком + `URL`/`URLSearchParams` в области воркера:
    /// ровно то, чем сервис-воркеры ветвятся на верхнем уровне. Пока полей не
    /// было, `self.location.host.includes(...)` роняло установку воркера
    /// целиком (живой пример — `sw.js` t-банка, 2026-08-17).
    #[test]
    fn sw_global_scope_has_full_location_and_url_classes() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://cdn.example.com:8443", "/invest/", MockCache::new(), None, None)
            .unwrap();
        for expr in [
            "typeof URLSearchParams === 'function'",
            "typeof URL === 'function'",
            "location.host === 'cdn.example.com:8443'",
            "location.hostname === 'cdn.example.com'",
            "location.protocol === 'https:'",
            "location.pathname === '/invest/'",
            "typeof location.search === 'string'",
            "new URLSearchParams('a=1&b=2').get('b') === '2'",
            "new URL('/x?y=1', 'https://h.example/').searchParams.get('y') === '1'",
        ] {
            assert_eq!(
                rt.eval(expr).unwrap(),
                lumen_core::JsValue::Bool(true),
                "{expr}"
            );
        }
    }

    /// Сетевой двойник области воркера. Различает два входа провайдера:
    /// обычный `fetch_sync` (через него запрос попал бы в перехватчик, то есть
    /// обратно в этот же воркер) и `fetch_bypassing_sw`. Тела разные — поэтому
    /// тест видит, каким путём ушёл запрос, а не только что «что-то вернулось».
    struct SwNet {
        /// URL → тело, отдаваемое обходным путём.
        bodies: std::collections::HashMap<String, String>,
        /// Сколько раз позвали обычный путь (должен остаться нулём).
        via_intercepted: Mutex<usize>,
    }

    impl SwNet {
        fn new(bodies: &[(&str, &str)]) -> Arc<Self> {
            Arc::new(Self {
                bodies: bodies.iter().map(|(u, b)| ((*u).to_owned(), (*b).to_owned())).collect(),
                via_intercepted: Mutex::new(0),
            })
        }

        /// Сколько запросов ушло перехватываемым путём.
        fn intercepted(&self) -> usize {
            self.via_intercepted.lock().map(|n| *n).unwrap_or(usize::MAX)
        }
    }

    impl lumen_core::ext::JsFetchProvider for SwNet {
        fn fetch_sync(
            &self,
            _url: &str,
            _method: &str,
        ) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
            if let Ok(mut n) = self.via_intercepted.lock() {
                *n += 1;
            }
            Ok(lumen_core::ext::JsFetchResult {
                status: 200,
                status_text: "OK".into(),
                headers: vec![],
                body: "путь через перехватчик".as_bytes().to_vec(),
            })
        }

        fn fetch_bypassing_sw(
            &self,
            url: &str,
            _method: &str,
        ) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
            match self.bodies.get(url) {
                Some(body) => Ok(lumen_core::ext::JsFetchResult {
                    status: 200,
                    status_text: "OK".into(),
                    headers: vec![("content-type".into(), "text/plain".into())],
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

    /// Рантайм области воркера с сетью.
    fn sw_rt(origin: &str, scope: &str, net: &Arc<SwNet>) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        let provider: Arc<dyn lumen_core::ext::JsFetchProvider> = Arc::clone(net) as _;
        install_sw_globals_v8(&rt, origin, scope, MockCache::new(), Some(provider), None).unwrap();
        rt
    }

    /// `importScripts` по сети. До правки функции в области воркера не было
    /// вовсе, и воркер, подключающий библиотеку (push-SDK t-банка), умирал на
    /// первой же строке с `importScripts is not defined` — то есть не
    /// регистрировал ни одного обработчика.
    #[test]
    fn sw_import_scripts_loads_over_network() {
        let net = SwNet::new(&[(
            "https://cdn.example.com/lib.js",
            "globalThis.__lib_loaded = 'да';",
        )]);
        let rt = sw_rt("https://example.com", "/", &net);
        rt.eval("importScripts('https://cdn.example.com/lib.js');").unwrap();
        assert_eq!(
            rt.eval("globalThis.__lib_loaded").unwrap(),
            lumen_core::JsValue::String("да".into())
        );
        assert_eq!(net.intercepted(), 0, "запрос воркера ушёл в перехватчик");
    }

    /// Относительный адрес считается от адреса самого воркера.
    #[test]
    fn sw_import_scripts_resolves_relative_to_scope() {
        let net = SwNet::new(&[(
            "https://example.com/invest/helper.js",
            "globalThis.__rel = 1;",
        )]);
        let rt = sw_rt("https://example.com", "/invest/", &net);
        rt.eval("importScripts('./helper.js');").unwrap();
        assert_eq!(rt.eval("globalThis.__rel").unwrap(), lumen_core::JsValue::Number(1.0));
    }

    /// Отказ сети обязан быть исключением, а не молчаливым пропуском: воркер с
    /// недогруженной библиотекой хуже, чем воркер, который честно не встал.
    #[test]
    fn sw_import_scripts_throws_on_http_error() {
        let net = SwNet::new(&[]);
        let rt = sw_rt("https://example.com", "/", &net);
        assert!(rt.eval("importScripts('https://example.com/gone.js');").is_err());
    }

    /// `fetch` в воркере — настоящая сеть и мимо перехватчика. Прежняя версия
    /// отвечала только из CacheStorage, поэтому воркер не мог НАПОЛНИТЬ кэш:
    /// его первый же `fetch` отклонялся.
    #[test]
    fn sw_fetch_goes_to_network_bypassing_interception() {
        let net = SwNet::new(&[("https://example.com/api", "тело из сети")]);
        let rt = sw_rt("https://example.com", "/", &net);
        rt.eval(
            "fetch('https://example.com/api').then(function(r) {
                 globalThis.__st = r.status; return r.text();
             }).then(function(t) { globalThis.__body = t; });",
        )
        .unwrap();
        assert_eq!(rt.eval("globalThis.__st").unwrap(), lumen_core::JsValue::Number(200.0));
        assert_eq!(
            rt.eval("globalThis.__body").unwrap(),
            lumen_core::JsValue::String("тело из сети".into())
        );
        assert_eq!(net.intercepted(), 0, "fetch воркера ушёл в перехватчик");
    }

    /// Заголовки ответа доезжают до `Response.headers` — по ним воркер решает,
    /// класть ли ответ в кэш.
    #[test]
    fn sw_fetch_exposes_response_headers() {
        let net = SwNet::new(&[("https://example.com/a.txt", "x")]);
        let rt = sw_rt("https://example.com", "/", &net);
        rt.eval(
            "fetch('https://example.com/a.txt').then(function(r) {
                 globalThis.__ct = r.headers.get('content-type'); });",
        )
        .unwrap();
        assert_eq!(
            rt.eval("globalThis.__ct").unwrap(),
            lumen_core::JsValue::String("text/plain".into())
        );
    }

    /// `indexedDB` в области воркера. `sw.js` t-банка обращается к ней на
    /// верхнем уровне; пока класса не было, воркер умирал там же, где и на
    /// `importScripts` — до регистрации обработчиков.
    #[test]
    fn sw_global_scope_has_indexed_db() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None)
            .unwrap();
        for expr in [
            "typeof indexedDB === 'object'",
            "typeof indexedDB.open === 'function'",
            "typeof IDBKeyRange === 'function'",
            "typeof IDBDatabase === 'function'",
        ] {
            assert_eq!(rt.eval(expr).unwrap(), lumen_core::JsValue::Bool(true), "{expr}");
        }
    }

    /// Полный цикл: открыть базу, создать хранилище, записать и прочитать —
    /// внутри области воркера. Очередь запросов IndexedDB прокачивается
    /// `queueMicrotask`, который в области воркера свой, поэтому проверяется
    /// не наличие классов, а доехавшее до `onsuccess` значение.
    #[test]
    fn sw_indexed_db_round_trip() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None)
            .unwrap();
        rt.eval(
            "var req = indexedDB.open('sw-db', 1);
             req.onupgradeneeded = function(e) {
                 e.target.result.createObjectStore('items', { keyPath: 'id' });
             };
             req.onsuccess = function(e) {
                 var db = e.target.result;
                 var tx = db.transaction('items', 'readwrite');
                 tx.objectStore('items').put({ id: 1, v: 'из воркера' });
                 var get = db.transaction('items').objectStore('items').get(1);
                 get.onsuccess = function(ev) { globalThis.__idb = ev.target.result.v; };
             };",
        )
        .unwrap();
        rt.eval("_lumen_idb_flush();").unwrap();
        assert_eq!(
            rt.eval("globalThis.__idb").unwrap(),
            lumen_core::JsValue::String("из воркера".into())
        );
    }

    /// Без провайдера (headless-режимы дампа) сети нет, но область воркера
    /// обязана остаться живой: `fetch` отклоняется, `importScripts` бросает —
    /// вместо `_lumen_sw_net_fetch is not defined`.
    #[test]
    fn sw_without_provider_rejects_instead_of_crashing() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None).unwrap();
        assert_eq!(
            rt.eval("typeof _lumen_sw_net_fetch").unwrap(),
            lumen_core::JsValue::String("function".into())
        );
        rt.eval(
            "fetch('https://example.com/x').then(function() { globalThis.__r = 'ok'; },
                                                function() { globalThis.__r = 'отказ'; });",
        )
        .unwrap();
        assert_eq!(
            rt.eval("globalThis.__r").unwrap(),
            lumen_core::JsValue::String("отказ".into())
        );
        assert!(rt.eval("importScripts('https://example.com/x.js');").is_err());
    }

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
            None,
            None,
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
            None,
            None,
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
            None,
            None,
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
