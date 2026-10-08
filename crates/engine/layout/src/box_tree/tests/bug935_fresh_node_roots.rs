//! BUG-935 срез 82 — a write on an element the cascade holds no style for yet (created after
//! the basis ran) adds no restyle root: the element is recascaded whole when its parent's
//! child-list change reaches it.
//!
//! Each scenario builds nodes on a real `Document` the way a script does (create, set
//! `class`, append) and reports the changes the engine thread's flush reports —
//! `NodeChange::ChildList` on the container, `AttrFrom { class, "" }` on the new node —
//! through `restyle_roots_for_node_changes_with_basis`, then demands the cascade *and* the
//! box tree of a full rebuild. The selectors are the ones a dropped write could get wrong:
//! a sibling combinator on the new token, a sibling's descendants, `:has()`, a moved
//! existing node, a nested new subtree.

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

/// What a step reports for one node.
enum Chg {
    ChildList,
    /// `class` written; the value it replaced.
    Class(&'static str),
    Attr(&'static str),
}

type Step = Box<dyn Fn(&mut Document) -> Vec<(NodeId, Chg)>>;

fn by_id(doc: &Document, id: &str) -> NodeId {
    doc.find_by_id(id).unwrap_or_else(|| panic!("fixture id {id}"))
}

/// A new element with `class` set, the way a framework builds it.
fn el(doc: &mut Document, tag: &str, class: Option<&str>) -> NodeId {
    let e = doc.create_element(lumen_dom::QualName::html(tag));
    if let Some(c) = class
        && let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(e).data
    {
        attrs.push(lumen_dom::Attribute { name: lumen_dom::QualName::html("class"), value: c.to_string() });
    }
    e
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

struct Outcome {
    /// Elements the cascade really recomputed.
    recomputed: u32,
    elements: usize,
    /// The same step through the root-set that knows nothing of the basis.
    recomputed_blind: u32,
}

fn to_changes(reported: &[(NodeId, Chg)]) -> Vec<(NodeId, crate::style::NodeChange<'static>)> {
    use crate::style::NodeChange;
    reported
        .iter()
        .map(|(n, c)| {
            (
                *n,
                match c {
                    Chg::ChildList => NodeChange::ChildList,
                    Chg::Class(old) => NodeChange::AttrFrom { name: "class", old },
                    Chg::Attr(name) => NodeChange::Attr(name),
                },
            )
        })
        .collect()
}

fn run_incremental(
    doc: &Document,
    sheet: &lumen_css_parser::Stylesheet,
    prev: crate::box_tree::LayoutBox,
    prev_counters: &crate::counters::CounterMap,
    changes: Vec<(NodeId, crate::style::NodeChange<'_>)>,
    journal: &std::collections::HashSet<NodeId>,
    with_basis: bool,
) -> (crate::box_tree::LayoutBox, crate::counters::CounterMap, u32) {
    use crate::counters::{set_incremental_restyle, take_cascade_stats, ContentDirty, RestyleDelta};
    use crate::style::{restyle_node_index, restyle_roots_for_node_changes, restyle_roots_for_node_changes_with_basis};

    let (vp, m, hp) = (Size::new(800.0, 600.0), Measurer, lumen_core::ext::NullHyphenationProvider);
    let node_index = restyle_node_index(doc, sheet);
    let styles = prev_counters.styles().clone();
    let roots = if with_basis {
        restyle_roots_for_node_changes_with_basis(doc, changes, &node_index, &|n| styles.contains_key(&n))
    } else {
        restyle_roots_for_node_changes(doc, changes, &node_index)
    };
    let delta = RestyleDelta {
        prev_styles: styles,
        dirty_roots: roots.deep,
        shallow_roots: roots.shallow,
        point_roots: roots.point,
        content_dirty: ContentDirty::Nodes(journal),
    };
    set_incremental_restyle(true);
    super::super::set_incremental_box_build(true);
    let _ = take_cascade_stats();
    let (tree, counters) = super::super::layout_mutation_incremental_restyle(doc, sheet, vp, &m, &hp, false, prev, delta);
    let stats = take_cascade_stats();
    super::super::set_incremental_box_build(false);
    set_incremental_restyle(false);
    (tree, counters, stats.recomputed)
}

fn drive(html: &str, css: &str, steps: Vec<Step>) -> Vec<Outcome> {
    let mut doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let (vp, m, hp) = (Size::new(800.0, 600.0), Measurer, lumen_core::ext::NullHyphenationProvider);
    assert!(doc.take_content_journal().is_none());
    let (mut prev, mut prev_counters) = super::super::layout_measured_hyp_with_counters(&doc, &sheet, vp, &m, &hp, false);

    let mut out = Vec::new();
    for (step, mutate) in steps.iter().enumerate() {
        let reported = mutate(&mut doc);
        let journal = doc.take_content_journal().expect("recording was started");
        // The blind run is a measurement only (its result is thrown away), so it needs its own
        // copy of the basis.
        let (_, _, recomputed_blind) =
            run_incremental(&doc, &sheet, prev.clone(), &prev_counters, to_changes(&reported), &journal, false);
        let (incr, incr_counters, recomputed) =
            run_incremental(&doc, &sheet, prev, &prev_counters, to_changes(&reported), &journal, true);

        let (full, full_counters) = super::super::layout_measured_hyp_with_counters(&doc, &sheet, vp, &m, &hp, false);
        assert!(
            incr_counters.styles() == full_counters.styles(),
            "step {step}: dropping the fresh node's writes left a node with a different style than a full cascade"
        );
        let (mut a, mut b) = (Vec::new(), Vec::new());
        collect(&incr, &mut a);
        collect(&full, &mut b);
        assert_eq!(a, b, "step {step}: the incremental tree diverged from a full rebuild");

        out.push(Outcome { recomputed, elements: full_counters.styles().len(), recomputed_blind });
        prev = incr;
        prev_counters = incr_counters;
    }
    out
}

fn list_html(n: usize) -> String {
    let items: String = (0..n).map(|i| format!("<li id=\"i{i}\"><p>item {i} <b>b</b></p></li>")).collect();
    format!("<ul id=\"l\">{items}</ul><div id=\"tail\"><p>tail</p></div>")
}

/// The case that cost: a sibling combinator mentions the new node's class and elements follow
/// the new node, so its `class` write used to widen to the whole list. Now only the container, its children and the new
/// subtree are recascaded.
#[test]
fn a_new_node_whose_class_a_sibling_selector_names_does_not_widen_to_the_container() {
    let html = list_html(30);
    let r = drive(
        &html,
        ".hot + .cool { margin-top: 3px } .cool p { color: red } li p b { font-weight: bold }",
        vec![
            Box::new(|d| {
                let l = by_id(d, "l");
                let li = el(d, "li", Some("cool"));
                let p = el(d, "p", None);
                let t = d.create_text("new");
                d.append_child(p, t);
                d.append_child(li, p);
                let first = by_id(d, "i0");
                d.insert_before(li, first);
                vec![(li, Chg::Class("")), (l, Chg::ChildList)]
            }),
            Box::new(|d| {
                let l = by_id(d, "l");
                let li = el(d, "li", Some("cool"));
                let at = by_id(d, "i7");
                d.insert_before(li, at);
                vec![(li, Chg::Class("")), (l, Chg::ChildList)]
            }),
        ],
    );
    for (step, o) in r.iter().enumerate() {
        assert!(
            (o.recomputed as usize) * 2 < o.elements,
            "step {step}: recascaded {} of {} elements — the fresh node still widened",
            o.recomputed,
            o.elements
        );
        assert!(
            o.recomputed < o.recomputed_blind,
            "step {step}: {} with the basis against {} without — the basis bought nothing",
            o.recomputed,
            o.recomputed_blind
        );
    }
}

/// The dropped write must not drop what the new token does *to the siblings*: a new `.cool`
/// after an existing `.hot` takes the margin, and a new `.cool` before `.x` restyles the
/// descendants of `.x` (`.cool + .x p`).
#[test]
fn a_new_node_still_styles_its_siblings_and_their_descendants() {
    let html = "<div id=\"c\"><div class=\"hot\" id=\"h\">hot</div><div class=\"x\" id=\"x\"><p>under x <b>b</b></p></div><div class=\"y\"><p>y</p></div></div>";
    drive(
        html,
        ".hot + .cool { margin-top: 9px } .cool + .x p { color: red } .cool + .x p b { font-size: 24px } .cool ~ .y { padding: 4px }",
        vec![
            Box::new(|d| {
                let c = by_id(d, "c");
                let x = by_id(d, "x");
                let n = el(d, "div", Some("cool"));
                d.insert_before(n, x);
                vec![(n, Chg::Class("")), (c, Chg::ChildList)]
            }),
            Box::new(|d| {
                let c = by_id(d, "c");
                let n = d.get(c).children[1];
                d.detach(n);
                vec![(c, Chg::ChildList)]
            }),
        ],
    );
}

/// `:has()` reads the new node from an ancestor: the `class` write on a fresh child still
/// restyles the existing container that `:has(.flag)` selects.
#[test]
fn a_new_node_still_flips_has_on_an_existing_ancestor() {
    let html = "<section id=\"s\"><div class=\"card\" id=\"card\"><p>card</p></div><div class=\"card\"><p>other</p></div></section>";
    drive(
        html,
        ".card:has(.flag) { border: 2px solid red; color: blue } .card:has(.flag) p { font-size: 20px }",
        vec![Box::new(|d| {
            let card = by_id(d, "card");
            let f = el(d, "span", Some("flag"));
            d.append_child(card, f);
            vec![(f, Chg::Class("")), (card, Chg::ChildList)]
        })],
    );
}

/// An existing node moved into a new wrapper: the wrapper's `class` write is dropped, the
/// moved node's descendants are still matched against the new ancestor chain (`.wrap p`).
#[test]
fn an_existing_node_moved_under_a_new_wrapper_follows_the_wrapper() {
    let html = "<div id=\"root\"><div id=\"m\"><p>moved <b>b</b></p></div><div id=\"other\"><p>other</p></div></div>";
    drive(
        html,
        ".wrap p { color: red } .wrap > div + div { margin: 5px } .wrap p b { font-size: 22px }",
        vec![Box::new(|d| {
            let root = by_id(d, "root");
            let m = by_id(d, "m");
            let other = by_id(d, "other");
            let w = el(d, "div", Some("wrap"));
            d.detach(m);
            d.detach(other);
            d.append_child(w, m);
            d.append_child(w, other);
            d.append_child(root, w);
            vec![(w, Chg::Class("")), (w, Chg::ChildList), (root, Chg::ChildList)]
        })],
    );
}

/// A write on an existing node is untouched by the basis: the class appears and its
/// sibling's match flips.
#[test]
fn an_existing_nodes_write_is_not_dropped() {
    let html = "<div id=\"c\"><div id=\"a\">a</div><div id=\"b\"><p>b</p></div></div>";
    drive(
        html,
        ".on + div p { color: red } .on + div { margin: 7px }",
        vec![Box::new(|d| {
            let a = by_id(d, "a");
            if let lumen_dom::NodeData::Element { attrs, .. } = &mut d.get_mut(a).data {
                attrs.push(lumen_dom::Attribute { name: lumen_dom::QualName::html("class"), value: "on".to_string() });
            }
            vec![(a, Chg::Class(""))]
        })],
    );
}

/// A `slot` write moves a node between composed parents without a child-list change, so it is
/// never taken as covered.
#[test]
fn a_slot_write_on_a_new_node_is_not_dropped() {
    let html = "<div id=\"c\"><p id=\"p\">p</p></div>";
    drive(
        html,
        "p { color: red }",
        vec![Box::new(|d| {
            let c = by_id(d, "c");
            let s = el(d, "span", None);
            d.append_child(c, s);
            vec![(s, Chg::Attr("slot")), (c, Chg::ChildList)]
        })],
    );
}
