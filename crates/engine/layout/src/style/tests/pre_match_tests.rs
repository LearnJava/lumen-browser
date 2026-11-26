//! LOAD-6 — parallel matching of the initial cascade: the pre-matched walk must produce
//! the sequential walk's styles byte for byte, on every rule group the record covers, and
//! with the interactive state (`:hover`/`:focus`/`:focus-visible`) that only the
//! `StyleEnvSnapshot` carries to the rayon workers.

use lumen_core::geom::Size;
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, FlatTree, NodeData, NodeId};

use crate::counters::{precompute_counters, CounterMap};
use crate::style::pre_match;

const CSS: &str = "
    .wrap { display: block; color: rgb(10, 20, 30); }
    .wrap .block { margin: 1px; }
    .wrap > .block > .cell { padding: 2px; }
    .block + .block { border-top: 1px solid rgb(1, 1, 1); }
    .cell:first-child { font-weight: 700; }
    .cell:hover { color: rgb(200, 0, 0); }
    .block:hover .cell { background-color: rgb(0, 200, 0); }
    .cell:focus-visible { outline: 2px solid rgb(0, 0, 255); }
    .block:has(.cell:hover) { opacity: 0.9; }
    @media (min-width: 800px) { .cell { letter-spacing: 1px; } }
    @supports (display: grid) { .wrap { grid-template-columns: 1fr; } }
    @layer base { .cell { color: rgb(3, 3, 3); } }
";

fn fixture() -> (Document, Stylesheet, FlatTree, Size) {
    let mut html = String::from("<html><body>");
    for w in 0..4 {
        html.push_str(&format!("<div class=\"wrap w{w}\">"));
        for b in 0..8 {
            html.push_str(&format!("<div class=\"block b{b}\">"));
            for c in 0..3 {
                html.push_str(&format!(
                    "<span class=\"cell c{c}\" style=\"line-height: 1.4\">x</span>"
                ));
            }
            html.push_str("</div>");
        }
        html.push_str("</div>");
    }
    html.push_str("</body></html>");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(CSS);
    let flat = lumen_dom::build_flat_tree(&doc);
    (doc, sheet, flat, Size::new(1280.0, 800.0))
}

fn elements(doc: &Document, flat: &FlatTree) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![doc.root()];
    while let Some(id) = stack.pop() {
        if matches!(doc.get(id).data, NodeData::Element { .. }) {
            out.push(id);
        }
        stack.extend(flat.children_of(doc, id).iter().rev().copied());
    }
    out
}

fn by_class(doc: &Document, flat: &FlatTree, class: &str) -> NodeId {
    elements(doc, flat)
        .into_iter()
        .find(|&id| doc.get(id).get_attr("class").is_some_and(|c| c.split_whitespace().any(|t| t == class)))
        .expect("fixture must hold the class")
}

/// Both roads in one process: the differential the slice exists for.
fn cascade(off: bool, doc: &Document, sheet: &Stylesheet, flat: &FlatTree, vp: Size) -> CounterMap {
    pre_match::set_disabled_for_test(off);
    let map = precompute_counters(doc, sheet, vp, flat, false);
    pre_match::set_disabled_for_test(false);
    map
}

#[test]
fn the_pre_matched_walk_reproduces_the_sequential_cascade_byte_for_byte() {
    let (doc, sheet, flat, vp) = fixture();
    assert!(elements(&doc, &flat).len() >= 64, "fixture must arm the fan-out");

    // Interactive state exercises the snapshot: without it the workers would cascade
    // `:hover`/`:focus-visible` as if nothing were hovered or keyboard-focused.
    let hovered = by_class(&doc, &flat, "b3");
    let focused = by_class(&doc, &flat, "c2");
    crate::style::set_interactive_state(Some(hovered), Some(focused), None);
    crate::style::set_keyboard_focus(true);

    let sequential = cascade(true, &doc, &sheet, &flat, vp);
    let _ = pre_match::take_hits();
    let parallel = cascade(false, &doc, &sheet, &flat, vp);
    let hits = pre_match::take_hits();

    crate::style::clear_interactive_state();

    assert!(hits >= 64, "the pre-matched road must actually run, got {hits} lookups");
    assert_eq!(
        sequential.styles(),
        parallel.styles(),
        "the pre-matched walk changed a computed style — the matched-rule record and the \
         sequential match disagree on this document"
    );
}

/// A hover/active subject left over from the outgoing document — an index past this
/// document's arena — cascades as "nothing hovered" on both roads. It used to take the
/// rayon workers down with the `BUG-986` panic, which unwound under the caller's
/// document lock and poisoned it.
#[test]
fn an_interactive_index_foreign_to_the_document_cascades_as_nothing_hovered() {
    let (doc, sheet, flat, vp) = fixture();
    let foreign = NodeId::from_index(1_000_000);
    assert!(!doc.contains_id(foreign), "fixture must not reach the foreign index");

    crate::style::set_interactive_state(None, None, None);
    let idle_sequential = cascade(true, &doc, &sheet, &flat, vp);
    let _ = pre_match::take_hits();
    let idle_parallel = cascade(false, &doc, &sheet, &flat, vp);
    crate::style::clear_interactive_state();

    crate::style::set_interactive_state(Some(foreign), None, Some(foreign));
    let foreign_sequential = cascade(true, &doc, &sheet, &flat, vp);
    let _ = pre_match::take_hits();
    let foreign_parallel = cascade(false, &doc, &sheet, &flat, vp);
    let hits = pre_match::take_hits();
    crate::style::clear_interactive_state();

    assert!(hits >= 64, "the pre-matched road must actually run, got {hits} lookups");
    assert_eq!(
        foreign_sequential.styles(),
        idle_sequential.styles(),
        "a foreign interactive index changed a style on the sequential road"
    );
    assert_eq!(
        foreign_parallel.styles(),
        idle_parallel.styles(),
        "a foreign interactive index changed a style on the pre-matched road"
    );
}

/// A document with an author shadow tree exercises the `StyleEnvSnapshot` half of the
/// pre-pass: `:host`/`::slotted()`/interior rules are not part of the record (the walk
/// re-matches them), but the shadow sheets and the shadow-context `shareable` verdict are
/// read on the workers.
#[test]
fn the_pre_matched_walk_agrees_with_the_sequential_one_in_a_shadow_tree() {
    use lumen_dom::{Attribute, NodeData, QualName, ShadowRootMode};

    let mut doc = lumen_html_parser::parse("<html><body><div id=\"host\"></div></body></html>");
    let host = doc.find_by_id("host").expect("fixture host");
    let sr = doc.attach_shadow(host, ShadowRootMode::Open);
    for _ in 0..70 {
        let cell = doc.create_element(QualName::html("span"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(cell).data {
            attrs.push(Attribute { name: QualName::html("class"), value: "in".to_string() });
        }
        doc.append_child(sr, cell);
    }
    let sheet = lumen_css_parser::parse(CSS);
    let flat = lumen_dom::build_flat_tree(&doc);
    let vp = Size::new(1280.0, 800.0);
    let mut sheets = std::collections::HashMap::new();
    sheets.insert(
        host,
        lumen_css_parser::parse(":host { color: rgb(1, 2, 3) } .in { padding: 1px } :host(.nope) { margin: 1px }"),
    );
    crate::style::set_shadow_sheets(sheets);

    let sequential = cascade(true, &doc, &sheet, &flat, vp);
    let _ = pre_match::take_hits();
    let parallel = cascade(false, &doc, &sheet, &flat, vp);
    let hits = pre_match::take_hits();
    crate::style::clear_shadow_sheets();

    assert!(hits >= 64, "the pre-matched road must actually run, got {hits} lookups");
    assert_eq!(
        sequential.styles(),
        parallel.styles(),
        "the pre-matched walk changed a shadow-tree style"
    );
}

/// PERF-PSEUDO-PRE — the pseudo-element probe must produce the sequential
/// road's `Option<ComputedStyle>` byte for byte: the worker's `PseudoMatch`
/// record and the in-place candidate scan have to agree on every rule group
/// (top-level, `@media`, `@supports`, and the empty-match `None` cases).
#[test]
fn pseudo_pre_match_reproduces_the_sequential_pseudo_cascade() {
    const PSEUDO_CSS: &str = "
        .wrap::before { content: \"B\"; color: rgb(1, 2, 3); }
        .wrap::after { content: \"A\"; margin: 1px; }
        .cell::before { content: \"c\"; font-weight: 700; }
        .cell:hover::after { content: \"h\"; }
        @media (min-width: 800px) { .block::after { content: \"m\"; padding: 1px; } }
        @supports (display: grid) { .wrap::first-line { color: rgb(9, 9, 9); } }
        ul::marker { color: rgb(4, 5, 6); }
        .nomatch::before { content: \"x\"; }
    ";
    let mut html = String::from("<html><body>");
    for w in 0..4 {
        html.push_str(&format!("<div class=\"wrap w{w}\">"));
        for b in 0..8 {
            html.push_str(&format!("<div class=\"block b{b}\">"));
            for c in 0..3 {
                html.push_str(&format!("<span class=\"cell c{c}\">x</span>"));
            }
            html.push_str("</div>");
        }
        html.push_str("<ul><li>i1</li><li>i2</li></ul>");
        html.push_str("</div>");
    }
    html.push_str("</body></html>");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(PSEUDO_CSS);
    let flat = lumen_dom::build_flat_tree(&doc);
    let vp = Size::new(1280.0, 800.0);
    let els = elements(&doc, &flat);
    assert!(els.len() >= 64, "fixture must arm the fan-out");
    let parent = crate::style::ComputedStyle::root();
    let pseudos = ["before", "after", "marker", "first-line", "first-letter", "selection"];

    crate::style::set_interactive_state(None, None, None);
    let mut sequential = Vec::new();
    for &n in &els {
        for p in pseudos {
            sequential.push(crate::style::compute_pseudo_element_style(&doc, n, p, &sheet, &parent, vp, false));
        }
    }

    let guard = pre_match::install(&doc, &sheet, vp, false, &flat);
    let mut parallel = Vec::new();
    for &n in &els {
        for p in pseudos {
            parallel.push(crate::style::compute_pseudo_element_style(&doc, n, p, &sheet, &parent, vp, false));
        }
    }
    drop(guard);

    assert_eq!(
        sequential, parallel,
        "the pseudo pre-match record changed a computed pseudo style"
    );
}

#[test]
fn the_table_holds_every_element_of_the_document_and_only_its_own_sheet() {
    let (doc, sheet, flat, vp) = fixture();
    let other = lumen_css_parser::parse(".cell { color: rgb(9, 9, 9); }");
    let guard = pre_match::install(&doc, &sheet, vp, false, &flat);
    for id in elements(&doc, &flat) {
        let entry = pre_match::lookup(id, &sheet, vp, false);
        assert!(entry.is_some(), "no pre-matched record for {id:?}");
        assert!(
            pre_match::lookup(id, &other, vp, false).is_none(),
            "a record from another sheet must not be served"
        );
        assert!(
            pre_match::lookup(id, &sheet, Size::new(640.0, 480.0), false).is_none(),
            "a record matched under another viewport must not be served"
        );
    }
    drop(guard);
    let first = elements(&doc, &flat)[0];
    assert!(
        pre_match::lookup(first, &sheet, vp, false).is_none(),
        "the table must be gone once the pass ended"
    );
}
