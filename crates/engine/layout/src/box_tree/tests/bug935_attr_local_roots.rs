//! BUG-935 срез 68 — a write to `class`/`id`/`style`/`data-*` that no selector reads from an
//! ancestor position restyles the element and its direct children
//! (`RestyleDelta::shallow_roots`), not its whole subtree.
//!
//! Each scenario writes real attributes on a `Document`, derives the `NodeChange`s the way
//! the engine thread's flush does (`AttrFrom` with the value from before the step for
//! `class`/`id`, `Attr` for the rest), runs the incremental cascade and demands the cascade
//! *and* the box tree of a full rebuild. The selectors are the ones a narrowed root could
//! get wrong: an ancestor class in a descendant combinator, a child combinator, `:not()`,
//! `:is()`, an ancestor reached from *inside* the subject compound, an attribute selector,
//! `#id`, a sibling combinator, `:has()`.

use lumen_core::geom::Size;
use lumen_dom::{Document, NodeId};

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

type Mutation = Box<dyn Fn(&mut Document)>;

fn by_id(doc: &Document, id: &str) -> NodeId {
    doc.find_by_id(id).unwrap_or_else(|| panic!("fixture id {id}"))
}

/// Sets (or adds) the attribute `name` on `id`.
fn set(doc: &mut Document, id: NodeId, name: &str, value: &str) {
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        match attrs.iter_mut().find(|a| a.name.local == name) {
            Some(a) => a.value = value.to_string(),
            None => attrs.push(lumen_dom::Attribute { name: lumen_dom::QualName::html(name), value: value.to_string() }),
        }
    }
}

fn collect(b: &crate::box_tree::LayoutBox, out: &mut Vec<String>) {
    let text = match &b.kind {
        crate::box_tree::BoxKind::InlineRun { segments, .. } => {
            segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("|")
        }
        _ => String::new(),
    };
    out.push(format!("{:?} {:.1},{:.1},{:.1},{:.1} {text}", b.node, b.rect.x, b.rect.y, b.rect.width, b.rect.height));
    for c in &b.children {
        collect(c, out);
    }
}

fn attrs_of(doc: &Document) -> Vec<(NodeId, Vec<(String, String)>)> {
    (0..doc.node_count() as u32)
        .map(NodeId::from_raw)
        .filter_map(|id| match &doc.try_get(id)?.data {
            lumen_dom::NodeData::Element { attrs, .. } => {
                Some((id, attrs.iter().map(|a| (a.name.local.to_string(), a.value.clone())).collect()))
            }
            _ => None,
        })
        .collect()
}

struct Step {
    /// Elements the cascade really recomputed.
    recomputed: u32,
    elements: usize,
    deep: usize,
    shallow: usize,
}

/// Runs `steps` against `html`/`css`.
fn drive(html: &str, css: &str, steps: Vec<Mutation>) -> Vec<Step> {
    use crate::counters::{set_incremental_restyle, take_cascade_stats, ContentDirty, RestyleDelta};
    use crate::style::{restyle_node_index, restyle_roots_for_node_changes, NodeChange};

    let mut doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let vp = Size::new(800.0, 600.0);
    let m = Measurer;
    let hp = lumen_core::ext::NullHyphenationProvider;
    assert!(doc.take_content_journal().is_none());
    let (mut prev, mut prev_counters) = super::super::layout_measured_hyp_with_counters(&doc, &sheet, vp, &m, &hp, false);

    let mut out = Vec::new();
    for (step, mutate) in steps.iter().enumerate() {
        let before = attrs_of(&doc);
        mutate(&mut doc);
        let journal = doc.take_content_journal().expect("recording was started");
        assert!(!journal.is_empty(), "step {step}: the mutation must be journaled");
        let after = attrs_of(&doc);

        // What the flush reports: every attribute whose value differs, by name; `class` and
        // `id` with the value from before the step.
        let mut changes: Vec<(NodeId, NodeChange<'_>)> = Vec::new();
        for ((id, old_attrs), (_, new_attrs)) in before.iter().zip(after.iter()) {
            let mut names: Vec<&str> = old_attrs.iter().chain(new_attrs.iter()).map(|(n, _)| n.as_str()).collect();
            names.sort_unstable();
            names.dedup();
            for name in names {
                let old = old_attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
                let new = new_attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
                if old == new {
                    continue;
                }
                changes.push((
                    *id,
                    if name == "class" || name == "id" {
                        NodeChange::AttrFrom { name, old: old.unwrap_or("") }
                    } else {
                        NodeChange::Attr(name)
                    },
                ));
            }
        }
        assert!(!changes.is_empty(), "step {step}: no attribute changed");

        let node_index = restyle_node_index(&doc, &sheet);
        let roots = restyle_roots_for_node_changes(&doc, changes, &node_index);
        let (deep, shallow) = (roots.deep.len(), roots.shallow.len());
        let delta = RestyleDelta {
            prev_styles: prev_counters.styles().clone(),
            dirty_roots: roots.deep,
            shallow_roots: roots.shallow,
            content_dirty: ContentDirty::Nodes(&journal),
        };
        set_incremental_restyle(true);
        super::super::set_incremental_box_build(true);
        let _ = take_cascade_stats();
        let (incr, incr_counters) =
            super::super::layout_mutation_incremental_restyle(&doc, &sheet, vp, &m, &hp, false, prev, delta);
        let stats = take_cascade_stats();
        super::super::set_incremental_box_build(false);
        set_incremental_restyle(false);

        let (full, full_counters) = super::super::layout_measured_hyp_with_counters(&doc, &sheet, vp, &m, &hp, false);
        assert!(
            incr_counters.styles() == full_counters.styles(),
            "step {step}: the narrowed cascade left a node with a different style than a full cascade"
        );
        let (mut a, mut b) = (Vec::new(), Vec::new());
        collect(&incr, &mut a);
        collect(&full, &mut b);
        assert_eq!(a, b, "step {step}: the narrowed incremental tree diverged from a full rebuild");

        out.push(Step { recomputed: stats.recomputed, elements: full_counters.styles().len(), deep, shallow });
        prev = incr;
        prev_counters = incr_counters;
    }
    out
}

/// A wrapper with `paras` paragraphs under an inner div: the subtree a deep root would
/// take whole.
fn wrapped(paras: usize) -> String {
    let ps: String = (0..paras).map(|i| format!("<p>para {i} <b>bold</b> <i>it</i></p>")).collect();
    format!("<div id=\"w\" class=\"wrap lazy\"><div id=\"inner\">{ps}</div></div><div id=\"tail\"><p>tail</p></div>")
}

fn assert_narrow(s: &Step, what: &str) {
    assert!(s.shallow >= 1 && s.deep == 0, "{what}: expected a shallow root, got deep={} shallow={}", s.deep, s.shallow);
    assert!(
        (s.recomputed as usize) * 4 < s.elements,
        "{what}: recascaded {} of {} elements — the root did not narrow",
        s.recomputed,
        s.elements
    );
}

fn assert_deep(s: &Step, what: &str) {
    assert!(s.deep >= 1 && s.shallow == 0, "{what}: expected a deep root, got deep={} shallow={}", s.deep, s.shallow);
}

/// A token no selector reads from an ancestor position: the wrapper's class changes and the
/// 20 paragraphs below it are left alone. A token that a descendant combinator reads takes
/// the whole subtree, and the paragraphs really do change.
#[test]
fn an_unreferenced_class_token_narrows_and_a_referenced_one_does_not() {
    let r = drive(
        &wrapped(20),
        "p { color: blue } .open p { margin: 3px; color: red } .wrap { padding: 1px } .lazy { margin: 2px }",
        vec![
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap lazy loaded");
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap loaded open");
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap loaded");
            }),
        ],
    );
    assert_narrow(&r[0], "`loaded` is in no selector");
    assert_deep(&r[1], "`open` is read by `.open p`");
    assert!(r[1].recomputed as usize * 2 > r[1].elements, "the deep root must recascade its subtree");
    assert_deep(&r[2], "removing `open` is the same reach");
}

/// Every way an ancestor token can reach a descendant must keep the whole-subtree path, and the
/// result must equal a full cascade either way.
#[test]
fn every_ancestor_reach_of_a_class_token_stays_deep() {
    let reaches = [
        ".x p { color: red }",
        ".x > div p { color: red }",
        ":not(.x) p { color: red }",
        ":is(.x, .y) p { color: red }",
        "p:is(.x p) { color: red }",
        "p:not(.x p) { color: red }",
        "div.x p b { color: red }",
        ".x:not(.q) p { color: red }",
        ":where(.x) i { color: red }",
        ".z:has(.x) p { color: red }",
    ];
    for css in reaches {
        let r = drive(
            &wrapped(6),
            &format!("p {{ color: blue }} {css}"),
            vec![
                Box::new(|d| {
                    let w = by_id(d, "w");
                    set(d, w, "class", "wrap lazy x");
                }),
                Box::new(|d| {
                    let w = by_id(d, "w");
                    set(d, w, "class", "wrap lazy");
                }),
            ],
        );
        if !css.contains(":has") {
            assert_deep(&r[0], css);
            assert_deep(&r[1], css);
        }
    }
}

/// Selectors that read the *subject* only are no reach: `p.x` styles the paragraph itself.
#[test]
fn a_token_read_only_by_a_subject_compound_does_not_reach_down() {
    let r = drive(
        &wrapped(20),
        "p { color: blue } .x { color: red; margin: 1px } div.x { padding: 2px } p:not(.x) { margin: 1px }",
        vec![Box::new(|d| {
            let w = by_id(d, "w");
            set(d, w, "class", "wrap lazy x");
        })],
    );
    // `.x { color: red }` changes the wrapper's colour, which its paragraphs inherit, so the
    // style moved and the subtree is recascaded — the equality with a full cascade is the point.
    assert!(r[0].shallow >= 1);
}

/// `id`: `#w p` reads the id from an ancestor position.
#[test]
fn an_id_read_by_a_descendant_combinator_stays_deep() {
    let r = drive(
        &wrapped(20),
        "p { color: blue } #special p { margin: 4px } #w2 { zzz-unknown: 1 }",
        vec![
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "id", "w2");
            }),
            Box::new(|d| {
                let w = by_id(d, "w2");
                set(d, w, "id", "special");
            }),
            Box::new(|d| {
                let w = by_id(d, "special");
                set(d, w, "id", "w");
            }),
        ],
    );
    assert_narrow(&r[0], "`w2` is read only by a subject compound");
    assert_deep(&r[1], "`#special p`");
    assert_deep(&r[2], "leaving `#special`");
}

/// `style`, `data-*`: narrowed unless an attribute selector keys on the name from an ancestor.
#[test]
fn style_and_data_attributes_narrow_unless_an_ancestor_selector_reads_them() {
    let r = drive(
        &wrapped(20),
        "p { color: blue } [data-open] p { margin: 4px } [data-view] { zzz-unknown: 1 }",
        vec![
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "data-view", "grid");
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "data-open", "1");
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "style", "width: 300px");
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "style", "color: green");
            }),
        ],
    );
    assert_narrow(&r[0], "`[data-view]` is a subject compound");
    assert_deep(&r[1], "`[data-open] p`");
    // A width changes the wrapper's own style, so the walk goes below it all the same.
    assert!(r[2].shallow >= 1 && r[2].deep == 0, "`style` is read by no ancestor selector");
    assert!(r[3].shallow >= 1, "a colour is inherited: the style moved, the subtree follows");
}

/// A sibling combinator on the changed element reaches its siblings, so the narrowing steps aside.
#[test]
fn a_sibling_combinator_keeps_the_fanout() {
    let r = drive(
        &wrapped(6),
        ".lazy + #tail { margin: 5px } .lazy ~ div p { color: red } p { color: blue }",
        vec![Box::new(|d| {
            let w = by_id(d, "w");
            set(d, w, "class", "wrap");
        })],
    );
    assert_deep(&r[0], "`.lazy + #tail`");
}

/// A sibling combinator that mentions other tokens is silent about this write: the element is
/// narrowed instead of widened to its parent — and a token it does mention still widens.
#[test]
fn a_sibling_combinator_is_silent_about_unrelated_tokens() {
    let r = drive(
        &wrapped(20),
        ".lazy + #tail { margin: 5px } .lazy ~ div p { color: red } p { color: blue }",
        vec![
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap lazy loaded");
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap loaded");
            }),
        ],
    );
    assert_narrow(&r[0], "`loaded` is in no selector");
    assert_deep(&r[1], "`lazy` is on the left of a sibling combinator");
}

/// Several attributes on one node: one reader among them takes the node deep.
#[test]
fn one_unnarrowable_write_among_several_makes_the_node_deep() {
    let r = drive(
        &wrapped(10),
        "p { color: blue } .open p { margin: 3px }",
        vec![Box::new(|d| {
            let w = by_id(d, "w");
            set(d, w, "data-view", "grid");
            set(d, w, "class", "wrap lazy open");
        })],
    );
    // The attribute write that could be narrowed is listed as a shallow root too; the deep
    // root wins in the walk, which is what the subtree's recascade shows.
    assert!(r[0].deep >= 1, "`open` among other writes: expected a deep root");
    assert!(r[0].recomputed as usize * 2 > r[0].elements, "the deep root must recascade its subtree");
}

/// A node that is its own ancestor-less subject and has children whose style it influences
/// by inheritance: the style change must still reach the whole subtree.
#[test]
fn an_inherited_value_changed_by_a_narrowed_write_reaches_the_subtree() {
    drive(
        &wrapped(8),
        ".lazy { color: red; font-size: 20px } .loaded { color: green; font-size: 12px } p { margin: 1px }",
        vec![
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap loaded");
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap lazy");
            }),
        ],
    );
}
