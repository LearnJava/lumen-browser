//! V8 port of the "classList / CSSStyleDeclaration", "Element event dispatch +
//! Event/CustomEvent ctors", "Service Worker + Cache Storage" and "Cache API —
//! SQLite backend" rows (S12b-24-events-cache) from the scoping table in
//! `docs/tasks/ph3-v8-migration.md`: `classList`, `style`/`CSSStyleDeclaration`,
//! `addEventListener`/`dispatchEvent`, `Event`/`CustomEvent` constructors,
//! `navigator.serviceWorker`, the `caches` API (mock `CacheBackend` dispatch,
//! both the in-memory-map "SQLite" dispatch tests and the plain `_lumen_cache_*`
//! primitive tests).
//!
//! 72 tests moved here from the QuickJS monolith above. Most bodies are
//! `rt.eval(...)` verbatim (only the runtime constructor changed) — the
//! classList/style/event-dispatch/sqlite-backend-dispatch families never
//! touch Promises. The `navigator.serviceWorker` and `caches.open()` families
//! do: their QuickJS bodies called `_lumen_drain_microtasks()` (a manual
//! `ctx.execute_pending_job()` pump, dom.rs:3024) between scheduling a
//! `.then()` callback and reading its result, all inside one `rt.eval()`
//! string. `V8JsRuntime` registers `_lumen_drain_microtasks` as a no-op
//! (`v8_runtime.rs:3611`) because V8 auto-runs its microtask queue after each
//! *script*, not mid-script (confirmed by `sw_worker.rs`'s S10 port notes) — so
//! a promise scheduled and awaited inside the same `eval()` string never
//! resolves before the trailing check runs. Ported by splitting each such body
//! into two separate `rt.eval()` calls (schedule in the first, read the
//! already-resolved value in the second) and dropping the drain call, per the
//! S12b-24 scoping brief's general instruction for this pattern.
//!
//! Gated on `v8-backend` like every other ported module.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// V8 twin of [`super::runtime_with_url`].
fn v8_runtime_with_url(url: &str) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), url, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

// ── classList ────────────────────────────────────────────────────────────

#[test]
fn classlist_contains_true() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('.highlight').classList.contains('highlight')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn classlist_contains_false() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('.highlight').classList.contains('missing')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(false));
}

#[test]
fn classlist_add_and_contains() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.getElementById('main').classList.add('active');").unwrap();
    let result = rt
        .eval("document.getElementById('main').classList.contains('active')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn classlist_remove() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.querySelector('.highlight').classList.remove('highlight');",
    )
    .unwrap();
    let result = rt
        .eval("document.querySelector('.highlight') === null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn classlist_toggle_adds_when_absent() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.getElementById('main').classList.toggle('open')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
    let has = rt
        .eval("document.getElementById('main').classList.contains('open')")
        .unwrap();
    assert_eq!(has, lumen_core::JsValue::Bool(true));
}

#[test]
fn classlist_toggle_removes_when_present() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.querySelector('.highlight').classList.toggle('highlight');").unwrap();
    let has = rt
        .eval("document.querySelector('.highlight') === null")
        .unwrap();
    assert_eq!(has, lumen_core::JsValue::Bool(true));
}

#[test]
fn classlist_replace() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.querySelector('.highlight').classList.replace('highlight', 'selected');",
    )
    .unwrap();
    let old = rt
        .eval("document.querySelector('.highlight') === null")
        .unwrap();
    assert_eq!(old, lumen_core::JsValue::Bool(true));
    let new = rt
        .eval("document.querySelector('.selected') !== null")
        .unwrap();
    assert_eq!(new, lumen_core::JsValue::Bool(true));
}

#[test]
fn classlist_length() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('.highlight').classList.length")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

#[test]
fn classlist_to_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('.highlight').classList.toString()")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("highlight".into()));
}

// ── style / CSSStyleDeclaration ──────────────────────────────────────────

#[test]
fn style_set_and_get_property() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.getElementById('main').style.setProperty('color', 'red');")
        .unwrap();
    let result = rt
        .eval("document.getElementById('main').style.getPropertyValue('color')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("red".into()));
}

#[test]
fn style_assignment_via_property_name() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.getElementById('main').style.color = 'blue';")
        .unwrap();
    let result = rt
        .eval("document.getElementById('main').style.color")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("blue".into()));
}

#[test]
fn style_camel_case_to_kebab() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.getElementById('main').style.backgroundColor = 'green';")
        .unwrap();
    let result = rt
        .eval("document.getElementById('main').style.getPropertyValue('background-color')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("green".into()));
}

#[test]
fn style_remove_property() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var el = document.getElementById('main'); \
                 el.style.color = 'red'; \
                 el.style.removeProperty('color');",
    )
    .unwrap();
    let result = rt
        .eval("document.getElementById('main').style.getPropertyValue('color')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("".into()));
}

#[test]
fn style_css_text_roundtrip() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.getElementById('main').style.cssText = 'color: red; font-size: 12px';",
    )
    .unwrap();
    let color = rt
        .eval("document.getElementById('main').style.getPropertyValue('color')")
        .unwrap();
    assert_eq!(color, lumen_core::JsValue::String("red".into()));
    let size = rt
        .eval("document.getElementById('main').style.getPropertyValue('font-size')")
        .unwrap();
    assert_eq!(size, lumen_core::JsValue::String("12px".into()));
}

// ── addEventListener / dispatchEvent on elements ─────────────────────────

#[test]
fn element_add_event_listener_and_dispatch() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var received = null; \
                     var el = document.getElementById('main'); \
                     el.addEventListener('click', function(e) { received = e.type; }); \
                     el.dispatchEvent(new Event('click')); \
                     received",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("click".into()));
}

#[test]
fn element_remove_event_listener_stops_dispatch() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var count = 0; \
                     var el = document.getElementById('main'); \
                     function h() { count++; } \
                     el.addEventListener('click', h); \
                     el.dispatchEvent(new Event('click')); \
                     el.removeEventListener('click', h); \
                     el.dispatchEvent(new Event('click')); \
                     count",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

#[test]
fn custom_event_detail_accessible_in_handler() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var got = null; \
                     var el = document.getElementById('main'); \
                     el.addEventListener('myevent', function(e) { got = e.detail; }); \
                     el.dispatchEvent(new CustomEvent('myevent', { detail: 42 })); \
                     got",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(42.0));
}

#[test]
fn event_prevent_default() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var el = document.getElementById('main'); \
                     el.addEventListener('submit', function(e) { e.preventDefault(); }); \
                     var ev = new Event('submit', { cancelable: true }); \
                     var ret = el.dispatchEvent(ev); \
                     ret",
        )
        .unwrap();
    // dispatchEvent returns false when defaultPrevented
    assert_eq!(result, lumen_core::JsValue::Bool(false));
}

#[test]
fn stop_immediate_propagation_stops_subsequent_listeners() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var calls = 0; \
                     var el = document.getElementById('main'); \
                     el.addEventListener('x', function(e) { calls++; e.stopImmediatePropagation(); }); \
                     el.addEventListener('x', function(e) { calls++; }); \
                     el.dispatchEvent(new Event('x')); \
                     calls",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

// ── Event / CustomEvent constructors ─────────────────────────────────────

#[test]
fn event_constructor_sets_type() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("new Event('load').type").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("load".into()));
}

#[test]
fn event_bubbles_cancelable_defaults_false() {
    let rt = v8_runtime_with_dom(make_doc());
    let bubbles = rt.eval("new Event('x').bubbles").unwrap();
    assert_eq!(bubbles, lumen_core::JsValue::Bool(false));
    let cancelable = rt.eval("new Event('x').cancelable").unwrap();
    assert_eq!(cancelable, lumen_core::JsValue::Bool(false));
}

#[test]
fn custom_event_detail_null_by_default() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("new CustomEvent('x').detail === null").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn event_is_trusted_false_by_default() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("new Event('click').isTrusted").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(false));
}

#[test]
fn event_is_trusted_true_when_specified() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("new Event('click', { isTrusted: true }).isTrusted").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn custom_event_is_trusted_inherits_from_event() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("new CustomEvent('x', { isTrusted: true }).isTrusted").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn dispatchevent_creates_untrusted_event() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(
        r#"
                var evt = new Event('test');
                var el = document.createElement('div');
                var receivedEvent = null;
                el.addEventListener('test', function(e) { receivedEvent = e; });
                el.dispatchEvent(evt);
                receivedEvent.isTrusted === false
                "#
    ).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// ── navigator.serviceWorker ───────────────────────────────────────────────

#[test]
fn navigator_has_service_worker() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("typeof navigator.serviceWorker === 'object'")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn sw_register_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            r#"
                    var p = navigator.serviceWorker.register('/sw.js', { scope: '/app/' });
                    typeof p.then === 'function'
                    "#,
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn sw_register_calls_lumen_primitive() {
    // Pass a file URL so that _sw_origin = 'file://' (protocol + '//' + host).
    let rt = v8_runtime_with_url("file:///test.html");
    rt.eval("navigator.serviceWorker.register('/sw.js', { scope: '/' });")
        .unwrap();
    let result = rt.eval("_lumen_sw_has_registration('file://')").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn sw_registration_has_installing_worker() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval(
        r#"
                var reg = null;
                navigator.serviceWorker.register('/sw.js', { scope: '/' })
                    .then(function(r) { reg = r; });
                "#,
    )
    .unwrap();
    let result = rt.eval("reg !== null && reg.installing !== null").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn sw_worker_has_state_installing() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval(
        r#"
                var reg = null;
                navigator.serviceWorker.register('/sw.js')
                    .then(function(r) { reg = r; });
                "#,
    )
    .unwrap();
    let result = rt.eval("reg.installing.state").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("installing".into()));
}

#[test]
fn sw_container_has_event_target() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            r#"
                    typeof navigator.serviceWorker.addEventListener === 'function' &&
                    typeof navigator.serviceWorker.removeEventListener === 'function'
                    "#,
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn sw_get_registration_returns_promise() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval("navigator.serviceWorker.register('/sw.js', { scope: '/' });")
        .unwrap();
    let result = rt
        .eval("typeof navigator.serviceWorker.getRegistration('/').then === 'function'")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn sw_get_registrations_returns_array() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval("navigator.serviceWorker.register('/sw.js');").unwrap();
    rt.eval(
        r#"
                var arr = null;
                navigator.serviceWorker.getRegistrations()
                    .then(function(regs) { arr = regs; });
                "#,
    )
    .unwrap();
    let result = rt.eval("Array.isArray(arr) && arr.length === 1").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn sw_ready_property_is_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("typeof navigator.serviceWorker.ready.then === 'function'")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn sw_registration_has_event_target() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval(
        r#"
                var reg = null;
                navigator.serviceWorker.register('/sw.js')
                    .then(function(r) { reg = r; });
                "#,
    )
    .unwrap();
    let result = rt
        .eval(
            r#"
                    typeof reg.addEventListener === 'function' &&
                    typeof reg.dispatchEvent === 'function'
                    "#,
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn sw_persist_and_load_no_throw() {
    let rt = v8_runtime_with_url("https://example.com/");
    // Without a backend, persist/load are no-ops — must not throw.
    rt.eval("_lumen_sw_persist('https://example.com', '[{\"scope\":\"/\"}]');")
        .unwrap();
    let result = rt.eval("_lumen_sw_load('https://example.com')").unwrap();
    assert!(matches!(
        result,
        lumen_core::JsValue::Null | lumen_core::JsValue::Undefined
    ));
}

#[test]
fn sw_unregister_removes_registration() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval("navigator.serviceWorker.register('/sw.js', { scope: '/app/' });")
        .unwrap();
    rt.eval(
        r#"
                navigator.serviceWorker.getRegistration('/app/')
                    .then(function(reg) { if (reg) reg.unregister(); });
                "#,
    )
    .unwrap();
    rt.eval(
        r#"
                var arr = null;
                navigator.serviceWorker.getRegistrations()
                    .then(function(r) { arr = r; });
                "#,
    )
    .unwrap();
    let result = rt.eval("arr.length").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(0.0));
}

#[test]
fn sw_worker_post_message_does_not_throw() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval(
        r#"
                var reg = null;
                navigator.serviceWorker.register('/sw.js')
                    .then(function(r) { reg = r; });
                "#,
    )
    .unwrap();
    let result = rt
        .eval(
            r#"
                    var threw = false;
                    try { reg.installing.postMessage('hello'); } catch(e) { threw = true; }
                    !threw
                    "#,
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// ── caches API ────────────────────────────────────────────────────────────

#[test]
fn caches_object_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("typeof caches === 'object'").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn caches_open_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("typeof caches.open('v1').then === 'function'")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn cache_has_returns_false_for_unknown() {
    let rt = v8_runtime_with_dom(make_doc());
    // has() returns promise; we check the primitive directly.
    let result = rt
        .eval("_lumen_cache_has('', 'nonexistent')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(false));
}

// helper: put a minimal GET 200 cache entry via the native binding
fn cache_put_test(rt: &V8JsRuntime, origin: &str, name: &str, url: &str) {
    rt.eval(&format!(
        r#"_lumen_cache_put('{origin}', '{name}', '{url}', '{{"method":"GET","status":200,"statusText":"OK","headers":{{}}}}', [72, 101, 108, 108, 111]);"#
    ))
    .unwrap();
}

#[test]
fn cache_put_and_match_roundtrip() {
    let rt = v8_runtime_with_dom(make_doc());
    cache_put_test(&rt, "", "v1", "https://x.com/a");
    assert_eq!(
        rt.eval("_lumen_cache_has('', 'v1')").unwrap(),
        lumen_core::JsValue::Bool(true)
    );
    let keys = rt.eval("_lumen_cache_keys('', 'v1')").unwrap();
    assert_eq!(
        keys,
        lumen_core::JsValue::Array(vec![lumen_core::JsValue::String("https://x.com/a".into())])
    );
}

#[test]
fn cache_match_returns_body_bytes() {
    let rt = v8_runtime_with_dom(make_doc());
    cache_put_test(&rt, "", "v1", "https://x.com/a");
    // _lumen_cache_match returns a Uint8Array-like value (body bytes)
    let len = rt
        .eval("_lumen_cache_match('', 'v1', 'https://x.com/a').length")
        .unwrap();
    assert_eq!(len, lumen_core::JsValue::Number(5.0)); // "Hello" = 5 bytes
}

#[test]
fn cache_match_info_returns_json_metadata() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"_lumen_cache_put('', 'v1', 'https://x.com/css', '{"method":"GET","status":304,"statusText":"Not Modified","headers":{"content-type":"text/css"}}', []);"#)
        .unwrap();
    let info_str = rt
        .eval("_lumen_cache_match_info('', 'v1', 'https://x.com/css')")
        .unwrap();
    if let lumen_core::JsValue::String(s) = info_str {
        assert!(s.contains("304"));
        assert!(s.contains("Not Modified"));
        assert!(s.contains("content-type"));
    } else {
        panic!("expected String from _lumen_cache_match_info");
    }
}

#[test]
fn cache_match_info_returns_none_on_miss() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("_lumen_cache_match_info('', 'v1', 'https://x.com/missing') === undefined")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn cache_match_any_returns_none_on_miss() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("_lumen_cache_match_any('', 'https://x.com/missing') === undefined")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn cache_match_any_info_finds_across_caches() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"_lumen_cache_put('', 'static', 'https://x.com/style.css', '{"method":"GET","status":200,"statusText":"OK","headers":{}}', []);"#)
        .unwrap();
    let r = rt
        .eval("_lumen_cache_match_any_info('', 'https://x.com/style.css') !== undefined")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn cache_delete_returns_true_when_found() {
    let rt = v8_runtime_with_dom(make_doc());
    cache_put_test(&rt, "", "v1", "https://x.com/b");
    let r = rt
        .eval("_lumen_cache_delete('', 'v1', 'https://x.com/b')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let keys = rt.eval("_lumen_cache_keys('', 'v1')").unwrap();
    assert_eq!(keys, lumen_core::JsValue::Array(vec![]));
}

#[test]
fn cache_delete_returns_false_on_miss() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("_lumen_cache_delete('', 'v1', 'https://x.com/nonexistent')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn cache_keys_full_returns_method() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"_lumen_cache_put('', 'v1', 'https://x.com/api', '{"method":"POST","status":201,"statusText":"Created","headers":{}}', []);"#)
        .unwrap();
    let r = rt
        .eval("_lumen_cache_keys_full('', 'v1').indexOf('POST') >= 0")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn cache_delete_cache_returns_true_when_found() {
    let rt = v8_runtime_with_dom(make_doc());
    cache_put_test(&rt, "", "v1", "https://x.com/r");
    let r = rt
        .eval("_lumen_cache_delete_cache('', 'v1')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    assert_eq!(
        rt.eval("_lumen_cache_has('', 'v1')").unwrap(),
        lumen_core::JsValue::Bool(false)
    );
}

#[test]
fn cache_delete_cache_returns_false_when_missing() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("_lumen_cache_delete_cache('', 'nonexistent')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn cache_names_lists_opened_caches() {
    let rt = v8_runtime_with_dom(make_doc());
    cache_put_test(&rt, "", "alpha", "https://x.com/r");
    cache_put_test(&rt, "", "beta", "https://x.com/s");
    let mut names = match rt.eval("_lumen_cache_names('')").unwrap() {
        lumen_core::JsValue::Array(a) => a
            .into_iter()
            .filter_map(|v| {
                if let lumen_core::JsValue::String(s) = v { Some(s) } else { None }
            })
            .collect::<Vec<_>>(),
        _ => vec![],
    };
    names.sort();
    assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
}

#[test]
fn caches_open_returns_cache_with_match() {
    let rt = v8_runtime_with_dom(make_doc());
    // Open cache first to obtain handle, then put with same _sw_origin, then match.
    rt.eval(
        r#"
                var _cache_oc = null;
                caches.open('my-cache').then(function(c) { _cache_oc = c; });
                "#,
    )
    .unwrap();
    rt.eval(r#"
                _lumen_cache_put(_sw_origin, 'my-cache', 'https://x.com/data',
                    '{"method":"GET","status":200,"statusText":"OK","headers":{}}', [1,2,3]);
                var _result_oc;
                _cache_oc.match('https://x.com/data').then(function(r) { _result_oc = r !== undefined; });
            "#).unwrap();
    let r = rt.eval("_result_oc").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn caches_has_returns_true_after_put() {
    let rt = v8_runtime_with_dom(make_doc());
    cache_put_test(&rt, "", "my-cache", "https://x.com/x");
    let r = rt
        .eval("_lumen_cache_has('', 'my-cache')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn caches_delete_returns_true_when_found() {
    let rt = v8_runtime_with_dom(make_doc());
    cache_put_test(&rt, "", "old-cache", "https://x.com/z");
    // caches.delete returns a Promise<bool>; verify via native binding
    let had = rt.eval("_lumen_cache_delete_cache('', 'old-cache')").unwrap();
    assert_eq!(had, lumen_core::JsValue::Bool(true));
}

#[test]
fn cache_matchall_returns_all_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        r#"
                var _cache_ma = null;
                caches.open('v1-ma').then(function(c) { _cache_ma = c; });
                "#,
    )
    .unwrap();
    rt.eval(r#"
                _lumen_cache_put(_sw_origin, 'v1-ma', 'https://x.com/a', '{"method":"GET","status":200,"statusText":"OK","headers":{}}', [1]);
                _lumen_cache_put(_sw_origin, 'v1-ma', 'https://x.com/b', '{"method":"GET","status":200,"statusText":"OK","headers":{}}', [2]);
                var _all;
                _cache_ma.matchAll().then(function(arr) { _all = arr.length; });
            "#).unwrap();
    let r = rt.eval("_all").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(2.0));
}

#[test]
fn cache_keys_returns_request_objects() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        r#"
                var _cache_kr = null;
                caches.open('v1-kr').then(function(c) { _cache_kr = c; });
                "#,
    )
    .unwrap();
    rt.eval(r#"
                _lumen_cache_put(_sw_origin, 'v1-kr', 'https://x.com/page', '{"method":"GET","status":200,"statusText":"OK","headers":{}}', []);
                var _url_kr;
                _cache_kr.keys().then(function(reqs) { _url_kr = reqs[0] && reqs[0].url; });
            "#).unwrap();
    let r = rt.eval("_url_kr").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("https://x.com/page".into()));
}

#[test]
fn window_has_caches() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("typeof window.caches === 'object'").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// ── Cache API — CacheBackend trait dispatch tests ─────────────────────────
//
// MockCacheBackend exercises the CacheBackend dispatch path (mirrored by
// `cache_meta_method` above and the V8 install path's own natives)
// without pulling in lumen-storage as a test dependency. The SQLite
// implementation is separately tested in lumen-storage::cache_storage.

type MockCacheEntry = (String, Vec<u8>);
type MockCacheMap = std::collections::HashMap<
    String, // origin
    std::collections::HashMap<
        String, // cache_name
        std::collections::HashMap<String, MockCacheEntry>, // url → (meta, body)
    >,
>;

struct MockCacheBackend {
    data: Mutex<MockCacheMap>,
}

impl MockCacheBackend {
    fn new() -> Self {
        Self { data: Mutex::new(std::collections::HashMap::new()) }
    }
}

impl lumen_core::ext::CacheBackend for MockCacheBackend {
    fn cache_put(&self, origin: &str, name: &str, url: &str, meta_json: &str, body: &[u8]) {
        self.data.lock().unwrap()
            .entry(origin.to_owned()).or_default()
            .entry(name.to_owned()).or_default()
            .insert(url.to_owned(), (meta_json.to_owned(), body.to_vec()));
    }
    fn cache_match(&self, origin: &str, name: &str, url: &str) -> Option<(String, Vec<u8>)> {
        self.data.lock().unwrap()
            .get(origin)?.get(name)?.get(url)
            .map(|(m, b)| (m.clone(), b.clone()))
    }
    fn cache_match_any(&self, origin: &str, url: &str) -> Option<(String, Vec<u8>)> {
        let g = self.data.lock().unwrap();
        let caches = g.get(origin)?;
        for c in caches.values() {
            if let Some((m, b)) = c.get(url) { return Some((m.clone(), b.clone())); }
        }
        None
    }
    fn cache_delete(&self, origin: &str, name: &str, url: &str) -> bool {
        self.data.lock().unwrap()
            .get_mut(origin).and_then(|c| c.get_mut(name))
            .and_then(|c| c.remove(url)).is_some()
    }
    fn cache_keys(&self, origin: &str, name: &str) -> Vec<(String, String)> {
        self.data.lock().unwrap()
            .get(origin).and_then(|c| c.get(name))
            .map(|c| c.iter().map(|(u, (meta, _))| {
                let method = cache_meta_method(meta);
                (u.clone(), method)
            }).collect())
            .unwrap_or_default()
    }
    fn cache_has(&self, origin: &str, name: &str) -> bool {
        self.data.lock().unwrap()
            .get(origin).and_then(|c| c.get(name))
            .map(|c| !c.is_empty()).unwrap_or(false)
    }
    fn cache_delete_cache(&self, origin: &str, name: &str) -> bool {
        self.data.lock().unwrap()
            .get_mut(origin).and_then(|c| c.remove(name)).is_some()
    }
    fn cache_names(&self, origin: &str) -> Vec<String> {
        self.data.lock().unwrap()
            .get(origin).map(|c| c.keys().cloned().collect()).unwrap_or_default()
    }
}

fn v8_runtime_with_cache_backend() -> V8JsRuntime {
    let be: Arc<dyn lumen_core::ext::CacheBackend> = Arc::new(MockCacheBackend::new());
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "https://example.com/", None, None, None, None, None, None, Some(be), None, false)
        .unwrap();
    rt
}

fn sqlite_cache_put(rt: &V8JsRuntime, cache: &str, url: &str) {
    rt.eval(&format!(
        r#"_lumen_cache_put('https://example.com/', '{cache}', '{url}', '{{"method":"GET","status":200,"statusText":"OK","headers":{{}}}}', [72,101,108,108,111]);"#
    ))
    .unwrap();
}

#[test]
fn sqlite_backend_put_and_has() {
    let rt = v8_runtime_with_cache_backend();
    sqlite_cache_put(&rt, "v1", "https://example.com/main.js");
    let r = rt.eval("_lumen_cache_has('https://example.com/', 'v1')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn sqlite_backend_match_returns_body() {
    let rt = v8_runtime_with_cache_backend();
    sqlite_cache_put(&rt, "v1", "https://example.com/style.css");
    let len = rt.eval("_lumen_cache_match('https://example.com/', 'v1', 'https://example.com/style.css').length").unwrap();
    assert_eq!(len, lumen_core::JsValue::Number(5.0)); // "Hello" = 5 bytes
}

#[test]
fn sqlite_backend_match_info_roundtrip() {
    let rt = v8_runtime_with_cache_backend();
    rt.eval(r#"_lumen_cache_put('https://example.com/', 'v1', 'https://example.com/api',
                '{"method":"GET","status":304,"statusText":"Not Modified","headers":{"etag":"abc123"}}', []);"#)
        .unwrap();
    let meta = rt.eval("_lumen_cache_match_info('https://example.com/', 'v1', 'https://example.com/api')").unwrap();
    if let lumen_core::JsValue::String(s) = meta {
        assert!(s.contains("304"));
        assert!(s.contains("etag"));
    } else {
        panic!("expected String from _lumen_cache_match_info (sqlite backend)");
    }
}

#[test]
fn sqlite_backend_match_any_searches_all_caches() {
    let rt = v8_runtime_with_cache_backend();
    sqlite_cache_put(&rt, "static", "https://example.com/logo.png");
    let body = rt.eval("_lumen_cache_match_any('https://example.com/', 'https://example.com/logo.png') !== null && _lumen_cache_match_any('https://example.com/', 'https://example.com/logo.png') !== undefined").unwrap();
    assert_eq!(body, lumen_core::JsValue::Bool(true));
}

#[test]
fn sqlite_backend_delete_entry() {
    let rt = v8_runtime_with_cache_backend();
    sqlite_cache_put(&rt, "v1", "https://example.com/old");
    let deleted = rt.eval("_lumen_cache_delete('https://example.com/', 'v1', 'https://example.com/old')").unwrap();
    assert_eq!(deleted, lumen_core::JsValue::Bool(true));
    let after = rt.eval("_lumen_cache_match('https://example.com/', 'v1', 'https://example.com/old') === undefined").unwrap();
    assert_eq!(after, lumen_core::JsValue::Bool(true));
}

#[test]
fn sqlite_backend_keys_lists_urls() {
    let rt = v8_runtime_with_cache_backend();
    sqlite_cache_put(&rt, "v1", "https://example.com/a");
    sqlite_cache_put(&rt, "v1", "https://example.com/b");
    let keys = rt.eval("_lumen_cache_keys('https://example.com/', 'v1')").unwrap();
    if let lumen_core::JsValue::Array(arr) = keys {
        assert_eq!(arr.len(), 2);
    } else {
        panic!("expected Array");
    }
}

#[test]
fn sqlite_backend_delete_cache() {
    let rt = v8_runtime_with_cache_backend();
    sqlite_cache_put(&rt, "tmp", "https://example.com/x");
    let del = rt.eval("_lumen_cache_delete_cache('https://example.com/', 'tmp')").unwrap();
    assert_eq!(del, lumen_core::JsValue::Bool(true));
    let has = rt.eval("_lumen_cache_has('https://example.com/', 'tmp')").unwrap();
    assert_eq!(has, lumen_core::JsValue::Bool(false));
}

#[test]
fn sqlite_backend_cache_names() {
    let rt = v8_runtime_with_cache_backend();
    sqlite_cache_put(&rt, "alpha", "https://example.com/1");
    sqlite_cache_put(&rt, "beta", "https://example.com/2");
    let names = rt.eval("_lumen_cache_names('https://example.com/')").unwrap();
    if let lumen_core::JsValue::Array(arr) = names {
        let strs: Vec<String> = arr
            .into_iter()
            .filter_map(|v| if let lumen_core::JsValue::String(s) = v { Some(s) } else { None })
            .collect();
        assert!(strs.contains(&"alpha".to_string()));
        assert!(strs.contains(&"beta".to_string()));
    } else {
        panic!("expected Array");
    }
}

#[test]
fn sqlite_backend_match_miss_returns_none() {
    let rt = v8_runtime_with_cache_backend();
    let r = rt.eval("_lumen_cache_match('https://example.com/', 'v1', 'https://example.com/missing') === undefined").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn sqlite_backend_keys_full_includes_method() {
    let rt = v8_runtime_with_cache_backend();
    rt.eval(r#"_lumen_cache_put('https://example.com/', 'v1', 'https://example.com/post',
                '{"method":"POST","status":201,"statusText":"Created","headers":{}}', []);"#)
        .unwrap();
    let r = rt.eval("_lumen_cache_keys_full('https://example.com/', 'v1').indexOf('POST') >= 0").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn sqlite_backend_has_false_when_empty() {
    let rt = v8_runtime_with_cache_backend();
    let r = rt.eval("_lumen_cache_has('https://example.com/', 'nonexistent')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn sqlite_backend_delete_returns_false_on_miss() {
    let rt = v8_runtime_with_cache_backend();
    let r = rt.eval("_lumen_cache_delete('https://example.com/', 'v1', 'https://example.com/nosuchurl')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}
