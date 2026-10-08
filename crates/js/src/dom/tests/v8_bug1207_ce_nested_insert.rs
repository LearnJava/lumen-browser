//! BUG-1207 — HTML LS §4.2.3 "insert" upgrades and connects every descendant
//! of the inserted node, not only the node itself. ShadyDOM stamps a template
//! into a detached tree and appends it to the host in one go; the nested
//! custom element (`ytd-page-manager` under `ytd-app`) used to stay a plain
//! `HTMLElement`, so its `ready()` — which registers `PAGE_TOKEN` — never ran.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn s(rt: &V8JsRuntime, expr: &str) -> String {
    let wrapped = format!(
        "String((function(){{ try {{ return {expr}; }} \
         catch (e) {{ return 'THROW:' + e.message; }} }})())"
    );
    match rt.eval(&wrapped) {
        Ok(lumen_core::JsValue::String(v)) => v,
        other => format!("{other:?}"),
    }
}

/// A defined element nested two levels down in a detached subtree is
/// connected (and so upgraded/`connectedCallback`ed) when the subtree is
/// appended, inserted before a reference node, or appended as a fragment.
#[test]
fn nested_custom_element_connects_with_inserted_subtree() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var log = []; \
         class XNest extends HTMLElement { \
             connectedCallback() { log.push('c:' + this.id); } \
         } \
         customElements.define('x-nest', XNest); \
         function tree(id) { \
             var d = document.createElement('div'), e = document.createElement('div'), \
                 n = document.createElement('x-nest'); n.id = id; \
             e.appendChild(n); d.appendChild(e); return d; } \
         document.body.appendChild(tree('a')); \
         var ref = document.createElement('p'); document.body.appendChild(ref); \
         document.body.insertBefore(tree('b'), ref); \
         var f = document.createDocumentFragment(); f.appendChild(tree('c')); \
         document.body.appendChild(f);",
    )
    .unwrap();
    assert_eq!(s(&rt, "log.join()"), "c:a,c:b,c:c");
}

/// The nested element is only touched once it is actually connected: a
/// detached parent gets no callback, the later attach delivers it once.
#[test]
fn nested_custom_element_waits_for_connection() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var log = []; \
         class XWait extends HTMLElement { \
             connectedCallback() { log.push('c'); } \
         } \
         customElements.define('x-wait', XWait); \
         var outer = document.createElement('div'), mid = document.createElement('div'); \
         var detached = document.createElement('section'); \
         mid.appendChild(document.createElement('x-wait')); \
         outer.appendChild(mid); detached.appendChild(outer);",
    )
    .unwrap();
    assert_eq!(s(&rt, "log.length"), "0");
    rt.eval("document.body.appendChild(detached);").unwrap();
    assert_eq!(s(&rt, "log.join()"), "c");
}
