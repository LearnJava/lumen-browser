//! BUG-1139 — a `message` event from `window.postMessage` is fired through a
//! real dispatch at the window (HTML LS §9.3.3 «window post message steps»):
//! `target`/`currentTarget` are the window during delivery.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true")
        .unwrap();
    rt.install_dom(
        doc,
        "https://example.test/",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        false, None,
    )
    .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// The bug report's repro — webmd `app-core.js` reads `t.target.location.origin`.
#[test]
fn post_message_event_targets_the_window() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "globalThis.__r = null; \
         window.addEventListener('message', function(t) { \
           globalThis.__r = { target: t.target === window, current: t.currentTarget === window, \
                          phase: t.eventPhase, self: this === window, \
                          loc: t.target.location.origin, source: t.source === window }; \
         }); \
         window.postMessage({ message: 'x' }, '*');",
    )
    .unwrap();
    assert!(is_true(&rt, "_lumen_tick_timers(); globalThis.__r !== null"));
    assert!(is_true(
        &rt,
        "globalThis.__r.target && globalThis.__r.current && globalThis.__r.self && globalThis.__r.source"
    ));
    assert!(is_true(&rt, "globalThis.__r.phase === 2"));
    assert!(is_true(&rt, "globalThis.__r.loc === 'https://example.test'"));
}

/// `onmessage` sees the same targets, and runs before the listeners.
#[test]
fn onmessage_handler_gets_the_window_as_target() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "globalThis.__order = []; \
         window.addEventListener('message', function(e) { globalThis.__order.push('listener'); }); \
         window.onmessage = function(e) { globalThis.__order.push(e.target === window ? 'on' : 'on-bad'); }; \
         window.postMessage('x', '*');",
    )
    .unwrap();
    assert!(is_true(
        &rt,
        "_lumen_tick_timers(); globalThis.__order.join() === 'on,listener'"
    ));
}

/// After the dispatch the event is back to the not-in-flight state (DOM §2.9).
#[test]
fn dispatch_state_is_cleared_after_delivery() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "globalThis.__ev = null; \
         window.addEventListener('message', function(e) { globalThis.__ev = e; }); \
         window.postMessage('x', '*');",
    )
    .unwrap();
    assert!(is_true(
        &rt,
        "_lumen_tick_timers(); globalThis.__ev.target === window && globalThis.__ev.currentTarget === null \
         && globalThis.__ev.eventPhase === 0"
    ));
}

/// `stopImmediatePropagation` in one listener silences the later ones.
#[test]
fn stop_immediate_propagation_is_honoured() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "globalThis.__n = 0; \
         window.addEventListener('message', function(e) { globalThis.__n++; e.stopImmediatePropagation(); }); \
         window.addEventListener('message', function(e) { globalThis.__n++; }); \
         window.postMessage('x', '*');",
    )
    .unwrap();
    assert!(is_true(&rt, "_lumen_tick_timers(); globalThis.__n === 1"));
}

/// Cross-frame delivery goes through the same dispatch.
#[test]
fn frame_message_delivery_targets_the_window() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "globalThis.__r = null; \
         window.addEventListener('message', function(e) { \
           globalThis.__r = e.target === window && e.currentTarget === window && e.data === 7; }); \
         _lumen_deliver_frame_message(7, 'https://other.test', null);",
    )
    .unwrap();
    assert!(is_true(&rt, "globalThis.__r === true"));
}

/// A page's own `window.dispatchEvent(new Event(...))` also targets the window.
#[test]
fn window_dispatch_event_sets_target() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(
        &rt,
        "var ok = false; \
         window.addEventListener('foo', function(e) { ok = e.target === window && e.currentTarget === window; }); \
         window.dispatchEvent(new Event('foo')); ok"
    ));
}
