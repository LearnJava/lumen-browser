//! BUG-569 — `HTMLImageElement.prototype.decode()` (HTML LS §4.8.4.4) over the
//! real V8 shim. Transcribes the shape of the vendored
//! `html/semantics/embedded-content/the-img-element/image-decode*.html` cases:
//! resolves once the shell's decode pipeline reports success, rejects with an
//! `EncodingError` `DOMException` once it reports failure, and settles
//! synchronously (off `_lumen_img_state`) when the image is already complete.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        Ok(other) => panic!("expected bool from script, got {other:?}: {script}"),
        Err(e) => panic!("eval error: {e} for {script}"),
    }
}

#[test]
fn decode_is_a_function_returning_a_promise() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var img = document.createElement('img');
typeof img.decode === 'function' && img.decode() instanceof Promise
"#
    ));
}

#[test]
fn decode_resolves_once_load_state_is_already_settled_successfully() {
    let rt = make_rt();
    let nid = rt.eval("var img = document.createElement('img'); img.__nid__").unwrap();
    let nid = match nid {
        lumen_core::JsValue::Number(n) => n as i64,
        other => panic!("expected node id, got {other:?}"),
    };
    rt.eval(&format!("_lumen_fire_image_load({nid}, 4, 8);")).unwrap();
    rt.eval(
        r#"
var _resolved = false;
img.decode().then(function() { _resolved = true; });
"#,
    )
    .unwrap();
    // A second eval drains the microtask queue accumulated by the first.
    assert!(bool_eval(&rt, "_resolved"));
    assert!(bool_eval(&rt, "img.complete === true && img.naturalWidth === 4 && img.naturalHeight === 8"));
}

#[test]
fn decode_rejects_with_encoding_error_once_load_state_is_already_broken() {
    let rt = make_rt();
    let nid = rt.eval("var img = document.createElement('img'); img.__nid__").unwrap();
    let nid = match nid {
        lumen_core::JsValue::Number(n) => n as i64,
        other => panic!("expected node id, got {other:?}"),
    };
    rt.eval(&format!("_lumen_fire_image_error({nid});")).unwrap();
    rt.eval(
        r#"
var _rejName = null;
img.decode().catch(function(e) { _rejName = e.name; });
"#,
    )
    .unwrap();
    assert!(bool_eval(&rt, "_rejName === 'EncodingError'"));
}

#[test]
fn decode_waits_for_the_load_event_when_still_in_flight() {
    let rt = make_rt();
    let nid = rt.eval("var img = document.createElement('img'); img.__nid__").unwrap();
    let nid = match nid {
        lumen_core::JsValue::Number(n) => n as i64,
        other => panic!("expected node id, got {other:?}"),
    };
    rt.eval(
        r#"
var _resolved = false;
img.decode().then(function() { _resolved = true; });
"#,
    )
    .unwrap();
    assert!(bool_eval(&rt, "_resolved === false"));
    rt.eval(&format!("_lumen_fire_image_load({nid}, 2, 2);")).unwrap();
    assert!(bool_eval(&rt, "_resolved"));
}

#[test]
fn decode_waits_for_the_error_event_when_still_in_flight() {
    let rt = make_rt();
    let nid = rt.eval("var img = document.createElement('img'); img.__nid__").unwrap();
    let nid = match nid {
        lumen_core::JsValue::Number(n) => n as i64,
        other => panic!("expected node id, got {other:?}"),
    };
    rt.eval(
        r#"
var _rejName = null;
img.decode().catch(function(e) { _rejName = e.name; });
"#,
    )
    .unwrap();
    assert!(bool_eval(&rt, "_rejName === null"));
    rt.eval(&format!("_lumen_fire_image_error({nid});")).unwrap();
    assert!(bool_eval(&rt, "_rejName === 'EncodingError'"));
}
