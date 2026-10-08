//! BUG-1162 — `createElement` в отсоединённом XML-документе: имя без приведения
//! регистра, namespace `null` (DOM §4.5); в HTML-документе — как раньше.

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

#[test]
fn xml_document_keeps_case_and_null_namespace() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { var e = document.implementation.createDocument(null, null, null).createElement('fooBar'); \
                          return e.localName === 'fooBar' && e.namespaceURI === null; })()"));
}

#[test]
fn html_document_still_lowercases_in_xhtml_namespace() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { var e = document.implementation.createHTMLDocument('').createElement('fooBar'); \
                          return e.localName === 'foobar' && e.namespaceURI === 'http://www.w3.org/1999/xhtml'; })()"));
}
