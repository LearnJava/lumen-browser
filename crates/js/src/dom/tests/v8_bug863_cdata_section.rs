//! BUG-863 — `document.createCDATASection` and the `CDATASection` interface
//! (DOM §4.5, §4.12): NotSupportedError on an HTML document, a Text-derived
//! node with nodeType 4 on an XML one, `]]>` rejected, verbatim XML
//! serialization.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// ShadyDOM (youtube) does `Object.create(window.CDATASection.prototype)`.
#[test]
fn interface_is_a_text_subclass_and_not_constructible() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "typeof CDATASection === 'function' && window.CDATASection === CDATASection"));
    assert!(is_true(&rt, "CDATASection.prototype instanceof Text"));
    assert!(is_true(&rt, "typeof Object.create(window.CDATASection.prototype) === 'object'"));
    assert!(is_true(&rt, "(function() { try { new CDATASection(); return false; } catch (e) { return e instanceof TypeError; } })()"));
}

#[test]
fn html_document_throws_not_supported_error() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { try { document.createCDATASection('x'); return false; } \
                          catch (e) { return e.name === 'NotSupportedError'; } })()"));
    assert!(is_true(&rt, "(function() { var d = document.implementation.createHTMLDocument(''); \
                          try { d.createCDATASection('x'); return false; } \
                          catch (e) { return e.name === 'NotSupportedError'; } })()"));
}

/// The `setupRangeTests()` shape from WPT `dom/common.js:59-61`.
#[test]
fn xml_document_builds_a_cdata_node() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { \
        var x = new Document(); \
        var c = x.createCDATASection('1234'); \
        var p = document.createElement('p'); \
        p.appendChild(c); p.appendChild(x.createCDATASection('5678')); \
        return c instanceof CDATASection && c instanceof Text && c instanceof CharacterData \
            && c.nodeType === 4 && c.nodeType === Node.CDATA_SECTION_NODE \
            && c.nodeName === '#cdata-section' && c.data === '1234' && c.length === 4 \
            && p.textContent === '12345678' && p.firstChild.nodeType === 4; })()"));
    assert!(is_true(&rt, "(function() { \
        var x = document.implementation.createDocument(null, 'root', null); \
        var c = x.createCDATASection(null); \
        return c.data === 'null' && c.cloneNode(false).nodeType === 4; })()"));
}

#[test]
fn terminator_in_data_throws_invalid_character_error() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { try { new Document().createCDATASection(' ]]>  '); return false; } \
                          catch (e) { return e.name === 'InvalidCharacterError'; } })()"));
}

#[test]
fn plain_text_nodes_are_unaffected() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { var t = new Document().createTextNode('a'); \
                          return t.nodeType === 3 && t.nodeName === '#text' \
                              && !(t instanceof CDATASection); })()"));
}

#[test]
fn xml_serializer_keeps_the_section_markers() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { var x = new Document(); \
                          var r = x.createElement('r'); r.appendChild(x.createCDATASection('a<b&c')); \
                          return new XMLSerializer().serializeToString(r) === '<r><![CDATA[a<b&c]]></r>'; })()"));
}

/// `DOMParser` XML documents are a separate virtual DOM (`dom_parser.rs`);
/// WPT `MutationObserver-textContent.html` calls the factory on one.
#[test]
fn dom_parser_xml_document_builds_a_cdata_node() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { \
        var xml = new DOMParser().parseFromString('<root></root>', 'text/xml'); \
        var el = xml.createElement('e'); var c = xml.createCDATASection('a<b'); el.appendChild(c); \
        return c.nodeType === 4 && c.nodeName === '#cdata-section' && c.data === 'a<b' \
            && el.textContent === 'a<b' \
            && new XMLSerializer().serializeToString(el) === '<e><![CDATA[a<b]]></e>'; })()"));
    assert!(is_true(&rt, "(function() { \
        var h = new DOMParser().parseFromString('<p></p>', 'text/html'); \
        try { h.createCDATASection('x'); return false; } \
        catch (e) { return e.name === 'NotSupportedError'; } })()"));
}
