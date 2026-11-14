//! BUG-1779 — `Object.prototype.toString.call(<HTML element>)` names the
//! interface (`[object HTMLDivElement]`), not `[object Object]`: every DOM
//! wrapper interface prototype owns its `@@toStringTag` (WebIDL §3.7.3, the
//! BUG-912/BUG-1138 shape — an inherited tag would make a subclass claim the
//! base's name). tippy.js sniffs elements through this string and routed
//! non-matching wrappers to `document.querySelectorAll(<element>)`.

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

/// The bug report's repro plus tippy.js's `Element]` sniff.
#[test]
fn html_elements_name_their_interface() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "Object.prototype.toString.call(document.createElement('div')) === '[object HTMLDivElement]'"));
    assert!(is_true(&rt, "Object.prototype.toString.call(document.head) === '[object HTMLHeadElement]'"));
    assert!(is_true(&rt, "/Element]/.test(Object.prototype.toString.call(document.createElement('span')))"));
    assert!(is_true(&rt, "Object.prototype.toString.call(document.createTextNode('')) === '[object Text]'"));
}

/// Each interface instance names itself — the nearest own `@@toStringTag`
/// wins, so the concrete tag shadows the base's one on the shared chain.
#[test]
fn node_family_instances_name_their_own_interface() {
    let rt = v8_runtime_with_dom(make_doc());
    let cases: &[(&str, &str)] = &[
        ("document.createElement('section')", "HTMLElement"),
        ("document.createElement('foobar')", "HTMLUnknownElement"),
        ("document.createElement('canvas')", "HTMLCanvasElement"),
        ("document.createElement('video')", "HTMLVideoElement"),
        ("document.createElement('img')", "HTMLImageElement"),
        ("document.createElement('form')", "HTMLFormElement"),
        ("document.createComment('')", "Comment"),
        ("document.createDocumentFragment()", "DocumentFragment"),
        ("document.createAttribute('x')", "Attr"),
        ("(function () { return document.implementation.createHTMLDocument('t').doctype; })()", "DocumentType"),
        ("document.createElementNS('http://www.w3.org/2000/svg', 'svg')", "SVGSVGElement"),
        ("document.createElementNS('http://www.w3.org/1998/Math/MathML', 'math')", "MathMLElement"),
    ];
    for (expr, iface) in cases {
        assert!(
            is_true(&rt, &format!("Object.prototype.toString.call({expr}) === '[object {iface}]'")),
            "{expr} must stringify as [object {iface}]"
        );
    }
}

/// Interfaces without a concrete HTML subclass name themselves off the shared
/// chain: an XML-document element is an `Element`, character-data siblings
/// keep their own tags, and a custom element (prototype without own tag)
/// inherits `HTMLElement`'s — same as Chrome.
#[test]
fn chain_members_and_custom_elements_name_the_nearest_tag() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(
        &rt,
        "var xd = document.implementation.createDocument(null, null, null); \
         Object.prototype.toString.call(xd.createElement('x')) === '[object Element]' && \
         Object.prototype.toString.call(xd.createProcessingInstruction('p', 't')) === '[object ProcessingInstruction]' && \
         Object.prototype.toString.call(xd.createCDATASection(']]') ) === '[object CDATASection]'"
    ));
    assert!(is_true(
        &rt,
        "customElements.define('x-bug1779', (class extends HTMLElement {})); \
         Object.prototype.toString.call(document.createElement('x-bug1779')) === '[object HTMLElement]'"
    ));
}

/// Every interface prototype owns its tag with its own name (the BUG-912
/// descriptor shape) — including the interfaces minted outside the two
/// generated HTML lists (`HTMLImageElement`, `HTMLMediaElement`, `MathMLElement`).
#[test]
fn interface_prototypes_own_their_tags() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(
        &rt,
        "['Node','Element','CharacterData','Attr','DocumentType','ProcessingInstruction', \
          'HTMLElement','Text','Comment','CDATASection','DocumentFragment', \
          'HTMLDivElement','HTMLInputElement','HTMLImageElement','HTMLAreaElement', \
          'HTMLMediaElement','HTMLVideoElement','MathMLElement'].every(function (C) { \
            var d = Object.getOwnPropertyDescriptor(globalThis[C].prototype, Symbol.toStringTag); \
            return d !== undefined && d.value === C && d.enumerable === false; \
         })"
    ));
}
