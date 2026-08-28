//! BUG-732 — the DOM half of the same report: `Node.contains()`,
//! `Node.compareDocumentPosition()`, `Element.attributes` and
//! `document.images` were all absent from the shim although every native
//! they need (`_lumen_get_parent`, `_lumen_get_children`,
//! `_lumen_get_attr_names`, `_lumen_query_selector_all`) already existed.
//! Each one used to surface as a `TypeError` thrown from the middle of
//! third-party code, taking the rest of that script down with it.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn num(rt: &V8JsRuntime, code: &str) -> f64 {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::Number(n) => n,
        other => panic!("{code}: expected a number, got {other:?}"),
    }
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn contains_self_descendant_and_foreign_node() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "document.getElementById('main').contains(document.getElementById('main'))"));
    assert!(is_true(&rt, "document.getElementById('main').contains(document.querySelector('.highlight'))"));
    assert!(is_true(&rt, "!document.querySelector('.highlight').contains(document.getElementById('main'))"));
    assert!(is_true(&rt, "!document.getElementById('main').contains(null)"));
    // A freshly created, still-detached element is inside nothing.
    assert!(is_true(&rt, "!document.getElementById('main').contains(document.createElement('div'))"));
}

/// `document` is an object literal, not a `Node.prototype` instance and
/// not the parent its own `documentElement` reports — the case an
/// identity-based parent walk would get wrong.
#[test]
fn document_contains_element() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "document.contains(document.getElementById('main'))"));
    assert!(is_true(&rt, "!document.contains(document.createElement('div'))"));
}

#[test]
fn compare_document_position_bits() {
    let rt = v8_runtime_with_dom(make_doc());
    let setup = "var _main = document.getElementById('main'); \
                         var _span = document.querySelector('.highlight');";
    rt.eval(setup).unwrap();
    assert_eq!(num(&rt, "_main.compareDocumentPosition(_main)"), 0.0);
    // CONTAINED_BY | FOLLOWING, and its mirror CONTAINS | PRECEDING.
    assert_eq!(num(&rt, "_main.compareDocumentPosition(_span)"), 20.0);
    assert_eq!(num(&rt, "_span.compareDocumentPosition(_main)"), 10.0);
    // A detached node is DISCONNECTED | IMPLEMENTATION_SPECIFIC plus a
    // direction bit, and the two directions must be mirrors.
    rt.eval("var _loose = document.createElement('div');").unwrap();
    let there = num(&rt, "_main.compareDocumentPosition(_loose)");
    let back = num(&rt, "_loose.compareDocumentPosition(_main)");
    assert_eq!(there as u32 & 33, 33, "expected DISCONNECTED|IMPLEMENTATION_SPECIFIC");
    assert_eq!((there as u32 & 6) ^ (back as u32 & 6), 6, "direction bits must mirror");
}

#[test]
fn document_position_constants_are_exposed() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(num(&rt, "Node.DOCUMENT_POSITION_DISCONNECTED"), 1.0);
    assert_eq!(num(&rt, "Node.DOCUMENT_POSITION_PRECEDING"), 2.0);
    assert_eq!(num(&rt, "Node.DOCUMENT_POSITION_FOLLOWING"), 4.0);
    assert_eq!(num(&rt, "Node.DOCUMENT_POSITION_CONTAINS"), 8.0);
    assert_eq!(num(&rt, "Node.DOCUMENT_POSITION_CONTAINED_BY"), 16.0);
    assert_eq!(num(&rt, "Node.DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC"), 32.0);
    // The bitmask is only usable together with the names.
    assert!(is_true(
        &rt,
        "!!(document.getElementById('main').compareDocumentPosition(document.querySelector('.highlight')) \
                    & Node.DOCUMENT_POSITION_CONTAINED_BY)"
    ));
}

#[test]
fn attributes_expose_name_value_and_stay_live() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _main = document.getElementById('main');").unwrap();
    assert!(is_true(&rt, "_main.attributes instanceof NamedNodeMap"));
    assert!(is_true(&rt, "_main.attributes[0] instanceof Attr"));
    assert_eq!(num(&rt, "_main.attributes.length"), 1.0);
    assert!(is_true(&rt, "_main.attributes[0].name === 'id' && _main.attributes[0].value === 'main'"));
    assert!(is_true(&rt, "_main.attributes.item(0).nodeType === 2"));
    assert!(is_true(&rt, "_main.attributes.getNamedItem('id').ownerElement === _main"));
    assert!(is_true(&rt, "_main.attributes.getNamedItem('missing') === null"));
    // The map itself is one object per element and tracks later writes.
    assert!(is_true(&rt, "_main.attributes === _main.attributes"));
    rt.eval("_main.setAttribute('data-x', '1');").unwrap();
    assert_eq!(num(&rt, "_main.attributes.length"), 2.0);
    assert!(is_true(&rt, "_main.attributes['data-x'].value === '1'"));
    rt.eval("_main.attributes.removeNamedItem('data-x');").unwrap();
    assert!(is_true(&rt, "_main.getAttribute('data-x') === null"));
    // Writing through an Attr writes through to the element.
    rt.eval("_main.attributes.getNamedItem('id').value = 'renamed';").unwrap();
    assert!(is_true(&rt, "_main.getAttribute('id') === 'renamed'"));
}

#[test]
fn attributes_are_iterable_and_enumerable() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _main = document.getElementById('main'); _main.setAttribute('data-x', '1');")
        .unwrap();
    let names = rt
        .eval("Array.from(_main.attributes).map(function(a) { return a.name; }).join(',')")
        .unwrap();
    assert_eq!(names, lumen_core::JsValue::String("id,data-x".to_string()));
}

#[test]
fn document_images_is_a_live_collection() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(num(&rt, "document.images.length"), 0.0);
    rt.eval(
        "var _img = document.createElement('img'); _img.setAttribute('id', 'hero'); \
                 document.getElementById('main').appendChild(_img);",
    )
    .unwrap();
    assert_eq!(num(&rt, "document.images.length"), 1.0);
    assert!(is_true(&rt, "document.images[0] === _img"));
    assert!(is_true(&rt, "document.images.namedItem('hero') === _img"));
    assert!(is_true(&rt, "document.images instanceof HTMLCollection"));
}
