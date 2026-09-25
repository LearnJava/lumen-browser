//! BUG-637 — the global `Window` interface object must exist with the shape
//! WebIDL §3.7 gives every interface object, so the WPT feature-detection
//! idiom `"X" in Window` evaluates instead of throwing `ReferenceError`
//! (`merchant-validation/constructor.tentative.http.html`).

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn runtime_with(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true")
        .unwrap();
    rt.install_dom(
        doc,
        "https://example.com/page.html",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap();
    rt
}

fn bool_of(rt: &V8JsRuntime, code: &str) -> bool {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::Bool(b) => b,
        other => panic!("{code}: expected a bool, got {other:?}"),
    }
}

/// The exact idiom from the WPT file: the identifier resolves and an absent
/// member yields `false`, a real one `true`.
#[test]
fn in_window_idiom_evaluates() {
    let rt = runtime_with(make_doc());
    assert!(bool_of(&rt, "typeof Window === 'function'"));
    assert!(bool_of(&rt, "!('MerchantValidationEvent' in Window)"));
    assert!(bool_of(&rt, "'prototype' in Window"));
    assert!(bool_of(
        &rt,
        "window instanceof Window && self instanceof Window"
    ));
}

/// WebIDL §3.7: the interface object is a writable, non-enumerable,
/// configurable property of the global; `name`/`length` are set; the
/// `prototype` property is neither writable, enumerable nor configurable;
/// calling or constructing it throws `TypeError`.
#[test]
fn window_interface_object_has_webidl_shape() {
    let rt = runtime_with(make_doc());
    assert!(bool_of(
        &rt,
        "(() => { const d = Object.getOwnPropertyDescriptor(globalThis, 'Window');
                  return d.writable && !d.enumerable && d.configurable; })()"
    ));
    assert!(bool_of(
        &rt,
        "Window.name === 'Window' && Window.length === 0"
    ));
    assert!(bool_of(
        &rt,
        "(() => { const d = Object.getOwnPropertyDescriptor(Window, 'prototype');
                  return !d.writable && !d.enumerable && !d.configurable; })()"
    ));
    assert!(bool_of(&rt, "(() => { try { new Window(); return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(bool_of(&rt, "(() => { try { Window(); return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(bool_of(
        &rt,
        "Object.getPrototypeOf(Window) === EventTarget"
    ));
}

/// `Window` must not show up when enumerating the global object.
#[test]
fn window_is_not_enumerated_on_global() {
    let rt = runtime_with(make_doc());
    assert!(bool_of(&rt, "!Object.keys(globalThis).includes('Window')"));
}
