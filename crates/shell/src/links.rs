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
/// Fragment-only hrefs (`#id`) return `false` — caller handles them as
/// same-page scroll via [`fragment_only`].
pub fn is_navigable_href(href: &str) -> bool {
    let lower = href.to_ascii_lowercase();
    !lower.starts_with('#')
        && !lower.starts_with("javascript:")
        && !lower.starts_with("mailto:")
}

/// If `href` is a fragment-only reference (starts with `#`), return the
/// fragment text without the leading `#`. Returns `None` for cross-page hrefs.
/// An empty string (`href = "#"`) returns `Some("")` — top-of-page scroll.
pub fn fragment_only(href: &str) -> Option<&str> {
    href.strip_prefix('#')
}

/// Build the absolute URL for a same-document fragment navigation: replaces the
/// fragment of `current` with `frag` (an element id, WITHOUT a leading `#`).
/// An empty `frag` strips the fragment entirely (top-of-page navigation).
/// Any query string in `current` is preserved; only the part after the first
/// `#` changes. Used to keep the JS `location` object and the session-history
/// stack in sync when the user clicks `<a href="#id">`.
pub fn fragment_url(current: &str, frag: &str) -> String {
    let base = match current.split_once('#') {
        Some((before, _)) => before,
        None => current,
    };
    if frag.is_empty() {
        base.to_owned()
    } else {
        format!("{base}#{frag}")
    }
}

/// Determine whether navigating from `current` to `resolved` is a same-document
/// fragment navigation. Returns `Some(fragment)` when both absolute URLs share
/// the same base (everything before the first `'#'`) but differ in their
/// fragment — `fragment` is the part after the first `'#'` in `resolved`,
/// WITHOUT the leading `'#'` (an empty string when `resolved` has no fragment,
/// i.e. a top-of-page navigation). Returns `None` when the base portions differ
/// (a real cross-document navigation) or when the fragments are identical (an
/// identical URL = a reload, not a fragment navigation).
///
/// Mirrors the JS `_lumen_navigate_or_fragment` decision so a full-URL link to
/// the current document (`<a href="/page#x">` clicked from `/page`) is treated
/// as a same-document fragment navigation, not a full reload.
pub fn same_document_fragment(current: &str, resolved: &str) -> Option<String> {
    let (current_base, current_frag) = match current.split_once('#') {
        Some((base, frag)) => (base, Some(frag)),
        None => (current, None),
    };
    let (resolved_base, resolved_frag) = match resolved.split_once('#') {
        Some((base, frag)) => (base, Some(frag)),
        None => (resolved, None),
    };

    if current_base != resolved_base {
        return None;
    }

    if current_frag == resolved_frag {
        return None;
    }

    Some(resolved_frag.unwrap_or("").to_string())
}

/// Walk the document tree and return the first element whose `id` attribute
/// equals `id_value` (case-sensitive per HTML LS §3.2.6). Returns `None` if
/// no such element exists.
pub fn find_element_by_id(doc: &Document, id_value: &str) -> Option<NodeId> {
    find_by_id_recursive(doc, doc.root(), id_value)
}

fn find_by_id_recursive(doc: &Document, node: NodeId, id_value: &str) -> Option<NodeId> {
    let n = doc.get(node);
    if let NodeData::Element { .. } = &n.data
        && n.get_attr("id") == Some(id_value)
    {
        return Some(node);
    }
    for &child in &n.children {
        if let Some(found) = find_by_id_recursive(doc, child, id_value) {
            return Some(found);
        }
    }
    None
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
    fn fragment_url_builds_same_document_urls() {
        assert_eq!(fragment_url("http://x/p?q=1", "sec"), "http://x/p?q=1#sec");
        assert_eq!(fragment_url("http://x/p#old", "new"), "http://x/p#new");
        assert_eq!(fragment_url("http://x/p#old", ""), "http://x/p");
        assert_eq!(fragment_url("http://x/p", ""), "http://x/p");
        // Only the first '#' splits the base from the fragment.
        assert_eq!(fragment_url("http://x/a#b#c", "z"), "http://x/a#z");
    }

    #[test]
    fn same_document_fragment_adds_fragment() {
        assert_eq!(
            same_document_fragment("http://x/p", "http://x/p#sec"),
            Some("sec".to_string())
        );
    }

    #[test]
    fn same_document_fragment_changes_fragment() {
        assert_eq!(
            same_document_fragment("http://x/p#old", "http://x/p#new"),
            Some("new".to_string())
        );
    }

    #[test]
    fn same_document_fragment_removes_fragment() {
        assert_eq!(
            same_document_fragment("http://x/p#old", "http://x/p"),
            Some("".to_string())
        );
    }

    #[test]
    fn same_document_fragment_identical_urls() {
        assert_eq!(same_document_fragment("http://x/p", "http://x/p"), None);
    }

    #[test]
    fn same_document_fragment_identical_with_fragment() {
        assert_eq!(same_document_fragment("http://x/p#a", "http://x/p#a"), None);
    }

    #[test]
    fn same_document_fragment_different_path() {
        assert_eq!(same_document_fragment("http://x/p", "http://x/q#sec"), None);
    }

    #[test]
    fn same_document_fragment_preserves_query_in_base() {
        assert_eq!(
            same_document_fragment("http://x/p?k=1", "http://x/p?k=1#sec"),
            Some("sec".to_string())
        );
    }

    #[test]
    fn same_document_fragment_different_query() {
        assert_eq!(
            same_document_fragment("http://x/p?k=1", "http://x/p?k=2#sec"),
            None
        );
    }

    #[test]
    fn same_document_fragment_only_first_hash_splits() {
        assert_eq!(
            same_document_fragment("http://x/a#b#c", "http://x/a#z"),
            Some("z".to_string())
        );
    }

    #[test]
    fn same_document_fragment_different_host() {
        assert_eq!(same_document_fragment("http://x/p", "http://y/p#sec"), None);
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
    }

    #[test]
    fn is_navigable_blocks_fragment_only() {
        assert!(!is_navigable_href("#section"));
        assert!(!is_navigable_href("#"));
    }

    #[test]
    fn fragment_only_extracts_id() {
        assert_eq!(fragment_only("#section"), Some("section"));
        assert_eq!(fragment_only("#"), Some(""));
        assert_eq!(fragment_only("https://example.com"), None);
        assert_eq!(fragment_only("/path#anchor"), None);
    }

    #[test]
    fn find_element_by_id_finds_element() {
        use lumen_dom::{Attribute, QualName};
        let mut doc = Document::new();
        let root = doc.root();
        let div = doc.create_element(QualName::html("div"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(Attribute { name: QualName::html("id"), value: "section".into() });
        }
        doc.append_child(root, div);
        assert_eq!(find_element_by_id(&doc, "section"), Some(div));
        assert_eq!(find_element_by_id(&doc, "missing"), None);
    }
}
