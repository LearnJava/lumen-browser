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
    /// Descendants of a shallow root the cascade is asked to recompute on their own.
    point: usize,
    /// Elements that took a parent's changed custom properties without a cascade (срез 98).
    inherited: u32,
}

impl Step {
    /// Elements whose style came out of the cascade (`recomputed` counts the inherited ones too).
    fn cascaded(&self) -> u32 {
        self.recomputed - self.inherited
    }
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
        let (deep, shallow, point) = (roots.deep.len(), roots.shallow.len(), roots.point.len());
        let delta = RestyleDelta {
            prev_styles: prev_counters.styles().clone(),
            dirty_roots: roots.deep,
            shallow_roots: roots.shallow,
            point_roots: roots.point,
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

        out.push(Step {
            recomputed: stats.recomputed,
            elements: full_counters.styles().len(),
            deep,
            shallow,
            point,
            inherited: stats.inherited,
        });
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

/// BUG-935 срез 73: a selector reads the token from an ancestor position, so the root is a shallow
/// one and the descendants that selector can style are listed as point roots.
fn assert_reaches(s: &Step, what: &str) {
    assert!(
        s.deep == 0 && s.shallow >= 1 && s.point >= 1,
        "{what}: expected a shallow root with point roots, got deep={} shallow={} point={}",
        s.deep,
        s.shallow,
        s.point
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
    assert_reaches(&r[1], "`open` is read by `.open p`");
    assert_reaches(&r[2], "removing `open` is the same reach");
}

/// Every way an ancestor token can reach a descendant must name the elements it can style, and the
/// result must equal a full cascade either way (`drive` checks that).
#[test]
fn every_ancestor_reach_of_a_class_token_names_its_subjects() {
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
            assert_reaches(&r[0], css);
            assert_reaches(&r[1], css);
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
fn an_id_read_by_a_descendant_combinator_names_its_subjects() {
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
    assert_reaches(&r[1], "`#special p`");
    assert_reaches(&r[2], "leaving `#special`");
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
    assert_reaches(&r[1], "`[data-open] p`");
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

/// BUG-935 срез 82: a sibling combinator reaches *following elements* only. An element with
/// none after it (alone among its parent's elements — text around it does not count — or the
/// last of them) has no sibling to fan out to, so a write of a token the combinator names stays
/// below it; one element after it brings the fanout back.
#[test]
fn an_element_with_no_element_after_it_has_no_sibling_to_fan_out_to() {
    let page = |before: &str, after: &str| {
        let ps: String = (0..20).map(|i| format!("<p>para {i} <b>bold</b></p>")).collect();
        format!("<section id=\"s\">text {before}<div id=\"w\" class=\"wrap lazy\"><div id=\"inner\">{ps}</div></div> more{after}</section><div id=\"tail\"><p>tail</p></div>")
    };
    let css = ".lazy + .after { margin: 5px } p { color: blue }";
    let step = || -> Vec<Mutation> {
        vec![Box::new(|d| {
            let w = by_id(d, "w");
            set(d, w, "class", "wrap");
        })]
    };
    let narrow = |html: String, what: &str| {
        let r = drive(&html, css, step());
        assert!(r[0].deep == 0, "{what}: widened, deep={}", r[0].deep);
        assert!(
            (r[0].recomputed as usize) * 4 < r[0].elements,
            "{what}: recascaded {} of {} elements — the element still took its parent's subtree",
            r[0].recomputed,
            r[0].elements
        );
    };
    narrow(page("", ""), "the only element of its parent");
    narrow(page("<div class=\"before\">earlier</div>", ""), "the last element of its parent");
    let beside = drive(&page("", "<div class=\"after\">sibling</div>"), css, step());
    assert_deep(&beside[0], "an element after it is what `.lazy + .after` reaches");
}

/// BUG-935 срез 83: the fanout of a sibling combinator is the node and the elements *after* it,
/// not the parent with the siblings before. 20 heavy blocks sit before the written one and
/// stay untouched; the one after it is restyled with its subtree, and the tree still equals a
/// full rebuild (`drive` checks both).
#[test]
fn a_sibling_combinator_fans_out_to_the_following_elements_only() {
    let block = |id: &str, class: &str| {
        let ps: String = (0..20).map(|i| format!("<p>{id} para {i} <b>bold</b> <i>it</i></p>")).collect();
        format!("<div id=\"{id}\" class=\"{class}\">{ps}</div>")
    };
    let page = format!(
        "<section id=\"s\">{}{}{}{}</section>",
        block("early1", "item"),
        block("early2", "item"),
        block("w", "item lazy"),
        block("late", "item after"),
    );
    let css = ".lazy + .after { margin: 5px } .lazy ~ .after p { color: red } p { color: blue }";
    let r = drive(
        &page,
        css,
        vec![Box::new(|d| {
            let w = by_id(d, "w");
            set(d, w, "class", "item");
        })],
    );
    let r = &r[0];
    assert_eq!(r.deep, 2, "the written node and the one element after it");
    assert!(r.shallow == 0, "shallow={}", r.shallow);
    // `w` and `late` with their 20 paragraphs each, out of four such blocks.
    assert!(
        (r.recomputed as usize) * 10 < r.elements * 7,
        "recascaded {} of {} elements — the siblings before the node were taken too",
        r.recomputed,
        r.elements
    );
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

/// Several attributes on one node: the one a selector reads names its subjects, the other stays local.
#[test]
fn several_writes_on_one_node_each_take_their_own_path() {
    let r = drive(
        &wrapped(10),
        "p { color: blue } .open p { margin: 3px }",
        vec![Box::new(|d| {
            let w = by_id(d, "w");
            set(d, w, "data-view", "grid");
            set(d, w, "class", "wrap lazy open");
        })],
    );
    assert_reaches(&r[0], "`open` among other writes");
}

/// The point roots are exactly the elements a selector can style: with a rule on `.open p` that
/// leaves every style as it was, the paragraphs are recomputed and their `<b>`/`<i>` children,
/// the sibling subtree and the rest of the document are not (a paragraph whose style *did* move
/// takes its inheriting subtree, as for any shallow node).
#[test]
fn only_the_subjects_of_the_reading_selector_are_recomputed() {
    let r = drive(
        &wrapped(20),
        "p { color: blue } .open p { zzz-unknown: 1 }",
        vec![
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap lazy open");
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap lazy");
            }),
        ],
    );
    for (i, step) in r.iter().enumerate() {
        assert_reaches(step, "`.open p`");
        // 20 `p` + the wrapper and its direct child, nothing of `<b>`/`<i>`/`#tail`.
        assert_eq!(step.point, 20, "step {i}: the subjects are the 20 paragraphs");
        assert!(
            (step.recomputed as usize) <= 20 + 3,
            "step {i}: recomputed {} elements, expected the paragraphs and the root's neighbourhood",
            step.recomputed
        );
    }
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

/// A link with 20 paragraphs under it: the subtree a deep root on `<a>` would take whole.
fn linked(paras: usize) -> String {
    let ps: String = (0..paras).map(|i| format!("<p>para {i} <b>bold</b></p>")).collect();
    format!(
        "<div id=\"w\"><a id=\"l\" href=\"/one\" title=\"t\"><div id=\"inner\">{ps}</div></a><img id=\"pic\" src=\"a.png\"></div>\
         <div id=\"tail\"><p>tail</p></div>"
    )
}

/// BUG-935 срез 73: `href`/`src`/`title` writes narrow on the elements that own them — unless
/// the sheet reads a link state or the attribute from an ancestor position.
#[test]
fn resource_attributes_narrow_unless_an_ancestor_selector_reads_them() {
    let step = |id: &'static str, name: &'static str, value: &'static str| -> Mutation {
        Box::new(move |d| {
            let n = by_id(d, id);
            set(d, n, name, value);
        })
    };
    let r = drive(
        &linked(20),
        "p { color: blue } a { color: red } img { width: 10px }",
        vec![step("l", "href", "/two"), step("pic", "src", "b.png"), step("l", "title", "u"), step("w", "href", "/x")],
    );
    assert_narrow(&r[0], "`href` on `<a>`");
    assert_narrow(&r[1], "`src` on `<img>`");
    assert_narrow(&r[2], "`title`");
    assert_deep(&r[3], "`href` on a `<div>` is not a link attribute: stays deep");

    // `a:link` in an ancestor position hands the `href` match to the paragraphs below.
    let r = drive(&linked(20), "a:link p { color: green } p { color: blue }", vec![step("l", "href", "/two")]);
    assert_deep(&r[0], "`a:link p`");
    // As the subject compound it styles `<a>` itself and stays inside the root.
    let r = drive(&linked(20), "a:any-link { color: green } p { color: blue }", vec![step("l", "href", "/two")]);
    assert_narrow(&r[0], "`a:any-link` as a subject");
    // An attribute selector on `href` / `src` in an ancestor position.
    let r = drive(&linked(20), "[href] p { color: green } p { color: blue }", vec![step("l", "href", "/two")]);
    assert_deep(&r[0], "`[href] p`");
    let r = drive(&linked(20), "[src] + p { color: green } p { color: blue }", vec![step("pic", "src", "b.png")]);
    assert_deep(&r[0], "`[src] + p`");
}

/// BUG-935 срез 81: the UI thread reads the tracker before the layout takes the document, so
/// the styles it updates may have been computed after its mark — here with `open`, which a
/// write then took back. The value at the mark alone (`wrap lazy`) has an empty difference
/// with the current one and leaves the `.open p` paragraphs red; every value held since the
/// mark names `open`, and the result is a full rebuild's.
#[test]
fn a_write_toggled_back_after_the_mark_is_covered_by_every_value_since_it() {
    use crate::counters::{set_incremental_restyle, ContentDirty, RestyleDelta};
    use crate::style::{restyle_node_index, restyle_roots_for_node_changes, NodeChange};

    let css = "p { color: blue } .open p { color: red; margin: 3px }";
    let matches_full = |olds: &[&str]| -> bool {
        let mut doc = lumen_html_parser::parse(&wrapped(5));
        let sheet = lumen_css_parser::parse(css);
        let (vp, m, hp) = (Size::new(800.0, 600.0), Measurer, lumen_core::ext::NullHyphenationProvider);
        let w = by_id(&doc, "w");
        // The mark: `wrap lazy`. A write after it, and the basis is computed from that.
        set(&mut doc, w, "class", "wrap lazy open");
        let (prev, prev_counters) = super::super::layout_measured_hyp_with_counters(&doc, &sheet, vp, &m, &hp, false);
        let _ = doc.take_content_journal();
        // Taken back before the next read.
        set(&mut doc, w, "class", "wrap lazy");
        let journal = doc.take_content_journal().expect("recording was started");
        let changes: Vec<(NodeId, NodeChange<'_>)> =
            olds.iter().map(|&old| (w, NodeChange::AttrFrom { name: "class", old })).collect();
        let node_index = restyle_node_index(&doc, &sheet);
        let roots = restyle_roots_for_node_changes(&doc, changes, &node_index);
        let delta = RestyleDelta {
            prev_styles: prev_counters.styles().clone(),
            dirty_roots: roots.deep,
            shallow_roots: roots.shallow,
            point_roots: roots.point,
            content_dirty: ContentDirty::Nodes(&journal),
        };
        set_incremental_restyle(true);
        super::super::set_incremental_box_build(true);
        let (_, incr_counters) =
            super::super::layout_mutation_incremental_restyle(&doc, &sheet, vp, &m, &hp, false, prev, delta);
        super::super::set_incremental_box_build(false);
        set_incremental_restyle(false);
        let (_, full_counters) = super::super::layout_measured_hyp_with_counters(&doc, &sheet, vp, &m, &hp, false);
        incr_counters.styles() == full_counters.styles()
    };
    assert!(!matches_full(&["wrap lazy"]), "the value at the mark alone must leave a stale style, or this test proves nothing");
    assert!(matches_full(&["wrap lazy", "wrap lazy open"]), "every value since the mark must cover the toggled write");
}

// ── срез 92: изменившийся стиль shallow-узла спускается по детям, пока стиль меняется ─────────────

/// The wrapper's own style moves (`width`, `margin`: nothing in the subtree inherits them): its
/// direct child is recomputed, comes out the same, and the 20 paragraphs below are not recomputed.
/// Before срез 92 the changed style forced the whole subtree (`forced_same` ≈ 100 % on ria.ru).
#[test]
fn a_changed_style_that_nothing_inherits_stops_at_the_direct_children() {
    let r = drive(
        &wrapped(20),
        ".wide { width: 300px; margin: 3px } .lazy { padding: 1px } p { margin: 1px }",
        vec![
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap lazy wide");
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap lazy");
            }),
        ],
    );
    for (i, step) in r.iter().enumerate() {
        assert_narrow(step, "a width/margin write");
        assert!((step.recomputed as usize) <= 3, "step {i}: recomputed {} of {}", step.recomputed, step.elements);
    }
}

/// A chain of `height: inherit` (a non-inherited property read from the parent): the wrapper's
/// change has to travel level by level through the inner div, the paragraphs and their `<b>`.
#[test]
fn a_non_inherited_value_read_through_inherit_travels_level_by_level() {
    drive(
        &wrapped(6),
        ".wide { height: 40px } #inner, #inner p, #inner b { height: inherit } .lazy { margin: 1px }",
        vec![
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap lazy wide");
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap lazy");
            }),
        ],
    );
}

/// Inherited and non-inherited changes at once, with a grandchild that re-declares the inherited
/// value (so the change stops there) and one that does not.
#[test]
fn an_inherited_change_stops_where_a_descendant_overrides_it() {
    drive(
        "<div id=\"w\" class=\"wrap\"><div id=\"a\"><p>a <b>x</b></p></div><div id=\"b\" class=\"own\"><p>b <b>y</b></p></div></div>",
        ".wrap { color: red } .hot { color: green; width: 200px } .own { color: blue }",
        vec![
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap hot");
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                set(d, w, "class", "wrap");
            }),
        ],
    );
}

// ── срез 98: custom property на shallow-узле не гонит весь каскад поддерева ───────────────────────

fn toggle_style(value: &'static str) -> Mutation {
    Box::new(move |d| {
        let w = by_id(d, "w");
        set(d, w, "style", value);
    })
}

/// `--sc` written on the wrapper. Only `.dep` reads it directly and `#inner` builds `--w` from it
/// for `.use`; the other paragraphs inherit the new map without a cascade, and the result is the
/// one a full cascade gives (the differential in [`drive`]).
#[test]
fn a_custom_property_write_cascades_only_the_elements_a_rule_reads_it_through() {
    let paras = 20;
    let html = {
        let ps: String = (0..paras)
            .map(|i| {
                let class = match i {
                    3 => " class=\"dep\"",
                    7 => " class=\"use\"",
                    11 => " style=\"margin-left: var(--sc)\"",
                    _ => "",
                };
                format!("<p{class}>para {i} <b>bold</b></p>")
            })
            .collect();
        format!("<div id=\"w\" class=\"wrap\"><div id=\"inner\">{ps}</div></div><div id=\"tail\"><p>tail</p></div>")
    };
    let css = ".dep { padding-left: var(--sc, 1px) } #inner { --w: calc(var(--sc, 0px) * 2) } .use { margin-left: var(--w) } p { margin-top: 1px }";
    let r = drive(
        &html,
        css,
        vec![toggle_style("--sc: 15px"), toggle_style("--sc: 40px"), toggle_style(""), toggle_style("--sc: 7px")],
    );
    for (i, step) in r.iter().enumerate() {
        assert!(step.shallow >= 1 && step.deep == 0, "step {i}: expected a shallow root");
        assert!(step.inherited >= paras as u32, "step {i}: only {} elements inherited without a cascade", step.inherited);
        assert!(step.cascaded() <= 12, "step {i}: {} of {} elements went through the cascade", step.cascaded(), step.elements);
    }
}

/// A custom property no rule reads: the whole subtree inherits, nothing below the wrapper is cascaded.
#[test]
fn an_unread_custom_property_cascades_nothing_below_the_element() {
    let r = drive(
        &wrapped(20),
        "p { margin: 1px } #inner { --own: 3px }",
        vec![toggle_style("--theme: dark"), toggle_style("--theme: light"), toggle_style("--own: 9px")],
    );
    for (i, step) in r.iter().enumerate() {
        assert!(step.inherited >= 20, "step {i}: {} elements inherited", step.inherited);
        assert!(step.cascaded() <= 3, "step {i}: {} elements went through the cascade", step.cascaded());
    }
}

/// A descendant that declares the changed name itself keeps its own value: the declaring rule is
/// reached, so it is cascaded and its descendants see its value, not the wrapper's.
#[test]
fn a_descendant_that_redeclares_the_property_is_cascaded() {
    drive(
        "<div id=\"w\" class=\"wrap\"><div id=\"a\"><p class=\"c\">a <b>x</b></p></div><div id=\"b\" class=\"own\"><p class=\"c\">b <b>y</b></p></div></div>",
        ".own { --sc: 5px } .c { padding-left: var(--sc, 0px) }",
        vec![toggle_style("--sc: 20px"), toggle_style("--sc: 30px")],
    );
}

/// `@property` can type, reset or give an initial value to a custom property in ways a text scan
/// cannot follow: the descendants go through the cascade as before.
#[test]
fn a_sheet_with_property_registrations_keeps_the_cascade() {
    let r = drive(
        &wrapped(10),
        "@property --sc { syntax: '<length>'; inherits: true; initial-value: 0px } p { padding-left: var(--sc) }",
        vec![toggle_style("--sc: 15px"), toggle_style("--sc: 3px")],
    );
    assert!(r.iter().all(|s| s.inherited == 0), "the analysis must stay off with @property");
}
