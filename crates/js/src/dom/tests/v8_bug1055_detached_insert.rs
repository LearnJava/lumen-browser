//! BUG-1055 — `appendChild`/`insertBefore`/`replaceChild`/ChildNode methods
//! silently dropped detached `new Text()`/`new Comment()`/PI nodes. They are
//! now promoted to arena nodes on first insertion, keeping object identity.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn append_child_inserts_detached_pi() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt,
        "var p = document.createElement('div'); \
         var pi = document.createProcessingInstruction('t', 'd'); \
         p.appendChild(pi) === pi && pi.getRootNode() === p && pi.parentNode === p \
           && p.firstChild === pi && pi.target === 't' && pi.data === 'd' && pi.nodeType === 7"));
}

#[test]
fn append_child_inserts_detached_text_and_comment() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt,
        "var p = document.createElement('div'); \
         var t = new Text('ab'); var c = new Comment('cd'); \
         t.data = 'xy'; \
         p.appendChild(t); p.appendChild(c); \
         p.childNodes.length === 2 && p.firstChild === t && p.lastChild === c \
           && p.textContent === 'xy' && t.data === 'xy' && c.data === 'cd' \
           && t.nodeType === 3 && c.nodeType === 8 && t instanceof Text \
           && (t.data = 'q', p.textContent === 'q')"));
}

#[test]
fn insert_before_and_after_promote() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt,
        "var p = document.createElement('div'); var e = document.createElement('b'); \
         p.appendChild(e); \
         var c = new Comment('c'); var t = new Text('t'); \
         p.insertBefore(c, e); e.after(t); \
         p.childNodes.length === 3 && p.childNodes[0] === c && p.childNodes[2] === t"));
}
