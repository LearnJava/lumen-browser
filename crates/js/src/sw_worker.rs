//! Service Worker execution thread (PH3-20: SW fetch interception).
//!
//! Each activated SW gets a persistent V8 runtime running in a dedicated
//! `std::thread`. The shell calls `spawn_sw_worker_v8` when a SW activates;
//! `ServiceWorkerInterceptor` (lumen-storage) sends `SwWorkerMessage::Fetch`
//! messages to the thread, which dispatches a `FetchEvent` and returns the
//! response body; `lumen-js::push_api` sends `Push`/`PushSubscriptionChange`
//! (Ph3 push-api РЎРѓРЎР‚Р ВµР В· 5). The rquickjs-backed `spawn_sw_worker` was removed
//! in S12b-B17.
//!
//! SW-MSG-S1: timer-task loop with `run_worker_tasks` + `recv_timeout` (modeled
//! on `worker.rs`), async `setTimeout`/`setInterval`, and fetch timeout.
//!
//! SW-MSG-S2: `ExtendableEvent`/`ExtendableMessageEvent` classes,
//! `waitUntil` collecting lifetime promises, and `install`/`activate` waiting
//! for them via `_sw_extend_pending()` (capped by [`EXTEND_TIMEOUT`]).
//!
//! SW-MSG-S3: page РІвЂ вЂ™ worker messages РІР‚вЂќ `ServiceWorker.postMessage` on the
//! page sends [`SwWorkerMessage::Message`], dispatched here as an
//! `ExtendableMessageEvent` with a `WindowClient` `source`.
//!
//! SW-MSG-S4: the return channel РІР‚вЂќ `Client.prototype.postMessage` in the
//! isolate calls `_lumen_sw_post_to_client`, which queues into the
//! process-global hub (`crate::sw_clients`); the destination page's
//! `V8JsRuntime::pump_workers` delivers it as a `message` event on
//! `navigator.serviceWorker`.

use std::time::Duration;

#[cfg(feature = "v8-backend")]
use std::sync::Arc;
#[cfg(feature = "v8-backend")]
use std::sync::mpsc::Receiver;

#[cfg(feature = "v8-backend")]
use lumen_core::ext::{CacheBackend, SwWorkerHandle, SwWorkerMessage};

#[cfg(feature = "v8-backend")]
use crate::v8_compat::{into_v8_fn1, into_v8_fn4};
#[cfg(feature = "v8-backend")]
use crate::v8_runtime::V8JsRuntime;
#[cfg(feature = "v8-backend")]
use lumen_core::JsResult;
#[cfg(feature = "v8-backend")]
use lumen_core::ext::JsRuntime as _;

#[cfg(feature = "v8-backend")]
use crate::worker::run_worker_tasks;

/// Timeout for a SW to call `event.respondWith()`.
const FETCH_TIMEOUT: Duration = Duration::from_millis(5_000);

/// SW-MSG-S2: cap on how long `install`/`activate` may be extended by
/// `waitUntil` promises before the thread gives up waiting.
const EXTEND_TIMEOUT: Duration = Duration::from_secs(30);

/// Build the `ServiceWorkerGlobalScope` shim source for a given (already
/// JS-string-literal-quoted) `scope_str`.
///
/// Pure JS (no engine-specific bits) РІР‚вЂќ used by [`install_sw_globals_v8`].
/// Provides `self`, `location`, `registration`, `skipWaiting`, `clients`,
/// `addEventListener`/
/// `removeEventListener`, minimal `Request`/`Response` classes (`Headers` is
/// [`crate::dom::HEADERS_SHIM`], part of `worker_exposed_shim` evaluated
/// before this shim РІР‚вЂќ see [`install_sw_globals_v8`]), the `caches`
/// API (backed by the Rust `CacheBackend` via `_lumen_sw_cache_*` natives),
/// a cache-first `fetch` stub, `_sw_fire_event`/`_sw_fire_fetch` dispatch
/// hooks called by the Rust message loop, `console`, and
/// `setTimeout`/`clearTimeout`/`setInterval`/`clearInterval` stubs.
#[cfg(feature = "v8-backend")]
fn sw_globals_shim(scope_str: &str, origin_str: &str) -> String {
    format!(r#"
(function(scope, origin) {{
  globalThis.self = globalThis;
  // `ServiceWorkerGlobalScope` (BUG-777) РІР‚вЂќ the third flavour of the same
  // factory, see `crate::dom::WORKER_LOCATION_NAVIGATOR_SHIM`.
  if (typeof _lumen_define_worker_scope === 'function') {{
    _lumen_define_worker_scope('ServiceWorkerGlobalScope');
  }}
  // `WorkerLocation` (HTML LS Р’В§8.1.5.4) РЎвЂ Р ВµР В»Р С‘Р С”Р С•Р С, Р В° Р Р…Р Вµ Р Т‘Р Р†Р В° Р С—Р С•Р В»РЎРЏ: РЎРѓР ВµРЎР‚Р Р†Р С‘РЎРѓ-Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚РЎвЂ№
  // Р Р†Р ВµРЎвЂљР Р†РЎРЏРЎвЂљРЎРѓРЎРЏ Р С—Р С• `location.host`/`location.search` Р С—РЎР‚РЎРЏР СР С• Р Р…Р В° Р Р†Р ВµРЎР‚РЎвЂ¦Р Р…Р ВµР С РЎС“РЎР‚Р С•Р Р†Р Р…Р Вµ, Р С‘
  // `undefined.includes(...)` РЎР‚Р С•Р Р…РЎРЏР ВµРЎвЂљ РЎС“РЎРѓРЎвЂљР В°Р Р…Р С•Р Р†Р С”РЎС“ Р Р†РЎРѓР ВµР С–Р С• Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В° Р ВµРЎвЂ°РЎвЂ Р Т‘Р С• Р С—Р ВµРЎР‚Р Р†Р С•Р С–Р С•
  // РЎРѓР В»РЎС“РЎв‚¬Р В°РЎвЂљР ВµР В»РЎРЏ (Р В¶Р С‘Р Р†Р С•Р в„– Р С—РЎР‚Р С‘Р СР ВµРЎР‚ РІР‚вЂќ `sw.js` t-Р В±Р В°Р Р…Р С”Р В°). Р С›Р В±РЎР‰Р ВµР С”РЎвЂљ РЎРѓРЎвЂљРЎР‚Р С•Р С‘РЎвЂљ РЎвЂљР С•РЎвЂљ Р В¶Р Вµ
  // `_lumen_make_worker_location`, РЎвЂЎРЎвЂљР С• Р С‘ РЎС“ Р Т‘Р Р†РЎС“РЎвЂ¦ Р Т‘РЎР‚РЎС“Р С–Р С‘РЎвЂ¦ Р Р†Р С‘Р Т‘Р С•Р Р† Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В°
  // (BUG-776 РІР‚вЂќ Р Т‘Р С• Р Р…Р ВµР С–Р С• РЎРЊРЎвЂљР С• Р В±РЎвЂ№Р В» Р В»Р С‘РЎвЂљР ВµРЎР‚Р В°Р В» Р С—РЎР‚РЎРЏР СР С• Р В·Р Т‘Р ВµРЎРѓРЎРЉ), Р С—Р С•РЎРЊРЎвЂљР С•Р СРЎС“ `origin` РІР‚вЂќ
  // Р Р…Р В°РЎРѓРЎвЂљР С•РЎРЏРЎвЂ°Р С‘Р в„–, Р В° Р Р…Р Вµ `scope` Р В±Р ВµР В· Р С—Р С•РЎРѓР В»Р ВµР Т‘Р Р…Р ВµР С–Р С• РЎРѓР ВµР С–Р СР ВµР Р…РЎвЂљР В° Р С—РЎС“РЎвЂљР С‘, Р С‘
  // `location instanceof WorkerLocation` Р С‘РЎРѓРЎвЂљР С‘Р Р…Р Р…Р С• Р С‘ РЎвЂљРЎС“РЎвЂљ.
  globalThis.location = _lumen_make_worker_location(
    (scope.indexOf('://') !== -1) ? scope
      : (origin + (scope.charAt(0) === '/' ? scope : '/' + scope)));
  // BUG-1208: `WindowOrWorkerGlobalScope.origin` РІР‚вЂќ same URL as `location`
  // above (a service worker's scope URL doubles as its own location URL,
  // per the comment just above).
  if (typeof _lumen_make_worker_origin === 'function') {{
    globalThis.origin = _lumen_make_worker_origin(globalThis.location.href);
  }}
  // `isSecureContext` (BUG-766) РІР‚вЂќ same rule as the other two worker
  // flavours; a service worker's scope URL doubles as its own location URL.
  if (typeof _lumen_worker_secure_context_for === 'function') {{
    Object.defineProperty(globalThis, 'isSecureContext', {{
      value: _lumen_worker_secure_context_for(globalThis.location.href),
      enumerable: true, configurable: true,
    }});
  }}
  // BUG-1688: `crypto.subtle`/`CryptoKey` are `[SecureContext]`; the crypto shim leaves
  // them to this point, where `isSecureContext` is known.
  if (typeof _lumen_crypto_expose_secure === 'function') {{
    if (globalThis.isSecureContext !== false) _lumen_crypto_expose_secure();
    delete globalThis._lumen_crypto_expose_secure;
  }}
  globalThis.registration = {{
    scope: scope,
    active: {{ state: 'activated', scriptURL: '' }},
    installing: null, waiting: null,
  }};
  globalThis.skipWaiting = function() {{ return Promise.resolve(); }};
  // SW-MSG-S6: `Clients` (SW Р’В§5.1) РІР‚вЂќ backed by the hub's client registry
  // through natives bound to this worker's origin and scope. `Client` /
  // `WindowClient` are declared further down; the lookups run at call time.
  globalThis.clients = {{
    get: function(id) {{
      if (typeof _lumen_sw_clients_get !== 'function') return Promise.resolve(undefined);
      var j = _lumen_sw_clients_get(Number(id));
      return Promise.resolve(j ? new WindowClient(JSON.parse(j)) : undefined);
    }},
    matchAll: function(options) {{
      options = options || {{}};
      if (typeof _lumen_sw_clients_match_all !== 'function') return Promise.resolve([]);
      var j = _lumen_sw_clients_match_all(
        !!options.includeUncontrolled, String(options.type || 'window'));
      var arr = j ? JSON.parse(j) : [];
      return Promise.resolve(arr.map(function(i) {{ return new WindowClient(i); }}));
    }},
    claim: function() {{
      if (typeof _lumen_sw_clients_claim === 'function') _lumen_sw_clients_claim();
      // SW Р’В§5.1: resolves to undefined either way.
      return Promise.resolve(undefined);
    }},
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

  // SW-MODULE: module scripts are strict, so a bare `onmessage = ...` at the
  // top level throws unless the handler attrs already exist on the scope
  // (in engines they are IDL attributes, null by default). Predefine the
  // ServiceWorkerGlobalScope/WorkerGlobalScope event-handler set.
  var _sw_handler_attrs = 'oninstall onactivate onfetch onmessage onmessageerror onpush onpushsubscriptionchange onnotificationclick'.split(' ');
  for (var _ha = 0; _ha < _sw_handler_attrs.length; _ha++) {{
    if (!(_sw_handler_attrs[_ha] in globalThis)) globalThis[_sw_handler_attrs[_ha]] = null;
  }}

  // base64 РІвЂ вЂ™ Р’В«Р В±Р В°Р в„–РЎвЂљР С•Р Р†Р В°РЎРЏР’В» РЎРѓРЎвЂљРЎР‚Р С•Р С”Р В° (Р С—Р С• РЎРѓР С‘Р СР Р†Р С•Р В»РЎС“ Р Р…Р В° Р В±Р В°Р в„–РЎвЂљ) Р С‘ Р С•Р Р…Р В° Р В¶Р Вµ РІвЂ вЂ™ РЎвЂљР ВµР С”РЎРѓРЎвЂљ UTF-8.
  //
  // Р СњР В°РЎвЂљР С‘Р Р†Р Р…РЎвЂ№Р в„– `atob` Р В·Р Т‘Р ВµРЎРѓРЎРЉ Р С•РЎвЂљР Т‘Р В°РЎвЂРЎвЂљ `undefined` Р Р…Р В° Р В»РЎР‹Р В±Р С•Р С РЎвЂљР ВµР В»Р Вµ, Р С”Р С•РЎвЂљР С•РЎР‚Р С•Р Вµ Р Р…Р Вµ
  // РЎРЏР Р†Р В»РЎРЏР ВµРЎвЂљРЎРѓРЎРЏ Р С”Р С•РЎР‚РЎР‚Р ВµР С”РЎвЂљР Р…РЎвЂ№Р С UTF-8 (Р С•Р Р… Р Т‘Р ВµР С”Р С•Р Т‘Р С‘РЎР‚РЎС“Р ВµРЎвЂљ РЎвЂЎР ВµРЎР‚Р ВµР В· `String::from_utf8`), Р В°
  // РЎРѓР ВµРЎвЂљР ВµР Р†Р С•Р в„– Р С•РЎвЂљР Р†Р ВµРЎвЂљ Р СР С•Р В¶Р ВµРЎвЂљ Р В±РЎвЂ№РЎвЂљРЎРЉ Р С”Р В°Р С”Р С‘Р С РЎС“Р С–Р С•Р Т‘Р Р…Р С•. Р РЋР Р†Р С•Р в„– РЎР‚Р В°Р В·Р В±Р С•РЎР‚ Р Т‘Р ВµРЎР‚Р В¶Р С‘РЎвЂљ Р В±Р В°Р в„–РЎвЂљРЎвЂ№ РЎвЂ Р ВµР В»РЎвЂ№Р СР С‘:
  // `Response.arrayBuffer` РЎвЂЎР С‘РЎвЂљР В°Р ВµРЎвЂљ Р С‘РЎвЂ¦ РЎвЂЎР ВµРЎР‚Р ВµР В· `charCodeAt`, Р В° `text()` РЎС“Р В¶Р Вµ
  // РЎРѓР С•Р В±Р С‘РЎР‚Р В°Р ВµРЎвЂљ Р С‘Р В· Р Р…Р С‘РЎвЂ¦ UTF-8.
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

  // `Headers` (BUG-748): the real class (`crate::dom::HEADERS_SHIM`, shared
  // with the page and BUG-369, part of `worker_exposed_shim`), evaluated
  // before this shim РІР‚вЂќ not a second, independent mini-class. That second
  // class used to store headers on a plain object, so `Set-Cookie` duplicates
  // collapsed into one and `for..of`/`.append()`/`.forEach()` were missing.

  // Request (Fetch Р’В§5.1) РІР‚вЂќ Р СР С‘Р Р…Р С‘Р СР В°Р В»РЎРЉР Р…Р С•, Р Р…Р С• РЎРѓ РЎвЂљР ВµР СР С‘ Р С—Р С•Р В»РЎРЏР СР С‘, Р С—Р С• Р С”Р С•РЎвЂљР С•РЎР‚РЎвЂ№Р С Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚
  // Р Р†Р ВµРЎвЂљР Р†Р С‘РЎвЂљРЎРѓРЎРЏ Р Р† Р С•Р В±РЎР‚Р В°Р В±Р С•РЎвЂљРЎвЂЎР С‘Р С”Р Вµ `fetch`: Р В±Р ВµР В· Р С”Р В»Р В°РЎРѓРЎРѓР В° `new Request(url)` Р Р† РЎвЂљР ВµР В»Р Вµ
  // Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В° Р С—Р В°Р Т‘Р В°Р В»Р С• Р Р…Р В° Р С—Р ВµРЎР‚Р Р†Р С•Р в„– Р В¶Р Вµ РЎРѓРЎвЂљРЎР‚Р С•Р С”Р Вµ, Р С‘ Р С•Р В±РЎР‚Р В°Р В±Р С•РЎвЂљРЎвЂЎР С‘Р С” Р Р…Р Вµ РЎР‚Р ВµР С–Р С‘РЎРѓРЎвЂљРЎР‚Р С‘РЎР‚Р С•Р Р†Р В°Р В»РЎРѓРЎРЏ.
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
    // Р СћР ВµР В»Р С• Р С‘Р В· РЎРѓР ВµРЎвЂљР С‘/Р С”РЎРЊРЎв‚¬Р В° Р С—РЎР‚Р С‘РЎвЂ¦Р С•Р Т‘Р С‘РЎвЂљ Р’В«Р В±Р В°Р в„–РЎвЂљР С•Р Р†Р С•Р в„–Р’В» РЎРѓРЎвЂљРЎР‚Р С•Р С”Р С•Р в„– (РЎРѓР С‘Р СР Р†Р С•Р В» = Р В±Р В°Р в„–РЎвЂљ), Р С‘ Р Р†
    // РЎвЂљР ВµР С”РЎРѓРЎвЂљ Р ВµР С–Р С• Р С—РЎР‚Р ВµР Р†РЎР‚Р В°РЎвЂ°Р В°Р ВµРЎвЂљ РЎР‚Р В°Р В·Р В±Р С•РЎР‚ UTF-8. Р СћР ВµР В»Р С•, РЎРѓР С•Р В±РЎР‚Р В°Р Р…Р Р…Р С•Р Вµ РЎРѓР В°Р СР С‘Р С Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р С•Р С
    // (`new Response('Р С—РЎР‚Р С‘Р Р†Р ВµРЎвЂљ')`), РЎС“Р В¶Р Вµ РЎвЂљР ВµР С”РЎРѓРЎвЂљ РІР‚вЂќ Р Р†РЎвЂљР С•РЎР‚Р С•Р в„– РЎР‚Р В°Р В·Р В±Р С•РЎР‚ Р ВµР С–Р С• Р В±РЎвЂ№ Р С‘РЎРѓР С—Р С•РЎР‚РЎвЂљР С‘Р В»,
    // Р С—Р С•РЎРЊРЎвЂљР С•Р СРЎС“ Р С—РЎР‚Р С•Р С‘РЎРѓРЎвЂ¦Р С•Р В¶Р Т‘Р ВµР Р…Р С‘Р Вµ Р С—Р С•Р СР ВµРЎвЂЎР В°Р ВµРЎвЂљРЎРѓРЎРЏ, Р В° Р Р…Р Вµ РЎС“Р С–Р В°Р Т‘РЎвЂ№Р Р†Р В°Р ВµРЎвЂљРЎРѓРЎРЏ Р С—Р С• РЎРѓР С•Р Т‘Р ВµРЎР‚Р В¶Р С‘Р СР С•Р СРЎС“.
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
      headers: this.headers, url: this.url, _binary: this._binary,
    }});
  }};
  globalThis.Response = Response;

  // importScripts(...) (HTML LS Р’В§8.1.5.1) РІР‚вЂќ РЎРѓР С‘Р Р…РЎвЂ¦РЎР‚Р С•Р Р…Р Р…Р С• Р С—Р С• РЎРѓР ВµРЎвЂљР С‘.
  //
  // Р вЂќР С• РЎРЊРЎвЂљР С•Р С–Р С• Р Р† Р С•Р В±Р В»Р В°РЎРѓРЎвЂљР С‘ РЎРѓР ВµРЎР‚Р Р†Р С‘РЎРѓ-Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В° РЎвЂћРЎС“Р Р…Р С”РЎвЂ Р С‘Р С‘ Р Р…Р Вµ Р В±РЎвЂ№Р В»Р С• Р Р†Р С•Р Р†РЎРѓР Вµ, Р С‘ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚,
  // Р С—Р С•Р Т‘Р С”Р В»РЎР‹РЎвЂЎР В°РЎР‹РЎвЂ°Р С‘Р в„– Р В±Р С‘Р В±Р В»Р С‘Р С•РЎвЂљР ВµР С”РЎС“ (push-SDK, workbox), Р С—Р В°Р Т‘Р В°Р В» Р Р…Р В° Р С—Р ВµРЎР‚Р Р†Р С•Р в„– РЎРѓРЎвЂљРЎР‚Р С•Р С”Р Вµ РЎРѓ
  // `importScripts is not defined` РІР‚вЂќ РЎвЂљР С• Р ВµРЎРѓРЎвЂљРЎРЉ Р Р…Р Вµ РЎР‚Р ВµР С–Р С‘РЎРѓРЎвЂљРЎР‚Р С‘РЎР‚Р С•Р Р†Р В°Р В» Р СњР В Р С›Р вЂќР СњР С›Р вЂњР С›
  // Р С•Р В±РЎР‚Р В°Р В±Р С•РЎвЂљРЎвЂЎР С‘Р С”Р В°. Р ВРЎРѓР С—Р С•Р В»Р Р…РЎРЏР ВµР С Р Р…Р ВµР С—РЎР‚РЎРЏР СРЎвЂ№Р С `eval`, РЎвЂЎРЎвЂљР С•Р В±РЎвЂ№ Р С•Р В±РЎР‰РЎРЏР Р†Р В»Р ВµР Р…Р С‘РЎРЏ Р В»Р ВµР С–Р В»Р С‘ Р Р†
  // Р С–Р В»Р С•Р В±Р В°Р В»РЎРЉР Р…РЎС“РЎР‹ Р С•Р В±Р В»Р В°РЎРѓРЎвЂљРЎРЉ, Р С”Р В°Р С” РЎвЂљРЎР‚Р ВµР В±РЎС“Р ВµРЎвЂљ РЎРѓР С—Р ВµРЎвЂ Р С‘РЎвЂћР С‘Р С”Р В°РЎвЂ Р С‘РЎРЏ.
  globalThis.importScripts = function() {{
    // SW-MODULE: Р Р† Р СР С•Р Т‘РЎС“Р В»РЎРЉР Р…Р С•Р С РЎРѓР ВµРЎР‚Р Р†Р С‘РЎРѓ-Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р Вµ importScripts Р В·Р В°Р С—РЎР‚Р ВµРЎвЂ°РЎвЂР Р… (SW
    // Р’В§Run Service Worker) РІР‚вЂќ РЎвЂћР В»Р В°Р С– РЎРѓРЎвЂљР В°Р Р†Р С‘РЎвЂљ run_sw_thread_v8 Р С—Р ВµРЎР‚Р ВµР Т‘ РЎв‚¬Р С‘Р СР С•Р С.
    if (globalThis._lumen_sw_is_module__) {{
      throw new TypeError('importScripts is not allowed in module service workers.');
    }}
    for (var i = 0; i < arguments.length; i++) {{
      var u = String(arguments[i]);
      var abs = (u.indexOf('://') !== -1) ? u : new URL(u, location.href).href;
      if (_lumen_sw_check_worker_src(abs)) {{
        throw new Error('importScripts: cannot load script: ' + abs);
      }}
      var raw = _lumen_sw_net_fetch(abs, 'GET');
      if (!raw) throw new Error('importScripts: cannot load script: ' + abs);
      var res = JSON.parse(raw);
      if (res.status < 200 || res.status >= 300) {{
        throw new Error('importScripts: HTTP ' + res.status + ' for ' + abs);
      }}
      (0, eval)(_sw_bin_to_utf8(_sw_b64_to_bin(res.body)));
    }}
  }};

  // caches API РІР‚вЂќ backed by Rust CacheStorage via _lumen_sw_cache_* bindings.
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
          btoa(unescape(encodeURIComponent(text))));
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

  // fetch() РІР‚вЂќ Р Р…Р В°РЎРѓРЎвЂљР С•РЎРЏРЎвЂ°Р В°РЎРЏ РЎРѓР ВµРЎвЂљРЎРЉ, Р Р† Р С•Р В±РЎвЂ¦Р С•Р Т‘ Р С—Р ВµРЎР‚Р ВµРЎвЂ¦Р Р†Р В°РЎвЂљР В° РЎРЊРЎвЂљР С‘Р С Р В¶Р Вµ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р С•Р С.
  //
  // Р СџРЎР‚Р ВµР В¶Р Р…РЎРЏРЎРЏ Р Р†Р ВµРЎР‚РЎРѓР С‘РЎРЏ Р С•РЎвЂљР Р†Р ВµРЎвЂЎР В°Р В»Р В° РЎвЂљР С•Р В»РЎРЉР С”Р С• Р С‘Р В· CacheStorage Р С‘ Р С•РЎвЂљР С”Р В»Р С•Р Р…РЎРЏР В»Р В° Р Р†РЎРѓРЎвЂ Р С•РЎРѓРЎвЂљР В°Р В»РЎРЉР Р…Р С•Р Вµ,
  // Р С—Р С•РЎРЊРЎвЂљР С•Р СРЎС“ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚, Р С”Р С•РЎвЂљР С•РЎР‚РЎвЂ№Р в„– Р С•Р В±Р Р…Р С•Р Р†Р В»РЎРЏР ВµРЎвЂљ Р С”РЎРЊРЎв‚¬ (Р’В«Р Т‘Р С•РЎРѓРЎвЂљР В°Р Р…РЎРЉ Р С‘Р В· РЎРѓР ВµРЎвЂљР С‘ РІР‚вЂќ Р С—Р С•Р В»Р С•Р В¶Р С‘ Р Р† Р С”РЎРЊРЎв‚¬Р’В»),
  // Р Р…Р Вµ Р СР С•Р С– Р Р…Р В°Р С—Р С•Р В»Р Р…Р С‘РЎвЂљРЎРЉ Р ВµР С–Р С• Р Р…Р С‘ РЎР‚Р В°Р В·РЎС“: Р ВµР С–Р С• Р С—Р ВµРЎР‚Р Р†РЎвЂ№Р в„– Р В¶Р Вµ `fetch` Р С•РЎвЂљР С”Р В»Р С•Р Р…РЎРЏР В»РЎРѓРЎРЏ, Р С‘ РЎвЂ Р ВµР С—Р С•РЎвЂЎР С”Р В°
  // `install` Р С•Р В±РЎР‚РЎвЂ№Р Р†Р В°Р В»Р В°РЎРѓРЎРЉ. Р РЋР ВµРЎвЂљРЎРЉ Р С‘Р Т‘РЎвЂРЎвЂљ Р СР С‘Р СР С• `FetchInterceptor` РІР‚вЂќ Р С‘Р Р…Р В°РЎвЂЎР Вµ Р В·Р В°Р С—РЎР‚Р С•РЎРѓ
  // Р Р†Р ВµРЎР‚Р Р…РЎС“Р В»РЎРѓРЎРЏ Р В±РЎвЂ№ Р Р† РЎРЊРЎвЂљР С•РЎвЂљ Р В¶Р Вµ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚, Р В° Р С•Р Р… РЎРѓРЎвЂљР С•Р С‘РЎвЂљ Р Р†Р Р…РЎС“РЎвЂљРЎР‚Р С‘ РЎРѓР Р†Р С•Р ВµР С–Р С• Р В¶Р Вµ `fetch`.
  globalThis.fetch = function(resource, init) {{
    var url = (typeof resource === 'string') ? resource : (resource && resource.url) || '';
    var method = (init && init.method) || (resource && resource.method) || 'GET';
    var abs = (url.indexOf('://') !== -1) ? url : new URL(url, location.href).href;
    var raw = _lumen_sw_net_fetch(abs, String(method).toUpperCase());
    if (!raw) {{
      // Р РЋР ВµРЎвЂљРЎРЉ Р Р…Р Вµ Р С•РЎвЂљР Р†Р ВµРЎвЂљР С‘Р В»Р В° РІР‚вЂќ Р С—Р С•РЎРѓР В»Р ВµР Т‘Р Р…Р С‘Р в„– РЎв‚¬Р В°Р Р…РЎРѓ Р С•РЎвЂљР Т‘Р В°РЎвЂљРЎРЉ Р С‘Р В· Р С”РЎРЊРЎв‚¬Р В°, Р С”Р В°Р С” РЎР‚Р В°Р Р…РЎРЉРЎв‚¬Р Вµ.
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

  // РІвЂќР‚РІвЂќР‚ ExtendableEvent (SW Р’В§4.5) / ExtendableMessageEvent (SW Р’В§4.7) РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚
  // SW-MSG-S2: `waitUntil` collects the lifetime promises of the dispatch in
  // flight; the Rust thread polls `_sw_extend_pending()` and keeps running
  // timers until it drops to zero or EXTEND_TIMEOUT. `waitUntil` outside a
  // dispatch with nothing still pending throws the spec InvalidStateError.
  var _extend_promises = [];
  var _extend_active = false;
  function _sw_ext_track(p) {{
    var pr = (p && typeof p.then === 'function') ? p : Promise.resolve(p);
    pr.then(function() {{ pr._sw_settled = true; }},
            function(reason) {{
              pr._sw_settled = true;
              // Read (and logged) by the Rust thread after `install` only РІР‚вЂќ
              // full failed-install behaviour is BUG-1742, not here.
              globalThis._sw_extend_error__ = String((reason && reason.message) || reason);
            }});
    _extend_promises.push(pr);
  }}
  function ExtendableEvent(type, init) {{
    Event.call(this, type, init);
  }}
  ExtendableEvent.prototype = Object.create(Event.prototype);
  ExtendableEvent.prototype.constructor = ExtendableEvent;
  Object.defineProperty(ExtendableEvent.prototype, Symbol.toStringTag, {{
    value: 'ExtendableEvent', configurable: true,
  }});
  ExtendableEvent.prototype.waitUntil = function(promise) {{
    if (!_extend_active && globalThis._sw_extend_pending() === 0) {{
      throw new DOMException(
        "Failed to execute 'waitUntil' on 'ExtendableEvent': The event has "
        + "already been dispatched or its lifetime has ended.",
        'InvalidStateError');
    }}
    _sw_ext_track(promise);
  }};
  globalThis.ExtendableEvent = ExtendableEvent;

  function ExtendableMessageEvent(type, init) {{
    ExtendableEvent.call(this, type, init);
    init = init || {{}};
    this.data = (init.data !== undefined) ? init.data : null;
    this.origin = (init.origin != null) ? String(init.origin) : '';
    this.lastEventId = (init.lastEventId != null) ? String(init.lastEventId) : '';
    var s = init.source;
    // SW Р’В§4.7: `source` is `Client`/`ServiceWorker`/`MessagePort` or null РІР‚вЂќ
    // anything else is a TypeError. `Client`/`ServiceWorker` classes may not
    // exist in this isolate yet (SW-MSG-S3/S5), so match by constructor name.
    var ok = (s == null)
      || (typeof MessagePort === 'function' && s instanceof MessagePort)
      || (s && typeof s === 'object'
          && /^(?:Window)?Client$|^ServiceWorker$/.test(
               String(s.constructor && s.constructor.name)));
    if (!ok) {{
      throw new TypeError(
        "Failed to construct 'ExtendableMessageEvent': member 'source' is "
        + "not of type Client, ServiceWorker or MessagePort.");
    }}
    this.source = (s === undefined) ? null : s;
    this.ports = Object.freeze(Array.isArray(init.ports) ? init.ports.slice() : []);
  }}
  ExtendableMessageEvent.prototype = Object.create(ExtendableEvent.prototype);
  ExtendableMessageEvent.prototype.constructor = ExtendableMessageEvent;
  Object.defineProperty(ExtendableMessageEvent.prototype, Symbol.toStringTag, {{
    value: 'ExtendableMessageEvent', configurable: true,
  }});
  globalThis.ExtendableMessageEvent = ExtendableMessageEvent;

  // How many `waitUntil` promises of the last dispatch are still unsettled.
  // Read by the Rust thread.
  globalThis._sw_extend_pending = function() {{
    var n = 0;
    for (var i = 0; i < _extend_promises.length; i++) {{
      if (!_extend_promises[i]._sw_settled) n++;
    }}
    return n;
  }};

  // SW-MSG-S2: dispatch an extendable event РІР‚вЂќ reset the collector, run the
  // listeners inside the dispatch window, close the window. The matching
  // `self.on<type>` handler runs inside the same window, so its `waitUntil`
  // calls are collected too.
  globalThis._sw_dispatch_extendable = function(evt) {{
    _extend_promises = [];
    _extend_active = true;
    try {{
      var fns = (_handlers[evt.type] || []).slice();
      for (var i = 0; i < fns.length; i++) {{
        try {{ fns[i].call(self, evt); }} catch(e) {{ }}
      }}
      var on = self['on' + evt.type];
      if (typeof on === 'function') {{
        try {{ on.call(self, evt); }} catch(e) {{ }}
      }}
    }} finally {{
      _extend_active = false;
    }}
  }};

  // _sw_fire_event: fire install/activate as ExtendableEvents.
  globalThis._sw_fire_event = function(type) {{
    globalThis._sw_dispatch_extendable(new ExtendableEvent(type));
  }};

  // РІвЂќР‚РІвЂќР‚ Client / WindowClient (SW Р’В§5.2/Р’В§5.3, SW-MSG-S3) РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚
  // The `source` of an incoming `message` event, rebuilt from the JSON
  // descriptor the page stamps onto the envelope. `postMessage` (the return
  // channel) landed in SW-MSG-S4; `focus()`/`navigate()` still arrive with
  // SW-MSG-S6.
  function Client(info) {{
    info = info || {{}};
    this.id = (info.id != null) ? String(info.id) : '';
    // SW-MSG-S4: the numeric client id of the posting page РІР‚вЂќ the key
    // `_lumen_sw_post_to_client` routes the reply by.
    this._cid = (typeof info.id === 'number') ? info.id : Number(info.id) || 0;
    this.url = String(info.url || '');
    this.type = String(info.type || 'window');
    this.frameType = String(info.frameType || 'top-level');
  }}
  globalThis.Client = Client;
  // SW-MSG-S4: the return channel (SW Р’В§3.2.5) РІР‚вЂќ posts into the process-global
  // hub under this page's client id; the page's own pump delivers it.
  // SW-MSG-S5: transferred `MessagePort`s ride as ids in a `__lumen_msg__`
  // envelope РІР‚вЂќ the same wire shape the page's `ServiceWorker.postMessage`
  // builds РІР‚вЂќ and the local partners bind through the hub's port outbox.
  // A dead client's id is simply never drained.
  Client.prototype.postMessage = function(data, transfer) {{
    if (typeof _lumen_sw_post_to_client !== 'function') return;
    var portIds = (typeof _lumen_port_prepare_transfer === 'function')
      ? _lumen_port_prepare_transfer(transfer, null) : [];
    var toSend = (portIds.length && typeof _lumen_port_walk_serialize === 'function')
      ? _lumen_port_walk_serialize(data) : data;
    var payload = JSON.stringify(structuredClone(toSend));
    if (portIds.length) {{
      payload = JSON.stringify({{ __lumen_msg__: true, data: JSON.parse(payload), ports: portIds }});
    }}
    _lumen_sw_post_to_client(this._cid, payload);
  }};
  function WindowClient(info) {{
    Client.call(this, info);
    info = info || {{}};
    this.visibilityState = String(info.visibilityState || 'visible');
    this.focused = !!info.focused;
  }}
  WindowClient.prototype = Object.create(Client.prototype);
  WindowClient.prototype.constructor = WindowClient;
  Object.defineProperty(WindowClient.prototype, Symbol.toStringTag, {{
    value: 'WindowClient', configurable: true,
  }});
  globalThis.WindowClient = WindowClient;
  // SW-MSG-S6: `WindowClient.focus()` (SW Р’В§5.3) resolves to `this` without
  // actually focusing the window (real focus is out of scope for the
  // slices); `navigate()`/`openWindow()` need shell navigation РІР‚вЂќ rejected.
  WindowClient.prototype.focus = function() {{
    return Promise.resolve(this);
  }};
  WindowClient.prototype.navigate = function() {{
    // SW-MSG: Р Р…РЎС“Р В¶Р Р…Р В° Р Р…Р В°Р Р†Р С‘Р С–Р В°РЎвЂ Р С‘РЎРЏ Р С•Р В±Р С•Р В»Р С•РЎвЂЎР С”Р С‘
    return Promise.reject(new DOMException(
      'WindowClient.navigate() is not supported.', 'NotSupportedError'));
  }};

  // SW-MSG-S3: dispatch a pageРІвЂ вЂ™SW message as an ExtendableMessageEvent (SW
  // Р’В§4.7). Data and the client descriptor arrive JSON-encoded (the same wire
  // shape the page side built); `origin` is this SW's own origin РІР‚вЂќ only
  // same-origin pages can post, so the page's origin equals it.
  // SW-MSG-S5: a `__lumen_msg__` envelope carries transferred ports РІР‚вЂќ reify
  // them (page-side `_lumenPortRegistry` ids) into `e.ports` and resolve port
  // sentinels embedded in `data`, exactly like the dedicated worker's
  // `_lumen_worker_dispatch_message`.
  globalThis._sw_fire_message = function(dataJson, sourceJson) {{
    var data = null;
    try {{ data = JSON.parse(dataJson); }} catch (e) {{ }}
    var ports = [];
    if (data && typeof data === 'object' && data.__lumen_msg__ === true) {{
      if (typeof _lumen_port_reify_list === 'function') ports = _lumen_port_reify_list(data.ports, null);
      if (typeof _lumen_port_walk_deserialize === 'function') data = _lumen_port_walk_deserialize(data.data, null);
    }}
    var info = {{}};
    try {{ info = JSON.parse(sourceJson) || {{}}; }} catch (e) {{ }}
    var evt = new ExtendableMessageEvent('message', {{
      data: data,
      origin: origin,
      source: new WindowClient(info),
      ports: ports,
    }});
    globalThis._sw_dispatch_extendable(evt);
  }};

  // SW-MSG-S5: the SW twin of the dedicated worker's
  // `_lumen_worker_deliver_port_message` РІР‚вЂќ a message posted by the page
  // endpoint of a bridge whose other end is a `MessagePort` inside this
  // isolate. `json` is the raw JSON text of the cloned payload.
  globalThis._sw_deliver_port_message = function(portId, json) {{
    if (typeof _lumenPortRegistry === 'undefined') return;
    var p = _lumenPortRegistry[portId];
    if (!p) return;
    var data;
    try {{ data = JSON.parse(json); }} catch(e) {{ data = null; }}
    if (typeof _lumen_port_walk_deserialize === 'function') data = _lumen_port_walk_deserialize(data, null);
    p._deliverOrQueue({{ type: 'message', data: data, target: p,
                        currentTarget: p, bubbles: false, cancelable: false }});
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
      waitUntil: function(p) {{ _sw_ext_track(p); }},
      preventDefault: function() {{}},
    }};
    for (var i = 0; i < fns.length; i++) {{
      try {{ fns[i](evt); }} catch(e) {{ }}
    }}
  }};

  // PushMessageData (Push API L1 Р’В§6) РІР‚вЂќ wraps the plaintext bytes handed over
  // by `_sw_fire_push`. Bytes cross the JS boundary as base64 (same reason as
  // everywhere else in this file), decoded once and cached on first access
  // to each accessor's own representation, per spec ("Get the data content").
  function PushMessageData(bytesB64) {{
    this._bin = _sw_b64_to_bin(bytesB64);
  }}
  PushMessageData.prototype.arrayBuffer = function() {{
    var b = this._bin, buf = new ArrayBuffer(b.length), view = new Uint8Array(buf);
    for (var i = 0; i < b.length; i++) view[i] = b.charCodeAt(i) & 0xFF;
    return buf;
  }};
  PushMessageData.prototype.text = function() {{ return _sw_bin_to_utf8(this._bin); }};
  PushMessageData.prototype.json = function() {{ return JSON.parse(this.text()); }};
  PushMessageData.prototype.blob = function() {{
    return new Blob([this.arrayBuffer()]);
  }};
  globalThis.PushMessageData = PushMessageData;

  // A minimal PushSubscription-shaped object for `pushsubscriptionchange`'s
  // `oldSubscription`/`newSubscription` (Push API L1 Р’В§5) РІР‚вЂќ not the page-side
  // `PushSubscription` class (`crate::push_api`'s shim, a different global
  // scope); this one only needs to carry the fields the spec puts on the
  // event, `endpoint` and `getKey()`.
  function _sw_make_subscription(endpoint, p256dhB64, authB64) {{
    return {{
      endpoint: endpoint,
      expirationTime: null,
      getKey: function(name) {{
        var b64 = name === 'p256dh' ? p256dhB64 : (name === 'auth' ? authB64 : null);
        if (!b64) return null;
        var b = _sw_b64_to_bin(b64), buf = new ArrayBuffer(b.length), view = new Uint8Array(buf);
        for (var i = 0; i < b.length; i++) view[i] = b.charCodeAt(i) & 0xFF;
        return buf;
      }},
    }};
  }}

  // _sw_fire_push: dispatch PushEvent (Push API L1 Р’В§5) as an ExtendableEvent
  // (SW-MSG-S2). No respondWith-style return value РІР‚вЂќ the SW's job is
  // `waitUntil(showNotification(...))` side effects, nothing the Rust side
  // reads back.
  globalThis._sw_fire_push = function(payloadB64) {{
    var evt = new ExtendableEvent('push');
    evt.data = new PushMessageData(payloadB64);
    globalThis._sw_dispatch_extendable(evt);
  }};

  // _sw_fire_push_subscription_change: dispatch PushSubscriptionChangeEvent
  // (Push API L1 Р’В§5) РІР‚вЂќ fired when `subscribe()` replaces an existing
  // subscription for this scope (see `crate::push_api`'s `subscribe` native).
  globalThis._sw_fire_push_subscription_change = function(
      oldEndpoint, oldP256dhB64, oldAuthB64, newEndpoint, newP256dhB64, newAuthB64) {{
    var evt = new ExtendableEvent('pushsubscriptionchange');
    evt.oldSubscription = _sw_make_subscription(oldEndpoint, oldP256dhB64, oldAuthB64);
    evt.newSubscription = _sw_make_subscription(newEndpoint, newP256dhB64, newAuthB64);
    globalThis._sw_dispatch_extendable(evt);
  }};

  // Console (BUG-1782): the full standard surface from `CONSOLE_SHIM`'s
  // factory; this scope has no console sink, so it stays silent as before.
  globalThis.console = _lumen_make_console(function(level, text) {{}});

  // queueMicrotask stub.
  globalThis.queueMicrotask = function(fn) {{ Promise.resolve().then(fn); }};

  // Timers come from the shim shared with the dedicated-worker scope
  // (`crate::worker_shims::WORKER_TIMERS_SHIM`, evaluated as its own IIFE right
  // after this one) РІР‚вЂќ deadline-ordered and driven by this thread's own wait
  // rather than only by an incoming message (SW-MSG-S1).

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
/// as base64 РІР‚вЂќ a JS string cannot carry arbitrary bytes intact.
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

// РІвЂќР‚РІвЂќР‚РІвЂќР‚ V8 backend port (Ph3 V8 migration S10) РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚РІвЂќР‚
//
// Mirrors `worker.rs`/`shared_worker.rs`'s V8 port: the SW thread owns a full
// `V8JsRuntime` instead of a bare `rquickjs::Runtime`/`Context`. Unlike the
// QuickJS version, no `flush_jobs`/`execute_pending_job` equivalent is
// needed here РІР‚вЂќ V8's microtask queue auto-runs (`MicrotasksPolicy::kAuto`,
// confirmed by the S3 slice notes in `docs/tasks/ph3-v8-migration.md`), so a
// `Promise` chain started by `_sw_fire_fetch`/`_sw_fire_event` fully drains
// by the time `V8JsRuntime::eval` returns РІР‚вЂќ verified empirically by
// `tests_v8::v8_sw_responds_from_cache` below (the QuickJS version's
// `caches.match(...).then(...)` chain needs the manual pump; the V8 version
// does not).
//
// `SwWorkerHandle`/`SwFetchRequest` (from `lumen_core::ext`) are reused
// unchanged РІР‚вЂќ plain channel plumbing, no engine-specific types.

/// V8 port of [`spawn_sw_worker`].
#[cfg(feature = "v8-backend")]
#[allow(clippy::expect_used)]  // РЎС“Р Р…Р В°РЎРѓР В»Р ВµР Т‘Р С•Р Р†Р В°Р Р…Р С•, docs/lint-policy.md Р’В§10
#[allow(clippy::too_many_arguments)]  // BUG-768 added determinism, same shape as worker.rs's spawn_worker_v8
pub(crate) fn spawn_sw_worker_v8(
    origin: String,
    scope: String,
    script: String,
    script_url: String,
    is_module: bool,
    cache_backend: Arc<dyn CacheBackend>,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    determinism: Option<crate::worker::WorkerDeterminism>,
) -> SwWorkerHandle {
    let (tx, rx) = std::sync::mpsc::channel::<SwWorkerMessage>();
    let thread_name = format!("lumen-sw-v8-{origin}{scope}");
    let handle = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            run_sw_thread_v8(origin, scope, script, script_url, is_module, rx, cache_backend, fetch_provider, idb_backend, determinism)
        })
        .expect("failed to spawn SW thread (v8)");
    SwWorkerHandle { tx, _thread: handle }
}

#[cfg(feature = "v8-backend")]
#[allow(clippy::too_many_arguments)]  // BUG-768 added determinism
fn run_sw_thread_v8(
    origin: String,
    scope: String,
    script: String,
    script_url: String,
    is_module: bool,
    rx: Receiver<SwWorkerMessage>,
    cache_backend: Arc<dyn CacheBackend>,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    determinism: Option<crate::worker::WorkerDeterminism>,
) {
    let rt = match V8JsRuntime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[sw {origin}{scope}] v8 RT init failed: {e:?}");
            return;
        }
    };

    // SW-MODULE: a module SW's `importScripts` must throw and its dynamic
    // `import()` must reject (SW Р’В§Run Service Worker) РІР‚вЂќ the shim checks the
    // flag, the ESM loader the thread-local denial.
    if is_module {
        let _ = rt.eval("globalThis._lumen_sw_is_module__ = true;");
    }

    if let Err(e) =
        install_sw_globals_v8(
            &rt,
            &origin,
            &scope,
            Arc::clone(&cache_backend),
            fetch_provider.clone(),
            idb_backend,
            determinism,
        )
    {
        eprintln!("[sw {origin}{scope}] v8 globals failed: {e:?}");
        return;
    }

    // SW-MODULE: the script of a `{type:'module'}` registration evaluates as
    // an ES module under its own URL (static imports fetch at install time
    // and join the byte-check input), same shape as a module dedicated worker.
    if is_module {
        // Static imports need the ESM loader's base+fetcher; dynamic `import()`
        // in a SW is forbidden by spec and must reject.
        rt.set_module_context(&script_url, fetch_provider.clone());
        rt.set_dynamic_import_denied(true);
        match rt.eval_module_at(&script_url, &script) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("[sw {origin}{scope}] v8 module script eval error: {e:?}");
                // Continue РІР‚вЂќ partial install may still handle some fetches.
            }
        }
    } else if let Err(e) = rt.eval(&script) {
        eprintln!("[sw {origin}{scope}] v8 script eval error: {e:?}");
        // Continue РІР‚вЂќ partial install may still handle some fetches.
    }

    // Fire install then activate. No manual microtask pump needed (V8 runs
    // its own queue to completion inside `eval`) РІР‚вЂќ see module doc comment.
    // SW-MSG-S2: each phase waits for its `waitUntil` promises (timers keep
    // running) before the next one starts.
    let _ = rt.eval("if(typeof _sw_fire_event==='function'){_sw_fire_event('install');}");
    drain_extend_v8(&rt, true);
    let _ = rt.eval("if(typeof _sw_fire_event==='function'){_sw_fire_event('activate');}");
    drain_extend_v8(&rt, false);

    // SW-MSG-S1: timerРІР‚вЂtask loop modelled on the dedicated worker's (BUGРІР‚вЂ815).
    loop {
        let wait = run_worker_tasks(&rt);
        let msg = match wait {
            Some(d) => match rx.recv_timeout(d) {
                Ok(m) => m,
                // Deadline reached with nothing incoming РІР‚вЂќ go round and let
                // `run_worker_tasks` fire the timer that woke us.
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(m) => m,
                Err(_) => break,
            },
        };
        match msg {
            SwWorkerMessage::Fetch(req) => {
                let body = dispatch_fetch_v8(&rt, &req.url, &req.method);
                let _ = req.response_tx.send(body);
            }
            SwWorkerMessage::Message(m) => {
                let _ = rt.set_global("_sw_msg_data__", lumen_core::JsValue::String(m.data_json));
                let _ =
                    rt.set_global("_sw_msg_source__", lumen_core::JsValue::String(m.source_json));
                let _ = rt.eval(
                    "if(typeof _sw_fire_message==='function'){\
                         _sw_fire_message(_sw_msg_data__,_sw_msg_source__);}",
                );
                // An ExtendableMessageEvent extends the worker's lifetime the
                // same way install/activate do (SW-MSG-S2's collector).
                drain_extend_v8(&rt, false);
            }
            SwWorkerMessage::PortPost(port_id, json) => {
                // SW-MSG-S5: message for a `MessagePort` reconstructed inside
                // this isolate from a page-side transfer РІР‚вЂќ the SW twin of the
                // dedicated worker's `_lumen_worker_deliver_port_message`.
                let _ = rt.set_global("_sw_port_id__", lumen_core::JsValue::Number(port_id as f64));
                let _ = rt.set_global("_sw_port_json__", lumen_core::JsValue::String(json));
                let _ = rt.eval(
                    "if(typeof _sw_deliver_port_message==='function'){\
                         _sw_deliver_port_message(_sw_port_id__,_sw_port_json__);}",
                );
            }
            SwWorkerMessage::Push(push) => dispatch_push_v8(&rt, &push.payload),
            SwWorkerMessage::PushSubscriptionChange(change) => {
                dispatch_push_subscription_change_v8(&rt, &change)
            }
        }
        // SW-MSG-S1: flush any timers that the dispatched message may have armed.
        let _ = rt.eval("if(typeof _lumen_flush_timers==='function')_lumen_flush_timers();");
    }
}

/// SW-MSG-S2: run the isolate's task loop until the last extendable dispatch
/// has no unsettled `waitUntil` promises left, capped at [`EXTEND_TIMEOUT`].
/// `log_rejected` is `install` only: a rejected promise there means a failed
/// install (full failed-install behaviour is BUG-1742), so it is logged.
#[cfg(feature = "v8-backend")]
fn drain_extend_v8(rt: &V8JsRuntime, log_rejected: bool) {
    let start = std::time::Instant::now();
    while matches!(
        rt.eval("typeof _sw_extend_pending==='function'?_sw_extend_pending():0"),
        Ok(lumen_core::JsValue::Number(n)) if n > 0.0
    ) {
        if start.elapsed() >= EXTEND_TIMEOUT {
            break;
        }
        // `None` РІР‚вЂќ no timer left that could settle anything (microtasks drain
        // inside each `eval`), so the promises can only be stuck.
        let Some(wait) = run_worker_tasks(rt) else {
            break;
        };
        std::thread::sleep(wait.min(Duration::from_millis(20)));
    }
    if log_rejected {
        if let Ok(lumen_core::JsValue::String(e)) =
            rt.eval("globalThis._sw_extend_error__ === undefined ? null : _sw_extend_error__")
        {
            eprintln!("[sw] waitUntil rejected during install: {e}");
        }
        let _ = rt.eval("globalThis._sw_extend_error__ = undefined;");
    }
}

/// V8 twin of [`dispatch_fetch`]. SW-MSG-S1: waits for async `respondWith`.
#[cfg(feature = "v8-backend")]
fn dispatch_fetch_v8(rt: &V8JsRuntime, url: &str, method: &str) -> Option<Vec<u8>> {
    let _ = rt.set_global("_sw_resp_body__", lumen_core::JsValue::Null);
    let _ = rt.set_global("_sw_req_url__", lumen_core::JsValue::String(url.to_string()));
    let _ = rt.set_global("_sw_req_method__", lumen_core::JsValue::String(method.to_string()));
    let _ = rt.eval(
        "if(typeof _sw_fire_fetch==='function'){_sw_fire_fetch(_sw_req_url__,_sw_req_method__);}",
    );

    // SW-MSG-S1: `respondWith` may hand over a promise that only settles after
    // its handler's own `setTimeout` fires. V8's `kAuto` microtask policy drains
    // everything that settles inside an `eval` (see the module comment above), so
    // an already-built body is there on the first read; otherwise the timer task
    // loop is what advances the clock until the body lands or `FETCH_TIMEOUT`
    // expires.
    let start = std::time::Instant::now();
    loop {
        match rt.eval("_sw_resp_body__") {
            Ok(lumen_core::JsValue::String(s)) => return Some(s.into_bytes()),
            Ok(lumen_core::JsValue::Null) => {}
            // A non-string, non-null body cannot become a string later.
            _ => return None,
        }
        let Some(wait) = run_worker_tasks(rt) else {
            // No timer can settle the promise any more.
            break;
        };
        let left = FETCH_TIMEOUT.saturating_sub(start.elapsed());
        if left.is_zero() {
            break;
        }
        std::thread::sleep(wait.min(left));
    }

    match rt.eval("_sw_resp_body__") {
        Ok(lumen_core::JsValue::String(s)) => Some(s.into_bytes()),
        _ => None,
    }
}

/// Dispatch a `push` event (Ph3 push-api РЎРѓРЎР‚Р ВµР В· 5): plaintext bytes cross as
/// base64, same convention as every other byte-carrying native in this file.
#[cfg(feature = "v8-backend")]
fn dispatch_push_v8(rt: &V8JsRuntime, payload: &[u8]) {
    let _ = rt.set_global(
        "_sw_push_payload__",
        lumen_core::JsValue::String(base64_encode(payload)),
    );
    let _ = rt.eval("if(typeof _sw_fire_push==='function'){_sw_fire_push(_sw_push_payload__);}");
}

/// Dispatch a `pushsubscriptionchange` event (Ph3 push-api РЎРѓРЎР‚Р ВµР В· 5).
#[cfg(feature = "v8-backend")]
fn dispatch_push_subscription_change_v8(
    rt: &V8JsRuntime,
    change: &lumen_core::ext::SwPushSubscriptionChangeMessage,
) {
    // Endpoints/keys are already plain strings (endpoint) or base64
    // (p256dh/auth) courtesy of `PushBackend::push_get`'s wire shape РІР‚вЂќ no
    // further encoding needed before crossing into JS.
    let args = [
        change.old.0.clone(),
        change.old.1.clone(),
        change.old.2.clone(),
        change.new.0.clone(),
        change.new.1.clone(),
        change.new.2.clone(),
    ];
    for (i, arg) in args.iter().enumerate() {
        let _ = rt.set_global(&format!("_sw_psc_arg{i}__"), lumen_core::JsValue::String(arg.clone()));
    }
    let _ = rt.eval(
        "if(typeof _sw_fire_push_subscription_change==='function'){\
             _sw_fire_push_subscription_change(_sw_psc_arg0__,_sw_psc_arg1__,_sw_psc_arg2__,\
                 _sw_psc_arg3__,_sw_psc_arg4__,_sw_psc_arg5__);}",
    );
}

/// V8 port of [`install_sw_globals`]. Registers the same three cache natives
/// (`_lumen_sw_cache_match`, `_lumen_sw_cache_put`, `_lumen_sw_cache_names`)
/// plus `atob`/`btoa` (non-throwing here, unlike `worker.rs`'s РІР‚вЂќ matching
/// [`install_sw_globals`]'s own `Option<String>`/plain-`String` signatures,
/// so the plain `into_v8_fnN` path is sufficient; no scoped native needed)
/// and evaluates the same globals shim JS used by the QuickJS SW thread.
#[cfg(feature = "v8-backend")]
#[allow(clippy::too_many_arguments)]  // BUG-768 added determinism, same shape as worker.rs's twin
fn install_sw_globals_v8(
    rt: &V8JsRuntime,
    origin: &str,
    scope: &str,
    cache_backend: Arc<dyn CacheBackend>,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    determinism: Option<crate::worker::WorkerDeterminism>,
) -> JsResult<()> {
    // Р РЋР ВµРЎвЂљРЎРЉ Р С•Р В±Р В»Р В°РЎРѓРЎвЂљР С‘ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В°: `importScripts` Р С‘ `fetch` Р Р†Р Р…РЎС“РЎвЂљРЎР‚Р С‘ Р Р…Р ВµР С–Р С•. Р С›РЎвЂљР Р†Р ВµРЎвЂљ Р ВµР Т‘Р ВµРЎвЂљ
    // Р С•Р Т‘Р Р…Р С•Р в„– JSON-РЎРѓРЎвЂљРЎР‚Р С•Р С”Р С•Р в„–, РЎвЂљР ВµР В»Р С• РІР‚вЂќ base64: РЎРѓР ВµРЎвЂљР ВµР Р†Р С•Р в„– Р С•РЎвЂљР Р†Р ВµРЎвЂљ Р Р…Р Вµ Р С•Р В±РЎРЏР В·Р В°Р Р… Р В±РЎвЂ№РЎвЂљРЎРЉ РЎвЂљР ВµР С”РЎРѓРЎвЂљР С•Р С,
    // Р В° РЎРѓРЎвЂљРЎР‚Р С•Р С”Р В° JS Р Р…Р Вµ Р С—Р ВµРЎР‚Р ВµР Р…Р С•РЎРѓР С‘РЎвЂљ Р С—РЎР‚Р С•Р С‘Р В·Р Р†Р С•Р В»РЎРЉР Р…РЎвЂ№Р Вµ Р В±Р В°Р в„–РЎвЂљРЎвЂ№.
    //
    // `fetch_bypassing_sw`, Р В° Р Р…Р Вµ `fetch_sync`: Р С•Р В±РЎвЂ№РЎвЂЎР Р…РЎвЂ№Р в„– Р С—РЎС“РЎвЂљРЎРЉ Р С•РЎвЂљР Т‘Р В°Р В» Р В±РЎвЂ№ Р В·Р В°Р С—РЎР‚Р С•РЎРѓ
    // Р С—Р ВµРЎР‚Р ВµРЎвЂ¦Р Р†Р В°РЎвЂљРЎвЂЎР С‘Р С”РЎС“, РЎвЂљР С•РЎвЂљ Р Р†РЎвЂ№Р В±РЎР‚Р В°Р В» Р В±РЎвЂ№ Р С—Р С• scope Р В­Р СћР С›Р Сћ Р вЂ“Р вЂў Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚ Р С‘ Р С—Р С•РЎРѓР В»Р В°Р В» Р В±РЎвЂ№ Р ВµР СРЎС“
    // РЎРѓР С•Р С•Р В±РЎвЂ°Р ВµР Р…Р С‘Р Вµ, Р С”Р С•РЎвЂљР С•РЎР‚Р С•Р С–Р С• Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚ Р Р…Р Вµ РЎР‚Р В°Р В·Р В±Р ВµРЎР‚РЎвЂРЎвЂљ РІР‚вЂќ Р С•Р Р… РЎРѓРЎвЂљР С•Р С‘РЎвЂљ Р Р†Р Р…РЎС“РЎвЂљРЎР‚Р С‘ РЎРѓР Р†Р С•Р ВµР С–Р С• Р В¶Р Вµ
    // Р В·Р В°Р С—РЎР‚Р С•РЎРѓР В°. Р СџР С•РЎвЂљР С•Р С” Р В¶Р Т‘Р В°Р В» Р В±РЎвЂ№ РЎРѓР В°Р С РЎРѓР ВµР В±РЎРЏ.
    // _lumen_sw_check_worker_src(url) РІвЂ вЂ™ true if `worker-src`/`default-src`
    // would refuse `url` as an `importScripts()` target inside the SW's own
    // execution.
    //
    // GAP-CSPENF РЎРѓРЎР‚Р ВµР В· 31: РЎРѓРЎР‚Р ВµР В· 28 gated `importScripts()` for dedicated/
    // shared workers but explicitly left the Service Worker's own
    // `importScripts` (this file, not `worker.rs`) unnarrowed РІР‚вЂќ its
    // constructor was never in scope for РЎРѓРЎР‚Р ВµР В· 13 either, so the whole call
    // path had no precheck at all. Reuses `worker::import_scripts_csp_blocked`
    // rather than re-deriving the `data:`/`blob:` skip logic РІР‚вЂќ same
    // I/O-free precheck shape, different runtime.
    {
        let provider = fetch_provider.clone();
        rt.register_native(
            "_lumen_sw_check_worker_src",
            into_v8_fn1(move |url: String| -> bool {
                crate::worker::import_scripts_csp_blocked(provider.as_deref(), &url)
            }),
        )?;
    }

    {
        let provider = fetch_provider.clone();
        rt.register_native(
            "_lumen_sw_net_fetch",
            crate::v8_compat::into_v8_fn2(move |url: String, method: String| -> Option<String> {
                let provider = provider.as_ref()?;
                let res = match provider.fetch_bypassing_sw(&url, &method) {
                    Ok(res) => res,
                    Err(e) => {
                        eprintln!("[sw] РЎРѓР ВµРЎвЂљРЎРЉ: {url}: {e}");
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

    // SW-MSG-S4: the return channel РІР‚вЂќ a SW's `Client.postMessage` lands here
    // keyed by the page client id stamped into every incoming message's
    // source descriptor; this worker's scope is captured (the page pump
    // routes by client id alone).
    {
        let sw_scope = scope.to_string();
        rt.register_native(
            "_lumen_sw_post_to_client",
            crate::v8_compat::into_v8_fn2(move |client_id: f64, data_json: String| {
                crate::sw_clients::push_out(client_id as u64, sw_scope.clone(), data_json);
            }),
        )?;
    }

    // SW-MSG-S6: `Clients` (SW Р’В§5.1) РІР‚вЂќ `matchAll`/`get` read the hub's
    // client registry scoped to this worker's origin; `claim` takes same-
    // origin clients in scope over and queues their `controllerchange`.
    {
        let orig = origin.to_string();
        let sc = scope.to_string();
        rt.register_native(
            "_lumen_sw_clients_match_all",
            crate::v8_compat::into_v8_fn2(
                move |include_uncontrolled: bool, type_filter: String| -> Option<String> {
                    crate::sw_clients::clients_json(
                        &orig,
                        &sc,
                        &type_filter,
                        include_uncontrolled,
                    )
                },
            ),
        )?;
    }
    {
        let orig = origin.to_string();
        rt.register_native(
            "_lumen_sw_clients_get",
            crate::v8_compat::into_v8_fn1(move |id: f64| -> Option<String> {
                crate::sw_clients::client_json(&orig, id as u64)
            }),
        )?;
    }
    {
        let orig = origin.to_string();
        let sc = scope.to_string();
        rt.register_native(
            "_lumen_sw_clients_claim",
            crate::v8_compat::into_v8_fn0(move || -> u32 {
                crate::sw_clients::claim(&orig, &sc)
            }),
        )?;
    }

    // SW-MSG-S5: page РІвЂ вЂ™ port delivery inside this isolate (the SW twin of the
    // dedicated worker's `_lumen_port_post_reply`) РІР‚вЂќ posts into the hub's
    // port-addressed outbox; the page's pump routes by port id alone.
    {
        rt.register_native(
            "_lumen_port_post_reply",
            crate::v8_compat::into_v8_fn2(move |port_id: u32, json: String| {
                crate::sw_clients::push_out_port(port_id, json);
            }),
        )?;
    }
    // SW-MSG-S5: port transfer ids. The page and its dedicated workers share
    // one per-runtime counter; the SW thread is spawned outside it, so it
    // allocates from a process-global one instead.
    //
    // shortcut: base offset 1<<30 instead of sharing each page's exact
    // counter (that would mean plumbing an Arc through ext.rs's
    // SwWorkerStore) РІР‚вЂќ page windows are 1M wide, so this stays collision-free
    // for the first ~4096 pages per session; revisit if that bites.
    {
        static SW_PORT_NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1 << 30);
        rt.register_native(
            "_lumen_next_port_id",
            crate::v8_compat::into_v8_fn0(move || -> u32 {
                SW_PORT_NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            }),
        )?;
    }

    // BUG-1193: the same HTML LS Р’В§8.3 `atob`/`btoa` as the dedicated worker
    // (binary strings, forgiving-base64, `InvalidCharacterError`). The throwing
    // wrapper needs `DOMException`, which `install_worker_scope_globals_v8`
    // below provides РІР‚вЂќ it is only looked up at call time.
    rt.register_native_scoped("_lumen_atob_impl", Box::new(crate::worker::atob_native_v8))?;
    rt.register_native_scoped("_lumen_btoa_impl", Box::new(crate::worker::btoa_native_v8))?;
    rt.eval(crate::worker::WORKER_ATOB_BTOA_SHIM)?;

    // `ServiceWorkerGlobalScope` is a `WorkerGlobalScope` too, so it gets the
    // same `EventTarget`/`performance` surface as the dedicated worker (BUG-401).
    crate::worker::install_worker_scope_globals_v8(rt, determinism)?;

    // IndexedDB РІР‚вЂќ РЎвЂљР С•РЎвЂљ Р В¶Р Вµ Р В±Р В»Р С•Р С” РЎв‚¬Р С‘Р СР В°, РЎвЂЎРЎвЂљР С• РЎС“ РЎРѓРЎвЂљРЎР‚Р В°Р Р…Р С‘РЎвЂ РЎвЂ№ (`[Exposed=(Window,Worker)]`),
    // Р С‘ РЎвЂљР В° Р В¶Р Вµ Р В±Р В°Р В·Р В°: Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚, Р С”Р С•РЎвЂљР С•РЎР‚РЎвЂ№Р в„– Р С—Р С‘РЎв‚¬Р ВµРЎвЂљ РЎРѓР Р†Р С•РЎР‹ Р С•РЎвЂЎР ВµРЎР‚Р ВµР Т‘РЎРЉ Р Р† `indexedDB`, Р С‘Р Р…Р В°РЎвЂЎР Вµ
    // РЎС“Р СР С‘РЎР‚Р В°Р В» Р Р…Р В° Р С—Р ВµРЎР‚Р Р†Р С•Р в„– РЎРѓРЎвЂљРЎР‚Р С•Р С”Р Вµ Р С•Р В±РЎР‚Р В°РЎвЂ°Р ВµР Р…Р С‘РЎРЏ Р С” Р Р…Р ВµР в„–. Р СњР В°РЎвЂљР С‘Р Р†РЎвЂ№ РЎРѓРЎвЂљР В°Р Р†РЎРЏРЎвЂљРЎРѓРЎРЏ РЎвЂљР С•Р В»РЎРЉР С”Р С• Р С—РЎР‚Р С‘
    // Р Р…Р В°Р В»Р С‘РЎвЂЎР С‘Р С‘ backend-Р В° РІР‚вЂќ Р В±Р ВµР В· Р Р…Р ВµР С–Р С• РЎв‚¬Р С‘Р С Р Т‘Р ВµРЎР‚Р В¶Р С‘РЎвЂљ Р В±Р В°Р В·РЎС“ Р Р† Р С”РЎС“РЎвЂЎР Вµ (Р ВµР С–Р С• РЎРѓР С•Р В±РЎРѓРЎвЂљР Р†Р ВµР Р…Р Р…РЎвЂ№Р Вµ
    // Р С•РЎвЂ¦РЎР‚Р В°Р Р…РЎвЂ№ `typeof РІР‚В¦ === 'function'`).
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

    // Timers (`setTimeout`/`setInterval`/`queueMicrotask`) come from the shim
    // shared with the dedicated-worker scope (SW-MSG-S1).
    rt.eval(crate::worker_shims::WORKER_TIMERS_SHIM)?;

    let scope_js = scope.replace('\'', "\\'");
    let scope_str = format!("'{scope_js}'");
    // `scope` Р С—РЎР‚Р С‘РЎвЂ¦Р С•Р Т‘Р С‘РЎвЂљ Р С—РЎС“РЎвЂљРЎвЂР С (`/invest/`), Р В° `WorkerLocation` Р С•Р В±РЎРЏР В·Р В°Р Р… Р В±РЎвЂ№РЎвЂљРЎРЉ
    // Р В°Р В±РЎРѓР С•Р В»РЎР‹РЎвЂљР Р…РЎвЂ№Р С Р В°Р Т‘РЎР‚Р ВµРЎРѓР С•Р С РІР‚вЂќ Р С‘Р Р…Р В°РЎвЂЎР Вµ `location.host` Р С—РЎС“РЎРѓРЎвЂљ, Р С‘ Р Р†Р ВµРЎвЂљР Р†Р В»Р ВµР Р…Р С‘Р Вµ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В° Р С—Р С•
    // РЎвЂ¦Р С•РЎРѓРЎвЂљРЎС“ Р СР С•Р В»РЎвЂЎР В° Р Р†РЎвЂ№Р В±Р С‘РЎР‚Р В°Р ВµРЎвЂљ Р Р…Р Вµ РЎвЂљРЎС“ Р Р†Р ВµРЎвЂљР С”РЎС“.
    let origin_js = origin.trim_end_matches('/').replace('\'', "\\'");
    let origin_str = format!("'{origin_js}'");
    // `Headers` (BUG-748) РІР‚вЂќ the real class shared with the page scope РІР‚вЂќ already
    // came from `install_worker_scope_globals_v8` above (WORKER-1 РЎРѓРЎР‚Р ВµР В· 5); the
    // shim below constructs it via `new Headers`.
    rt.eval(&sw_globals_shim(&scope_str, &origin_str))?;
    // `MessageChannel`/`MessagePort` already came from
    // `install_worker_scope_globals_v8` above (GAP-WORKERSCOPE); nothing left
    // to evaluate for them here.
    rt.eval(crate::dom::IDB_SHIM)?;
    Ok(())
}

/// Test coverage for the SW execution thread (Ph3 V8 migration S10, extended
/// to full parity in S12b-B17 when the rquickjs-backed `spawn_sw_worker` was
/// removed). Same three scenarios (cache hit, cache miss, no fetch handler) РІР‚вЂќ
/// the cache-hit case is the load-bearing proof that `_sw_fire_fetch`'s
/// `respondWith(caches.match(...))` promise chain fully resolves by the time
/// `dispatch_fetch_v8` reads `_sw_resp_body__`, with no manual microtask
/// pump (see the V8-port module doc comment above `spawn_sw_worker_v8`).
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // Р ТђР ВµР В»Р С—Р ВµРЎР‚РЎвЂ№ РЎвЂљР ВµРЎРѓРЎвЂљР С•Р Р†Р С•Р С–Р С• Р СР С•Р Т‘РЎС“Р В»РЎРЏ: Р С‘РЎРѓР С”Р В»РЎР‹РЎвЂЎР ВµР Р…Р С‘Р Вµ Р С‘Р В· clippy.toml Р С—Р С•Р С”РЎР‚РЎвЂ№Р Р†Р В°Р ВµРЎвЂљ
    // РЎвЂљР С•Р В»РЎРЉР С”Р С• РЎвЂљР ВµР В»Р С• `#[test]` (docs/lint-policy.md Р’В§10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use lumen_core::ext::CacheBackend;
    use std::sync::Mutex;

    /// BUG-401: `ServiceWorkerGlobalScope` is a `WorkerGlobalScope` too, so it
    /// gets the same `performance` as the page and the other worker flavours РІР‚вЂќ
    /// same shim source, hence the same prototype chain.
    #[test]
    fn sw_global_scope_has_performance() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None).unwrap();
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

    /// BUG-766: `ServiceWorkerGlobalScope.isSecureContext` follows the same
    /// rule as the other two worker flavours' РІР‚вЂќ see `worker.rs`'s sibling
    /// test on the dedicated scope.
    #[test]
    fn sw_global_scope_has_is_secure_context() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
            .unwrap();
        assert_eq!(
            rt.eval("self.isSecureContext").unwrap(),
            lumen_core::JsValue::Bool(true)
        );
    }

    /// `WorkerLocation` РЎвЂ Р ВµР В»Р С‘Р С”Р С•Р С + `URL`/`URLSearchParams` Р Р† Р С•Р В±Р В»Р В°РЎРѓРЎвЂљР С‘ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В°:
    /// РЎР‚Р С•Р Р†Р Р…Р С• РЎвЂљР С•, РЎвЂЎР ВµР С РЎРѓР ВµРЎР‚Р Р†Р С‘РЎРѓ-Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚РЎвЂ№ Р Р†Р ВµРЎвЂљР Р†РЎРЏРЎвЂљРЎРѓРЎРЏ Р Р…Р В° Р Р†Р ВµРЎР‚РЎвЂ¦Р Р…Р ВµР С РЎС“РЎР‚Р С•Р Р†Р Р…Р Вµ. Р СџР С•Р С”Р В° Р С—Р С•Р В»Р ВµР в„– Р Р…Р Вµ
    /// Р В±РЎвЂ№Р В»Р С•, `self.location.host.includes(...)` РЎР‚Р С•Р Р…РЎРЏР В»Р С• РЎС“РЎРѓРЎвЂљР В°Р Р…Р С•Р Р†Р С”РЎС“ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В°
    /// РЎвЂ Р ВµР В»Р С‘Р С”Р С•Р С (Р В¶Р С‘Р Р†Р С•Р в„– Р С—РЎР‚Р С‘Р СР ВµРЎР‚ РІР‚вЂќ `sw.js` t-Р В±Р В°Р Р…Р С”Р В°, 2026-08-17).
    #[test]
    fn sw_global_scope_has_full_location_and_url_classes() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://cdn.example.com:8443", "/invest/", MockCache::new(), None, None, None)
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

    /// Р РЋР ВµРЎвЂљР ВµР Р†Р С•Р в„– Р Т‘Р Р†Р С•Р в„–Р Р…Р С‘Р С” Р С•Р В±Р В»Р В°РЎРѓРЎвЂљР С‘ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В°. Р В Р В°Р В·Р В»Р С‘РЎвЂЎР В°Р ВµРЎвЂљ Р Т‘Р Р†Р В° Р Р†РЎвЂ¦Р С•Р Т‘Р В° Р С—РЎР‚Р С•Р Р†Р В°Р в„–Р Т‘Р ВµРЎР‚Р В°:
    /// Р С•Р В±РЎвЂ№РЎвЂЎР Р…РЎвЂ№Р в„– `fetch_sync` (РЎвЂЎР ВµРЎР‚Р ВµР В· Р Р…Р ВµР С–Р С• Р В·Р В°Р С—РЎР‚Р С•РЎРѓ Р С—Р С•Р С—Р В°Р В» Р В±РЎвЂ№ Р Р† Р С—Р ВµРЎР‚Р ВµРЎвЂ¦Р Р†Р В°РЎвЂљРЎвЂЎР С‘Р С”, РЎвЂљР С• Р ВµРЎРѓРЎвЂљРЎРЉ
    /// Р С•Р В±РЎР‚Р В°РЎвЂљР Р…Р С• Р Р† РЎРЊРЎвЂљР С•РЎвЂљ Р В¶Р Вµ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚) Р С‘ `fetch_bypassing_sw`. Р СћР ВµР В»Р В° РЎР‚Р В°Р В·Р Р…РЎвЂ№Р Вµ РІР‚вЂќ Р С—Р С•РЎРЊРЎвЂљР С•Р СРЎС“
    /// РЎвЂљР ВµРЎРѓРЎвЂљ Р Р†Р С‘Р Т‘Р С‘РЎвЂљ, Р С”Р В°Р С”Р С‘Р С Р С—РЎС“РЎвЂљРЎвЂР С РЎС“РЎв‚¬РЎвЂР В» Р В·Р В°Р С—РЎР‚Р С•РЎРѓ, Р В° Р Р…Р Вµ РЎвЂљР С•Р В»РЎРЉР С”Р С• РЎвЂЎРЎвЂљР С• Р’В«РЎвЂЎРЎвЂљР С•-РЎвЂљР С• Р Р†Р ВµРЎР‚Р Р…РЎС“Р В»Р С•РЎРѓРЎРЉР’В».
    struct SwNet {
        /// URL РІвЂ вЂ™ РЎвЂљР ВµР В»Р С•, Р С•РЎвЂљР Т‘Р В°Р Р†Р В°Р ВµР СР С•Р Вµ Р С•Р В±РЎвЂ¦Р С•Р Т‘Р Р…РЎвЂ№Р С Р С—РЎС“РЎвЂљРЎвЂР С.
        bodies: std::collections::HashMap<String, String>,
        /// Р РЋР С”Р С•Р В»РЎРЉР С”Р С• РЎР‚Р В°Р В· Р С—Р С•Р В·Р Р†Р В°Р В»Р С‘ Р С•Р В±РЎвЂ№РЎвЂЎР Р…РЎвЂ№Р в„– Р С—РЎС“РЎвЂљРЎРЉ (Р Т‘Р С•Р В»Р В¶Р ВµР Р… Р С•РЎРѓРЎвЂљР В°РЎвЂљРЎРЉРЎРѓРЎРЏ Р Р…РЎС“Р В»РЎвЂР С).
        via_intercepted: Mutex<usize>,
    }

    impl SwNet {
        fn new(bodies: &[(&str, &str)]) -> Arc<Self> {
            Arc::new(Self {
                bodies: bodies.iter().map(|(u, b)| ((*u).to_owned(), (*b).to_owned())).collect(),
                via_intercepted: Mutex::new(0),
            })
        }

        /// Р РЋР С”Р С•Р В»РЎРЉР С”Р С• Р В·Р В°Р С—РЎР‚Р С•РЎРѓР С•Р Р† РЎС“РЎв‚¬Р В»Р С• Р С—Р ВµРЎР‚Р ВµРЎвЂ¦Р Р†Р В°РЎвЂљРЎвЂ№Р Р†Р В°Р ВµР СРЎвЂ№Р С Р С—РЎС“РЎвЂљРЎвЂР С.
        fn intercepted(&self) -> usize {
            self.via_intercepted.lock().map(|n| *n).unwrap_or(usize::MAX)
        }
    }

    impl lumen_core::ext::JsFetchProvider for SwNet {
        fn fetch_sync(
            &self,
            url: &str,
            _method: &str,
        ) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
            if let Ok(mut n) = self.via_intercepted.lock() {
                *n += 1;
            }
            Ok(lumen_core::ext::JsFetchResult {
                status: 200,
                status_text: "OK".into(),
                headers: vec![],
                body: "Р С—РЎС“РЎвЂљРЎРЉ РЎвЂЎР ВµРЎР‚Р ВµР В· Р С—Р ВµРЎР‚Р ВµРЎвЂ¦Р Р†Р В°РЎвЂљРЎвЂЎР С‘Р С”".as_bytes().to_vec(),
                url: url.to_string(),
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
                    url: url.to_string(),
                }),
                None => Ok(lumen_core::ext::JsFetchResult {
                    status: 404,
                    status_text: "Not Found".into(),
                    headers: vec![],
                    body: Vec::new(),
                    url: url.to_string(),
                }),
            }
        }
    }

    /// Р В Р В°Р Р…РЎвЂљР В°Р в„–Р С Р С•Р В±Р В»Р В°РЎРѓРЎвЂљР С‘ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В° РЎРѓ РЎРѓР ВµРЎвЂљРЎРЉРЎР‹.
    fn sw_rt(origin: &str, scope: &str, net: &Arc<SwNet>) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        let provider: Arc<dyn lumen_core::ext::JsFetchProvider> = Arc::clone(net) as _;
        install_sw_globals_v8(&rt, origin, scope, MockCache::new(), Some(provider), None, None).unwrap();
        rt
    }

    /// `importScripts` Р С—Р С• РЎРѓР ВµРЎвЂљР С‘. Р вЂќР С• Р С—РЎР‚Р В°Р Р†Р С”Р С‘ РЎвЂћРЎС“Р Р…Р С”РЎвЂ Р С‘Р С‘ Р Р† Р С•Р В±Р В»Р В°РЎРѓРЎвЂљР С‘ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В° Р Р…Р Вµ Р В±РЎвЂ№Р В»Р С•
    /// Р Р†Р С•Р Р†РЎРѓР Вµ, Р С‘ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚, Р С—Р С•Р Т‘Р С”Р В»РЎР‹РЎвЂЎР В°РЎР‹РЎвЂ°Р С‘Р в„– Р В±Р С‘Р В±Р В»Р С‘Р С•РЎвЂљР ВµР С”РЎС“ (push-SDK t-Р В±Р В°Р Р…Р С”Р В°), РЎС“Р СР С‘РЎР‚Р В°Р В» Р Р…Р В°
    /// Р С—Р ВµРЎР‚Р Р†Р С•Р в„– Р В¶Р Вµ РЎРѓРЎвЂљРЎР‚Р С•Р С”Р Вµ РЎРѓ `importScripts is not defined` РІР‚вЂќ РЎвЂљР С• Р ВµРЎРѓРЎвЂљРЎРЉ Р Р…Р Вµ
    /// РЎР‚Р ВµР С–Р С‘РЎРѓРЎвЂљРЎР‚Р С‘РЎР‚Р С•Р Р†Р В°Р В» Р Р…Р С‘ Р С•Р Т‘Р Р…Р С•Р С–Р С• Р С•Р В±РЎР‚Р В°Р В±Р С•РЎвЂљРЎвЂЎР С‘Р С”Р В°.
    #[test]
    fn sw_import_scripts_loads_over_network() {
        let net = SwNet::new(&[(
            "https://cdn.example.com/lib.js",
            "globalThis.__lib_loaded = 'Р Т‘Р В°';",
        )]);
        let rt = sw_rt("https://example.com", "/", &net);
        rt.eval("importScripts('https://cdn.example.com/lib.js');").unwrap();
        assert_eq!(
            rt.eval("globalThis.__lib_loaded").unwrap(),
            lumen_core::JsValue::String("Р Т‘Р В°".into())
        );
        assert_eq!(net.intercepted(), 0, "Р В·Р В°Р С—РЎР‚Р С•РЎРѓ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В° РЎС“РЎв‚¬РЎвЂР В» Р Р† Р С—Р ВµРЎР‚Р ВµРЎвЂ¦Р Р†Р В°РЎвЂљРЎвЂЎР С‘Р С”");
    }

    /// Р С›РЎвЂљР Р…Р С•РЎРѓР С‘РЎвЂљР ВµР В»РЎРЉР Р…РЎвЂ№Р в„– Р В°Р Т‘РЎР‚Р ВµРЎРѓ РЎРѓРЎвЂЎР С‘РЎвЂљР В°Р ВµРЎвЂљРЎРѓРЎРЏ Р С•РЎвЂљ Р В°Р Т‘РЎР‚Р ВµРЎРѓР В° РЎРѓР В°Р СР С•Р С–Р С• Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В°.
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

    /// Р С›РЎвЂљР С”Р В°Р В· РЎРѓР ВµРЎвЂљР С‘ Р С•Р В±РЎРЏР В·Р В°Р Р… Р В±РЎвЂ№РЎвЂљРЎРЉ Р С‘РЎРѓР С”Р В»РЎР‹РЎвЂЎР ВµР Р…Р С‘Р ВµР С, Р В° Р Р…Р Вµ Р СР С•Р В»РЎвЂЎР В°Р В»Р С‘Р Р†РЎвЂ№Р С Р С—РЎР‚Р С•Р С—РЎС“РЎРѓР С”Р С•Р С: Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚ РЎРѓ
    /// Р Р…Р ВµР Т‘Р С•Р С–РЎР‚РЎС“Р В¶Р ВµР Р…Р Р…Р С•Р в„– Р В±Р С‘Р В±Р В»Р С‘Р С•РЎвЂљР ВµР С”Р С•Р в„– РЎвЂ¦РЎС“Р В¶Р Вµ, РЎвЂЎР ВµР С Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚, Р С”Р С•РЎвЂљР С•РЎР‚РЎвЂ№Р в„– РЎвЂЎР ВµРЎРѓРЎвЂљР Р…Р С• Р Р…Р Вµ Р Р†РЎРѓРЎвЂљР В°Р В».
    #[test]
    fn sw_import_scripts_throws_on_http_error() {
        let net = SwNet::new(&[]);
        let rt = sw_rt("https://example.com", "/", &net);
        assert!(rt.eval("importScripts('https://example.com/gone.js');").is_err());
    }

    /// GAP-CSPENF РЎРѓРЎР‚Р ВµР В· 31: `worker-src`/`default-src` gates the Service
    /// Worker's own `importScripts()`, mirroring `worker.rs`'s
    /// `CspBlockedImportNet` РІР‚вЂќ `check_worker_src` refuses unconditionally
    /// while `fetch_bypassing_sw` would succeed if reached, proving the
    /// precheck short-circuits before any network I/O.
    struct CspBlockedSwNet;
    impl lumen_core::ext::JsFetchProvider for CspBlockedSwNet {
        fn fetch_sync(&self, url: &str, _method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
            Ok(lumen_core::ext::JsFetchResult {
                status: 200,
                status_text: "OK".into(),
                headers: vec![],
                body: b"globalThis.__unreachable = true;".to_vec(),
                url: url.to_string(),
            })
        }
        fn fetch_bypassing_sw(&self, url: &str, _method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
            self.fetch_sync(url, _method)
        }
        fn check_worker_src(&self, _url: &str) -> lumen_core::error::Result<()> {
            Err(lumen_core::error::Error::CspWorkerSrcBlocked {
                blocked_uri: "https://blocked.example/lib.js".into(),
                original_policy: "worker-src 'none'".into(),
            })
        }
    }

    #[test]
    fn sw_import_scripts_blocked_by_worker_src_never_reaches_fetch() {
        let rt = V8JsRuntime::new().unwrap();
        let provider: Arc<dyn lumen_core::ext::JsFetchProvider> = Arc::new(CspBlockedSwNet);
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), Some(provider), None, None).unwrap();

        let err = rt.eval("importScripts('https://blocked.example/lib.js')");
        assert!(err.is_err(), "a worker-src-blocked importScripts() must throw");
        assert_eq!(
            rt.eval("typeof __unreachable").unwrap(),
            lumen_core::JsValue::String("undefined".into()),
            "the blocked script body must never execute",
        );
    }

    /// No `worker-src` policy installed at all (default `JsFetchProvider`
    /// impl) must never be treated as a block.
    #[test]
    fn sw_import_scripts_allowed_when_no_worker_src_policy() {
        let net = SwNet::new(&[("https://cdn.example.com/lib.js", "globalThis.__lib_loaded = 'Р Т‘Р В°';")]);
        let rt = sw_rt("https://example.com", "/", &net);
        rt.eval("importScripts('https://cdn.example.com/lib.js');").unwrap();
        assert_eq!(
            rt.eval("globalThis.__lib_loaded").unwrap(),
            lumen_core::JsValue::String("Р Т‘Р В°".into())
        );
    }

    /// `fetch` Р Р† Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р Вµ РІР‚вЂќ Р Р…Р В°РЎРѓРЎвЂљР С•РЎРЏРЎвЂ°Р В°РЎРЏ РЎРѓР ВµРЎвЂљРЎРЉ Р С‘ Р СР С‘Р СР С• Р С—Р ВµРЎР‚Р ВµРЎвЂ¦Р Р†Р В°РЎвЂљРЎвЂЎР С‘Р С”Р В°. Р СџРЎР‚Р ВµР В¶Р Р…РЎРЏРЎРЏ Р Р†Р ВµРЎР‚РЎРѓР С‘РЎРЏ
    /// Р С•РЎвЂљР Р†Р ВµРЎвЂЎР В°Р В»Р В° РЎвЂљР С•Р В»РЎРЉР С”Р С• Р С‘Р В· CacheStorage, Р С—Р С•РЎРЊРЎвЂљР С•Р СРЎС“ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚ Р Р…Р Вµ Р СР С•Р С– Р СњР С’Р СџР С›Р вЂєР СњР ВР СћР В¬ Р С”РЎРЊРЎв‚¬:
    /// Р ВµР С–Р С• Р С—Р ВµРЎР‚Р Р†РЎвЂ№Р в„– Р В¶Р Вµ `fetch` Р С•РЎвЂљР С”Р В»Р С•Р Р…РЎРЏР В»РЎРѓРЎРЏ.
    #[test]
    fn sw_fetch_goes_to_network_bypassing_interception() {
        let net = SwNet::new(&[("https://example.com/api", "РЎвЂљР ВµР В»Р С• Р С‘Р В· РЎРѓР ВµРЎвЂљР С‘")]);
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
            lumen_core::JsValue::String("РЎвЂљР ВµР В»Р С• Р С‘Р В· РЎРѓР ВµРЎвЂљР С‘".into())
        );
        assert_eq!(net.intercepted(), 0, "fetch Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В° РЎС“РЎв‚¬РЎвЂР В» Р Р† Р С—Р ВµРЎР‚Р ВµРЎвЂ¦Р Р†Р В°РЎвЂљРЎвЂЎР С‘Р С”");
    }

    /// Р вЂ”Р В°Р С–Р С•Р В»Р С•Р Р†Р С”Р С‘ Р С•РЎвЂљР Р†Р ВµРЎвЂљР В° Р Т‘Р С•Р ВµР В·Р В¶Р В°РЎР‹РЎвЂљ Р Т‘Р С• `Response.headers` РІР‚вЂќ Р С—Р С• Р Р…Р С‘Р С Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚ РЎР‚Р ВµРЎв‚¬Р В°Р ВµРЎвЂљ,
    /// Р С”Р В»Р В°РЎРѓРЎвЂљРЎРЉ Р В»Р С‘ Р С•РЎвЂљР Р†Р ВµРЎвЂљ Р Р† Р С”РЎРЊРЎв‚¬.
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

    /// BUG-748: РЎРѓР С”Р С•РЎС“Р С— service worker Р С—Р С•Р В»РЎС“РЎвЂЎР В°Р ВµРЎвЂљ РЎвЂљР С•РЎвЂљ Р В¶Р Вµ `Headers`, РЎвЂЎРЎвЂљР С• Р С‘
    /// РЎРѓРЎвЂљРЎР‚Р В°Р Р…Р С‘РЎвЂ Р В° (`crate::dom::HEADERS_SHIM`), Р В° Р Р…Р Вµ Р С•РЎвЂљР Т‘Р ВµР В»РЎРЉР Р…РЎвЂ№Р в„– Р СР С‘Р Р…Р С‘-РЎв‚¬Р С‘Р С Р Р…Р В°
    /// Р С—Р В»Р С•РЎРѓР С”Р С•Р С Р С•Р В±РЎР‰Р ВµР С”РЎвЂљР Вµ РІР‚вЂќ `append`/`forEach`/Р С‘РЎвЂљР ВµРЎР‚Р В°РЎвЂ Р С‘РЎРЏ Р С—РЎР‚Р С‘РЎРѓРЎС“РЎвЂљРЎРѓРЎвЂљР Р†РЎС“РЎР‹РЎвЂљ,
    /// Р С•Р Т‘Р Р…Р С•Р С‘Р СРЎвЂР Р…Р Р…РЎвЂ№Р Вµ Р В·Р В°Р С–Р С•Р В»Р С•Р Р†Р С”Р С‘ Р Р…Р Вµ РЎРѓРЎвЂ¦Р В»Р С•Р С—РЎвЂ№Р Р†Р В°РЎР‹РЎвЂљРЎРѓРЎРЏ, Р В° `clone()` Р С—Р ВµРЎР‚Р ВµР Р…Р С•РЎРѓР С‘РЎвЂљ Р С‘РЎвЂ¦
    /// Р С—Р С•Р В»Р Р…Р С•РЎРѓРЎвЂљРЎРЉРЎР‹ (РЎР‚Р В°Р Р…РЎРЉРЎв‚¬Р Вµ Р С•Р С—Р С‘РЎР‚Р В°Р В»РЎРѓРЎРЏ Р Р…Р В° Р С—РЎР‚Р С‘Р Р†Р В°РЎвЂљР Р…Р С•Р Вµ Р С—Р С•Р В»Р Вµ `_h`, Р С”Р С•РЎвЂљР С•РЎР‚Р С•Р С–Р С• Р В±Р С•Р В»РЎРЉРЎв‚¬Р Вµ Р Р…Р ВµРЎвЂљ).
    #[test]
    fn sw_headers_support_full_fetch_api() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
            .unwrap();
        rt.eval(
            "var h = new Headers();
             h.append('Set-Cookie', 'a=1');
             h.append('Set-Cookie', 'b=2');
             globalThis.__hasIter = typeof h[Symbol.iterator] === 'function';
             globalThis.__hasAppend = typeof h.append === 'function';
             var seen = [];
             h.forEach(function(v, k) { seen.push(k + '=' + v); });
             globalThis.__forEach = seen.join('|');
             globalThis.__getSetCookie = h.getSetCookie().join('|');
             var r = new Response('body', { headers: h });
             var c = r.clone();
             globalThis.__cloneCookies = c.headers.getSetCookie().join('|');",
        )
        .unwrap();
        assert_eq!(rt.eval("globalThis.__hasIter").unwrap(), lumen_core::JsValue::Bool(true));
        assert_eq!(rt.eval("globalThis.__hasAppend").unwrap(), lumen_core::JsValue::Bool(true));
        assert_eq!(
            rt.eval("globalThis.__getSetCookie").unwrap(),
            lumen_core::JsValue::String("a=1|b=2".into())
        );
        assert_eq!(
            rt.eval("globalThis.__cloneCookies").unwrap(),
            lumen_core::JsValue::String("a=1|b=2".into())
        );
    }

    /// Ph3 push-api РЎРѓРЎР‚Р ВµР В· 5: `_sw_fire_push` builds a `PushEvent` whose `.data`
    /// (`PushMessageData`) round-trips through `.text()`/`.json()`/
    /// `.arrayBuffer()` РІР‚вЂќ the three accessors a `push` handler actually uses.
    #[test]
    fn sw_fire_push_dispatches_push_event_with_data() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
            .unwrap();
        rt.eval(
            "self.addEventListener('push', function(event) {
                 globalThis.__text = event.data.text();
                 globalThis.__json = event.data.json().hello;
                 globalThis.__abLen = event.data.arrayBuffer().byteLength;
             });",
        )
        .unwrap();
        let json_text = r#"{"hello":"Р СР С‘РЎР‚"}"#;
        let payload_b64 = base64_encode(json_text.as_bytes());
        rt.eval(&format!("_sw_fire_push('{payload_b64}');")).unwrap();
        assert_eq!(
            rt.eval("globalThis.__text").unwrap(),
            lumen_core::JsValue::String(json_text.into())
        );
        assert_eq!(
            rt.eval("globalThis.__json").unwrap(),
            lumen_core::JsValue::String("Р СР С‘РЎР‚".into())
        );
        assert_eq!(
            rt.eval("globalThis.__abLen").unwrap(),
            lumen_core::JsValue::Number(json_text.len() as f64)
        );
    }

    /// BUG-1193: SW `atob`/`btoa` follow HTML LS Р’В§8.3 like the dedicated worker's.
    #[test]
    fn sw_atob_btoa_follow_html_spec() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
            .unwrap();
        rt.eval(
            "function thr(f) { try { f(); return 'no'; } catch (e) { return e.name; } }
             globalThis.__r = [
               atob(btoa('\\xff')) === '\\xff',
               atob('YQ==') === 'a',
               thr(function() { atob('YQ==YQ=='); }),
               thr(function() { atob('!!!'); }),
               thr(function() { btoa('\\u0100'); }),
             ].join('|');",
        )
        .unwrap();
        assert_eq!(
            rt.eval("globalThis.__r").unwrap(),
            lumen_core::JsValue::String(
                "true|true|InvalidCharacterError|InvalidCharacterError|InvalidCharacterError".into()
            )
        );
    }

    /// A `push` handler is optional (spec allows a SW with none registered) РІР‚вЂќ
    /// firing must not panic/throw when `_handlers['push']` is empty.
    #[test]
    fn sw_fire_push_without_handler_is_a_no_op() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
            .unwrap();
        assert!(rt.eval(&format!("_sw_fire_push('{}');", base64_encode(b"x"))).is_ok());
    }

    /// Ph3 push-api РЎРѓРЎР‚Р ВµР В· 5: `_sw_fire_push_subscription_change` exposes both
    /// `oldSubscription`/`newSubscription` with a working `endpoint`/`getKey()`.
    #[test]
    fn sw_fire_push_subscription_change_dispatches_event() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
            .unwrap();
        rt.eval(
            "self.addEventListener('pushsubscriptionchange', function(event) {
                 globalThis.__old = event.oldSubscription.endpoint;
                 globalThis.__new = event.newSubscription.endpoint;
                 globalThis.__newKeyLen = new Uint8Array(event.newSubscription.getKey('p256dh')).length;
             });",
        )
        .unwrap();
        let old_p256dh = base64_encode(&[1u8; 65]);
        let new_p256dh = base64_encode(&[2u8; 65]);
        rt.eval(&format!(
            "_sw_fire_push_subscription_change('https://push.example/old','{old_p256dh}','',
                                                'https://push.example/new','{new_p256dh}','');"
        ))
        .unwrap();
        assert_eq!(
            rt.eval("globalThis.__old").unwrap(),
            lumen_core::JsValue::String("https://push.example/old".into())
        );
        assert_eq!(
            rt.eval("globalThis.__new").unwrap(),
            lumen_core::JsValue::String("https://push.example/new".into())
        );
        assert_eq!(rt.eval("globalThis.__newKeyLen").unwrap(), lumen_core::JsValue::Number(65.0));
    }

    /// End-to-end through the real SW thread: `SwWorkerMessage::Push` sent on
    /// `handle.tx` reaches the running V8 isolate and fires the `push`
    /// handler. There is no response channel for `Push` (unlike `Fetch`), so
    /// the push handler stashes its result where a follow-up `fetch` can read
    /// it back РІР‚вЂќ the SW's message loop is a single `rx.recv()` FIFO, so the
    /// `Fetch` sent right after is guaranteed to process after the `Push`.
    #[test]
    fn v8_sw_push_message_dispatches_through_worker_thread() {
        let cache = MockCache::new();
        let handle = spawn_sw_worker_v8(
            "https://example.com".to_string(),
            "/".to_string(),
            r#"
var __received = null;
self.addEventListener('push', function(event) { __received = event.data.text(); });
self.addEventListener('fetch', function(event) {
    event.respondWith(Promise.resolve(new Response(__received || 'Р Р…Р С‘РЎвЂЎР ВµР С–Р С•')));
});
"#
            .to_string(),
            String::new(),
            false,
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
            None,
            None,
            None,
        );

        handle
            .tx
            .send(lumen_core::ext::SwWorkerMessage::Push(lumen_core::ext::SwPushMessage {
                payload: "Р С—РЎР‚Р С‘Р Р†Р ВµРЎвЂљ РЎРѓ РЎРѓР ВµРЎР‚Р Р†Р ВµРЎР‚Р В°".as_bytes().to_vec(),
            }))
            .unwrap();

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwWorkerMessage::Fetch(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/marker".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            }))
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(result, Some("Р С—РЎР‚Р С‘Р Р†Р ВµРЎвЂљ РЎРѓ РЎРѓР ВµРЎР‚Р Р†Р ВµРЎР‚Р В°".as_bytes().to_vec()));
    }

    /// SW-MSG-S2: `waitUntil` Р Р† Р С•Р В±РЎР‚Р В°Р В±Р С•РЎвЂљРЎвЂЎР С‘Р С”Р Вµ `install` Р С—РЎР‚Р С•Р Т‘Р В»Р ВµР Р†Р В°Р ВµРЎвЂљ Р ВµР С–Р С• Р В¶Р С‘Р В·Р Р…РЎРЉ РІР‚вЂќ
    /// `activate` РЎРѓРЎвЂљР В°РЎР‚РЎвЂљРЎС“Р ВµРЎвЂљ Р Р…Р Вµ РЎР‚Р В°Р Р…РЎРЉРЎв‚¬Р Вµ, РЎвЂЎР ВµР С РЎР‚Р В°Р В·РЎР‚Р ВµРЎв‚¬Р С‘РЎвЂљРЎРѓРЎРЏ Р С—РЎР‚Р С•Р СР С‘РЎРѓ. Р СџР С•РЎР‚РЎРЏР Т‘Р С•Р С” Р Р†Р С‘Р Т‘Р ВµР Р… Р Р†
    /// Р СР В°РЎРѓРЎРѓР С‘Р Р†Р Вµ, Р С”Р С•РЎвЂљР С•РЎР‚РЎвЂ№Р в„– РЎвЂЎР С‘РЎвЂљР В°Р ВµРЎвЂљРЎРѓРЎРЏ РЎвЂЎР ВµРЎР‚Р ВµР В· fetch-Р С•РЎвЂљР Р†Р ВµРЎвЂљ: РЎвЂљР В°Р в„–Р СР ВµРЎР‚ 5 Р СРЎРѓ Р Р†Р Р…РЎС“РЎвЂљРЎР‚Р С‘ Р С•Р С”Р Р…Р В°
    /// Р С—РЎР‚Р С•Р Т‘Р В»Р ВµР Р…Р С‘РЎРЏ Р Р†РЎвЂ№РЎРѓРЎвЂљР В°Р Р†Р В»РЎРЏР ВµРЎвЂљ `__wait_done`, Р С‘ Р ВµРЎРѓР В»Р С‘ `activate` РЎС“РЎРѓР С—Р ВµР В» РЎР‚Р В°Р Р…РЎРЉРЎв‚¬Р Вµ
    /// Р С”Р С•Р Р…РЎвЂ Р В° 40-Р СРЎРѓ Р С—РЎР‚Р С•Р СР С‘РЎРѓР В°, РЎвЂљР В°Р С Р С•РЎРѓРЎвЂљР В°Р В»РЎРѓРЎРЏ Р В±РЎвЂ№ `undefined`.
    #[test]
    fn sw_extendable_event_wait_until_extends_install() {
        let cache = MockCache::new();
        let handle = spawn_sw_worker_v8(
            "https://example.com".to_string(),
            "/".to_string(),
            r#"
var __order = [];
self.addEventListener('install', function(e) {
    __order.push('install');
    e.waitUntil(new Promise(function(r) { setTimeout(r, 40); }));
});
self.addEventListener('activate', function() {
    __order.push('activate:' + globalThis.__wait_done);
});
setTimeout(function() { globalThis.__wait_done = true; }, 5);
self.addEventListener('fetch', function(e) {
    e.respondWith(Promise.resolve(new Response(JSON.stringify(__order))));
});
"#
            .to_string(),
            String::new(),
            false,
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
            None,
            None,
            None,
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwWorkerMessage::Fetch(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/order".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            }))
            .unwrap();
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .expect("SW Р Т‘Р С•Р В»Р В¶Р ВµР Р… Р С•РЎвЂљР Р†Р ВµРЎвЂљР С‘РЎвЂљРЎРЉ Р Р…Р В° fetch");
        let order = String::from_utf8(result).unwrap();
        assert!(order.contains("install"), "{order}");
        assert!(
            order.contains("activate:true"),
            "activate Р Т‘Р С•Р В»Р В¶Р ВµР Р… Р С—РЎР‚Р С‘Р в„–РЎвЂљР С‘ Р С—Р С•РЎРѓР В»Р Вµ Р С”Р С•Р Р…РЎвЂ Р В° waitUntil-Р С—РЎР‚Р С•Р СР С‘РЎРѓР В° (40 Р СРЎРѓ), Р В° Р Р…Р Вµ РЎРѓРЎР‚Р В°Р В·РЎС“: {order}"
        );
    }

    /// SW-MSG-S2: `waitUntil` Р Р†Р Р…Р Вµ Р С•Р С”Р Р…Р В° Р Т‘Р С‘РЎРѓР С—Р В°РЎвЂљРЎвЂЎР В°, Р С”Р С•Р С–Р Т‘Р В° Р С•Р В¶Р С‘Р Т‘Р В°РЎР‹РЎвЂ°Р С‘РЎвЂ¦ Р С—РЎР‚Р С•Р СР С‘РЎРѓР С•Р Р†
    /// Р В±Р С•Р В»РЎРЉРЎв‚¬Р Вµ Р Р…Р ВµРЎвЂљ РІР‚вЂќ РЎРѓР С—Р ВµРЎвЂ Р С‘РЎвЂћР С‘Р С”Р В°РЎвЂ Р С‘Р С•Р Р…Р Р…РЎвЂ№Р в„– `InvalidStateError` (SW Р’В§4.5).
    #[test]
    fn sw_extendable_event_wait_until_after_dispatch_throws_invalid_state() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
            .unwrap();
        rt.eval(
            "globalThis.__thrown = null;
             _sw_fire_event('install');
             try {
               (new ExtendableEvent('x')).waitUntil(Promise.resolve());
             } catch (err) { globalThis.__thrown = err.name; }",
        )
        .unwrap();
        assert_eq!(
            rt.eval("globalThis.__thrown").unwrap(),
            lumen_core::JsValue::String("InvalidStateError".into())
        );
    }

    /// SW-MSG-S2: Р С”Р С•Р Р…РЎРѓРЎвЂљРЎР‚РЎС“Р С”РЎвЂљР С•РЎР‚ `ExtendableMessageEvent` (SW Р’В§4.7) РІР‚вЂќ Р В·Р Р…Р В°РЎвЂЎР ВµР Р…Р С‘РЎРЏ
    /// Р С—Р С• РЎС“Р СР С•Р В»РЎвЂЎР В°Р Р…Р С‘РЎР‹, Р Р…Р В°РЎРѓР В»Р ВµР Т‘Р С•Р Р†Р В°Р Р…Р С‘Р Вµ Р С•РЎвЂљ `ExtendableEvent`, Р В·Р В°Р СР С•РЎР‚Р С•Р В¶Р ВµР Р…Р Р…РЎвЂ№Р в„– `ports`
    /// Р С‘ `TypeError` Р Р…Р В° Р С—Р С•РЎРѓРЎвЂљР С•РЎР‚Р С•Р Р…Р Р…Р ВµР С `source`.
    #[test]
    fn sw_extendable_message_event_constructor() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
            .unwrap();
        rt.eval(
            "var e = new ExtendableMessageEvent('message');
             globalThis.__r = [
               e instanceof ExtendableEvent,
               e.data === null, e.origin === '', e.lastEventId === '',
               e.source === null, e.ports.length === 0,
               Object.isFrozen(e.ports),
               e.type === 'message',
             ];
             var p = new MessagePort();
             var e2 = new ExtendableMessageEvent('message',
                 { data: 5, origin: 'https://x.test', source: p, ports: [p] });
             globalThis.__r.push(e2.data === 5, e2.origin === 'https://x.test',
                 e2.source === p, e2.ports[0] instanceof MessagePort);
             globalThis.__bad = 'no-throw';
             try { new ExtendableMessageEvent('m', { source: {} }); }
             catch (err) { globalThis.__bad = err instanceof TypeError; }",
        )
        .unwrap();
        match rt.eval("globalThis.__r.every(function(v) { return v === true; })") {
            Ok(lumen_core::JsValue::Bool(true)) => {}
            other => panic!("Р С—Р С•Р В»РЎРЏ ExtendableMessageEvent: {other:?}"),
        }
        assert_eq!(
            rt.eval("globalThis.__bad").unwrap(),
            lumen_core::JsValue::Bool(true),
            "Р С—Р С•РЎРѓРЎвЂљР С•РЎР‚Р С•Р Р…Р Р…Р С‘Р в„– source Р С•Р В±РЎРЏР В·Р В°Р Р… Р Т‘Р В°РЎвЂљРЎРЉ TypeError"
        );
    }

    /// `indexedDB` Р Р† Р С•Р В±Р В»Р В°РЎРѓРЎвЂљР С‘ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В°. `sw.js` t-Р В±Р В°Р Р…Р С”Р В° Р С•Р В±РЎР‚Р В°РЎвЂ°Р В°Р ВµРЎвЂљРЎРѓРЎРЏ Р С” Р Р…Р ВµР в„– Р Р…Р В°
    /// Р Р†Р ВµРЎР‚РЎвЂ¦Р Р…Р ВµР С РЎС“РЎР‚Р С•Р Р†Р Р…Р Вµ; Р С—Р С•Р С”Р В° Р С”Р В»Р В°РЎРѓРЎРѓР В° Р Р…Р Вµ Р В±РЎвЂ№Р В»Р С•, Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚ РЎС“Р СР С‘РЎР‚Р В°Р В» РЎвЂљР В°Р С Р В¶Р Вµ, Р С–Р Т‘Р Вµ Р С‘ Р Р…Р В°
    /// `importScripts` РІР‚вЂќ Р Т‘Р С• РЎР‚Р ВµР С–Р С‘РЎРѓРЎвЂљРЎР‚Р В°РЎвЂ Р С‘Р С‘ Р С•Р В±РЎР‚Р В°Р В±Р С•РЎвЂљРЎвЂЎР С‘Р С”Р С•Р Р†.
    #[test]
    fn sw_global_scope_has_indexed_db() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
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

    /// Р СџР С•Р В»Р Р…РЎвЂ№Р в„– РЎвЂ Р С‘Р С”Р В»: Р С•РЎвЂљР С”РЎР‚РЎвЂ№РЎвЂљРЎРЉ Р В±Р В°Р В·РЎС“, РЎРѓР С•Р В·Р Т‘Р В°РЎвЂљРЎРЉ РЎвЂ¦РЎР‚Р В°Р Р…Р С‘Р В»Р С‘РЎвЂ°Р Вµ, Р В·Р В°Р С—Р С‘РЎРѓР В°РЎвЂљРЎРЉ Р С‘ Р С—РЎР‚Р С•РЎвЂЎР С‘РЎвЂљР В°РЎвЂљРЎРЉ РІР‚вЂќ
    /// Р Р†Р Р…РЎС“РЎвЂљРЎР‚Р С‘ Р С•Р В±Р В»Р В°РЎРѓРЎвЂљР С‘ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В°. Р С›РЎвЂЎР ВµРЎР‚Р ВµР Т‘РЎРЉ Р В·Р В°Р С—РЎР‚Р С•РЎРѓР С•Р Р† IndexedDB Р С—РЎР‚Р С•Р С”Р В°РЎвЂЎР С‘Р Р†Р В°Р ВµРЎвЂљРЎРѓРЎРЏ
    /// `queueMicrotask`, Р С”Р С•РЎвЂљР С•РЎР‚РЎвЂ№Р в„– Р Р† Р С•Р В±Р В»Р В°РЎРѓРЎвЂљР С‘ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В° РЎРѓР Р†Р С•Р в„–, Р С—Р С•РЎРЊРЎвЂљР С•Р СРЎС“ Р С—РЎР‚Р С•Р Р†Р ВµРЎР‚РЎРЏР ВµРЎвЂљРЎРѓРЎРЏ
    /// Р Р…Р Вµ Р Р…Р В°Р В»Р С‘РЎвЂЎР С‘Р Вµ Р С”Р В»Р В°РЎРѓРЎРѓР С•Р Р†, Р В° Р Т‘Р С•Р ВµРЎвЂ¦Р В°Р Р†РЎв‚¬Р ВµР Вµ Р Т‘Р С• `onsuccess` Р В·Р Р…Р В°РЎвЂЎР ВµР Р…Р С‘Р Вµ.
    #[test]
    fn sw_indexed_db_round_trip() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
            .unwrap();
        rt.eval(
            "var req = indexedDB.open('sw-db', 1);
             req.onupgradeneeded = function(e) {
                 e.target.result.createObjectStore('items', { keyPath: 'id' });
             };
             req.onsuccess = function(e) {
                 var db = e.target.result;
                 var tx = db.transaction('items', 'readwrite');
                 tx.objectStore('items').put({ id: 1, v: 'Р С‘Р В· Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В°' });
                 var get = db.transaction('items').objectStore('items').get(1);
                 get.onsuccess = function(ev) { globalThis.__idb = ev.target.result.v; };
             };",
        )
        .unwrap();
        rt.eval("_lumen_idb_flush();").unwrap();
        assert_eq!(
            rt.eval("globalThis.__idb").unwrap(),
            lumen_core::JsValue::String("Р С‘Р В· Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В°".into())
        );
    }

    /// Р вЂР ВµР В· Р С—РЎР‚Р С•Р Р†Р В°Р в„–Р Т‘Р ВµРЎР‚Р В° (headless-РЎР‚Р ВµР В¶Р С‘Р СРЎвЂ№ Р Т‘Р В°Р СР С—Р В°) РЎРѓР ВµРЎвЂљР С‘ Р Р…Р ВµРЎвЂљ, Р Р…Р С• Р С•Р В±Р В»Р В°РЎРѓРЎвЂљРЎРЉ Р Р†Р С•РЎР‚Р С”Р ВµРЎР‚Р В°
    /// Р С•Р В±РЎРЏР В·Р В°Р Р…Р В° Р С•РЎРѓРЎвЂљР В°РЎвЂљРЎРЉРЎРѓРЎРЏ Р В¶Р С‘Р Р†Р С•Р в„–: `fetch` Р С•РЎвЂљР С”Р В»Р С•Р Р…РЎРЏР ВµРЎвЂљРЎРѓРЎРЏ, `importScripts` Р В±РЎР‚Р С•РЎРѓР В°Р ВµРЎвЂљ РІР‚вЂќ
    /// Р Р†Р СР ВµРЎРѓРЎвЂљР С• `_lumen_sw_net_fetch is not defined`.
    #[test]
    fn sw_without_provider_rejects_instead_of_crashing() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None).unwrap();
        assert_eq!(
            rt.eval("typeof _lumen_sw_net_fetch").unwrap(),
            lumen_core::JsValue::String("function".into())
        );
        rt.eval(
            "fetch('https://example.com/x').then(function() { globalThis.__r = 'ok'; },
                                                function() { globalThis.__r = 'Р С•РЎвЂљР С”Р В°Р В·'; });",
        )
        .unwrap();
        assert_eq!(
            rt.eval("globalThis.__r").unwrap(),
            lumen_core::JsValue::String("Р С•РЎвЂљР С”Р В°Р В·".into())
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
            String::new(),
            false,
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
            None,
            None,
            None,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwWorkerMessage::Fetch(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/api/data".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            }))
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
            String::new(),
            false,
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
            None,
            None,
            None,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwWorkerMessage::Fetch(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/missing.js".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            }))
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
            String::new(),
            false,
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
            None,
            None,
            None,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwWorkerMessage::Fetch(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/page".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            }))
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(result, None);
    }

    // SW-MODULE tests ========================================================

    /// Модульный SW исполняется как ES-модуль: статический `import` графа
    /// грузится загрузчиком по своим URL и переменная попадает в изолят.
    #[test]
    fn v8_sw_module_script_loads_static_import_graph() {
        let cache = MockCache::new();
        struct DepNet;
        impl lumen_core::ext::JsFetchProvider for DepNet {
            fn fetch_sync(
                &self,
                url: &str,
                _method: &str,
            ) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
                let body = if url.ends_with("/dep.js") {
                    b"export const dep = 'module-dep';".to_vec()
                } else {
                    Vec::new()
                };
                Ok(lumen_core::ext::JsFetchResult {
                    status: 200,
                    status_text: "OK".into(),
                    headers: vec![],
                    body,
                    url: url.to_string(),
                })
            }
        }
        let handle = spawn_sw_worker_v8(
            "https://example.com".to_string(),
            "/".to_string(),
            r#"
import { dep } from './dep.js';
globalThis.__dep = dep;
self.addEventListener('fetch', function(event) {
    event.respondWith(Promise.resolve(new Response(globalThis.__dep)));
});
"#
            .to_string(),
            "https://example.com/sw.js".to_string(),
            true,
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
            Some(Arc::new(DepNet) as Arc<dyn lumen_core::ext::JsFetchProvider>),
            None,
            None,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwWorkerMessage::Fetch(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/page".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            }))
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&result.unwrap_or_default()),
            "module-dep"
        );
    }

    /// В модульном SW `importScripts` бросает TypeError, динамический
    /// `import()` отклоняется (SW §Run Service Worker); в классическом оба
    /// пути не меняются — dynamic import здесь не проверяем, у шима SW его
    /// нет.
    #[test]
    fn v8_sw_module_rejects_import_scripts_and_dynamic_import() {
        let cache = MockCache::new();
        let handle = spawn_sw_worker_v8(
            "https://example.com".to_string(),
            "/".to_string(),
            r#"
globalThis.__verdict = 'untouched';
try { importScripts('lib.js'); } catch (e) { __verdict += '|importScripts:' + e.constructor.name + '|flag:' + globalThis._lumen_sw_is_module__; }
import('./late.js').then(function() { __verdict += '|dyn:fulfilled'; },
                          function(e) { __verdict += '|dyn:' + e.constructor.name; });
self.addEventListener('fetch', function(event) {
    event.respondWith(Promise.resolve(new Response(__verdict)));
});
"#
            .to_string(),
            "https://example.com/sw.js".to_string(),
            true,
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
            None,
            None,
            None,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwWorkerMessage::Fetch(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/page".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            }))
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let verdict = String::from_utf8_lossy(&result.unwrap_or_default()).into_owned();
        assert!(
            verdict.contains("importScripts:TypeError"),
            "importScripts must throw TypeError in a module SW, got: {verdict}"
        );
        assert!(
            verdict.contains("dyn:TypeError"),
            "dynamic import() must reject with TypeError in a module SW, got: {verdict}"
        );
    }

    /// Модульный SW строг: присваивание в несуществующую глобальную переменную
    /// (`onmessage = ...` без `self.`) бросает ReferenceError, если атрибут-обработчик
    /// не предопределён на scope. В движках это IDL-атрибуты (null по умолчанию) —
    /// шим должен так же предопределить их до eval скрипта.
    #[test]
    fn v8_sw_module_scope_predefines_event_handlers() {
        let cache = MockCache::new();
        let handle = spawn_sw_worker_v8(
            "https://example.com".to_string(),
            "/".to_string(),
            r#"
onmessage = function(event) { globalThis.__last_message = event.data; };
self.addEventListener('fetch', function(event) {
    event.respondWith(Promise.resolve(new Response(typeof onmessage)));
});
"#
            .to_string(),
            "https://example.com/sw.js".to_string(),
            true,
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
            None,
            None,
            None,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwWorkerMessage::Fetch(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/page".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            }))
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&result.unwrap_or_default()),
            "function",
            "bare `onmessage = ...` must define the handler in a strict module SW"
        );
    }

    // SW-MSG-S1 tests ========================================================

    #[test]
    fn sw_timers_fire_asynchronously() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
            .unwrap();

        // Set a timer that sets a flag after 20ms
        rt.eval(r#"
            globalThis.__timerFired = false;
            setTimeout(() => {
                globalThis.__timerFired = true;
            }, 20);
        "#).unwrap();

        // Timer should not fire immediately
        assert_eq!(
            rt.eval("globalThis.__timerFired").unwrap(),
            lumen_core::JsValue::Bool(false)
        );

        // Run worker tasks to process timers
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(200) {
            if let Some(wait) = run_worker_tasks(&rt) {
                std::thread::sleep(std::time::Duration::from_millis(10));
                if wait > std::time::Duration::from_millis(10) {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // Check if timer fired
            match rt.eval("globalThis.__timerFired") {
                Ok(lumen_core::JsValue::Bool(true)) => break,
                _ => continue,
            }
        }

        // Timer should have fired within 200ms
        assert_eq!(
            rt.eval("globalThis.__timerFired").unwrap(),
            lumen_core::JsValue::Bool(true)
        );
    }

    #[test]
    fn sw_fetch_waits_for_async_respond_with() {
        let cache = MockCache::new();
        
        let handle = spawn_sw_worker_v8(
            "https://example.com".to_string(),
            "/".to_string(),
            r#"
self.addEventListener('fetch', function(event) {
    // Respond with a promise that resolves after 30ms
    event.respondWith(new Promise(resolve => {
        setTimeout(() => {
            resolve(new Response('late response'));
        }, 30);
    }));
});
"#.to_string(),
            String::new(),
            false,
            Arc::clone(&cache) as Arc<dyn CacheBackend>,
            None,
            None,
            None,
        );

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle
            .tx
            .send(lumen_core::ext::SwWorkerMessage::Fetch(lumen_core::ext::SwFetchRequest {
                url: "https://example.com/api".to_string(),
                method: "GET".to_string(),
                response_tx: tx,
            }))
            .unwrap();

        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(result, Some(b"late response".to_vec()));
    }

    #[test]
    fn sw_setinterval_fires_multiple_times() {
        let rt = V8JsRuntime::new().unwrap();
        install_sw_globals_v8(&rt, "https://example.com", "/", MockCache::new(), None, None, None)
            .unwrap();

        // Set an interval that increments a counter every 10ms
        rt.eval(r#"
            globalThis.__intervalCount = 0;
            globalThis.__intervalId = setInterval(() => {
                globalThis.__intervalCount++;
                if (globalThis.__intervalCount >= 3) {
                    clearInterval(globalThis.__intervalId);
                }
            }, 10);
        "#).unwrap();

        // Run worker tasks to process timers
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(200) {
            if let Some(wait) = run_worker_tasks(&rt) {
                std::thread::sleep(std::time::Duration::from_millis(10));
                if wait > std::time::Duration::from_millis(10) {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // Check if interval fired at least 3 times
            match rt.eval("globalThis.__intervalCount") {
                Ok(lumen_core::JsValue::Number(n)) if n >= 3.0 => break,
                _ => continue,
            }
        }

        // Interval should have fired at least 3 times
        let count = rt.eval("globalThis.__intervalCount").unwrap();
        assert!(matches!(count, lumen_core::JsValue::Number(n) if n >= 3.0));
    }
}
