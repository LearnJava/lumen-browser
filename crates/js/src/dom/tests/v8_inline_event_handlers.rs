//! BUG-360: `on<type>` event handler content attributes / IDL attributes on
//! live elements. Covers both compilation (parser-shaped attribute via
//! `innerHTML`, `setAttribute`, `removeAttribute`) and the three live
//! dispatch paths (`Element.dispatchEvent`, `_lumen_dispatch_bubble` — the
//! path real mouse/keyboard input takes — and direct assignment
//! `el.onclick = fn`), plus the `<body onload>` → `window.onload` forward.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

#[test]
fn onclick_content_attribute_is_a_compiled_function() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.innerHTML = '<div id=\"d1\" onclick=\"window.hit=1\"></div>';",
    )
    .unwrap();
    let result = rt.eval("typeof document.getElementById('d1').onclick").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("function".into()));
}

#[test]
fn getattribute_still_returns_raw_source_after_compilation() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.innerHTML = '<div id=\"d1\" onclick=\"window.hit=1\"></div>';",
    )
    .unwrap();
    let result = rt
        .eval("document.getElementById('d1').getAttribute('onclick')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("window.hit=1".into()));
}

#[test]
fn dispatch_event_fires_onclick_content_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.innerHTML = '<div id=\"d1\" onclick=\"window.hit=1\"></div>'; \
                 document.getElementById('d1').dispatchEvent(new Event('click'));",
    )
    .unwrap();
    let result = rt.eval("window.hit").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

#[test]
fn bubble_dispatch_fires_onclick_content_attribute() {
    // _lumen_dispatch_bubble is the path the shell drives for real mouse
    // clicks (see _lumen_dispatch_mouse_event) — this is the exact gap
    // BUG-360 reported: dispatchEvent() alone is not enough.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.innerHTML = '<div id=\"d1\" onclick=\"window.hit=1\"></div>'; \
                 _lumen_dispatch_bubble(document.getElementById('d1').__nid__, 'click');",
    )
    .unwrap();
    let result = rt.eval("window.hit").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

#[test]
fn assigned_onclick_property_fires_on_bubble_dispatch() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.getElementById('main').onclick = function() { window.hit = 1; }; \
                 _lumen_dispatch_bubble(document.getElementById('main').__nid__, 'click');",
    )
    .unwrap();
    let result = rt.eval("window.hit").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

#[test]
fn gc_collect_clears_on_handler_alongside_listeners() {
    // _lumen_gc_collect purges _lumen_listeners for a nid; BUG-360 adds
    // _lumen_on_handlers to that same purge so a node's on<type> handler
    // has the same lifetime as its other per-nid JS-side state (the shell
    // only calls this for detached nodes with zero live JS references, so
    // "still gets dispatched to afterwards" is the regression to guard).
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var el = document.getElementById('main'); \
                 el.onclick = function() { window.hit = 1; }; \
                 _lumen_gc_collect([el.__nid__]); \
                 _lumen_dispatch_bubble(el.__nid__, 'click');",
    )
    .unwrap();
    let result = rt.eval("window.hit").unwrap();
    assert!(
        matches!(result, lumen_core::JsValue::Null | lumen_core::JsValue::Undefined),
        "expected window.hit to be unset, got {result:?}"
    );
}

#[test]
fn set_attribute_after_wrapper_built_compiles_handler() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var el = document.getElementById('main'); \
                 el.setAttribute('onclick', 'window.hit = 1'); \
                 _lumen_dispatch_bubble(el.__nid__, 'click');",
    )
    .unwrap();
    let result = rt.eval("window.hit").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

#[test]
fn remove_attribute_clears_handler() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.innerHTML = '<div id=\"d1\" onclick=\"window.hit=1\"></div>'; \
                 document.getElementById('d1').removeAttribute('onclick'); \
                 _lumen_dispatch_bubble(document.getElementById('d1').__nid__, 'click');",
    )
    .unwrap();
    // Spec-conforming: an unset event handler IDL attribute reads back
    // as `null`, not `undefined`.
    let handler = rt.eval("document.getElementById('d1').onclick").unwrap();
    assert_eq!(handler, lumen_core::JsValue::Null);
    let result = rt.eval("window.hit").unwrap();
    assert!(
        matches!(result, lumen_core::JsValue::Null | lumen_core::JsValue::Undefined),
        "expected window.hit to be unset, got {result:?}"
    );
}

#[test]
fn unparsable_handler_body_does_not_throw_and_leaves_no_handler() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(
        "document.body.innerHTML = '<div id=\"d1\" onclick=\"(((\"></div>'; \
                 typeof document.getElementById('d1').onclick",
    );
    assert_eq!(result.unwrap(), lumen_core::JsValue::String("object".into()));
}

#[test]
fn body_onload_attribute_forwards_to_window_onload() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.body.setAttribute('onload', 'window.hit = 1');")
        .unwrap();
    let is_fn = rt.eval("typeof window.onload").unwrap();
    assert_eq!(is_fn, lumen_core::JsValue::String("function".into()));
    let same = rt.eval("document.body.onload === window.onload").unwrap();
    assert_eq!(same, lumen_core::JsValue::Bool(true));
}

#[test]
fn body_onload_fires_at_document_complete_not_via_bubble() {
    // Confirms the forward doesn't get double-invoked: `load` never
    // dispatches through node bubbling, only through the window-level
    // listener loop in `_lumen_apply_ready_state('complete')`.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.setAttribute('onload', 'window.hitCount = (window.hitCount||0)+1'); \
                 _lumen_apply_ready_state('interactive'); \
                 _lumen_apply_ready_state('complete');",
    )
    .unwrap();
    let result = rt.eval("window.hitCount").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}
