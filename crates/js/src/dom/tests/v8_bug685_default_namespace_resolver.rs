//! GAP-XMLDOC срез 39 (BUG-685) — XML Namespaces §6 "default namespace"
//! resolution, reduced from the vendored WPT `html/webappapis/
//! dynamic-markup-insertion/the-innerhtml-property/
//! innerhtml-and-xml-namespaces.svg`. Live end-to-end through
//! `Element.innerHTML=`: the fragment parser's own `Document` is separate
//! from the page's, so this exercises `FragmentContext::default_namespace`
//! (computed in `dom_helpers::parse_html_fragment_with_context` from the
//! REAL context element's ancestors), not just the in-crate tree-builder
//! unit tests in `lumen-html-parser`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn innerhtml_on_a_prefix_forced_element_inherits_the_real_ancestor_svg_default() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.innerHTML = '<svg xmlns=\"http://www.w3.org/2000/svg\" \
         xmlns:h=\"http://www.w3.org/1999/xhtml\"><foreignObject>\
         <h:div id=fx></h:div></foreignObject></svg>';",
    )
    .unwrap();
    // Prerequisite: `<h:div>` itself is XHTML — the prefix breakout (срез 5),
    // unaffected by this срез.
    assert!(is_true(
        &rt,
        "document.getElementById('fx').namespaceURI === 'http://www.w3.org/1999/xhtml'"
    ));
    rt.eval("document.getElementById('fx').innerHTML = '<e></e>';").unwrap();
    // The fix under test: `<e>` is a genuinely unprefixed child of an
    // element the `h:`-prefix forced into XHTML, so it must inherit the
    // real `<svg>` ancestor's default namespace across the innerHTML
    // fragment-parsing boundary, not `<h:div>`'s own (prefix-derived) one.
    assert!(is_true(
        &rt,
        "document.getElementById('fx').firstChild.namespaceURI === 'http://www.w3.org/2000/svg'"
    ));
}

#[test]
fn innerhtml_fragment_content_can_reset_and_redeclare_the_default_namespace() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.innerHTML = '<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=gx></g></svg>';",
    )
    .unwrap();
    rt.eval("document.getElementById('gx').innerHTML = \"<e><f xmlns=''><h></h></f></e>\";")
        .unwrap();
    assert!(is_true(
        &rt,
        "document.getElementById('gx').firstChild.namespaceURI === 'http://www.w3.org/2000/svg'"
    ));
    assert!(is_true(
        &rt,
        "document.getElementById('gx').firstChild.firstChild.namespaceURI === null"
    ));
    assert!(is_true(
        &rt,
        "document.getElementById('gx').firstChild.firstChild.firstChild.namespaceURI === null"
    ));
}
