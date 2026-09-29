//! BUG-1138 — the page `document` is an `HTMLDocument : Document`, and every
//! document interface carries its own `Symbol.toStringTag`, so
//! `Object.prototype.toString.call(document)` names the interface.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// The bug report's repro plus yahoo.co.jp `ual`'s sniff.
#[test]
fn document_is_an_html_document() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "typeof HTMLDocument === 'function'"));
    assert!(is_true(&rt, "Object.prototype.toString.call(document) === '[object HTMLDocument]'"));
    assert!(is_true(&rt, "/^(HTML)?Document$/.test(Object.prototype.toString.call(document).slice(8, -1))"));
    assert!(is_true(&rt, "Object.getPrototypeOf(document) === HTMLDocument.prototype"));
    assert!(is_true(&rt, "Object.getPrototypeOf(HTMLDocument.prototype) === Document.prototype"));
    assert!(is_true(&rt, "document instanceof HTMLDocument && document instanceof Document && document instanceof Node"));
    assert!(is_true(&rt, "document.constructor === HTMLDocument"));
}

#[test]
fn interface_object_is_not_constructible_and_not_enumerable() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { try { new HTMLDocument(); return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(is_true(&rt, "Object.getOwnPropertyDescriptor(globalThis, 'HTMLDocument').enumerable === false"));
}

/// Each interface names itself — an inherited tag would make `HTMLDocument`
/// and `XMLDocument` both claim `Document`.
#[test]
fn document_interfaces_have_their_own_tags() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "Object.prototype.toString.call(new Document()) === '[object Document]'"));
    assert!(is_true(&rt, "Object.prototype.toString.call(document.implementation.createDocument(null, null)) === '[object XMLDocument]'"));
    assert!(is_true(&rt, "Object.prototype.toString.call(HTMLDocument.prototype) === '[object HTMLDocument]'"));
}
