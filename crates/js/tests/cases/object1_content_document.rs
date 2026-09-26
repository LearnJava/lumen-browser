//! OBJECT-1 срез 3 — `HTMLObjectElement.contentDocument`/`contentWindow` и
//! `getSVGDocument()` у `<object>`/`<embed>` (HTML LS §4.8.6/§4.8.7) поверх
//! настоящего V8-шима: геттеры идут в тот же бридж под-документов, что у
//! `<iframe>`, куда shell регистрирует вложенный документ `<object>` в
//! `spawn_frame`. Форма — WPT `the-object-element/object-attributes.html` и
//! `document-getters-return-null-for-cross-origin.html`.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, None, false)
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

/// Элемент `tag` и его node id — ключ биндинга под-документа.
fn object_nid(rt: &V8JsRuntime, tag: &str) -> u32 {
    let script = format!("var el = document.createElement('{tag}'); el.__nid__");
    match rt.eval(&script).unwrap() {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected node id, got {other:?}"),
    }
}

fn register(rt: &V8JsRuntime, nid: u32, accessible: bool) {
    let child = lumen_html_parser::parse("<html><body><p id='x'>inner</p></body></html>");
    rt.register_frame_document(
        nid,
        Arc::new(Mutex::new(child)),
        "https://example.com/inner.html".to_owned(),
        Some("o".to_owned()),
        accessible,
        None,
    );
}

#[test]
fn object_without_nested_document_gives_null() {
    let rt = make_rt();
    object_nid(&rt, "object");
    assert!(bool_eval(
        &rt,
        "el.contentDocument === null && el.contentWindow === null && el.getSVGDocument() === null"
    ));
}

#[test]
fn object_exposes_registered_nested_document() {
    let rt = make_rt();
    let nid = object_nid(&rt, "object");
    register(&rt, nid, true);
    assert!(bool_eval(
        &rt,
        "var d = el.contentDocument; \
         d !== null && d.getElementById('x').textContent === 'inner'"
    ));
    assert!(bool_eval(&rt, "el.contentWindow !== null && el.contentWindow.name === 'o'"));
    // Документ HTML, не SVG — getSVGDocument() всё равно null.
    assert!(bool_eval(&rt, "el.getSVGDocument() === null"));
}

#[test]
fn cross_origin_object_hides_document_but_not_window() {
    let rt = make_rt();
    let nid = object_nid(&rt, "object");
    register(&rt, nid, false);
    assert!(bool_eval(&rt, "el.contentDocument === null && el.contentWindow !== null"));
}

#[test]
fn getters_live_on_prototype() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "['contentDocument', 'contentWindow', 'getSVGDocument'].every(function(k) { \
             return Object.getOwnPropertyDescriptor(HTMLObjectElement.prototype, k) !== undefined; }) \
         && typeof HTMLEmbedElement.prototype.getSVGDocument === 'function' \
         && !('contentDocument' in HTMLEmbedElement.prototype)"
    ));
}

#[test]
fn embed_has_only_get_svg_document() {
    let rt = make_rt();
    object_nid(&rt, "embed");
    assert!(bool_eval(
        &rt,
        "el.getSVGDocument() === null && el.contentDocument === undefined"
    ));
}
