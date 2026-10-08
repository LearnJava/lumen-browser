//! BUG-689 — the `Attr` half of DOM §4.9. `document.createAttribute`/
//! `createAttributeNS` did not exist, `Attr` objects had no identity (every
//! lookup minted a fresh one), and the `Attr` `setAttributeNode` returned for
//! the replaced attribute stayed live, so it reported the value that had just
//! replaced it. WPT `trusted-types` builds most of its attribute cases on
//! these calls.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false, None)
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

#[test]
fn create_attribute_builds_a_detached_attr() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _a = document.createAttribute('Data-X');").unwrap();
    assert_eq!(s(&rt, "_a instanceof Attr"), "true");
    // Lowercased in an HTML document, like createElement.
    assert_eq!(s(&rt, "_a.name + '|' + _a.localName + '|' + _a.prefix + '|' + _a.namespaceURI"),
        "data-x|data-x|null|null");
    assert_eq!(s(&rt, "_a.value === '' && _a.ownerElement === null && _a.nodeType === 2"), "true");
    rt.eval("_a.value = 'v';").unwrap();
    assert_eq!(s(&rt, "_a.value + '|' + _a.nodeValue + '|' + _a.textContent"), "v|v|v");
    assert_eq!(s(&rt, "document.createAttribute('')"), "THROW:InvalidCharacterError");
    assert_eq!(s(&rt, "document.createAttribute('a=b')"), "THROW:InvalidCharacterError");
}

#[test]
fn create_attribute_ns_validates_and_extracts() {
    let rt = v8_runtime_with_dom(make_doc());
    let xlink = "'http://www.w3.org/1999/xlink'";
    assert_eq!(
        s(&rt, &format!("(function(a) {{ return a.name + '|' + a.prefix + '|' + a.localName + '|' + a.namespaceURI; }})\
                          (document.createAttributeNS({xlink}, 'xlink:Href'))")),
        "xlink:Href|xlink|Href|http://www.w3.org/1999/xlink"
    );
    assert_eq!(s(&rt, "document.createAttributeNS('', 'plain').namespaceURI"), "null");
    assert_eq!(s(&rt, "document.createAttributeNS(null, 'p:x')"), "THROW:NamespaceError");
    assert_eq!(s(&rt, &format!("document.createAttributeNS({xlink}, 'xml:lang')")), "THROW:NamespaceError");
    assert_eq!(s(&rt, &format!("document.createAttributeNS({xlink}, 'xmlns')")), "THROW:NamespaceError");
    assert_eq!(s(&rt, "document.createAttributeNS('http://www.w3.org/2000/xmlns/', 'x')"),
        "THROW:NamespaceError");
    assert_eq!(s(&rt, "document.createAttributeNS('http://www.w3.org/2000/xmlns/', 'xmlns:x').prefix"),
        "xmlns");
    assert_eq!(s(&rt, &format!("document.createAttributeNS({xlink}, ':x')")), "THROW:InvalidCharacterError");
}

#[test]
fn set_attribute_node_attaches_and_keeps_identity() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _el = document.getElementById('main'); \
         var _a = document.createAttribute('title'); _a.value = 'one'; \
         var _ret = _el.setAttributeNode(_a);",
    )
    .unwrap();
    assert_eq!(s(&rt, "_ret"), "null");
    assert_eq!(s(&rt, "_el.getAttribute('title')"), "one");
    assert_eq!(s(&rt, "_a.ownerElement === _el"), "true");
    assert_eq!(s(&rt, "_el.getAttributeNode('title') === _a"), "true");
    assert_eq!(s(&rt, "_el.attributes.getNamedItem('title') === _el.attributes.title"), "true");
    // Attached: writes go through to the element and back.
    rt.eval("_a.value = 'two';").unwrap();
    assert_eq!(s(&rt, "_el.getAttribute('title')"), "two");
    rt.eval("_el.setAttribute('title', 'three');").unwrap();
    assert_eq!(s(&rt, "_a.value"), "three");
    // Re-setting the same node is a no-op that returns it.
    assert_eq!(s(&rt, "_el.setAttributeNode(_a) === _a"), "true");
}

#[test]
fn replaced_attr_is_detached_with_its_old_value() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _el = document.getElementById('main'); \
         var _old = _el.getAttributeNode('id'); \
         var _n = document.createAttribute('id'); _n.value = 'fresh'; \
         var _ret = _el.setAttributeNode(_n);",
    )
    .unwrap();
    assert_eq!(s(&rt, "_ret === _old"), "true");
    assert_eq!(s(&rt, "_ret.value + '|' + _ret.ownerElement"), "main|null");
    assert_eq!(s(&rt, "_el.id + '|' + _el.attributes.length"), "fresh|1");
}

#[test]
fn attr_in_use_elsewhere_is_rejected() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _el = document.getElementById('main'); \
         var _other = document.createElement('div');",
    )
    .unwrap();
    assert_eq!(s(&rt, "_other.setAttributeNode(_el.getAttributeNode('id'))"), "THROW:InUseAttributeError");
    assert_eq!(s(&rt, "_other.setAttributeNode({ name: 'x', value: 'y' })"), "THROW:TypeError");
}

#[test]
fn remove_attribute_node_detaches_the_exact_node() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _el = document.getElementById('main'); var _a = _el.getAttributeNode('id');").unwrap();
    // A same-named Attr that is not this element's attribute is not found.
    assert_eq!(s(&rt, "_el.removeAttributeNode(document.createAttribute('id'))"), "THROW:NotFoundError");
    assert_eq!(s(&rt, "_el.removeAttributeNode(_a) === _a"), "true");
    assert_eq!(s(&rt, "_el.hasAttribute('id') + '|' + _a.value + '|' + _a.ownerElement"), "false|main|null");
    // The detached node can be attached again.
    assert_eq!(s(&rt, "_el.setAttributeNode(_a)"), "null");
    assert_eq!(s(&rt, "_el.getAttribute('id')"), "main");
}

#[test]
fn namespaced_attr_node_round_trip() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _el = document.getElementById('main'); \
         var _a = document.createAttributeNS('urn:x', 'p:foo'); _a.value = 'v'; \
         _el.setAttributeNodeNS(_a);",
    )
    .unwrap();
    assert_eq!(s(&rt, "_el.getAttributeNS('urn:x', 'foo')"), "v");
    assert_eq!(s(&rt, "_el.getAttributeNodeNS('urn:x', 'foo') === _a"), "true");
    assert_eq!(s(&rt, "_a.namespaceURI"), "urn:x");
    assert_eq!(s(&rt, "_el.getAttributeNodeNS('urn:other', 'foo')"), "null");
    assert_eq!(s(&rt, "_el.attributes.removeNamedItemNS('urn:x', 'foo') === _a"), "true");
    // The native lookup answers "not found" with `undefined`, which the
    // `*AttributeNS` wrappers used to read as a found name.
    assert_eq!(s(&rt, "_el.hasAttributeNS('urn:x', 'foo')"), "false");
    assert_eq!(s(&rt, "_el.hasAttributeNS(null, 'missing')"), "false");
    assert_eq!(s(&rt, "_el.getAttributeNS('urn:x', 'foo')"), "null");
}

#[test]
fn detached_documents_create_attributes_too() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "document.implementation.createHTMLDocument('t').createAttribute('AB').name"),
        "ab"
    );
    assert_eq!(s(&rt, "new Document().createAttribute('AB').name"), "AB");
    assert_eq!(
        s(&rt, "(function(d) { return d.createAttributeNS(null, 'x').ownerDocument === d; })(new Document())"),
        "true"
    );
}

#[test]
fn remove_attribute_leaves_the_attr_with_its_value() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _el = document.createElement('div'); _el.setAttribute('foo', 'bar'); \
         var _a = _el.getAttributeNode('foo'); _el.setAttribute('foo', 'baz'); \
         _el.removeAttribute('foo'); \
         var _el2 = document.createElement('div'); _el2.setAttributeNode(_a);",
    )
    .unwrap();
    assert_eq!(s(&rt, "_a.value + '|' + (_a.ownerElement === _el2)"), "baz|true");
    assert_eq!(s(&rt, "_el2.attributes[0] === _a && _el2.getAttribute('foo')"), "baz");
}

#[test]
fn no_namespace_attr_keeps_its_colon_in_the_local_name() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _el = document.createElement('div'); _el.setAttribute('pre:fix', 'v');").unwrap();
    assert_eq!(s(&rt, "_el.attributes[0].localName + '|' + _el.attributes[0].prefix"), "pre:fix|null");
    assert_eq!(s(&rt, "document.createAttribute('x:y').localName"), "x:y");
}

#[test]
fn named_attributes_do_not_shadow_the_prototype() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _el = document.createElement('div'); var _m = _el.attributes; \
         _m.setNamedItem(document.createAttribute('item')); \
         _el.setAttributeNS('foo', 'toString', '1'); _el.setAttribute('plain', '2');",
    )
    .unwrap();
    assert_eq!(s(&rt, "_m.item === NamedNodeMap.prototype.item"), "true");
    assert_eq!(s(&rt, "typeof _m.toString"), "function");
    assert_eq!(s(&rt, "_m.plain.value + '|' + _m.length"), "2|3");
    assert_eq!(s(&rt, "Object.getOwnPropertyNames(_m).join(',')"), "0,1,2,plain");
    assert_eq!(s(&rt, "NamedNodeMap.prototype.item.call({}, 0)"), "THROW:TypeError");
}

/// WPT `Node-lookupNamespaceURI.html` reached these only once
/// `createAttribute` existed: an `Attr` defers to its element, and an element
/// answers the two predefined prefixes itself.
#[test]
fn attr_lookup_namespace_uri_goes_through_its_element() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _a = document.createAttribute('foo');").unwrap();
    assert_eq!(s(&rt, "_a.lookupNamespaceURI('xml')"), "null");
    rt.eval("document.body.setAttributeNode(_a);").unwrap();
    assert_eq!(s(&rt, "_a.lookupNamespaceURI('xml')"), "http://www.w3.org/XML/1998/namespace");
    assert_eq!(s(&rt, "_a.lookupNamespaceURI('xmlns')"), "http://www.w3.org/2000/xmlns/");
}
