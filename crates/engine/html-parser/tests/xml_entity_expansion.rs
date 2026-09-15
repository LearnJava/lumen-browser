//! GAP-XMLDOC срез 28 (BUG-786) — DOCTYPE internal-subset general entities
//! (`<!ENTITY name "value">`) whose replacement text is markup.
//!
//! Measured on the vendored `tests/wpt/dom/nodes/
//! Element-firstElementChild-entity{.svg,-xhtml.xhtml}`: both declare
//! `&tree;` as an entity whose value is a `<span>`/`<tspan>` element, then
//! reference it in element content and assert `firstElementChild` on the
//! surrounding element resolves to that element — i.e. the reference must
//! expand to real markup, not to a literal text node.

#![allow(clippy::panic)] // test helper, not a `#[test]` fn.

use lumen_dom::{Document, NodeData, NodeId};
use lumen_html_parser::parse_xml_flavoured;

fn find_node(doc: &Document, id: NodeId, local: &str) -> Option<NodeId> {
    if matches!(&doc.get(id).data, NodeData::Element { name, .. } if name.local.eq_ignore_ascii_case(local))
    {
        return Some(id);
    }
    doc.get(id)
        .children
        .iter()
        .find_map(|&c| find_node(doc, c, local))
}

/// First child of `id` that is an `Element` — mirrors DOM `firstElementChild`
/// (skips leading text/comment nodes), which is exactly what the two corpus
/// WPT tests assert on.
fn first_element_child(doc: &Document, id: NodeId) -> Option<NodeId> {
    doc.get(id)
        .children
        .iter()
        .copied()
        .find(|&c| matches!(&doc.get(c).data, NodeData::Element { .. }))
}

#[test]
fn xhtml_general_entity_expands_to_element() {
    let doc = parse_xml_flavoured(
        r#"<!DOCTYPE html [
<!ENTITY tree "<span id='first_element_child' style='font-weight:bold;'>unknown.</span>">
]>
<html xmlns="http://www.w3.org/1999/xhtml">
<body>
<p id="parentEl">The result of this test is &tree;</p>
</body>
</html>"#,
    );
    let p = find_node(&doc, doc.root(), "p").unwrap_or_else(|| panic!("<p>: {doc}"));
    let span = find_node(&doc, p, "span").unwrap_or_else(|| panic!("<span>: {doc}"));
    assert_eq!(
        first_element_child(&doc, p),
        Some(span),
        "&tree; reference must expand to the <span> as firstElementChild, not a text node: {doc}"
    );
    assert_eq!(doc.get(span).get_attr("id"), Some("first_element_child"));
}

#[test]
fn svg_general_entity_expands_to_element() {
    let doc = parse_xml_flavoured(
        r#"<!DOCTYPE svg [
<!ENTITY tree "<tspan id='first_element_child' font-weight='bold'>unknown.</tspan>">
]>
<svg xmlns="http://www.w3.org/2000/svg">
<text id="parentEl">The result of this test is &tree;</text>
</svg>"#,
    );
    let text = find_node(&doc, doc.root(), "text").unwrap_or_else(|| panic!("<text>: {doc}"));
    let tspan = find_node(&doc, text, "tspan").unwrap_or_else(|| panic!("<tspan>: {doc}"));
    assert_eq!(
        first_element_child(&doc, text),
        Some(tspan),
        "&tree; reference must expand to the <tspan> as firstElementChild: {doc}"
    );
    assert_eq!(doc.get(tspan).get_attr("id"), Some("first_element_child"));
}

#[test]
fn undeclared_entity_reference_stays_literal() {
    let doc = parse_xml_flavoured(
        r#"<!DOCTYPE html [
<!ENTITY tree "x">
]>
<html xmlns="http://www.w3.org/1999/xhtml"><body><p id="a">&nope;</p></body></html>"#,
    );
    let p = find_node(&doc, doc.root(), "p").unwrap_or_else(|| panic!("<p>: {doc}"));
    let text = doc.get(p).children.first().copied().unwrap_or_else(|| panic!("text child: {doc}"));
    assert!(
        matches!(&doc.get(text).data, NodeData::Text(t) if t.contains("&nope;")),
        "an undeclared entity name must be left as literal text: {doc}"
    );
}

#[test]
fn plain_html_document_is_unaffected() {
    // Regression: `parse` (non-XML-flavoured) must never consult this
    // machinery at all — a bare `&name;` with no DOCTYPE stays untouched
    // (already covered by `tokenizer::entity_unknown_kept_literal`, pinned
    // here at the `parse_xml_flavoured` boundary specifically).
    let doc = parse_xml_flavoured("<html><body><p>&nope;</p></body></html>");
    let p = find_node(&doc, doc.root(), "p").unwrap_or_else(|| panic!("<p>: {doc}"));
    let text = doc.get(p).children.first().copied().unwrap_or_else(|| panic!("text child: {doc}"));
    assert!(matches!(&doc.get(text).data, NodeData::Text(t) if t.contains("&nope;")));
}
