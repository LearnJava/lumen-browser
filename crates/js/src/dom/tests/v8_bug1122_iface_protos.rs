//! BUG-1122 — the shared node members live on the WebIDL interface prototypes
//! (`Node.prototype`, `Element.prototype`, `CharacterData.prototype`), not on a
//! hidden object between the instance and `HTMLDivElement.prototype`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// Evaluates `expr`; an exception comes back as `THROW:<name>`.
fn s(rt: &V8JsRuntime, expr: &str) -> String {
    let wrapped = format!(
        "String((function(){{ try {{ return {expr}; }} \
         catch (e) {{ return 'THROW:' + e.name; }} }})())"
    );
    match rt.eval(&wrapped) {
        Ok(lumen_core::JsValue::String(v)) => v,
        other => format!("{other:?}"),
    }
}

fn own(rt: &V8JsRuntime, proto: &str, member: &str) -> String {
    s(rt, &format!("Object.prototype.hasOwnProperty.call({proto}.prototype, '{member}')"))
}

/// The ShadyDOM (youtube) feature check and the members the report listed.
#[test]
fn members_are_own_properties_of_interface_prototypes() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(s(&rt, "!(Element.prototype.attachShadow && Node.prototype.getRootNode)"), "false");
    let node = [
        "appendChild", "insertBefore", "removeChild", "replaceChild", "cloneNode", "getRootNode",
        "parentNode", "childNodes", "lastChild", "textContent", "nodeType", "nodeName",
        "isConnected", "ownerDocument", "addEventListener", "firstChild", "nextSibling",
    ];
    for m in node {
        assert_eq!(own(&rt, "Node", m), "true", "Node.prototype.{m}");
    }
    let element = [
        "hasAttribute", "getAttribute", "setAttribute", "querySelector", "querySelectorAll",
        "attachShadow", "closest", "matches", "append", "children", "innerHTML",
        "firstElementChild", "classList", "onclick",
    ];
    for m in element {
        assert_eq!(own(&rt, "Element", m), "true", "Element.prototype.{m}");
    }
    for m in ["remove", "before", "after", "replaceWith", "nextElementSibling", "data", "nodeValue"] {
        assert_eq!(own(&rt, "CharacterData", m), "true", "CharacterData.prototype.{m}");
    }
    // An element's chain is the interface chain itself, with nothing in between.
    assert_eq!(s(&rt, "Object.getPrototypeOf(document.createElement('div')) === HTMLDivElement.prototype"), "true");
    assert_eq!(s(&rt, "Object.getPrototypeOf(document.createTextNode('x')) === Text.prototype"), "true");
    // Element-only members do not leak onto Text (DOM §4.10).
    assert_eq!(s(&rt, "'getAttribute' in document.createTextNode('x')"), "false");
}

/// The members taken off the prototype work when called on a node.
#[test]
fn prototype_members_work_through_call() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var el = document.getElementById('main'); el.setAttribute('x', '1');").unwrap();
    assert_eq!(
        s(&rt, "Object.getOwnPropertyDescriptor(Element.prototype, 'hasAttribute').value.call(el, 'x')"),
        "true"
    );
    assert_eq!(
        s(&rt, "Object.getOwnPropertyDescriptor(Node.prototype, 'parentNode').get.call(el) === document.body"),
        "true"
    );
    assert_eq!(s(&rt, "Object.getOwnPropertyDescriptor(Node.prototype, 'nodeName').get.call(el)"), "DIV");
    // DOMPurify KEEP_CONTENT: clone through the looked-up native, then an expando.
    assert_eq!(
        s(
            &rt,
            "(function(){ var c = Object.getOwnPropertyDescriptor(Node.prototype, 'cloneNode').value.call(el, true); \
             c.__removalCount = 1; return c.nodeName + c.__removalCount; })()"
        ),
        "DIV1"
    );
    // A page patching the interface prototype is seen by every element.
    assert_eq!(
        s(
            &rt,
            "(function(){ var o = Element.prototype.getAttribute; \
             Element.prototype.getAttribute = function(n){ return 'patched:' + o.call(this, n); }; \
             try { return el.getAttribute('x'); } finally { Element.prototype.getAttribute = o; } })()"
        ),
        "patched:1"
    );
}

/// A receiver with no node behind it — the prototype itself — gets the
/// "absent member" answers, never another node's data and never a lazy slot
/// frozen onto `Element.prototype` for every element to inherit.
#[test]
fn members_read_on_the_prototype_itself_are_inert() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(s(&rt, "Element.prototype.tagName"), "undefined");
    assert_eq!(s(&rt, "Element.prototype.classList"), "undefined");
    assert_eq!(s(&rt, "Object.prototype.hasOwnProperty.call(Element.prototype, '__classList__')"), "false");
    assert_eq!(s(&rt, "Node.prototype.firstChild"), "undefined");
    assert_eq!(s(&rt, "Element.prototype.getAttribute('id')"), "THROW:TypeError");
    assert_eq!(s(&rt, "Element.prototype.hasAttribute.name + Element.prototype.hasAttribute.length"), "hasAttribute1");
}

/// A custom element's own methods win over the shared ones — the hidden
/// prototype used to shadow them from below.
#[test]
fn custom_element_class_members_are_not_shadowed() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "class XDlg extends HTMLElement { close() { return 'mine'; } remove() { return 'rm'; } }\
         customElements.define('x-dlg', XDlg);",
    )
    .unwrap();
    assert_eq!(s(&rt, "new XDlg().close()"), "mine");
    assert_eq!(s(&rt, "document.createElement('x-dlg').remove()"), "rm");
    assert_eq!(s(&rt, "new XDlg().hasAttribute('a')"), "false");
}

/// `<select>.remove(index)`, the document-side constraint validation and a
/// ShadowRoot's `nodeType` still answer as before (BUG-383, BUG-441).
#[test]
fn select_remove_validity_and_shadow_root_keep_their_behaviour() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(
            &rt,
            "(function(){ var sel = document.createElement('select'); \
             sel.appendChild(document.createElement('option')); sel.appendChild(document.createElement('option')); \
             sel.remove(0); return sel.options.length; })()"
        ),
        "1"
    );
    assert_eq!(
        s(
            &rt,
            "(function(){ var i = document.createElement('input'); i.setCustomValidity('bad'); \
             return i.validationMessage + '/' + i.validity.customError + '/' + i.checkValidity(); })()"
        ),
        "bad/true/false"
    );
    assert_eq!(s(&rt, "document.createElement('p').attachShadow({mode:'open'}).nodeType"), "11");
    assert_eq!(s(&rt, "document.createElement('p').attachShadow({mode:'open'}).nodeName"), "#document-fragment");
}

/// A JS-only node (a created document, its doctype and nodes) has no tree
/// links, so the `Node.prototype` link getters answer `null` for it, as they
/// did through BUG-1101's raw getters (WPT `Node-properties.html`); and a
/// doctype, which now reaches `appendChild` through `Node.prototype`, still
/// refuses children — `null` with a TypeError first (`Node-appendChild.html`).
#[test]
fn detached_nodes_and_doctype_keep_node_semantics() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(
            &rt,
            "(function(){ var d = document.implementation.createHTMLDocument(''); \
             return [d.nextSibling, d.parentNode, d.previousSibling].map(String).join(','); })()"
        ),
        "null,null,null"
    );
    assert_eq!(
        s(
            &rt,
            "(function(){ var x = document.implementation.createDocument(null, null, null); \
             var pi = x.createProcessingInstruction('a', 'b'); return String(pi.nextSibling); })()"
        ),
        "null"
    );
    assert_eq!(s(&rt, "Element.prototype.nextSibling"), "undefined");
    assert_eq!(s(&rt, "document.body.appendChild(null)"), "THROW:TypeError");

    let mut doc = Document::new();
    let dt = doc.create_doctype("html", "", "");
    let html = doc.create_element(QualName::html("html"));
    doc.append_child(doc.root(), dt);
    doc.append_child(doc.root(), html);
    let rt = v8_runtime_with_dom(Arc::new(Mutex::new(doc)));
    assert_eq!(s(&rt, "document.doctype.appendChild(null)"), "THROW:TypeError");
    assert_eq!(
        s(&rt, "document.doctype.appendChild(document.createTextNode('x'))"),
        "THROW:HierarchyRequestError"
    );
}
