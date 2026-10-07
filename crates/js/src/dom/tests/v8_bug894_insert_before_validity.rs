//! BUG-894 — `Element.insertBefore` must throw `NotFoundError` for a reference
//! node that is not a child, and `TypeError` for a non-node reference.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt_with_dom() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn foreign_reference_throws_not_found() {
    let rt = rt_with_dom();
    assert!(is_true(&rt,
        "var p = document.createElement('div'), q = document.createElement('div'); \
         var foreign = q.appendChild(document.createElement('span')); \
         var n = document.createElement('b'); var name = ''; \
         try { p.insertBefore(n, foreign); } catch (e) { name = e.name; } \
         name === 'NotFoundError' && n.parentNode === null"));
}

#[test]
fn non_node_reference_throws_type_error() {
    let rt = rt_with_dom();
    assert!(is_true(&rt,
        "var p = document.createElement('div'); var name = ''; \
         try { p.insertBefore(document.createElement('b'), 'x'); } catch (e) { name = e.name; } \
         name === 'TypeError' && p.childNodes.length === 0"));
}

#[test]
fn child_and_null_reference_still_work() {
    let rt = rt_with_dom();
    assert!(is_true(&rt,
        "var p = document.createElement('div'); \
         var a = p.appendChild(document.createElement('a')); \
         var b = p.insertBefore(document.createElement('b'), a); \
         var c = p.insertBefore(document.createElement('i'), null); \
         p.firstChild === b && p.lastChild === c && p.childNodes.length === 3"));
}
