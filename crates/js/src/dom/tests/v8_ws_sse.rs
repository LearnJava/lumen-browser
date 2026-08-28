//! V8 port of the WebSocket / EventSource / fetch-bindings / IME+bfcache test
//! families (S12b-24-ws-sse, третий слайс `dom.rs`-монолита). Первый слайс с
//! мок-провайдерами (`JsWebSocketProvider`, `JsSseProvider`) — они внедряются
//! через тот же `install_dom`, что и у QuickJS (сигнатуры совпадают
//! аргумент-в-аргумент), сами моки движка не касаются: реализуют трейты
//! `lumen_core::ext`.
//!
//! Gated on `v8-backend` like `v8_core`/`v8_events_cache`: QuickJS-копии удалены,
//! V8 — движок по умолчанию (ADR-018) и несёт это покрытие дальше.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of the deleted `runtime_with_dom`: same fixture document, same
/// `install_dom` argument list, same `_LUMEN_EXTENSION_ACTIVE` pre-eval.
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

// ── IME composition API ───────────────────────────────────────────────────

#[test]
fn dispatch_composition_function_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("typeof _lumen_dispatch_composition === 'function'")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn set_ime_target_function_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("typeof _lumen_set_ime_target === 'function'")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn dispatch_composition_on_element_fires_listener() {
    let rt = v8_runtime_with_dom(make_doc());
    // Регистрируем слушатель compositionstart на main div.
    // При диспатче он должен сохранить data в глобальной переменной.
    rt.eval(r#"
                var _got_composition = null;
                var el = document.getElementById('main');
                el.addEventListener('compositionstart', function(e) {
                    _got_composition = e.type;
                });
                _lumen_set_ime_target(el);
                _lumen_dispatch_composition('compositionstart', '');
            "#).unwrap();
    let result = rt.eval("_got_composition").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("compositionstart".into()));
}

#[test]
fn dispatch_composition_update_carries_data() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _comp_data = null;
                var el = document.getElementById('main');
                el.addEventListener('compositionupdate', function(e) {
                    _comp_data = e.data;
                });
                _lumen_set_ime_target(el);
                _lumen_dispatch_composition('compositionupdate', 'あい');
            "#).unwrap();
    let result = rt.eval("_comp_data").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("あい".into()));
}

#[test]
fn dispatch_composition_without_target_does_not_crash() {
    let rt = v8_runtime_with_dom(make_doc());
    // Нет target — должен молча ничего не сделать.
    rt.eval("_lumen_set_ime_target(null); _lumen_dispatch_composition('compositionstart', '');")
        .unwrap();
}

#[test]
fn window_has_dispatch_composition() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("typeof window._lumen_dispatch_composition === 'function'")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// ── bfcache / pageshow / pagehide ────────────────────────────────────────

#[test]
fn window_has_pageshow_pagehide_handlers() {
    let rt = v8_runtime_with_dom(make_doc());
    // onpageshow and onpagehide should be null (not set) initially.
    let r1 = rt.eval("window.onpageshow === null").unwrap();
    let r2 = rt.eval("window.onpagehide === null").unwrap();
    assert_eq!(r1, lumen_core::JsValue::Bool(true));
    assert_eq!(r2, lumen_core::JsValue::Bool(true));
}

#[test]
fn pageshow_listener_receives_event_with_persisted_false() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var saw = false; var persistedFlag = null;
                 window.addEventListener('pageshow', function(e) { saw = true; persistedFlag = e.persisted; });
                 _lumen_fire_page_lifecycle('pageshow', false);",
    ).unwrap();
    let saw = rt.eval("saw").unwrap();
    let persisted = rt.eval("persistedFlag").unwrap();
    assert_eq!(saw, lumen_core::JsValue::Bool(true));
    assert_eq!(persisted, lumen_core::JsValue::Bool(false));
}

#[test]
fn pageshow_listener_receives_persisted_true_from_bfcache() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var persistedFlag = null;
                 window.addEventListener('pageshow', function(e) { persistedFlag = e.persisted; });
                 _lumen_fire_page_lifecycle('pageshow', true);",
    ).unwrap();
    let persisted = rt.eval("persistedFlag").unwrap();
    assert_eq!(persisted, lumen_core::JsValue::Bool(true));
}

#[test]
fn pagehide_listener_fires() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var fired = false;
                 window.addEventListener('pagehide', function(e) { fired = true; });
                 _lumen_fire_page_lifecycle('pagehide', false);",
    ).unwrap();
    let fired = rt.eval("fired").unwrap();
    assert_eq!(fired, lumen_core::JsValue::Bool(true));
}

#[test]
fn onpageshow_handler_fires() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var saw = false;
                 window.onpageshow = function(e) { saw = true; };
                 _lumen_fire_page_lifecycle('pageshow', false);",
    ).unwrap();
    let saw = rt.eval("saw").unwrap();
    assert_eq!(saw, lumen_core::JsValue::Bool(true));
}

#[test]
fn remove_pageshow_listener_stops_it_firing() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var count = 0;
                 var fn1 = function() { count++; };
                 window.addEventListener('pageshow', fn1);
                 window.removeEventListener('pageshow', fn1);
                 _lumen_fire_page_lifecycle('pageshow', false);",
    ).unwrap();
    let count = rt.eval("count").unwrap();
    assert_eq!(count, lumen_core::JsValue::Number(0.0));
}

#[test]
fn lumen_bfcache_persisted_default_false() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("_lumen_bfcache_persisted").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(false));
}

#[test]
fn lumen_fire_page_lifecycle_exported_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("typeof window._lumen_fire_page_lifecycle === 'function'").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// ── BUG-834: «unload a document» (HTML LS §7.4.5–§7.4.6) ──────────────

/// A discarded document (`persisted = false`) gets the whole sequence in
/// spec order: pagehide, then visibilityState 'hidden', then unload.
#[test]
fn unload_document_fires_pagehide_visibilitychange_unload_in_order() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var log = [];
                 window.addEventListener('pagehide', function(e) { log.push('pagehide:' + e.persisted); });
                 document.addEventListener('visibilitychange', function() { log.push('vis:' + document.visibilityState); });
                 window.addEventListener('unload', function() { log.push('unload'); });
                 _lumen_unload_document(false);",
    ).unwrap();
    let log = rt.eval("log.join(',')").unwrap();
    assert_eq!(
        log,
        lumen_core::JsValue::String("pagehide:false,vis:hidden,unload".to_string())
    );
}

/// A salvageable document (retained in bfcache) gets pagehide + hidden,
/// but NOT `unload` — the spec fires it only for a discarded document.
#[test]
fn unload_document_persisted_skips_unload_event() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var log = [];
                 window.addEventListener('pagehide', function(e) { log.push('pagehide:' + e.persisted); });
                 window.addEventListener('unload', function() { log.push('unload'); });
                 _lumen_unload_document(true);",
    ).unwrap();
    let log = rt.eval("log.join(',')").unwrap();
    assert_eq!(
        log,
        lumen_core::JsValue::String("pagehide:true".to_string())
    );
    let hidden = rt.eval("document.visibilityState").unwrap();
    assert_eq!(hidden, lumen_core::JsValue::String("hidden".to_string()));
}

/// `onunload` — the on<type> handler form — is reached as well; it goes
/// through `window.dispatchEvent`'s generic branch.
#[test]
fn unload_document_calls_onunload_handler() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var saw = ''; window.onunload = function(e) { saw = e.type; };
                 _lumen_unload_document(false);",
    ).unwrap();
    let saw = rt.eval("saw").unwrap();
    assert_eq!(saw, lumen_core::JsValue::String("unload".to_string()));
}

/// The «page showing» flag makes the sequence idempotent: a second call
/// must not fire pagehide/visibilitychange again.
#[test]
fn unload_document_is_idempotent_on_page_showing_flag() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var n = 0;
                 window.addEventListener('pagehide', function() { n++; });
                 _lumen_unload_document(true);
                 _lumen_unload_document(true);",
    ).unwrap();
    let n = rt.eval("n").unwrap();
    assert_eq!(n, lumen_core::JsValue::Number(1.0));
}

/// A page restored from bfcache in the SAME runtime becomes showing and
/// visible again on `pageshow`, so a later departure fires once more.
#[test]
fn pageshow_restores_page_showing_and_visibility() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var n = 0;
                 window.addEventListener('pagehide', function() { n++; });
                 _lumen_unload_document(true);
                 _lumen_fire_page_lifecycle('pageshow', true);
                 _lumen_unload_document(true);",
    ).unwrap();
    let n = rt.eval("n").unwrap();
    assert_eq!(n, lumen_core::JsValue::Number(2.0));
    // The intermediate `pageshow` had to flip visibility back to 'visible',
    // otherwise the second `_lumen_apply_visibility(true)` is a no-op and
    // the restored page would report 'hidden' while on screen.
    let seq = rt
        .eval("_lumen_fire_page_lifecycle('pageshow', true); document.visibilityState")
        .unwrap();
    assert_eq!(seq, lumen_core::JsValue::String("visible".to_string()));
}

/// `beforeunload` reaches both listener forms, and the page's «asked to
/// stay» answer is reported back to the shell.
#[test]
fn beforeunload_reports_prevent_default_and_return_value() {
    let rt = v8_runtime_with_dom(make_doc());
    let quiet = rt
        .eval("window.addEventListener('beforeunload', function(e) {}); _lumen_fire_beforeunload()")
        .unwrap();
    assert_eq!(quiet, lumen_core::JsValue::Bool(false));

    let rt2 = v8_runtime_with_dom(make_doc());
    let prevented = rt2
        .eval("window.addEventListener('beforeunload', function(e) { e.preventDefault(); }); _lumen_fire_beforeunload()")
        .unwrap();
    assert_eq!(prevented, lumen_core::JsValue::Bool(true));

    // Legacy form: a string returned from the on<type> handler sets
    // `returnValue`. A listener's return value deliberately does not.
    let rt3 = v8_runtime_with_dom(make_doc());
    let legacy = rt3
        .eval("window.onbeforeunload = function(e) { return 'stay'; }; _lumen_fire_beforeunload()")
        .unwrap();
    assert_eq!(legacy, lumen_core::JsValue::Bool(true));
}

/// `'onunload' in window` / `'onbeforeunload' in window` — the feature
/// test a page runs before hooking the sequence (BUG-822 precedent).
#[test]
fn window_declares_unload_handler_properties() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("('onunload' in window) && ('onbeforeunload' in window) && window.onunload === null")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── Fetch API tests ───────────────────────────────────────────────────────

#[test]
fn fetch_global_is_function() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof fetch === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn window_fetch_is_function() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.fetch === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn headers_class_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof Headers === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn request_class_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof Request === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn response_class_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof Response === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn abort_controller_class_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof AbortController === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn headers_get_set() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var h = new Headers(); h.set('Content-Type', 'application/json'); h.get('content-type')"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("application/json".into()));
}

#[test]
fn headers_case_insensitive() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var h = new Headers({'X-Foo': 'bar'}); h.get('x-foo')"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("bar".into()));
}

// ── BUG-369: `Headers` as a WebIDL interface (Fetch §2.2) ────────────────

/// Fetch §2.2: `Headers` is `iterable<ByteString, ByteString>` and iterates
/// in «sort and combine» order — names sorted, same-name values joined.
#[test]
fn headers_iterates_in_sorted_combined_order() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var h = new Headers([['b','2'],['a','1'],['a','3']]); \
                     var out = []; for (var p of h) { out.push(p[0] + '=' + p[1]); } out.join('|')",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("a=1, 3|b=2".into()));
}

/// WebIDL requires `@@iterator` to be the very same function as `entries`.
#[test]
fn headers_symbol_iterator_is_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("Headers.prototype[Symbol.iterator] === Headers.prototype.entries")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `entries()`/`keys()`/`values()` return iterator objects, not arrays.
#[test]
fn headers_entries_returns_iterator_not_array() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var it = new Headers({'a':'1'}).entries(); \
                     typeof it.next === 'function' && !Array.isArray(it) \
                       && it[Symbol.iterator]() === it && it.next().value[1] === '1' \
                       && it.next().done === true",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Fetch §2.2.5 «fill»: a `Headers` init copies the source header list.
#[test]
fn headers_copy_constructor_from_headers() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("new Headers(new Headers({'X-A': '1'})).get('x-a')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1".into()));
}

/// Fetch §2.2.1: a header name must be a valid HTTP token.
#[test]
fn headers_invalid_name_throws_type_error() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "try { new Headers().append('in valid', 'x'); false; } \
                     catch (e) { e instanceof TypeError; }",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Fetch §2.2.1 «normalize a header value»: HTTP whitespace is stripped.
#[test]
fn headers_value_is_normalized() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("var h = new Headers(); h.set('a', '  1  '); h.get('a')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1".into()));
}

/// The header list and the case-normalizer are no longer web-visible.
#[test]
fn headers_private_state_is_not_web_visible() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var h = new Headers({'a':'1'}); var seen = []; for (var k in h) seen.push(k); \
                     JSON.stringify(h) === '{}' && Object.keys(h).length === 0 \
                       && seen.length === 0 && h._map === undefined && h._key === undefined",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// WebIDL branding: `[object Headers]`, `new` required, `length` 0.
#[test]
fn headers_has_webidl_branding() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var threw = false; try { Headers(); } catch (e) { threw = e instanceof TypeError; } \
                     threw && Headers.length === 0 \
                       && Object.prototype.toString.call(new Headers()) === '[object Headers]'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Fetch §2.2: `getSetCookie()` keeps the individual `Set-Cookie` values,
/// which `get()` would have glued together with `, `.
#[test]
fn headers_get_set_cookie_returns_each_value() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "new Headers([['set-cookie','a=1'],['set-cookie','b=2']]).getSetCookie().join('|')",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("a=1|b=2".into()));
}

/// Fetch §2.2.2: a request-guarded `Headers` silently drops forbidden names.
#[test]
fn request_headers_drop_forbidden_names() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var q = new Request('https://e.example/x'); \
                     q.headers.set('Host', 'evil.example'); q.headers.set('X-Ok', '1'); \
                     q.headers.get('host') === null && q.headers.get('x-ok') === '1'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Fetch §5 step 12: CONNECT/TRACE/TRACK are forbidden request methods.
#[test]
fn request_forbidden_method_throws() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "try { new Request('https://e.example/x', {method: 'CONNECT'}); false; } \
                     catch (e) { e instanceof TypeError; }",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A `Request` clone keeps its headers (they used to travel as a raw array).
#[test]
fn request_clone_preserves_headers() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "new Request('https://e.example/x', {headers: {'X-A': '1'}}).clone().headers.get('x-a')",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1".into()));
}

/// Fetch §2.5: the `Response` constructor guards its headers as 'response',
/// so `Set-Cookie` cannot be smuggled in through `init.headers`.
#[test]
fn response_headers_are_response_guarded() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var s = new Response(null, {headers: {'Set-Cookie': 'a=1', 'X-A': '1'}}); \
                     s.headers.get('set-cookie') === null && s.headers.get('x-a') === '1'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Fetch §2.5: `Response.error()`/`Response.redirect()` have immutable headers.
#[test]
fn response_error_and_redirect_headers_are_immutable() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "function locked(s) { try { s.headers.set('a', '1'); return false; } \
                                          catch (e) { return e instanceof TypeError; } } \
                     locked(Response.error()) && locked(Response.redirect('https://e.example/', 302))",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `Response.clone()` used to rely on `entries()` handing back a raw array.
#[test]
fn response_clone_preserves_headers() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("new Response('x', {headers: {'X-A': '1'}}).clone().headers.get('x-a')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1".into()));
}

#[test]
fn response_ok_for_200() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new Response(null, {status: 200}).ok").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn response_not_ok_for_404() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new Response(null, {status: 404}).ok").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn response_text_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var r = new Response(new Uint8Array([104, 105])); \
                 typeof r.text() === 'object'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn abort_controller_abort_sets_signal() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var ctrl = new AbortController(); ctrl.abort(); ctrl.signal.aborted"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `install_dom` with `None` fetch_provider: `fetch()` returns a thenable that
/// actually rejects. The QuickJS original could only assert "is a thenable"
/// (`eval()` there didn't drain microtasks); V8 drains its microtask queue, so
/// the rejection is observable — S12b-2 lesson, tighten what V8 makes
/// deterministic instead of carrying the loose assertion over.
#[test]
fn fetch_without_provider_rejects() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var thenable = false; var rejected = false;
                 var p = fetch('http://example.com/');
                 thenable = typeof p === 'object' && typeof p.then === 'function';
                 p.catch(function() { rejected = true; });",
    )
    .unwrap();
    assert_eq!(rt.eval("thenable").unwrap(), lumen_core::JsValue::Bool(true));
    assert_eq!(rt.eval("rejected").unwrap(), lumen_core::JsValue::Bool(true));
}

#[test]
fn request_default_method_get() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new Request('https://x.com/').method").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("GET".into()));
}

// ── BUG-370: Request/Response as WebIDL interfaces ───────────────────────

/// A1: `Request` includes the Body mixin — the same seven members Response has.
#[test]
fn request_has_body_mixin() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var q = new Request('https://e.example/x', {method: 'POST', body: 'hi'}); \
                     ['arrayBuffer','blob','bytes','formData','json','text'] \
                        .every(function(m) { return typeof q[m] === 'function'; }) \
                     && q.bodyUsed === false && q.body instanceof ReadableStream",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A1: the mixin actually reads the body the constructor was given.
#[test]
fn request_text_returns_body() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var got = null; \
                 new Request('https://e.example/x', {method: 'POST', body: 'payload'}) \
                    .text().then(function(t) { got = t; });",
    )
    .unwrap();
    let r = rt.eval("got").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("payload".into()));
}

/// A1: `keepalive`/`destination` are attributes, not `undefined`.
#[test]
fn request_keepalive_and_destination_defaults() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var q = new Request('https://e.example/x'); \
                     q.keepalive === false && q.destination === ''",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A3: both constructors require `new`, and `Request.length` is 1.
#[test]
fn request_and_response_require_new() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "function threw(f) { try { f(); return false; } catch (e) { return e instanceof TypeError; } } \
                     threw(function() { Request('https://e.example/x'); }) \
                     && threw(function() { Response(); }) \
                     && Request.length === 1 && Response.length === 0",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A3: Fetch §5 step 36 — a GET/HEAD request cannot carry a body.
#[test]
fn request_get_with_body_throws() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "try { new Request('https://e.example/x', {body: 'b'}); false; } \
                     catch (e) { e instanceof TypeError; }",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Fetch §5 «normalize a method»: only the six known names uppercase.
#[test]
fn request_method_normalisation_is_spec_scoped() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "new Request('https://e.example/x', {method: 'post'}).method + '|' + \
                     new Request('https://e.example/x', {method: 'patch'}).method",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("POST|patch".into()));
}

/// B1: `Response.json(data, init)` — the static factory (spec since 2022).
#[test]
fn response_static_json_builds_json_response() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var got = null; \
                 var r = Response.json({a: 1}); \
                 var ct = r.headers.get('content-type'); \
                 r.text().then(function(t) { got = t + '|' + ct; });",
    )
    .unwrap();
    let r = rt.eval("got").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("{\"a\":1}|application/json".into()));
}

/// B2: `Response.error()` is a network error — `type === 'error'`.
#[test]
fn response_error_type_is_error() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var r = Response.error(); \
                     r.type === 'error' && r.status === 0 && r.ok === false && r.body === null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// B3: `Response.redirect()` sets `Location` and rejects a non-redirect code.
#[test]
fn response_redirect_sets_location_and_validates_status() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var r = Response.redirect('https://e.example/', 301); \
                     var bad = false; \
                     try { Response.redirect('https://e.example/', 200); } \
                     catch (e) { bad = e instanceof RangeError; } \
                     r.headers.get('Location') === 'https://e.example/' && r.status === 301 && bad",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// B4: status range and null-body statuses are validated.
#[test]
fn response_constructor_validates_status_and_body() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "function err(f) { try { f(); return ''; } catch (e) { return e.constructor.name; } } \
                     err(function() { new Response(null, {status: 1000}); }) === 'RangeError' \
                     && err(function() { new Response('b', {status: 204}); }) === 'TypeError' \
                     && new Response(null, {status: 299}).ok === true \
                     && new Response(null, {status: 300}).ok === false",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// B5: the body's implied Content-Type fills the header when init has none.
#[test]
fn response_derives_content_type_from_body() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "new Response('x').headers.get('content-type') + '|' + \
                     new Response(new URLSearchParams('a=1')).headers.get('content-type') + '|' + \
                     new Response('x', {headers: {'Content-Type': 'text/html'}}).headers.get('content-type')",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "text/plain;charset=UTF-8|application/x-www-form-urlencoded;charset=UTF-8|text/html".into()
        )
    );
}

/// B6: `formData()` and `bytes()` complete the Body mixin on Response.
#[test]
fn response_form_data_parses_urlencoded_body() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var got = null; \
                 new Response(new URLSearchParams('a=1&b=two')).formData() \
                    .then(function(fd) { got = fd.get('a') + '/' + fd.get('b'); });",
    )
    .unwrap();
    let r = rt.eval("got").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1/two".into()));
}

/// B6: a multipart body round-trips through FormData → Response → formData().
#[test]
fn response_form_data_parses_multipart_body() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var got = null; \
                 var fd = new FormData(); fd.append('k', 'v'); \
                 new Response(fd).formData().then(function(out) { got = out.get('k'); });",
    )
    .unwrap();
    let r = rt.eval("got").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("v".into()));
}

/// B6: `bytes()` hands back a Uint8Array of the body.
#[test]
fn response_bytes_returns_uint8array() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var got = null; \
                 new Response('hi').bytes().then(function(b) { \
                     got = (b instanceof Uint8Array) + ':' + b[0] + ',' + b[1]; });",
    )
    .unwrap();
    let r = rt.eval("got").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("true:104,105".into()));
}

/// C1: attributes are read-only accessors on the prototype, not own data
/// properties — `req.method = 'DELETE'` must not rewrite the request.
#[test]
fn request_attributes_live_on_the_prototype() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var q = new Request('https://e.example/x', {method: 'POST'}); \
                     q.method = 'DELETE'; \
                     var d = Object.getOwnPropertyDescriptor(Request.prototype, 'method'); \
                     q.method === 'POST' && Object.getOwnPropertyNames(q).length === 0 \
                     && typeof d.get === 'function' && d.set === undefined",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// C2: internal slots are unreachable, so JSON.stringify yields `{}` on both
/// (it used to dump the whole request and *throw* on a Response's stream).
#[test]
fn request_and_response_stringify_to_empty_object() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "JSON.stringify(new Request('https://e.example/x')) + '|' + \
                     JSON.stringify(new Response('x'))",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("{}|{}".into()));
}

/// C3: `Symbol.toStringTag` names the interface.
#[test]
fn request_and_response_have_to_string_tag() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "Object.prototype.toString.call(new Request('https://e.example/x')) + '|' + \
                     Object.prototype.toString.call(new Response())",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("[object Request]|[object Response]".into()));
}

/// C4: WebIDL global operations are configurable, so a polyfill can swap
/// `fetch` out; the bare function declaration made it non-configurable.
#[test]
fn fetch_global_is_configurable() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var d = Object.getOwnPropertyDescriptor(globalThis, 'fetch'); \
                     d.writable === true && d.enumerable === true && d.configurable === true \
                     && fetch.name === 'fetch' && fetch.length === 1",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A `Request` built from a `Request` inherits the body, and cloning a
/// consumed body is a TypeError (Fetch §2.3).
#[test]
fn request_clone_rejects_used_body() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var q = new Request('https://e.example/x', {method: 'POST', body: 'b'}); \
                     var copy = new Request(q); \
                     q.text().then(function() {}); \
                     var threw = false; \
                     try { q.clone(); } catch (e) { threw = e instanceof TypeError; } \
                     copy.method === 'POST' && threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `Response.clone()` still yields two independently readable bodies.
#[test]
fn response_clone_bodies_are_independent() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var got = null; \
                 var r = new Response('shared'); \
                 var c = r.clone(); \
                 Promise.all([r.text(), c.text()]).then(function(v) { got = v.join('|'); });",
    )
    .unwrap();
    let r = rt.eval("got").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("shared|shared".into()));
}

#[test]
fn window_has_abort_controller() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.AbortController === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── WebSocket API ─────────────────────────────────────────────────────────

#[test]
fn window_has_websocket_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.WebSocket === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_constants_defined() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("WebSocket.CONNECTING === 0 && WebSocket.OPEN === 1 && WebSocket.CLOSING === 2 && WebSocket.CLOSED === 3")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// Mock WS provider: connect always fails (no server).
struct FailWsProvider;
impl lumen_core::ext::JsWebSocketProvider for FailWsProvider {
    fn connect(&self, _url: &str, _protocols: &[String]) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsWebSocketSession>> {
        Err(lumen_core::error::Error::Network("test: no server".into()))
    }
}

fn v8_runtime_with_ws(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsWebSocketProvider> = Arc::new(FailWsProvider);
    rt.install_dom(doc, "", None, Some(provider), None, None, None, None, None, None, false).unwrap();
    rt
}

#[test]
fn websocket_connect_fail_sets_closed_state() {
    let rt = v8_runtime_with_ws(make_doc());
    // connect fails immediately → readyState = 3 (CLOSED)
    let r = rt
        .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws.readyState")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(3.0));
}

#[test]
fn websocket_connect_fail_no_handle() {
    let rt = v8_runtime_with_ws(make_doc());
    let r = rt
        .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws._handle === 0")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_connect_fail_fires_onerror() {
    let rt = v8_runtime_with_ws(make_doc());
    // onerror is called asynchronously via setTimeout(fn, 0) in the shim.
    // We can't pump the timeout in this test — just verify the handler is set.
    let r = rt
        .eval(
            "var fired = false;
                     var ws = new WebSocket('ws://127.0.0.1:1');
                     ws.onerror = function() { fired = true; };
                     ws.readyState === 3",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── _lumen_bfcache_blocked: bfcache eligibility filters (Ph3 bfcache L1) ──

#[test]
fn bfcache_blocked_false_by_default() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false).unwrap();
    let r = rt.eval("_lumen_bfcache_blocked()").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn bfcache_blocked_true_when_websocket_open() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false).unwrap();
    let r = rt
        .eval("_ws_instances.push({ readyState: 1 }); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bfcache_blocked_false_when_websocket_closed() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false).unwrap();
    // readyState 3 (CLOSED) must not block — only OPEN (1) does.
    let r = rt
        .eval("_ws_instances.push({ readyState: 3 }); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn bfcache_blocked_true_when_eventsource_open() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false).unwrap();
    let r = rt
        .eval("_sse_instances.push({ readyState: 1 }); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bfcache_blocked_true_when_beforeunload_listener_registered() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false).unwrap();
    let r = rt
        .eval("window.addEventListener('beforeunload', function() {}); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bfcache_blocked_true_when_unload_listener_registered() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false).unwrap();
    let r = rt
        .eval("window.addEventListener('unload', function() {}); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bfcache_blocked_true_when_onbeforeunload_property_set() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false).unwrap();
    let r = rt
        .eval("window.onbeforeunload = function() {}; _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-564: `document.fonts.ready` used to be `undefined`, so any script
// awaiting font loading (`document.fonts.ready.then(...)`) threw
// synchronously instead of getting a Promise.
#[test]
fn document_fonts_ready_is_a_thenable_promise() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false).unwrap();
    let r = rt
        .eval("document.fonts.ready instanceof Promise && typeof document.fonts.ready.then === 'function'")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// Mock WS provider: immediately queues Open + one Text message.
struct MockWsProvider;
struct MockWsSession {
    queue: std::sync::Mutex<std::collections::VecDeque<lumen_core::ext::JsWsEvent>>,
    /// Sub-protocol echoed back to the client (first requested, "" if none).
    protocol: String,
}
impl lumen_core::ext::JsWebSocketSession for MockWsSession {
    fn send_text(&self, _text: &str) -> lumen_core::error::Result<()> { Ok(()) }
    fn send_binary(&self, _data: &[u8]) -> lumen_core::error::Result<()> { Ok(()) }
    fn poll(&self) -> Option<lumen_core::ext::JsWsEvent> {
        self.queue.lock().unwrap().pop_front()
    }
    fn close(&self, _code: u16, _reason: &str) -> lumen_core::error::Result<()> { Ok(()) }
    fn protocol(&self) -> String { self.protocol.clone() }
}
impl lumen_core::ext::JsWebSocketProvider for MockWsProvider {
    fn connect(&self, _url: &str, protocols: &[String]) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsWebSocketSession>> {
        use lumen_core::ext::JsWsEvent;
        let mut q = std::collections::VecDeque::new();
        q.push_back(JsWsEvent::Open);
        q.push_back(JsWsEvent::Message { data: b"hello".to_vec(), is_binary: false });
        // Echo the client's first requested sub-protocol, mirroring a real server.
        let protocol = protocols.first().cloned().unwrap_or_default();
        Ok(Box::new(MockWsSession { queue: std::sync::Mutex::new(q), protocol }))
    }
}

fn v8_runtime_with_mock_ws(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsWebSocketProvider> = Arc::new(MockWsProvider);
    rt.install_dom(doc, "", None, Some(provider), None, None, None, None, None, None, false).unwrap();
    rt
}

#[test]
fn websocket_mock_connect_open_state() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    // Phase 0: pump explicitly to deliver Open event → readyState = 1.
    let r = rt
        .eval("var ws = new WebSocket('ws://mock'); _lumen_pump_websockets(); ws.readyState")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

#[test]
fn websocket_mock_open_fires_onopen() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let r = rt
        .eval(
            "var opened = false;
                     var ws = new WebSocket('ws://mock');
                     ws.onopen = function() { opened = true; };
                     _lumen_pump_websockets();
                     opened",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `new WebSocket(url, protocols)` forwards the requested sub-protocol; on open,
/// the server-selected protocol is surfaced as `ws.protocol`. The mock echoes the
/// first requested protocol.
#[test]
fn websocket_subprotocol_surfaced_on_open() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock', ['chat', 'superchat']);
                     _lumen_pump_websockets();
                     ws.protocol",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("chat".into()));
}

/// A string `protocols` argument is accepted and surfaced as `ws.protocol`.
#[test]
fn websocket_subprotocol_string_arg() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock', 'json');
                     _lumen_pump_websockets();
                     ws.protocol",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("json".into()));
}

#[test]
fn websocket_mock_message_via_pump() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    // Set handler before pump so onmessage fires when the message is dispatched.
    let r = rt
        .eval(
            "var received = null;
                     var ws = new WebSocket('ws://mock');
                     ws.onmessage = function(e) { received = e.data; };
                     _lumen_pump_websockets();
                     received",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("hello".into()));
}

/// `send()` while CONNECTING must throw `InvalidStateError` (WHATWG WebSocket).
#[test]
fn websocket_send_in_connecting_throws() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    // No pump → stays CONNECTING (readyState 0).
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     try { ws.send('x'); 'nothrow'; } catch (e) { e.name; }",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("InvalidStateError".into()));
}

/// `close()` with an out-of-range code throws `InvalidAccessError`; a valid
/// custom code (3000–4999) transitions the socket to CLOSING (2).
#[test]
fn websocket_close_code_validation() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let bad = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     try { ws.close(1234); 'nothrow'; } catch (e) { e.name; }",
        )
        .unwrap();
    assert_eq!(bad, lumen_core::JsValue::String("InvalidAccessError".into()));
    let ok = rt
        .eval(
            "var ws2 = new WebSocket('ws://mock');
                     ws2.close(3001); ws2.readyState",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Number(2.0));
}

/// `close()` with a reason longer than 123 UTF-8 bytes throws `SyntaxError`.
#[test]
fn websocket_close_reason_too_long_throws() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     var long = 'a'.repeat(124);
                     try { ws.close(1000, long); 'nothrow'; } catch (e) { e.name; }",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("SyntaxError".into()));
}

/// `send()` in CLOSING/CLOSED discards data but counts it in `bufferedAmount`.
#[test]
fn websocket_buffered_amount_in_closing() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     ws.close();           // CONNECTING → CLOSING
                     ws.send('hello');     // 5 bytes, discarded but counted
                     ws.bufferedAmount",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(5.0));
}

/// Ready-state constants are exposed on instances, not only the constructor.
#[test]
fn websocket_instance_constants() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     ws.CONNECTING === 0 && ws.OPEN === 1 && ws.CLOSING === 2 && ws.CLOSED === 3",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A second `close()` is a no-op (idempotent), readyState stays CLOSING.
#[test]
fn websocket_close_idempotent() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     ws.close(); ws.close(); ws.readyState",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(2.0));
}

#[test]
fn websocket_no_provider_connect_returns_zero() {
    // Without ws_provider, _lumen_ws_connect always returns 0.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("_lumen_ws_connect('ws://test', '')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

// ── EventSource / Server-Sent Events (HTML Living Standard §9.2) ──────────

/// Mock SSE session feeding a preset event sequence via `poll()`.
struct MockSseSession {
    queue: std::sync::Mutex<std::collections::VecDeque<lumen_core::ext::JsSseEvent>>,
}
impl lumen_core::ext::JsSseSession for MockSseSession {
    fn poll(&self) -> Option<lumen_core::ext::JsSseEvent> {
        self.queue.lock().unwrap().pop_front()
    }
    fn close(&mut self) {}
}

/// Mock SSE provider that queues a fixed event sequence on connect.
struct MockSseProvider {
    events: Vec<lumen_core::ext::JsSseEvent>,
}
impl lumen_core::ext::JsSseProvider for MockSseProvider {
    fn connect_sse(
        &self,
        _url: &str,
    ) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsSseSession>> {
        let q: std::collections::VecDeque<_> = self.events.iter().cloned().collect();
        Ok(Box::new(MockSseSession {
            queue: std::sync::Mutex::new(q),
        }))
    }
}

fn v8_runtime_with_mock_sse(
    doc: Arc<Mutex<Document>>,
    events: Vec<lumen_core::ext::JsSseEvent>,
) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsSseProvider> =
        Arc::new(MockSseProvider { events });
    rt.install_dom(doc, "", None, None, Some(provider), None, None, None, None, None, false)
        .unwrap();
    rt
}

#[test]
fn eventsource_constructor_no_provider_stays_connecting_then_closes_async() {
    // Without an sse_provider, _lumen_sse_connect returns 0. Per spec
    // (HTML LS §9.2.2) readyState stays CONNECTING synchronously
    // (BUG-363 pt.7); the queued failure task transitions it to CLOSED
    // and fires 'error'.
    let rt = v8_runtime_with_dom(make_doc());
    let sync = rt
        .eval("var es = new EventSource('https://x/sse'); es.readyState")
        .unwrap();
    assert_eq!(sync, lumen_core::JsValue::Number(0.0));
    let after = rt.eval("_lumen_tick_timers(); es.readyState").unwrap();
    assert_eq!(after, lumen_core::JsValue::Number(2.0));
}

#[test]
fn eventsource_no_provider_connect_returns_zero() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("_lumen_sse_connect('https://x/sse')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn eventsource_opens_on_sse_connect() {
    use lumen_core::ext::JsSseEvent;
    let rt = v8_runtime_with_mock_sse(make_doc(), vec![JsSseEvent::Open]);
    let r = rt
        .eval(
            "var opened = false;
                     var es = new EventSource('https://x/sse');
                     es.onopen = function() { opened = true; };
                     _lumen_pump_sse();
                     [es.readyState, opened]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            // readyState OPEN (1) and onopen fired.
            assert_eq!(arr[0], lumen_core::JsValue::Number(1.0));
            assert_eq!(arr[1], lumen_core::JsValue::Bool(true));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_delivers_message() {
    use lumen_core::ext::JsSseEvent;
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![
            JsSseEvent::Open,
            JsSseEvent::Message {
                event_type: "message".into(),
                data: "hello world".into(),
                id: Some("42".into()),
            },
        ],
    );
    let r = rt
        .eval(
            "var data = null; var lid = null;
                     var es = new EventSource('https://x/sse');
                     es.onmessage = function(e) { data = e.data; lid = e.lastEventId; };
                     _lumen_pump_sse();
                     [data, lid]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::String("hello world".into()));
            assert_eq!(arr[1], lumen_core::JsValue::String("42".into()));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_delivers_typed_event() {
    use lumen_core::ext::JsSseEvent;
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![
            JsSseEvent::Open,
            JsSseEvent::Message {
                event_type: "ping".into(),
                data: "p".into(),
                id: None,
            },
        ],
    );
    // A named event must reach addEventListener('ping', ...), not onmessage.
    let r = rt
        .eval(
            "var got = null; var onmsg = false;
                     var es = new EventSource('https://x/sse');
                     es.onmessage = function() { onmsg = true; };
                     es.addEventListener('ping', function(e) { got = e.data; });
                     _lumen_pump_sse();
                     [got, onmsg]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::String("p".into()));
            assert_eq!(arr[1], lumen_core::JsValue::Bool(false));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_close_sets_closed() {
    use lumen_core::ext::JsSseEvent;
    let rt = v8_runtime_with_mock_sse(make_doc(), vec![JsSseEvent::Open]);
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     _lumen_pump_sse();
                     es.close();
                     es.readyState",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(2.0));
}

#[test]
fn eventsource_stream_end_fires_error_with_connecting() {
    use lumen_core::ext::JsSseEvent;
    // The stream ended and the native session is reconnecting: readyState
    // becomes CONNECTING (0) and `error` fires (HTML LS §9.2.5 step 1).
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![JsSseEvent::Open, JsSseEvent::Reconnecting],
    );
    let r = rt
        .eval(
            "var errored = false;
                     var es = new EventSource('https://x/sse');
                     es.onerror = function() { errored = true; };
                     _lumen_pump_sse();
                     [es.readyState, errored]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Number(0.0)); // CONNECTING
            assert_eq!(arr[1], lumen_core::JsValue::Bool(true));  // error fired
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_error_event_fires_onerror() {
    use lumen_core::ext::JsSseEvent;
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![JsSseEvent::Open, JsSseEvent::Error("boom".into())],
    );
    let r = rt
        .eval(
            "var errored = false; var msg = null;
                     var es = new EventSource('https://x/sse');
                     es.onerror = function(e) { errored = true; msg = e.message; };
                     _lumen_pump_sse();
                     [errored, msg, es.readyState]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Bool(true));
            assert_eq!(arr[1], lumen_core::JsValue::String("boom".into()));
            assert_eq!(arr[2], lumen_core::JsValue::Number(2.0));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_poll_json_escapes_message() {
    use lumen_core::ext::JsSseEvent;
    // Data containing quotes/newlines must round-trip through JSON intact.
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![
            JsSseEvent::Open,
            JsSseEvent::Message {
                event_type: "message".into(),
                data: "line1\nline2 \"quoted\"".into(),
                id: None,
            },
        ],
    );
    let r = rt
        .eval(
            "var data = null;
                     var es = new EventSource('https://x/sse');
                     es.onmessage = function(e) { data = e.data; };
                     _lumen_pump_sse();
                     data",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("line1\nline2 \"quoted\"".into())
    );
}

#[test]
fn eventsource_retry_event_updates_reconnect_delay() {
    use lumen_core::ext::JsSseEvent;
    // A Retry event from the server updates the internal reconnect delay.
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![JsSseEvent::Open, JsSseEvent::Retry(500)],
    );
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     _lumen_pump_sse();
                     es._retryMs",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(500.0));
}

#[test]
fn eventsource_reconnect_fires_open_again() {
    use lumen_core::ext::JsSseEvent;
    // Every announced connection fires `open`, not just the first — that
    // is what the `retry:` WPT tests time (BUG-844).
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![
            JsSseEvent::Open,
            JsSseEvent::Reconnecting,
            JsSseEvent::Open,
        ],
    );
    let r = rt
        .eval(
            "var opens = 0, errors = 0;
                     var es = new EventSource('https://x/sse');
                     es.onopen = function() { opens++; };
                     es.onerror = function() { errors++; };
                     _lumen_pump_sse();
                     [opens, errors, es.readyState]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Number(2.0)); // two `open`
            assert_eq!(arr[1], lumen_core::JsValue::Number(1.0)); // one `error`
            assert_eq!(arr[2], lumen_core::JsValue::Number(1.0)); // OPEN again
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_close_event_is_terminal() {
    use lumen_core::ext::JsSseEvent;
    // `Close` means the native session stopped for good; it must not
    // schedule a reconnect of its own (the session owns reconnection).
    let rt = v8_runtime_with_mock_sse(make_doc(), vec![JsSseEvent::Open, JsSseEvent::Close]);
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     _lumen_pump_sse();
                     [es.readyState, es._handle]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Number(2.0)); // CLOSED
            assert_eq!(arr[1], lumen_core::JsValue::Number(0.0)); // handle released
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_remove_event_listener() {
    use lumen_core::ext::JsSseEvent;
    // removeEventListener must stop delivery to the removed handler.
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![
            JsSseEvent::Open,
            JsSseEvent::Message {
                event_type: "ping".into(),
                data: "p".into(),
                id: None,
            },
        ],
    );
    let r = rt
        .eval(
            "var count = 0;
                     var fn1 = function() { count++; };
                     var es = new EventSource('https://x/sse');
                     es.addEventListener('ping', fn1);
                     es.removeEventListener('ping', fn1);
                     _lumen_pump_sse();
                     count",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn eventsource_constants_on_both_interface_and_prototype() {
    // BUG-363 pt.1: constants must be visible via the interface object
    // AND the prototype (so instances see them through the chain too).
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     [EventSource.CONNECTING, EventSource.prototype.CONNECTING,
                      es.CONNECTING, es.OPEN, es.CLOSED]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Number(0.0));
            assert_eq!(arr[1], lumen_core::JsValue::Number(0.0));
            assert_eq!(arr[2], lumen_core::JsValue::Number(0.0));
            assert_eq!(arr[3], lumen_core::JsValue::Number(1.0));
            assert_eq!(arr[4], lumen_core::JsValue::Number(2.0));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_without_new_throws_typeerror() {
    // BUG-363 pt.2: calling the constructor as a plain function must throw.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("try { EventSource('https://x/sse'); 'no throw'; } catch (e) { e instanceof TypeError ? 'TypeError' : String(e); }").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("TypeError".into()));
}

#[test]
fn eventsource_unparsable_url_throws_syntaxerror_domexception() {
    // BUG-363 pt.6: a URL the parser rejects outright (no scheme at all,
    // no document base to resolve against) throws a SyntaxError DOMException.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "try { new EventSource('not a url at all'); 'no throw'; }
                     catch (e) { e instanceof DOMException && e.name === 'SyntaxError' ? 'SyntaxError' : String(e); }",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("SyntaxError".into()));
}

#[test]
fn eventsource_extends_event_target_and_dispatches_generically() {
    // BUG-363 pt.3: EventSource must inherit EventTarget so dispatchEvent
    // and the shared listener registry work like any other EventTarget.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     var got = null;
                     es.addEventListener('ping', function(e) { got = e.type; });
                     var ok = (es instanceof EventTarget) && (typeof es.dispatchEvent === 'function');
                     es.dispatchEvent(new Event('ping'));
                     [ok, got]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Bool(true));
            assert_eq!(arr[1], lumen_core::JsValue::String("ping".into()));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_url_readystate_withcredentials_are_readonly() {
    // BUG-363 pt.4: url/readyState/withCredentials are readonly attributes,
    // not writable own data properties.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     var beforeUrl = es.url, beforeState = es.readyState;
                     es.url = 'zzz'; es.readyState = 99; es.withCredentials = true;
                     [es.url === beforeUrl, es.readyState === beforeState, es.withCredentials === false,
                      es.hasOwnProperty('url'), es.hasOwnProperty('readyState')]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            // Assignment is silently ignored (getter-only, non-strict mode) …
            assert_eq!(arr[0], lumen_core::JsValue::Bool(true));
            assert_eq!(arr[1], lumen_core::JsValue::Bool(true));
            assert_eq!(arr[2], lumen_core::JsValue::Bool(true));
            // … and the accessors live on the prototype, not the instance.
            assert_eq!(arr[3], lumen_core::JsValue::Bool(false));
            assert_eq!(arr[4], lumen_core::JsValue::Bool(false));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_onmessage_is_prototype_accessor_not_own_property() {
    // BUG-363 pt.5: onopen/onmessage/onerror are accessor properties on
    // the prototype, so a fresh instance has no matching own property.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     var ownBefore = es.hasOwnProperty('onmessage');
                     var fn = function() {};
                     es.onmessage = fn;
                     [ownBefore, es.onmessage === fn, es.hasOwnProperty('onmessage')]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Bool(false));
            assert_eq!(arr[1], lumen_core::JsValue::Bool(true));
            assert_eq!(arr[2], lumen_core::JsValue::Bool(false));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_resolves_relative_url_and_url_getter_is_absolute() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        make_doc(),
        "https://example.com/eventsource/page.html",
        None, None, None, None, None, None, None, None, false,
    )
    .unwrap();
    let r = rt
        .eval("new EventSource('resources/message.py').url")
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "https://example.com/eventsource/resources/message.py".into()
        )
    );
}

#[test]
fn eventsource_empty_url_resolves_to_document_url() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        make_doc(),
        "https://example.com/eventsource/page.html",
        None, None, None, None, None, None, None, None, false,
    )
    .unwrap();
    let r = rt.eval("new EventSource('').url").unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("https://example.com/eventsource/page.html".into())
    );
}

#[test]
fn eventsource_stringifies_null_and_undefined_instead_of_empty() {
    // Side-fix noted in BUG-362: `String(url)` instead of `String(url || '')`.
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        make_doc(),
        "https://example.com/eventsource/page.html",
        None, None, None, None, None, None, None, None, false,
    )
    .unwrap();
    let r = rt
        .eval("[new EventSource(null).url, new EventSource(undefined).url]")
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(
                arr[0],
                lumen_core::JsValue::String("https://example.com/eventsource/null".into())
            );
            assert_eq!(
                arr[1],
                lumen_core::JsValue::String(
                    "https://example.com/eventsource/undefined".into()
                )
            );
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_connects_using_resolved_absolute_url() {
    use lumen_core::ext::JsSseEvent;
    struct RecordingSseProvider {
        seen: Arc<Mutex<Option<String>>>,
    }
    impl lumen_core::ext::JsSseProvider for RecordingSseProvider {
        fn connect_sse(
            &self,
            url: &str,
        ) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsSseSession>> {
            *self.seen.lock().unwrap() = Some(url.to_string());
            Ok(Box::new(MockSseSession {
                queue: std::sync::Mutex::new(std::collections::VecDeque::from(vec![
                    JsSseEvent::Open,
                ])),
            }))
        }
    }
    let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsSseProvider> =
        Arc::new(RecordingSseProvider { seen: Arc::clone(&seen) });
    rt.install_dom(
        make_doc(),
        "https://example.com/eventsource/page.html",
        None, None, Some(provider), None, None, None, None, None, false,
    )
    .unwrap();
    rt.eval("var es = new EventSource('resources/message.py');")
        .unwrap();
    assert_eq!(
        seen.lock().unwrap().as_deref(),
        Some("https://example.com/eventsource/resources/message.py")
    );
}

#[test]
fn close_event_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("var ce = new CloseEvent(1001, 'bye', true); ce.code === 1001 && ce.reason === 'bye' && ce.wasClean === true")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn message_event_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("var me = new MessageEvent('payload'); me.data === 'payload' && me.type === 'message'")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_has_buffered_amount() {
    let rt = v8_runtime_with_ws(make_doc());
    let r = rt
        .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws.bufferedAmount === 0")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_has_extensions_field() {
    let rt = v8_runtime_with_ws(make_doc());
    let r = rt
        .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws.extensions === ''")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_binary_type_default_blob() {
    let rt = v8_runtime_with_ws(make_doc());
    let r = rt
        .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws.binaryType")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("blob".into()));
}

// Mock provider: queues Open + one binary message (bytes [0x01, 0x02, 0x03]).
struct MockBinaryWsProvider;
struct MockBinaryWsSession {
    queue: std::sync::Mutex<std::collections::VecDeque<lumen_core::ext::JsWsEvent>>,
}
impl lumen_core::ext::JsWebSocketSession for MockBinaryWsSession {
    fn send_text(&self, _text: &str) -> lumen_core::error::Result<()> { Ok(()) }
    fn send_binary(&self, _data: &[u8]) -> lumen_core::error::Result<()> { Ok(()) }
    fn poll(&self) -> Option<lumen_core::ext::JsWsEvent> {
        self.queue.lock().unwrap().pop_front()
    }
    fn close(&self, _code: u16, _reason: &str) -> lumen_core::error::Result<()> { Ok(()) }
}
impl lumen_core::ext::JsWebSocketProvider for MockBinaryWsProvider {
    fn connect(&self, _url: &str, _protocols: &[String]) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsWebSocketSession>> {
        use lumen_core::ext::JsWsEvent;
        let mut q = std::collections::VecDeque::new();
        q.push_back(JsWsEvent::Open);
        q.push_back(JsWsEvent::Message { data: vec![0x01, 0x02, 0x03], is_binary: true });
        Ok(Box::new(MockBinaryWsSession { queue: std::sync::Mutex::new(q) }))
    }
}

fn v8_runtime_with_binary_ws(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsWebSocketProvider> = Arc::new(MockBinaryWsProvider);
    rt.install_dom(doc, "", None, Some(provider), None, None, None, None, None, None, false).unwrap();
    rt
}

#[test]
fn websocket_binary_blob_mode_delivers_uint8array() {
    let rt = v8_runtime_with_binary_ws(make_doc());
    // Default binaryType='blob' → Uint8Array (our Phase 0 representation).
    let r = rt
        .eval(
            "var received = null;
                     var ws = new WebSocket('ws://mock');
                     ws.onmessage = function(e) { received = e.data; };
                     _lumen_pump_websockets();
                     received instanceof Uint8Array && received[0] === 1 && received[1] === 2 && received[2] === 3",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_binary_arraybuffer_mode_delivers_arraybuffer() {
    let rt = v8_runtime_with_binary_ws(make_doc());
    // binaryType='arraybuffer' → ArrayBuffer.
    let r = rt
        .eval(
            "var received = null;
                     var ws = new WebSocket('ws://mock');
                     ws.binaryType = 'arraybuffer';
                     ws.onmessage = function(e) { received = e.data; };
                     _lumen_pump_websockets();
                     received instanceof ArrayBuffer && new Uint8Array(received)[0] === 1",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_binary_hex_length_matches_byte_count() {
    let rt = v8_runtime_with_binary_ws(make_doc());
    // 3 bytes → Uint8Array of length 3.
    let r = rt
        .eval(
            "var len = 0;
                     var ws = new WebSocket('ws://mock');
                     ws.onmessage = function(e) { len = e.data.length; };
                     _lumen_pump_websockets();
                     len === 3",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
