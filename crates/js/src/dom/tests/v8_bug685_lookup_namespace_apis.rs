//! GAP-XMLDOC срез 38 (BUG-685) — `Node.lookupNamespaceURI`/`lookupPrefix`/
//! `isDefaultNamespace` did not exist at all. All three walk the node's own
//! resolved namespace/prefix, then its `xmlns`/`xmlns:*` attributes, then its
//! ancestor elements, per DOM §4.4 "locate a namespace"/"locate a prefix" —
//! measured live on `dom/nodes/Node-lookupPrefix.xhtml`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn lookup_namespace_uri_walks_ancestor_xmlns_attributes() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.innerHTML = '<div id=outer xmlns:x=\"urn:x\"><div id=inner></div></div>';",
    )
    .unwrap();
    assert!(is_true(
        &rt,
        "document.getElementById('inner').lookupNamespaceURI('x') === 'urn:x'"
    ));
    assert!(is_true(
        &rt,
        "document.getElementById('inner').lookupNamespaceURI('missing') === null"
    ));
    assert!(is_true(
        &rt,
        "document.getElementById('inner').lookupNamespaceURI(null) === document.getElementById('inner').namespaceURI"
    ));
}

#[test]
fn lookup_prefix_finds_the_nearest_ancestor_binding() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.innerHTML = '<div id=outer xmlns:x=\"urn:x\"><div id=inner></div></div>';",
    )
    .unwrap();
    assert!(is_true(
        &rt,
        "document.getElementById('inner').lookupPrefix('urn:x') === 'x'"
    ));
    assert!(is_true(
        &rt,
        "document.getElementById('inner').lookupPrefix('urn:missing') === null"
    ));
    assert!(is_true(&rt, "document.body.lookupPrefix(null) === null"));
    assert!(is_true(&rt, "document.body.lookupPrefix('') === null"));
}

#[test]
fn is_default_namespace_matches_the_elements_own_namespace() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(
        &rt,
        "document.body.isDefaultNamespace(document.body.namespaceURI) === true"
    ));
    assert!(is_true(
        &rt,
        "document.body.isDefaultNamespace('urn:not-it') === false"
    ));
}

#[test]
fn document_and_document_fragment_delegate_or_stop() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(
        &rt,
        "document.lookupNamespaceURI(null) === document.documentElement.namespaceURI"
    ));
    rt.eval("var frag = document.createDocumentFragment();").unwrap();
    assert!(is_true(&rt, "frag.lookupNamespaceURI('x') === null"));
    assert!(is_true(&rt, "frag.lookupPrefix('urn:x') === null"));
}
