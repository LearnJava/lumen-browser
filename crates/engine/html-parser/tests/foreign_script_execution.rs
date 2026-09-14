//! GAP-XMLDOC срез 13 (BUG-685) — HTML LS §13.2.6.5 execution-eligibility
//! for a foreign (SVG) `<script>`.
//!
//! A real engine sets a script's "already started" flag from exactly two
//! places: a literal `</script>` end tag while the script is still the
//! current node, or a self-closing start tag. Any other way the parser
//! closes it — an ancestor's end tag implicitly popping it, a foreign
//! content breakout tag — leaves the flag unset, so the script must not
//! run, even though its already-parsed text stays in the tree as ordinary
//! content. Lumen defers execution to a post-parse walk
//! (`collect_scripts_ordered` in `lumen-shell`) rather than modelling that
//! flag during parsing, so the parser records the exception on `Document`
//! (`is_script_executable`) for that walk to consult instead. Measured on
//! the vendored `tests/wpt/html/syntax/parsing/unclosed-svg-script.html`.

#![allow(clippy::panic)] // test helper, not a `#[test]` fn — clippy's
// `allow-panic-in-tests` heuristic does not reach it (same as
// `fragment_parsing.rs`'s file-level `#![allow(clippy::unwrap_used)]`).

use lumen_dom::{Document, NodeData, NodeId};
use lumen_html_parser::parse;

/// Depth-first search under `id` for the first element whose local name is
/// `local`, ASCII-case-insensitively.
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

fn script_of(doc: &Document) -> NodeId {
    find_node(doc, doc.root(), "script").unwrap_or_else(|| panic!("script element: {doc}"))
}

#[test]
fn svg_script_closed_by_its_own_end_tag_stays_executable() {
    let doc = parse("<svg><script>a=1;</script></svg>");
    assert!(
        doc.is_script_executable(script_of(&doc)),
        "a script closed by a literal </script> while it is the current node must run: {doc}"
    );
}

#[test]
fn svg_script_without_end_tag_is_not_executable() {
    // Closed only because the enclosing </svg> end tag's generic search
    // (§13.2.6.5 "any other end tag") happens to pop it too — never the
    // current-node-matches-"script" special case.
    let doc = parse("<svg><script>a=1;\n</svg>");
    assert!(
        !doc.is_script_executable(script_of(&doc)),
        "a script closed only as a side effect of an ancestor's end tag must not run: {doc}"
    );
}

#[test]
fn svg_script_ended_by_html_breakout_is_not_executable() {
    // `<s>` is on the ordinary §13.2.6.5 breakout list — it pops every
    // foreign element off the stack, including the still-open script.
    let doc = parse("<svg><script>a=1;<s></script></svg>");
    assert!(
        !doc.is_script_executable(script_of(&doc)),
        "a script closed by a foreign-content breakout tag must not run: {doc}"
    );
}

#[test]
fn svg_self_closing_script_stays_executable() {
    let doc = parse(r#"<svg><script href="a.js"/></svg>"#);
    let script = script_of(&doc);
    assert!(
        doc.is_script_executable(script),
        "a self-closing foreign <script> start tag must run (HTML LS §13.2.6.5 step 7): {doc}"
    );
    assert_eq!(doc.get(script).get_attr("href"), Some("a.js"));
}

#[test]
fn svg_script_with_bogus_end_tag_inside_stays_executable() {
    // Regression for срез 12: an end tag that matches nothing on the open
    // stack before an HTML-namespace boundary is silently ignored, not
    // treated as an implicit close.
    let doc = parse("<svg><script>a=1;</g></script></svg>");
    assert!(
        doc.is_script_executable(script_of(&doc)),
        "an ignored bogus end tag inside must not affect the real </script> close: {doc}"
    );
}
