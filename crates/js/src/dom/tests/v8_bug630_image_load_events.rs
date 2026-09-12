//! BUG-630 (GAP-LOADEV срез 1) — `HTMLImageElement.complete`/`naturalWidth`/
//! `naturalHeight` and the `load`/`error` events the shell's decode pipeline
//! fires via `_lumen_fire_image_load`/`_lumen_fire_image_error`.
//!
//! The shell-side wiring (`page_pipeline.rs`, `page_load.rs`) has no decoded
//! bytes to hand this JS-only test, so these cover exactly what the shim owns:
//! the pre-load defaults, the state write + event dispatch the two natives
//! perform, and that a failed decode still flips `complete`.

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
fn image_state_defaults_before_any_load() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var img = document.createElement('img');").unwrap();
    assert_eq!(rt.eval("'complete' in img").unwrap(), lumen_core::JsValue::Bool(true));
    assert_eq!(rt.eval("'naturalWidth' in img").unwrap(), lumen_core::JsValue::Bool(true));
    assert_eq!(rt.eval("'naturalHeight' in img").unwrap(), lumen_core::JsValue::Bool(true));
    assert_eq!(rt.eval("img.complete").unwrap(), lumen_core::JsValue::Bool(false));
    assert_eq!(rt.eval("img.naturalWidth").unwrap(), lumen_core::JsValue::Number(0.0));
    assert_eq!(rt.eval("img.naturalHeight").unwrap(), lumen_core::JsValue::Number(0.0));
}

#[test]
fn fire_image_load_sets_state_and_dispatches_load() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var img = document.createElement('img'); \
         var loaded = false, errored = false; \
         img.onload = function() { loaded = true; }; \
         img.onerror = function() { errored = true; };",
    )
    .unwrap();
    rt.eval("_lumen_fire_image_load(img.__nid__, 42, 24);").unwrap();
    assert_eq!(rt.eval("loaded").unwrap(), lumen_core::JsValue::Bool(true));
    assert_eq!(rt.eval("errored").unwrap(), lumen_core::JsValue::Bool(false));
    assert_eq!(rt.eval("img.complete").unwrap(), lumen_core::JsValue::Bool(true));
    assert_eq!(rt.eval("img.naturalWidth").unwrap(), lumen_core::JsValue::Number(42.0));
    assert_eq!(rt.eval("img.naturalHeight").unwrap(), lumen_core::JsValue::Number(24.0));
}

#[test]
fn fire_image_error_sets_complete_true_with_zero_size() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var img = document.createElement('img'); \
         var errored = false; \
         img.onerror = function() { errored = true; };",
    )
    .unwrap();
    rt.eval("_lumen_fire_image_error(img.__nid__);").unwrap();
    assert_eq!(rt.eval("errored").unwrap(), lumen_core::JsValue::Bool(true));
    // HTML LS §4.8.4.3: a failed decode still ends the load attempt — `complete`
    // becomes `true`, not stuck at `false` forever.
    assert_eq!(rt.eval("img.complete").unwrap(), lumen_core::JsValue::Bool(true));
    assert_eq!(rt.eval("img.naturalWidth").unwrap(), lumen_core::JsValue::Number(0.0));
}

#[test]
fn image_load_event_does_not_bubble() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var img = document.createElement('img'); \
         document.body.appendChild(img); \
         var doc_fired = false; \
         document.addEventListener('load', function() { doc_fired = true; });",
    )
    .unwrap();
    rt.eval("_lumen_fire_image_load(img.__nid__, 1, 1);").unwrap();
    assert_eq!(
        rt.eval("doc_fired").unwrap(),
        lumen_core::JsValue::Bool(false),
        "img load event must not bubble to document"
    );
}
