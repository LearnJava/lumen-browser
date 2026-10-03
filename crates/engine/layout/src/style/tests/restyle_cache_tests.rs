//! PERF-16 срез 4: [`RestyleIndexCache`] отдаёт те же индексы, что построенные заново, и сканирует
//! таблицу стилей только когда меняется то, что индексы читают.

use super::*;
use lumen_css_parser::parse as parse_css;
use lumen_html_parser::parse as parse_html;

const SHEET: &str = ".item { color: black; } .item:hover { color: red; } .item + .item { color: blue; } \
                     ul:has(.item) { margin: 0; }";

fn fixture() -> Document {
    parse_html(
        r#"<ul id="menu"><li id="a" class="item" data-x="1">a</li><li id="b" class="item">b</li></ul>
           <div id="unrelated"><p>x</p></div>"#,
    )
}

#[test]
fn a_kept_index_answers_like_a_fresh_one_and_the_sheet_is_scanned_once() {
    let doc = fixture();
    let sheet = parse_css(SHEET);
    let (a, b, menu) = (
        doc.find_by_id("a").expect("#a"),
        doc.find_by_id("b").expect("#b"),
        doc.find_by_id("menu").expect("#menu"),
    );
    let mut cache = RestyleIndexCache::default();
    for _ in 0..4 {
        let (state, node) = cache.indexes(&doc, &sheet);
        let fresh_state = restyle_state_index(&doc, &sheet);
        let fresh_node = restyle_node_index(&doc, &sheet);
        for (prev, new) in [(None, Some(a)), (Some(a), Some(b)), (Some(b), None), (Some(a), None)] {
            assert_eq!(
                restyle_root_set_for_state_change(&doc, prev, new, state),
                restyle_root_set_for_state_change(&doc, prev, new, &fresh_state),
                "{prev:?} -> {new:?}",
            );
        }
        for attr in ["data-x", "class", "id"] {
            let change = [(a, NodeChange::Attr(attr))];
            assert_eq!(
                restyle_root_set_for_node_change(&doc, change, node),
                restyle_root_set_for_node_change(&doc, change, &fresh_node),
                "attr {attr}",
            );
        }
        let change = [(menu, NodeChange::ChildList)];
        assert_eq!(
            restyle_roots_for_node_changes(&doc, change, node).shallow,
            restyle_roots_for_node_changes(&doc, change, &fresh_node).shallow,
            "child list",
        );
        assert_eq!(state.needs_fanout(), fresh_state.needs_fanout());
        assert_eq!(state.is_conservative(), fresh_state.is_conservative());
    }
    assert_eq!(cache.builds(), 1, "four passes over one sheet revision scan it once");
}

#[test]
fn a_new_sheet_revision_or_a_shadow_root_rebuilds_the_indexes() {
    let mut doc = fixture();
    let mut sheet = parse_css(".item { color: black; }");
    let mut cache = RestyleIndexCache::default();
    assert!(!cache.indexes(&doc, &sheet).0.needs_fanout());
    assert_eq!(cache.builds(), 1);

    // Another sheet — the rule that reaches a sibling from a state compound changes the answer.
    sheet = parse_css(".item:hover + .item { color: red; }");
    assert!(cache.indexes(&doc, &sheet).0.needs_fanout(), "the new sheet's index is the one answering");
    assert_eq!(cache.builds(), 2);

    // The same sheet mutated in place announces itself with a new revision.
    let before = sheet.revision();
    sheet.mark_mutated();
    assert_ne!(sheet.revision(), before);
    cache.indexes(&doc, &sheet);
    assert_eq!(cache.builds(), 3);

    // A shadow root in the document switches the narrowing off.
    let host = doc.find_by_id("unrelated").expect("#unrelated");
    doc.attach_shadow(host, lumen_dom::ShadowRootMode::Open);
    assert!(cache.indexes(&doc, &sheet).0.is_conservative(), "shadow root => conservative");
    assert_eq!(cache.builds(), 4);
}
