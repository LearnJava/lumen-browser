//! BUG-935 срез 89 — `NodeChange::ChildListEnds`: правка списка детей, о которой известно, что
//! она задела начало/конец списка элементов, пересчитывает глубоко только тех детей, у кого
//! `:first-child`/`:last-child`/`:only-child` мог сменить ответ, — а не каждого `div`, который
//! подходит под `div:first-child .x`.
//!
//! Журнал правок ведётся так же, как его ведёт трекер `lumen-js` (`dom_helpers::child_edit_kind`):
//! элемент — «начало», если перед ним нет элемента, «конец» — если после; текст — не правка.
//! Каждый сценарий сверяется с полным каскадом и деревом боксов.

use lumen_core::geom::Size;
use lumen_dom::{Document, NodeData, NodeId};

use crate::style::NodeChange;

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

/// `(container, front, back, edits)` per container, as the tracker would report it.
#[derive(Default)]
struct Log(Vec<(NodeId, bool, bool, u32)>);

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Element { front: bool, back: bool },
}

fn kind(doc: &Document, node: NodeId) -> Option<Kind> {
    let parent = doc.get(node).parent?;
    match &doc.get(node).data {
        NodeData::Text(_) | NodeData::Comment(_) => return Some(Kind::Text),
        NodeData::Element { .. } => {}
        _ => return None,
    }
    let is_el = |c: &NodeId| matches!(doc.get(*c).data, NodeData::Element { .. });
    let kids = &doc.get(parent).children;
    Some(Kind::Element {
        front: kids.iter().find(|c| is_el(c)) == Some(&node),
        back: kids.iter().rev().find(|c| is_el(c)) == Some(&node),
    })
}

impl Log {
    fn note(&mut self, container: NodeId, k: Kind) {
        let slot = match self.0.iter().position(|e| e.0 == container) {
            Some(i) => i,
            None => {
                self.0.push((container, false, false, 0));
                self.0.len() - 1
            }
        };
        if let Kind::Element { front, back } = k {
            let e = &mut self.0[slot];
            e.1 |= front;
            e.2 |= back;
            e.3 += 1;
        }
    }

    fn append(&mut self, d: &mut Document, parent: NodeId, child: NodeId) {
        let old = d.get(child).parent.zip(kind(d, child));
        d.append_child(parent, child);
        if let Some((o, k)) = old {
            self.note(o, k);
        }
        self.note(parent, kind(d, child).expect("classifiable"));
    }

    fn insert_before(&mut self, d: &mut Document, child: NodeId, reference: NodeId) {
        let parent = d.get(reference).parent.expect("attached reference");
        let old = d.get(child).parent.zip(kind(d, child));
        d.insert_before(child, reference);
        if let Some((o, k)) = old {
            self.note(o, k);
        }
        self.note(parent, kind(d, child).expect("classifiable"));
    }

    fn remove(&mut self, d: &mut Document, child: NodeId) {
        let old = d.get(child).parent.zip(kind(d, child));
        d.detach(child);
        if let Some((o, k)) = old {
            self.note(o, k);
        }
    }
}

type Step = Box<dyn Fn(&mut Document, &mut Log)>;

fn el(doc: &mut Document, tag: &str, text: Option<&str>) -> NodeId {
    let e = doc.create_element(lumen_dom::QualName::html(tag));
    if let Some(t) = text {
        let t = doc.create_text(t);
        doc.append_child(e, t);
    }
    e
}

fn by_id(doc: &Document, id: &str) -> NodeId {
    doc.find_by_id(id).unwrap_or_else(|| panic!("fixture id {id}"))
}

fn nth_el(doc: &Document, parent: NodeId, n: usize) -> NodeId {
    doc.get(parent)
        .children
        .iter()
        .copied()
        .filter(|&c| matches!(doc.get(c).data, NodeData::Element { .. }))
        .nth(n)
        .expect("fixture element")
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

/// Runs `steps`; the change set of a step is what its [`Log`] holds, as `plain` (`ChildList`,
/// the pre-срез-89 report) or as `ChildListEnds`. Returns `(recomputed, elements)` per step.
fn drive(html: &str, css: &str, steps: Vec<Step>, plain: bool) -> Vec<(u32, usize)> {
    use crate::counters::{set_incremental_restyle, take_cascade_stats, ContentDirty, RestyleDelta};
    use crate::style::{restyle_node_index, restyle_roots_for_node_changes};

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
        let mut log = Log::default();
        mutate(&mut doc, &mut log);
        let journal = doc.take_content_journal().expect("recording was started");
        assert!(!journal.is_empty(), "step {step}: the mutation must be journaled");

        let node_index = restyle_node_index(&doc, &sheet);
        let roots = restyle_roots_for_node_changes(
            &doc,
            log.0.iter().map(|&(n, front, back, edits)| {
                (n, if plain { NodeChange::ChildList } else { NodeChange::ChildListEnds { front, back, edits } })
            }),
            &node_index,
        );
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
            "step {step}: the ends-limited cascade left a node with a different style than a full cascade"
        );
        let (mut a, mut b) = (Vec::new(), Vec::new());
        collect(&incr, &mut a);
        collect(&full, &mut b);
        assert_eq!(a, b, "step {step}: the incremental tree diverged from a full rebuild");

        out.push((stats.recomputed, full_counters.styles().len()));
        prev = incr;
        prev_counters = incr_counters;
    }
    out
}

fn list_html(n: usize) -> String {
    let items: String =
        (0..n).map(|i| format!("<div id=\"i{i}\" class=\"it\"><p>row <b>{i}</b> <i>x</i></p></div>")).collect();
    format!("<div id=\"l\">{items}</div><div id=\"tail\"><p>tail</p></div>")
}

const ENDS_CSS: &str = "div:first-child p { color: red } div:last-child p { font-weight: bold } \
                        div:last-child b { font-size: 20px } div:only-child p { margin: 3px } \
                        .it:first-child b { color: blue } .it:last-child i { color: green } .it { margin: 1px }";

/// The ria.ru case: an element appended after the last one, a sheet with `div:first-child …`.
/// The container's other children stay put; before срез 89 every `div` among them was recascaded.
#[test]
fn an_append_does_not_recascade_the_middle_of_the_list() {
    let steps = || -> Vec<Step> {
        vec![Box::new(|d, log| {
            let l = by_id(d, "l");
            let e = el(d, "div", Some("new"));
            log.append(d, l, e);
        })]
    };
    let ends = drive(&list_html(30), ENDS_CSS, steps(), false);
    let plain = drive(&list_html(30), ENDS_CSS, steps(), true);
    // The container, its 31 direct children and the new subtree are recascaded either way; what
    // the plain report adds is the subtree of every `div` child (they all could match `div:first-child`).
    let (recomputed, elements) = ends[0];
    assert!(recomputed * 2 < plain[0].0, "ends: {recomputed} of {elements}, plain ChildList: {}", plain[0].0);
}

/// Soundness over every kind of edit the log describes: each step must equal a full cascade.
#[test]
fn end_edits_match_a_full_cascade() {
    drive(
        &list_html(6),
        ENDS_CSS,
        vec![
            // append, prepend, insert in the middle
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let e = el(d, "div", Some("tail"));
                log.append(d, l, e);
            }),
            Box::new(|d, log| {
                let first = by_id(d, "i0");
                let e = el(d, "div", Some("head"));
                log.insert_before(d, e, first);
            }),
            Box::new(|d, log| {
                let mid = by_id(d, "i3");
                let e = el(d, "div", Some("mid"));
                log.insert_before(d, e, mid);
            }),
            // remove the first, the last and a middle one
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let first = nth_el(d, l, 0);
                log.remove(d, first);
            }),
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let n = d.get(l).children.len();
                let last = nth_el(d, l, n - 1);
                log.remove(d, last);
            }),
            Box::new(|d, log| {
                let mid = by_id(d, "i2");
                log.remove(d, mid);
            }),
        ],
        false,
    );
}

/// Several edits in one cycle push the old first/last away from the end: append `E1`, then insert
/// `E2` *before* `E1` — the old last element is three places from the end, and the middle insert
/// is neither a front nor a back edit.
#[test]
fn several_edits_in_a_cycle_widen_the_reach() {
    drive(
        &list_html(5),
        ENDS_CSS,
        vec![
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let e1 = el(d, "div", Some("e1"));
                log.append(d, l, e1);
                let e2 = el(d, "div", Some("e2"));
                log.insert_before(d, e2, e1);
                let e3 = el(d, "div", Some("e3"));
                log.insert_before(d, e3, e1);
            }),
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let first = nth_el(d, l, 0);
                let e1 = el(d, "div", Some("f1"));
                log.insert_before(d, e1, first);
                let e2 = el(d, "div", Some("f2"));
                log.insert_before(d, e2, first);
                let e3 = el(d, "div", Some("f3"));
                log.insert_before(d, e3, first);
            }),
            // removals at both ends in one cycle
            Box::new(|d, log| {
                let l = by_id(d, "l");
                for _ in 0..2 {
                    let first = nth_el(d, l, 0);
                    log.remove(d, first);
                    let n = d.get(l).children.len();
                    let last = nth_el(d, l, n - 1);
                    log.remove(d, last);
                }
            }),
        ],
        false,
    );
}

/// `:only-child` flips on the one existing element when a second one arrives or leaves.
#[test]
fn only_child_follows_an_edit_at_either_end() {
    drive(
        "<div id=\"l\"><div id=\"i0\" class=\"it\"><p>only <b>x</b> <i>y</i></p></div></div>",
        ENDS_CSS,
        vec![
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let e = el(d, "div", Some("after"));
                log.append(d, l, e);
            }),
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let second = nth_el(d, l, 1);
                log.remove(d, second);
            }),
            Box::new(|d, log| {
                let first = by_id(d, "i0");
                let e = el(d, "div", Some("before"));
                log.insert_before(d, e, first);
            }),
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let first = nth_el(d, l, 0);
                log.remove(d, first);
            }),
        ],
        false,
    );
}

/// Text nodes in the list are no edits; a mixed list with text between elements stays sound.
#[test]
fn text_between_elements_is_no_end_edit() {
    drive(
        "<div id=\"l\">lead<div id=\"i0\" class=\"it\"><p>a <b>x</b> <i>y</i></p></div> gap \
         <div id=\"i1\" class=\"it\"><p>b <b>x</b> <i>y</i></p></div>tail</div>",
        ENDS_CSS,
        vec![
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let t = d.create_text("more");
                log.append(d, l, t);
            }),
            Box::new(|d, log| {
                let first = by_id(d, "i0");
                let t = d.create_text("before");
                log.insert_before(d, t, first);
            }),
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let e = el(d, "div", Some("new"));
                log.append(d, l, e);
            }),
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let last = nth_el(d, l, 2);
                log.remove(d, last);
            }),
        ],
        false,
    );
}

/// An element moved inside its own list: the log holds its leaving and its arrival.
#[test]
fn a_move_inside_the_list_matches_a_full_cascade() {
    drive(
        &list_html(5),
        ENDS_CSS,
        vec![
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let first = by_id(d, "i0");
                log.append(d, l, first);
            }),
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let first = nth_el(d, l, 0);
                let last = nth_el(d, l, 4);
                log.insert_before(d, last, first);
            }),
        ],
        false,
    );
}

/// A move between two lists reports both.
#[test]
fn a_move_between_lists_matches_a_full_cascade() {
    drive(
        &list_html(4),
        ENDS_CSS,
        vec![
            Box::new(|d, log| {
                let tail = by_id(d, "tail");
                let last = by_id(d, "i3");
                log.append(d, tail, last);
            }),
            Box::new(|d, log| {
                let first = by_id(d, "i0");
                let tail = by_id(d, "tail");
                let p = d.get(tail).children[0];
                log.insert_before(d, p, first);
            }),
        ],
        false,
    );
}

/// `:nth-child`, `:empty` and sibling combinators are reached by an edit wherever it is: the
/// ends report must not narrow them.
#[test]
fn positional_selectors_beyond_the_ends_still_restyle_every_child() {
    drive(
        &list_html(7),
        "div:nth-child(2n) p { color: red } div:nth-last-child(3) b { font-size: 18px } \
         div + div p { margin: 2px } div:first-of-type p { padding: 1px } div:first-child b { color: blue } .it { margin: 1px }",
        vec![
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let e = el(d, "div", Some("new"));
                log.append(d, l, e);
            }),
            Box::new(|d, log| {
                let mid = by_id(d, "i3");
                log.remove(d, mid);
            }),
            Box::new(|d, log| {
                let first = by_id(d, "i0");
                let e = el(d, "div", Some("head"));
                log.insert_before(d, e, first);
            }),
        ],
        false,
    );
}

// ── срез 92: что достаёт до потомков ребёнка — потомки, подходящие под субъект селектора ─────────

/// A page-sized wrapper per row, `.x` in a few of them: `div:first-child .x` can only restyle `.x`.
fn wrapped_list_html(rows: usize) -> String {
    let rows: String = (0..rows)
        .map(|i| {
            let x = if i % 7 == 0 { "<span class=\"x\">x</span>" } else { "" };
            format!(
                "<div id=\"r{i}\" class=\"row\"><section><p>a <b>b{i}</b> <i>c</i></p><ul><li>1</li><li>2 {x}</li></ul>\
                 <em>e</em></section></div>"
            )
        })
        .collect();
    format!("<div id=\"l\">{rows}</div>")
}

const REACH_CSS: &str = "div:first-child .x { color: red } div:last-child .x { font-weight: bold } \
                         .row:nth-child(2) em { color: blue } .row + .row li { margin: 2px } .row { margin: 1px }";

/// The recascade of a child no longer takes its subtree: only what the selector's subject can match.
#[test]
fn a_child_feeding_a_selector_recascades_only_the_subject_matches() {
    let steps = || -> Vec<Step> {
        vec![Box::new(|d, log| {
            let l = by_id(d, "l");
            let e = el(d, "div", Some("new"));
            log.append(d, l, e);
        })]
    };
    let ends = drive(&wrapped_list_html(40), REACH_CSS, steps(), false);
    let (recomputed, elements) = ends[0];
    // 40 rows × 9 elements; the container, the 41 children and the new row are recascaded either
    // way, and a `.row + .row li` reader makes every row's `li`s part of the answer (80 of them).
    assert!((recomputed as usize) * 2 < elements, "recomputed {recomputed} of {elements}");
}

/// Every kind of edit, on a list where the subject elements sit under the first, the last and the
/// second row: each step has to equal a full cascade.
#[test]
fn subject_reach_matches_a_full_cascade_across_edits() {
    drive(
        &wrapped_list_html(8),
        REACH_CSS,
        vec![
            Box::new(|d, log| {
                let first = by_id(d, "r0");
                let e = el(d, "div", Some("head"));
                log.insert_before(d, e, first);
            }),
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let first = nth_el(d, l, 0);
                log.remove(d, first);
            }),
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let e = el(d, "div", Some("tail"));
                log.append(d, l, e);
            }),
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let n = d.get(l).children.len();
                let last = nth_el(d, l, n - 1);
                log.remove(d, last);
            }),
            Box::new(|d, log| {
                let mid = by_id(d, "r4");
                let e = el(d, "div", Some("mid"));
                log.insert_before(d, e, mid);
            }),
            Box::new(|d, log| {
                let mid = by_id(d, "r3");
                log.remove(d, mid);
            }),
        ],
        false,
    );
}

/// The same edits reported as a plain `ChildList` (no end information) are sound too.
#[test]
fn subject_reach_with_a_plain_child_list_report_matches_a_full_cascade() {
    drive(
        &wrapped_list_html(8),
        REACH_CSS,
        vec![
            Box::new(|d, log| {
                let first = by_id(d, "r0");
                let e = el(d, "div", Some("head"));
                log.insert_before(d, e, first);
            }),
            Box::new(|d, log| {
                let l = by_id(d, "l");
                let first = nth_el(d, l, 0);
                log.remove(d, first);
            }),
        ],
        true,
    );
}
