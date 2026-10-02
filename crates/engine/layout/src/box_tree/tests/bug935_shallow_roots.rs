//! BUG-935 срез 60 — a child-list change restyles the container and its direct children
//! (`RestyleDelta::shallow_roots`), not the parent's whole subtree.
//!
//! Every scenario drives real `Document` mutations through the contract the engine
//! thread's same-tick flush uses (`ContentDirty::Nodes(journal)`, roots from
//! `restyle_roots_for_node_changes`), each cycle's output being the next cycle's basis,
//! and demands the cascade *and* the box tree of a full rebuild. The selectors are the
//! ones a shallow restyle could get wrong — a positional compound on the way down
//! (`li:first-child a`), a sibling combinator (`h2 + div p`), `:empty`, an inherited
//! value that changes with a sibling (`li:only-child`), a node moved between parents.

use lumen_core::geom::Size;
use lumen_dom::Document;

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

fn el(doc: &mut Document, tag: &str, text: Option<&str>) -> lumen_dom::NodeId {
    let e = doc.create_element(lumen_dom::QualName::html(tag));
    if let Some(t) = text {
        let t = doc.create_text(t);
        doc.append_child(e, t);
    }
    e
}

fn by_id(doc: &Document, id: &str) -> lumen_dom::NodeId {
    doc.find_by_id(id).unwrap_or_else(|| panic!("fixture id {id}"))
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

/// Runs `steps` against `html`/`css`; returns, per step, `(recomputed, total elements)`.
fn drive(html: &str, css: &str, steps: Vec<Mutation>) -> Vec<(u32, usize)> {
    use crate::counters::{set_incremental_restyle, take_cascade_stats, ContentDirty, RestyleDelta};
    use crate::style::{restyle_node_index, restyle_roots_for_node_changes, NodeChange};

    let mut doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let vp = Size::new(800.0, 600.0);
    let m = Measurer;
    let hp = lumen_core::ext::NullHyphenationProvider;
    assert!(doc.take_content_journal().is_none());
    let (mut prev, mut prev_counters) =
        super::super::layout_measured_hyp_with_counters(&doc, &sheet, vp, &m, &hp, false);

    let mut out = Vec::new();
    for (step, mutate) in steps.iter().enumerate() {
        mutate(&mut doc);
        let journal = doc.take_content_journal().expect("recording was started");
        assert!(!journal.is_empty(), "step {step}: the mutation must be journaled");

        let node_index = restyle_node_index(&doc, &sheet);
        let roots = restyle_roots_for_node_changes(
            &doc,
            journal.iter().copied().map(|n| (n, NodeChange::ChildList)),
            &node_index,
        );
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
            "step {step}: the shallow cascade left a node with a different style than a full cascade"
        );
        let (mut a, mut b) = (Vec::new(), Vec::new());
        collect(&incr, &mut a);
        collect(&full, &mut b);
        assert_eq!(a, b, "step {step}: the shallow incremental tree diverged from a full rebuild");

        out.push((stats.recomputed, full_counters.styles().len()));
        prev = incr;
        prev_counters = incr_counters;
    }
    out
}

fn list_html(n: usize, inner: &str) -> String {
    let items: String = (0..n).map(|i| format!("<li id=\"i{i}\">{inner}</li>")).collect();
    format!("<ul id=\"l\">{items}</ul><div id=\"tail\"><p>tail</p></div>")
}

/// No selector reads structure: appending to a 20-item list recascades the list, its
/// children and the new subtree — not the whole document.
#[test]
fn append_recascades_only_the_container_and_its_children() {
    let html = list_html(20, "<p><b>x</b> <i>y</i></p>");
    let r = drive(
        &html,
        "li { margin: 1px } p { color: blue } b { font-weight: bold }",
        vec![
            Box::new(|d| {
                let l = by_id(d, "l");
                let li = el(d, "li", None);
                let p = el(d, "p", Some("new"));
                d.append_child(li, p);
                d.append_child(l, li);
            }),
            Box::new(|d| {
                let l = by_id(d, "l");
                let first = d.get(l).children[0];
                d.detach(first);
            }),
        ],
    );
    for (step, (recomputed, elements)) in r.iter().enumerate() {
        assert!(
            (*recomputed as usize) * 2 < *elements,
            "step {step}: recascaded {recomputed} of {elements} elements — the shallow root did not narrow"
        );
    }
}

/// Positional compounds on the way down: a child whose position changed takes its
/// subtree with it.
#[test]
fn positional_selectors_below_the_children_follow_a_changed_position() {
    let html = list_html(6, "<a href=\"#\">link</a><p>para <b>b</b></p><p>second <b>c</b></p>");
    drive(
        &html,
        "li:first-child a { color: red } li:last-child { font-weight: bold } li:last-child a { text-decoration: underline } \
         li:not(:first-child) p b { color: green } li p:first-of-type b { font-size: 20px } \
         li:nth-child(odd) a { margin: 2px } li:nth-child(2n) p { padding: 1px }",
        vec![
            Box::new(|d| {
                let l = by_id(d, "l");
                let li = el(d, "li", Some("tail item"));
                d.append_child(l, li);
            }),
            Box::new(|d| {
                let first = by_id(d, "i0");
                let li = el(d, "li", None);
                let a = el(d, "a", Some("new first"));
                d.append_child(li, a);
                d.insert_before(li, first);
            }),
            Box::new(|d| {
                let first = by_id(d, "i1");
                d.detach(first);
            }),
            Box::new(|d| {
                let last = by_id(d, "i5");
                d.detach(last);
            }),
        ],
    );
}

/// A sibling combinator reaches a descendant of a later sibling (`h2 + div p`).
#[test]
fn sibling_combinators_reach_through_a_child_to_its_descendants() {
    let html = "<section id=\"s\"><div id=\"d\"><p>one <b>b</b></p></div><span>sp</span><div><p>two</p></div></section>";
    drive(
        html,
        "h2 + div p { color: red } h2 ~ span { margin: 4px } h2 + div + span { padding: 2px } div + div p b { font-size: 20px }",
        vec![
            Box::new(|d| {
                let first = by_id(d, "d");
                let h = el(d, "h2", Some("heading"));
                d.insert_before(h, first);
            }),
            Box::new(|d| {
                let s = by_id(d, "s");
                let h = d.get(s).children[0];
                d.detach(h);
            }),
        ],
    );
}

/// `:empty` flips on the container itself, and a sibling combinator on it reaches its sibling.
#[test]
fn an_emptied_container_restyles_itself_and_its_sibling() {
    let html = "<div class=\"box\" id=\"b\"></div><p id=\"after\">after <b>x</b></p><div class=\"box\" id=\"b2\"><span>x</span></div>";
    drive(
        html,
        ".box:empty { height: 20px; color: red } .box:empty + p { margin-top: 5px } .box:empty + p b { color: blue } .box { color: green }",
        vec![
            Box::new(|d| {
                let b = by_id(d, "b");
                let s = el(d, "span", Some("in"));
                d.append_child(b, s);
            }),
            Box::new(|d| {
                let b = by_id(d, "b");
                let c = d.get(b).children[0];
                d.detach(c);
            }),
            Box::new(|d| {
                let b2 = by_id(d, "b2");
                let c = d.get(b2).children[0];
                d.detach(c);
            }),
        ],
    );
}

/// An inherited value that changes because a *sibling* appeared: the first item stops being
/// an `:only-child`, and everything inside it inherits another colour and size.
#[test]
fn a_child_whose_style_changes_takes_its_inheriting_subtree() {
    let html = "<ul id=\"u\"><li id=\"only\"><span>deep <b>x</b> <i>y</i></span></li></ul>";
    drive(
        html,
        "ul > li:only-child { color: red; font-size: 20px } li { margin: 1px }",
        vec![
            Box::new(|d| {
                let u = by_id(d, "u");
                let li = el(d, "li", Some("second"));
                d.append_child(u, li);
            }),
            Box::new(|d| {
                let u = by_id(d, "u");
                let second = d.get(u).children[1];
                d.detach(second);
            }),
        ],
    );
}

/// A node moved between parents keeps its entry and — here — its own style, but its
/// descendants were matched against another ancestor chain (`.a span`).
#[test]
fn a_moved_child_is_restyled_through_its_descendants() {
    let html = "<div class=\"a\" id=\"a\"><div id=\"m\"><p>lead <span>s <b>x</b></span></p></div><div><span>t</span></div></div><div class=\"b\" id=\"b\"><p>p</p></div>";
    drive(
        html,
        ".a span { color: red } .b span { color: blue; font-size: 18px } .b b { font-weight: bold }",
        vec![
            Box::new(|d| {
                let (m, b) = (by_id(d, "m"), by_id(d, "b"));
                d.append_child(b, m);
            }),
            Box::new(|d| {
                let (m, a) = (by_id(d, "m"), by_id(d, "a"));
                let first = d.get(a).children[0];
                d.insert_before(m, first);
            }),
        ],
    );
}

/// A text node's data changing is a child-list change of its parent (`:empty`).
#[test]
fn text_becoming_empty_flips_the_parents_empty_state() {
    let html = "<div id=\"t\">x</div><p>after</p>";
    drive(
        html,
        "div:empty { height: 12px; color: red } div:empty + p { margin: 6px }",
        vec![Box::new(|d| {
            let t = by_id(d, "t");
            let text = d.get(t).children[0];
            d.detach(text);
        })],
    );
}

/// BUG-1242: a box the pass leaves clean keeps what its margins, auto-centring and
/// `position: relative` put between the parent's origin and its border box.
#[test]
fn clean_boxes_keep_their_margins_centering_and_relative_offsets() {
    let html = "<div id=\"w\"><div class=\"m\"><p>margin</p></div><div class=\"c\"><p>centred</p></div>\
                <div class=\"r\"><p>relative</p></div><div class=\"j\"><p>justified</p></div></div>";
    drive(
        html,
        ".m { margin: 3px 5px 7px 9px; padding: 1px } .c { width: 100px; margin: 0 auto } \
         .r { position: relative; left: 5px; top: 2px; margin-left: 4px } .j { width: 120px; justify-self: center }",
        vec![
            Box::new(|d| {
                let w = by_id(d, "w");
                let extra = el(d, "div", Some("appended"));
                d.append_child(w, extra);
            }),
            Box::new(|d| {
                let w = by_id(d, "w");
                let first = d.get(w).children[0];
                d.detach(first);
            }),
        ],
    );
}
