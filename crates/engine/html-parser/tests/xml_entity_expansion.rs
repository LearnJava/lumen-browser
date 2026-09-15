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

/// GAP-XMLDOC срез 30 (BUG-786): a CDATA section is literal character data
/// (XML §2.7) — the one construct whose whole purpose is that `&` and `<`
/// inside it are *not* markup. Before this срез the entity pre-pass rewrote
/// its contents like any other text, so `<![CDATA[&t;]]>` built the `<i>`
/// element the author had explicitly escaped.
#[test]
fn entity_reference_inside_cdata_section_stays_literal() {
    let doc = parse_xml_flavoured(
        r#"<!DOCTYPE html [
<!ENTITY t "<i>M</i>">
]>
<html xmlns="http://www.w3.org/1999/xhtml"><body><p id="a"><![CDATA[x&t;y]]></p></body></html>"#,
    );
    let p = find_node(&doc, doc.root(), "p").unwrap_or_else(|| panic!("<p>: {doc}"));
    assert_eq!(find_node(&doc, p, "i"), None, "CDATA content must not become markup: {doc}");
    let text = doc.get(p).children.first().copied().unwrap_or_else(|| panic!("text child: {doc}"));
    assert!(
        matches!(&doc.get(text).data, NodeData::Text(t) if t == "x&t;y"),
        "CDATA content must survive verbatim: {doc}"
    );
}

/// Same root as the CDATA case, and the one that can execute code: a comment's
/// content is not parsed for entity references (XML §2.5), but the pre-pass
/// used to expand them — so a replacement text carrying `-->` closed the
/// comment early and the rest of it landed in the document as live markup.
#[test]
fn entity_reference_inside_comment_cannot_inject_markup() {
    let doc = parse_xml_flavoured(
        r#"<!DOCTYPE html [
<!ENTITY t "--><script>BOOM</script><!--">
]>
<html xmlns="http://www.w3.org/1999/xhtml"><body><!-- &t; --><p id="a">z</p></body></html>"#,
    );
    assert_eq!(
        find_node(&doc, doc.root(), "script"),
        None,
        "a comment must not be able to inject a <script>: {doc}"
    );
}

/// A processing instruction's content is likewise not parsed for references
/// (XML §2.6) — and since срез 27 wired `<?xml-stylesheet href=…?>` into the
/// cascade, expanding one there would have fetched a URL the document never
/// literally contained.
#[test]
fn entity_reference_inside_processing_instruction_stays_literal() {
    let doc = parse_xml_flavoured(
        r#"<!DOCTYPE html [
<!ENTITY h "other.css">
]>
<?xml-stylesheet type="text/css" href="&h;"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body><p id="a">z</p></body></html>"#,
    );
    let pi = doc
        .get(doc.root())
        .children
        .iter()
        .copied()
        .find(|&c| matches!(&doc.get(c).data, NodeData::ProcessingInstruction { .. }))
        .unwrap_or_else(|| panic!("PI node: {doc}"));
    let NodeData::ProcessingInstruction { data, .. } = &doc.get(pi).data else {
        panic!("PI node: {doc}")
    };
    assert!(data.contains("&h;"), "PI data must stay literal, got {data:?}: {doc}");
}

/// XML §4.4.2: an entity's replacement text is itself parsed, so a reference
/// inside it resolves too. A single non-recursive pass left `&b;` as text.
#[test]
fn nested_entity_reference_expands_to_markup() {
    let doc = parse_xml_flavoured(
        r#"<!DOCTYPE html [
<!ENTITY c "<span id='deep'>D</span>">
<!ENTITY b "&c;">
<!ENTITY a "&b;">
]>
<html xmlns="http://www.w3.org/1999/xhtml"><body><p id="a">&a;</p></body></html>"#,
    );
    let p = find_node(&doc, doc.root(), "p").unwrap_or_else(|| panic!("<p>: {doc}"));
    let span = find_node(&doc, p, "span").unwrap_or_else(|| panic!("<span>: {doc}"));
    assert_eq!(first_element_child(&doc, p), Some(span), "chain must resolve: {doc}");
    assert_eq!(doc.get(span).get_attr("id"), Some("deep"));
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
