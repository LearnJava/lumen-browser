//! CSS Multicol L1 §3.4 / Fragmentation L3 §4 — blocks and text runs are cut across the columns
//! at the break points of the box tree (`multicol_flow`).

use crate::{BoxKind, LayoutBox, first_element_child, lay_measured};

/// Line boxes in the subtree of `b`.
fn line_count(b: &LayoutBox) -> usize {
    let own = match &b.kind {
        BoxKind::InlineRun { lines, .. } => lines.len(),
        _ => 0,
    };
    own + b.children.iter().map(line_count).sum::<usize>()
}

/// Lines per column, columns told apart by the `x` of their top-level fragment.
fn lines_per_column(c: &LayoutBox) -> Vec<usize> {
    let mut xs: Vec<f32> = Vec::new();
    for f in &c.children {
        if !xs.iter().any(|x| (x - f.rect.x).abs() < 1.0) {
            xs.push(f.rect.x);
        }
    }
    xs.sort_by(f32::total_cmp);
    xs.iter()
        .map(|x| c.children.iter().filter(|f| (f.rect.x - x).abs() < 1.0).map(line_count).sum())
        .collect()
}

#[test]
fn a_block_with_two_children_is_cut_between_and_through_them() {
    let root = lay_measured(
        "<div id='c'><div id='w'><div id='a'></div><div id='b'></div></div></div>",
        "#c { width: 300px; column-count: 2; column-gap: 0px; column-fill: auto; height: 100px; } \
         #a { height: 150px; } #b { height: 50px; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(c.children.len(), 2, "one fragment of #w per column");
    let (f0, f1) = (&c.children[0], &c.children[1]);
    assert!(f0.rect.x < 1.0 && f1.rect.x > 100.0, "x: {} {}", f0.rect.x, f1.rect.x);
    assert!((f0.rect.height - 100.0).abs() < 0.5, "first column full: {}", f0.rect.height);
    assert!((f1.rect.height - 100.0).abs() < 0.5, "second column: 50 of #a and #b: {}", f1.rect.height);
    assert_eq!(f0.children.len(), 1);
    assert_eq!(f1.children.len(), 2);
    assert!((f1.children[0].rect.height - 50.0).abs() < 0.5);
    assert!((f1.children[1].rect.y - (f1.rect.y + 50.0)).abs() < 0.5, "#b follows the tail of #a");
}

#[test]
fn a_block_with_a_gap_between_children_breaks_there_and_not_through_a_child() {
    // #a (60) and #b (60) in 100px columns: the break falls between them although #b would fit
    // 40px of the first column - `break-inside: avoid` keeps #b whole.
    let root = lay_measured(
        "<div id='c'><div id='w'><div id='a'></div><div id='b'></div></div></div>",
        "#c { width: 300px; column-count: 2; column-gap: 0px; column-fill: auto; height: 100px; } \
         #a { height: 60px; } #b { height: 60px; break-inside: avoid; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(c.children.len(), 2);
    assert!((c.children[0].rect.height - 60.0).abs() < 0.5, "{}", c.children[0].rect.height);
    assert!((c.children[1].rect.height - 60.0).abs() < 0.5, "{}", c.children[1].rect.height);
}

#[test]
fn text_lines_are_dealt_out_to_columns_by_height() {
    let root = lay_measured(
        "<div id='c'><p>a<br>b<br>c<br>d<br>e<br>f</p></div>",
        "#c { width: 300px; column-count: 3; column-gap: 0px; column-fill: auto; height: 40px; line-height: 20px; font-size: 16px; } \
         p { margin: 0; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(lines_per_column(c), vec![2, 2, 2]);
}

#[test]
fn balanced_text_fills_several_columns() {
    let root = lay_measured(
        "<div id='c'><p>a<br>b<br>c<br>d<br>e<br>f</p></div>",
        "#c { width: 300px; column-count: 3; column-gap: 0px; line-height: 20px; font-size: 16px; } \
         p { margin: 0; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(lines_per_column(c), vec![2, 2, 2]);
    assert!((c.rect.height - 40.0).abs() < 1.0, "height={}", c.rect.height);
}

#[test]
fn orphans_and_widows_move_the_break() {
    // 5 lines, 3 fit a column: with `widows: 2` the break moves up to leave two lines behind.
    let root = lay_measured(
        "<div id='c'><p>a<br>b<br>c<br>d<br>e</p></div>",
        "#c { width: 200px; column-count: 2; column-gap: 0px; column-fill: auto; height: 80px; line-height: 20px; font-size: 16px; } \
         p { margin: 0; widows: 2; orphans: 2; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(lines_per_column(c), vec![3, 2]);
    let root = lay_measured(
        "<div id='c'><p>a<br>b<br>c<br>d<br>e</p></div>",
        "#c { width: 200px; column-count: 2; column-gap: 0px; column-fill: auto; height: 80px; line-height: 20px; font-size: 16px; } \
         p { margin: 0; widows: 3; orphans: 2; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(lines_per_column(c), vec![2, 3]);
}

#[test]
fn a_bordered_block_is_cut_and_loses_the_border_on_the_cut_edges() {
    let root = lay_measured(
        "<div id='c'><div id='a'></div></div>",
        "#c { width: 200px; column-count: 2; column-gap: 0px; column-fill: auto; height: 100px; } \
         #a { height: 142px; border-top: 4px solid #000; border-bottom: 4px solid #000; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(c.children.len(), 2);
    let (f0, f1) = (&c.children[0], &c.children[1]);
    assert!((f0.rect.height - 100.0).abs() < 0.5, "{}", f0.rect.height);
    assert!((f1.rect.height - 50.0).abs() < 0.5, "{}", f1.rect.height);
    assert_eq!(f0.style.border_top_width, 4.0);
    assert_eq!(f0.style.border_bottom_width, 0.0, "cut edge");
    assert_eq!(f1.style.border_top_width, 0.0, "cut edge");
    assert_eq!(f1.style.border_bottom_width, 4.0);
}

#[test]
fn a_scroll_container_is_monolithic() {
    let root = lay_measured(
        "<div id='c'><div id='a'></div><div id='s'></div></div>",
        "#c { width: 200px; column-count: 2; column-gap: 0px; column-fill: auto; height: 100px; } \
         #a { height: 80px; } #s { height: 60px; overflow: hidden; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(c.children.len(), 2);
    assert!(c.children[1].rect.x > 90.0, "the scroll container moves whole to the second column");
    assert!((c.children[1].rect.height - 60.0).abs() < 0.5);
}

#[test]
fn a_forced_break_inside_a_nested_block_starts_a_column() {
    let root = lay_measured(
        "<div id='c'><div id='w'><div id='a'></div><div id='b'></div></div></div>",
        "#c { width: 200px; column-count: 2; column-gap: 0px; column-fill: auto; height: 100px; } \
         #a { height: 20px; } #b { height: 20px; break-before: column; }",
        800.0,
    );
    let c = first_element_child(&root);
    assert_eq!(c.children.len(), 2);
    assert!((c.children[0].rect.height - 20.0).abs() < 0.5, "{}", c.children[0].rect.height);
    assert!(c.children[1].rect.x > 90.0);
}
