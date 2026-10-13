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
pub(super) fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
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
fn dispatch_composition_at_targets_node_and_bubbles() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _log = [];
                var el = document.getElementById('main');
                document.body.addEventListener('compositionupdate', function(e) {
                    _log.push(e.type + ':' + e.data);
                });
                _lumen_dispatch_composition_at(el.__nid__, 'compositionupdate', 'あい');
            "#).unwrap();
    let result = rt.eval("_log.join('|')").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("compositionupdate:あい".into()));
}

#[test]
fn window_has_dispatch_composition() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("typeof __lumen_C._lumen_dispatch_composition === 'function'")
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
    let result = rt.eval("typeof __lumen_C._lumen_fire_page_lifecycle === 'function'").unwrap();
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
