//! Link click navigation — DOM ancestor walk to find `<a href>` elements.
//!
//! Browsers navigate on click only when the click target is an anchor element or
//! a descendant of one. This module provides the ancestor walk used by the shell
//! to detect link clicks after a hit test resolves a DOM node.

use lumen_dom::{Document, NodeData, NodeId};

/// Walk up the ancestor chain from `node_id` to find the nearest `<a>` element
/// with a non-empty `href` attribute. Returns the raw href value (not resolved),
/// or `None` if no such ancestor exists.
///
/// This mirrors the HTML5 spec "activation behavior" for the `<a>` element:
/// the click target can be any descendant of the anchor, not the anchor itself.
pub fn find_link_href(doc: &Document, mut node_id: NodeId) -> Option<String> {
    loop {
        let node = doc.get(node_id);
        if let NodeData::Element { name, attrs } = &node.data
            && name.local == "a"
        {
            let href = attrs
                .iter()
                .find(|a| a.name.local == "href")
                .map(|a| a.value.trim());
            if let Some(h) = href
                && !h.is_empty()
            {
                return Some(h.to_owned());
            }
        }
        match node.parent {
            Some(parent) => node_id = parent,
            None => return None,
        }
    }
}

/// Return true if `href` is a URL scheme the browser should navigate to.
/// Suppresses `javascript:` and `mailto:` — no JS navigation handler and no
/// mail client in this shell.
pub fn is_navigable_href(href: &str) -> bool {
    let lower = href.to_ascii_lowercase();
    !lower.starts_with("javascript:") && !lower.starts_with("mailto:")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use lumen_dom::{Attribute, Document, NodeData, QualName};

    use super::*;

    fn push_href(doc: &mut Document, elem: lumen_dom::NodeId, href: &str) {
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(elem).data {
            attrs.push(Attribute {
                name: QualName::html("href"),
                value: href.to_owned(),
            });
        }
    }

    fn make_anchor_with_child(href: &str) -> (Document, lumen_dom::NodeId) {
        let mut doc = Document::new();
        let root = doc.root();
        let anchor = doc.create_element(QualName::html("a"));
        push_href(&mut doc, anchor, href);
        doc.append_child(root, anchor);
        let span = doc.create_element(QualName::html("span"));
        doc.append_child(anchor, span);
        let text = doc.create_text("click me");
        doc.append_child(span, text);
        (doc, text)
    }

    #[test]
    fn finds_href_on_direct_anchor() {
        let mut doc = Document::new();
        let root = doc.root();
        let anchor = doc.create_element(QualName::html("a"));
        push_href(&mut doc, anchor, "https://example.com");
        doc.append_child(root, anchor);
        assert_eq!(
            find_link_href(&doc, anchor),
            Some("https://example.com".into())
        );
    }

    #[test]
    fn finds_href_via_nested_descendant() {
        let (doc, text_node) = make_anchor_with_child("https://example.com/page");
        assert_eq!(
            find_link_href(&doc, text_node),
            Some("https://example.com/page".into())
        );
    }

    #[test]
    fn returns_none_when_no_anchor() {
        let mut doc = Document::new();
        let root = doc.root();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(root, div);
        let text = doc.create_text("no link");
        doc.append_child(div, text);
        assert_eq!(find_link_href(&doc, text), None);
    }

    #[test]
    fn skips_anchor_with_empty_href() {
        let mut doc = Document::new();
        let root = doc.root();
        let anchor = doc.create_element(QualName::html("a"));
        push_href(&mut doc, anchor, "");
        doc.append_child(root, anchor);
        let text = doc.create_text("text");
        doc.append_child(anchor, text);
        assert_eq!(find_link_href(&doc, text), None);
    }

    #[test]
    fn skips_anchor_without_href_attr() {
        let mut doc = Document::new();
        let root = doc.root();
        let anchor = doc.create_element(QualName::html("a"));
        doc.append_child(root, anchor);
        let text = doc.create_text("text");
        doc.append_child(anchor, text);
        assert_eq!(find_link_href(&doc, text), None);
    }

    #[test]
    fn trims_whitespace_from_href() {
        let (doc, text_node) = make_anchor_with_child("  https://example.com  ");
        assert_eq!(
            find_link_href(&doc, text_node),
            Some("https://example.com".into())
        );
    }

    #[test]
    fn is_navigable_blocks_javascript() {
        assert!(!is_navigable_href("javascript:void(0)"));
        assert!(!is_navigable_href("JavaScript:alert(1)"));
    }

    #[test]
    fn is_navigable_blocks_mailto() {
        assert!(!is_navigable_href("mailto:user@example.com"));
    }

    #[test]
    fn is_navigable_allows_http_https_and_relative() {
        assert!(is_navigable_href("https://example.com"));
        assert!(is_navigable_href("http://example.com"));
        assert!(is_navigable_href("/path/to/page.html"));
        assert!(is_navigable_href("../sibling.html"));
        assert!(is_navigable_href("#section"));
    }
}
