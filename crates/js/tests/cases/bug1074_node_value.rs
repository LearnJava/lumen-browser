//! BUG-1074 — DOM §4.4: `Element.nodeValue`/`DocumentFragment.nodeValue` are
//! `null` (with a no-op setter), not `undefined`; CharacterData still carries
//! the data string.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn check(rt: &V8JsRuntime, script: &str) {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(true)) => {}
        other => panic!("expected true, got {other:?}: {script}"),
    }
}

#[test]
fn element_and_fragment_node_value_is_null() {
    let rt = rt();
    check(&rt, "document.createElement('div').nodeValue === null");
    check(&rt, "new DocumentFragment().nodeValue === null");
    check(&rt, "document.createDocumentFragment().nodeValue === null");
}

#[test]
fn node_value_setter_is_a_noop() {
    let rt = rt();
    check(&rt, "var d = document.createElement('div'); d.nodeValue = 'x'; d.nodeValue === null");
    check(&rt, "var f = new DocumentFragment(); f.nodeValue = 'x'; f.nodeValue === null");
}

#[test]
fn character_data_node_value_still_works() {
    let rt = rt();
    check(&rt, "document.createTextNode('a').nodeValue === 'a'");
    check(&rt, "var t = document.createTextNode('a'); t.nodeValue = 'b'; t.nodeValue === 'b'");
}
