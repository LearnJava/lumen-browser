//! PERF-INVALIDATION-S1 — the restricted cascade (previous matched rules ∪ invalidated rules)
//! must publish exactly what a full cascade publishes, and must actually match fewer rules.
//!
//! The form is a class write on a container (the ebay shape): the rule set that flips is a
//! handful of rules far down the tree, while the candidate set of every recomputed node is the
//! whole BEM block. A differential against a full pass is the gate that says the invalidation
//! set is a superset; the candidate count is what the slice is for (a mechanism that narrows
//! nothing still publishes the same styles, just slowly — the S8 lesson).

use super::*;

const HTML: &str = r##"<body>
  <div class="wrap" id="root">
    <div class="b0 item" data-x="1">
      <div class="b0__row"><span class="b0__cell"><a class="link" href="#">x</a></span><span class="b0__cell">y</span></div>
      <div class="b0__row"><span class="b0__cell"><a class="link" href="#">z</a></span><span class="b0__cell">w</span></div>
    </div>
    <div class="other"><span class="cell">q</span><span class="cell">r</span></div>
  </div>
</body>"##;

const CSS: &str = r#"
.b0 { color: #111; padding: 1px }
.b0.is-open { margin: 0 }
.b0.is-open .b0__cell { color: red }
.b0__row:hover .b0__cell { color: blue }
.b0__cell + .b0__cell { border: 0 }
[data-x] { outline: 0 }
.other[data-x="1"] { padding: 0 }
#root .link { text-decoration: none }
.other:has(.b0__cell) { background: #eee }
.is-hot + .b0__cell { border-radius: 1px }
.b0.ink { color: rgb(9, 9, 9) }
@layer base { .b0__row { gap: 1px } }
@layer base { .b0.is-open .b0__row { gap: 2px } }
@media (min-width: 100px) { .b0__cell { line-height: 1 } }
@media (min-width: 100px) { .b0.is-open .b0__cell { letter-spacing: 1px } }
"#;

fn node_by_class(doc: &lumen_dom::Document, class: &str) -> lumen_dom::NodeId {
    (0..doc.node_count() as u32)
        .map(lumen_dom::NodeId::from_raw)
        .find(|&n| doc.get(n).get_attr("class").is_some_and(|c| c.split_whitespace().any(|t| t == class)))
        .expect("node with class")
}

fn set_attr(doc: &mut lumen_dom::Document, node: lumen_dom::NodeId, name: &str, value: &str) {
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(node).data {
        match attrs.iter_mut().find(|a| a.name.local == name) {
            Some(attr) => attr.value = value.to_string(),
            None => attrs.push(lumen_dom::Attribute { name: lumen_dom::QualName::html(name), value: value.to_string() }),
        }
    }
}

/// One change cycle: mutate, then either the restricted or the wide incremental cascade from
/// the pre-mutation pass, then compare against a full cascade of the mutated document.
/// Returns (restricted candidate count, recomputed nodes).
fn cycle(
    doc: &lumen_dom::Document,
    sheet: &lumen_css_parser::Stylesheet,
    prev: &crate::counters::CounterMap,
    node: lumen_dom::NodeId,
    change: crate::style::NodeChange<'_>,
    restricted: bool,
) -> (u32, u32) {
    use crate::counters::{
        incremental_precompute_counters, precompute_counters, set_incremental_restyle, take_cascade_stats,
        ContentDirty, RestyleDelta,
    };
    use crate::style::{restyle_node_index, restyle_roots_for_node_changes};

    let vp = Size::new(800.0, 600.0);
    let flat = lumen_dom::build_flat_tree(doc);
    let index = restyle_node_index(doc, sheet);
    let roots = restyle_roots_for_node_changes(doc, [(node, change)], &index);
    let invalidation = restricted
        .then(|| index.invalidated_rules(doc, &[(node, change)], false, false))
        .flatten();
    let mut content = std::collections::HashSet::new();
    content.insert(node);
    let delta = RestyleDelta {
        invalidation,
        prev_styles: prev.styles().clone(),
        dirty_roots: roots.deep,
        shallow_roots: roots.shallow,
        point_roots: roots.point,
        content_dirty: ContentDirty::Nodes(&content),
    };
    let _ = take_cascade_stats();
    set_incremental_restyle(true);
    let incr = incremental_precompute_counters(doc, sheet, vp, &flat, false, delta);
    set_incremental_restyle(false);
    let stats = take_cascade_stats();
    let full = precompute_counters(doc, sheet, vp, &flat, false);
    assert_eq!(
        incr.styles(),
        full.styles(),
        "the restricted cascade must reproduce the full cascade exactly"
    );
    (stats.candidates, stats.recomputed)
}

#[test]
fn a_class_write_with_an_invalidation_set_matches_fewer_rules_and_the_same_styles() {
    use crate::counters::precompute_counters;

    let mut doc = lumen_html_parser::parse(HTML);
    let sheet = lumen_css_parser::parse(CSS);
    let vp = Size::new(800.0, 600.0);
    let flat = lumen_dom::build_flat_tree(&doc);
    let block = node_by_class(&doc, "b0");
    let old = doc.get(block).get_attr("class").unwrap_or("").to_string();
    let prev = precompute_counters(&doc, &sheet, vp, &flat, false);
    set_attr(&mut doc, block, "class", "b0 item is-open");
    let change = crate::style::NodeChange::AttrFrom { name: "class", old: &old };

    let (narrow_cands, recomputed) = cycle(&doc, &sheet, &prev, block, change, true);
    // A second pass from the same basis with the wide road, for the candidate ratio.
    let (wide_cands, _) = cycle(&doc, &sheet, &prev, block, change, false);
    assert!(
        wide_cands >= 3 * narrow_cands,
        "the invalidation set must cut candidates at least 3x: wide {wide_cands}, restricted {narrow_cands}"
    );
    assert!(recomputed > 0, "the cycle must recascade something");
}

#[test]
fn an_attribute_write_and_a_style_write_stay_exact() {
    use crate::counters::precompute_counters;

    let mut doc = lumen_html_parser::parse(HTML);
    let sheet = lumen_css_parser::parse(CSS);
    let vp = Size::new(800.0, 600.0);
    let flat = lumen_dom::build_flat_tree(&doc);
    let other = node_by_class(&doc, "other");
    let prev = precompute_counters(&doc, &sheet, vp, &flat, false);
    set_attr(&mut doc, other, "data-x", "1");
    cycle(&doc, &sheet, &prev, other, crate::style::NodeChange::Attr("data-x"), true);

    // The inline `style` write: no sheet rule reads the attribute, so the invalidated set is
    // empty and the node is recascaded from its previous matched rules alone — plus the
    // inline declarations, which `compute_style` applies unconditionally.
    let mut doc = lumen_html_parser::parse(HTML);
    let flat = lumen_dom::build_flat_tree(&doc);
    let other = node_by_class(&doc, "other");
    let prev = precompute_counters(&doc, &sheet, vp, &flat, false);
    set_attr(&mut doc, other, "style", "color: rgb(1, 2, 3)");
    cycle(&doc, &sheet, &prev, other, crate::style::NodeChange::Attr("style"), true);
}

#[test]
fn an_inherited_change_under_a_shallow_root_stays_exact() {
    use crate::counters::precompute_counters;

    let mut doc = lumen_html_parser::parse(HTML);
    let sheet = lumen_css_parser::parse(CSS);
    let vp = Size::new(800.0, 600.0);
    let flat = lumen_dom::build_flat_tree(&doc);
    let block = node_by_class(&doc, "b0");
    let old = doc.get(block).get_attr("class").unwrap_or("").to_string();
    let prev = precompute_counters(&doc, &sheet, vp, &flat, false);
    // `.b0.ink { color }` styles only the block itself: every descendant's style moves through
    // inheritance while none of their own matches flip, so the restricted path must re-apply
    // their previous rule sets under the new inherited style.
    set_attr(&mut doc, block, "class", "b0 item ink");
    cycle(&doc, &sheet, &prev, block, crate::style::NodeChange::AttrFrom { name: "class", old: &old }, true);
}

#[test]
fn a_has_argument_and_a_sibling_combinator_stay_exact() {
    use crate::counters::precompute_counters;

    // `:has(.b0__cell)` flips only when a token *inside* its argument changes: the index must
    // read the nested selector's keys, not just the outer compound's.
    let mut doc = lumen_html_parser::parse(HTML);
    let sheet = lumen_css_parser::parse(CSS);
    let vp = Size::new(800.0, 600.0);
    let flat = lumen_dom::build_flat_tree(&doc);
    let cell = node_by_class(&doc, "cell");
    let old = doc.get(cell).get_attr("class").unwrap_or("").to_string();
    let prev = precompute_counters(&doc, &sheet, vp, &flat, false);
    set_attr(&mut doc, cell, "class", "cell b0__cell");
    cycle(&doc, &sheet, &prev, cell, crate::style::NodeChange::AttrFrom { name: "class", old: &old }, true);

    // A sibling combinator keyed on the written token reaches the *following* element, which is
    // a root of its own — its previous matched set plus the invalidated rule must reproduce it.
    let mut doc = lumen_html_parser::parse(HTML);
    let flat = lumen_dom::build_flat_tree(&doc);
    let cell = node_by_class(&doc, "b0__cell");
    let old = doc.get(cell).get_attr("class").unwrap_or("").to_string();
    let prev = precompute_counters(&doc, &sheet, vp, &flat, false);
    set_attr(&mut doc, cell, "class", "b0__cell is-hot");
    cycle(&doc, &sheet, &prev, cell, crate::style::NodeChange::AttrFrom { name: "class", old: &old }, true);
}

#[test]
fn an_id_change_stays_exact() {
    use crate::counters::precompute_counters;

    let mut doc = lumen_html_parser::parse(HTML);
    let sheet = lumen_css_parser::parse(CSS);
    let vp = Size::new(800.0, 600.0);
    let flat = lumen_dom::build_flat_tree(&doc);
    let root = doc.find_by_id("root").expect("#root");
    let prev = precompute_counters(&doc, &sheet, vp, &flat, false);
    set_attr(&mut doc, root, "id", "root2");
    cycle(&doc, &sheet, &prev, root, crate::style::NodeChange::AttrFrom { name: "id", old: "root" }, true);
}
