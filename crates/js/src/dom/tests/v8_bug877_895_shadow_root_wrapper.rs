//! BUG-877 — `host.shadowRoot` built a fresh wrapper object on every read
//! (`_lumen_make_shadow_root` had no cache), so `host.shadowRoot ===
//! host.shadowRoot` and `host.attachShadow(...) === host.shadowRoot` were
//! both `false`. BUG-895 — the same wrapper (and `document`) never carried
//! the `ParentNode` mixin (`append`/`prepend`/`replaceChildren`), because
//! those methods live only on the element wrapper's own object literal and
//! the ad hoc `DocumentFragment` instance, neither of which a `ShadowRoot`
//! or `document` goes through.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

#[test]
fn shadow_root_getter_returns_same_object_on_repeated_reads() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        rt.eval(
            "var host = document.createElement('div'); \
             document.body.appendChild(host); \
             var root = host.attachShadow({ mode: 'open' }); \
             root === host.shadowRoot && host.shadowRoot === host.shadowRoot",
        )
        .unwrap(),
        lumen_core::JsValue::Bool(true),
    );
}

#[test]
fn shadow_root_wrapper_survives_a_weak_map_key() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        rt.eval(
            "var host = document.createElement('div'); \
             document.body.appendChild(host); \
             host.attachShadow({ mode: 'open' }); \
             var wm = new WeakMap(); \
             wm.set(host.shadowRoot, 'tagged'); \
             wm.get(host.shadowRoot) === 'tagged'",
        )
        .unwrap(),
        lumen_core::JsValue::Bool(true),
    );
}

#[test]
fn shadow_root_has_parentnode_append_prepend_replace_children() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        rt.eval(
            "var host = document.createElement('div'); \
             document.body.appendChild(host); \
             var root = host.attachShadow({ mode: 'open' }); \
             root.append(document.createElement('a')); \
             root.prepend(document.createElement('b')); \
             root.replaceChildren(document.createElement('c'), 'text'); \
             root.children.length === 1 && \
             root.children[0].tagName === 'C' && \
             root.textContent === 'text'",
        )
        .unwrap(),
        lumen_core::JsValue::Bool(true),
    );
}

#[test]
fn document_has_parentnode_append_prepend_replace_children() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        rt.eval(
            "typeof document.append === 'function' && \
             typeof document.prepend === 'function' && \
             typeof document.replaceChildren === 'function'",
        )
        .unwrap(),
        lumen_core::JsValue::Bool(true),
    );
}

#[test]
fn document_append_and_prepend_grow_document_child_nodes() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        rt.eval(
            "var before = document.childNodes.length; \
             document.append(document.createComment('tail')); \
             document.prepend(document.createComment('head')); \
             document.childNodes.length === before + 2 && \
             document.firstChild.nodeValue === 'head' && \
             document.lastChild.nodeValue === 'tail'",
        )
        .unwrap(),
        lumen_core::JsValue::Bool(true),
    );
}
