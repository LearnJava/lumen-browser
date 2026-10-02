//! CSS Multicol L1 §6.1 — `column-span: all` below a plain block wrapper
//! (`multicol_span::hoist_nested_spanners`).

use crate::{LayoutBox, first_element_child, lay_measured};

/// Full-container-width children of the multicol container `c` (the spanners).
fn multicol_spanners(c: &LayoutBox, width: f32) -> Vec<&LayoutBox> {
    c.children
        .iter()
        .filter(|b| (b.rect.width - width).abs() < 1.0)
        .collect()
}

#[test]
fn multicol_nested_column_span_all_spans_container() {
    // The spanner sits inside <section>, not directly in the container: it must
    // still take the full 300px width and split the section around itself.
    let root = lay_measured(
        "<div id='c'><section><div id='a'></div><div id='s'></div><div id='b'></div></section></div>",
        "#c { width: 300px; column-count: 2; column-gap: 10px; } \
         #a { height: 20px; } #b { height: 20px; } #s { column-span: all; height: 10px; }",
        800.0,
    );
    let c = first_element_child(&root);
    let spanners = multicol_spanners(c, 300.0);
    assert_eq!(spanners.len(), 1, "exactly one full-width spanner expected");
    let s = spanners[0];
    assert!(s.rect.x.abs() < 1.0, "spanner x={}", s.rect.x);
    assert_eq!(
        c.children.len(),
        3,
        "section splits into fragment + spanner + fragment"
    );
    let before = &c.children[0];
    let after = &c.children[2];
    assert!(
        before.rect.y + before.rect.height <= s.rect.y + 0.5,
        "first fragment ends above the spanner"
    );
    assert!(
        after.rect.y >= s.rect.y + s.rect.height - 0.5,
        "second fragment starts below the spanner"
    );
    assert!(
        (c.rect.height - 50.0).abs() < 1.0,
        "container height={} should be 20+10+20",
        c.rect.height
    );
}

#[test]
fn multicol_nested_column_span_all_slices_decoration() {
    // box-decoration-break: slice — padding-top stays on the first fragment,
    // padding-bottom on the last; the cut edges carry none.
    let root = lay_measured(
        "<div id='c'><section><div id='a'></div><div id='s'></div><div id='b'></div></section></div>",
        "#c { width: 300px; column-count: 2; column-gap: 10px; } \
         section { padding: 5px 0; } \
         #a { height: 20px; } #b { height: 20px; } #s { column-span: all; height: 10px; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(c.children.len(), 3);
    assert!(
        (c.children[0].rect.height - 25.0).abs() < 1.0,
        "first={}",
        c.children[0].rect.height
    );
    assert!(
        (c.children[2].rect.height - 25.0).abs() < 1.0,
        "last={}",
        c.children[2].rect.height
    );
    assert!(
        (c.rect.height - 60.0).abs() < 1.0,
        "container height={}",
        c.rect.height
    );
}

#[test]
fn multicol_column_span_all_two_levels_deep() {
    let root = lay_measured(
        "<div id='c'><section><article><div id='a'></div><div id='s'></div><div id='b'></div></article></section></div>",
        "#c { width: 300px; column-count: 2; column-gap: 10px; } \
         #a { height: 20px; } #b { height: 20px; } #s { column-span: all; height: 10px; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(multicol_spanners(c, 300.0).len(), 1);
    assert_eq!(c.children.len(), 3);
    assert!(
        (c.rect.height - 50.0).abs() < 1.0,
        "container height={}",
        c.rect.height
    );
}

#[test]
fn multicol_column_span_all_blocked_by_scroll_container() {
    // overflow: hidden makes the wrapper a formatting-context boundary — a
    // spanner inside it cannot escape (CSS Multicol §6.1), so nothing is split.
    let root = lay_measured(
        "<div id='c'><section><div id='a'></div><div id='s'></div></section></div>",
        "#c { width: 300px; column-count: 2; column-gap: 10px; } section { overflow: hidden; } \
         #a { height: 20px; } #s { column-span: all; height: 10px; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(c.children.len(), 1, "wrapper stays whole");
    assert!(multicol_spanners(c, 300.0).is_empty());
}

#[test]
fn multicol_column_span_all_ignored_on_float() {
    // The property does not apply to floats.
    let root = lay_measured(
        "<div id='c'><div id='a'></div><div id='f'></div><div id='b'></div></div>",
        "#c { width: 300px; column-count: 2; column-gap: 10px; } \
         #a { height: 20px; } #b { height: 20px; } #f { float: left; column-span: all; width: 40px; height: 10px; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert!(
        multicol_spanners(c, 300.0).is_empty(),
        "a floated 'spanner' must not become full-width"
    );
}
