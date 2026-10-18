//! CSS Fragmentation L3 §4.4 / CSS Position L3 — absolutely positioned boxes in a multicol
//! container: the offset of the box is measured in the stack of the columns, so it is cut across
//! them and may open columns of its own.

use crate::{first_element_child, lay_measured, LayoutBox};
use crate::style::Position;

/// `(column index by x, rect.y, rect.height)` of the absolutely positioned boxes that are direct
/// children of the multicol container; columns are 25px wide at `x = 0, 25, 50, 75`.
fn abs_pieces(root: &LayoutBox) -> Vec<(usize, f32, f32)> {
    first_element_child(root)
        .children
        .iter()
        .filter(|b| b.style.position == Position::Absolute)
        .map(|b| ((b.rect.x / 25.0).round() as usize, b.rect.y, b.rect.height))
        .collect()
}

const MULTICOL: &str = "#c { width: 100px; column-count: 4; column-gap: 0px; column-fill: auto; height: 100px; }";

#[test]
fn an_abspos_child_of_the_container_starts_in_the_column_its_top_falls_in() {
    let root = lay_measured(
        "<div id='c'><div id='f'></div><div id='a'></div></div>",
        &format!(
            "{MULTICOL} #c {{ position: relative; }} #f {{ height: 50px; }} \
             #a {{ position: absolute; top: 150px; left: 0; width: 20px; height: 20px; }}"
        ),
        800.0,
    );
    let a = abs_pieces(&root);
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0].0, 1, "second column: {a:?}");
    assert!((a[0].1 - 50.0).abs() < 0.5 && (a[0].2 - 20.0).abs() < 0.5, "y = 150 - 100: {a:?}");
}

#[test]
fn an_abspos_child_of_the_container_is_cut_at_the_column_edge() {
    let root = lay_measured(
        "<div id='c'><div id='f'></div><div id='a'></div></div>",
        &format!(
            "{MULTICOL} #c {{ position: relative; }} #f {{ height: 50px; }} \
             #a {{ position: absolute; top: 90px; left: 0; width: 20px; height: 30px; }}"
        ),
        800.0,
    );
    let a = abs_pieces(&root);
    assert_eq!(a.len(), 2, "{a:?}");
    assert_eq!((a[0].0, a[1].0), (0, 1));
    assert!((a[0].1 - 90.0).abs() < 0.5 && (a[0].2 - 10.0).abs() < 0.5, "first piece: {a:?}");
    assert!(a[1].1.abs() < 0.5 && (a[1].2 - 20.0).abs() < 0.5, "second piece at the column top: {a:?}");
}

#[test]
fn an_abspos_box_in_a_relative_wrapper_is_cut_across_the_columns() {
    // The wrapper ends in the first column; the box hangs 60px below its top and reaches
    // the second one.
    let root = lay_measured(
        "<div id='c'><div id='f'></div><div id='r'><div id='a'></div></div></div>",
        &format!(
            "{MULTICOL} #f {{ height: 70px; }} #r {{ position: relative; height: 10px; }} \
             #a {{ position: absolute; top: 20px; left: 0; width: 25px; height: 40px; }}"
        ),
        800.0,
    );
    let a = abs_pieces(&root);
    assert_eq!(a.len(), 2, "{a:?}");
    assert_eq!((a[0].0, a[1].0), (0, 1));
    assert!((a[0].1 - 90.0).abs() < 0.5 && (a[0].2 - 10.0).abs() < 0.5, "{a:?}");
    assert!(a[1].1.abs() < 0.5 && (a[1].2 - 30.0).abs() < 0.5, "{a:?}");
}

#[test]
fn an_abspos_box_with_a_static_position_below_the_column_opens_the_next_one() {
    let root = lay_measured(
        "<div id='c'><div id='r'><div id='f'></div><div id='a'></div></div></div>",
        &format!(
            "{MULTICOL} #r {{ position: relative; }} #f {{ height: 100px; width: 25px; }} \
             #a {{ position: absolute; width: 25px; height: 200px; }}"
        ),
        800.0,
    );
    let a = abs_pieces(&root);
    // The box is a child of #r, lifted to the container: two columns of 100px.
    assert_eq!(a.len(), 2, "{a:?}");
    assert_eq!(a.iter().map(|p| p.0).collect::<Vec<_>>(), vec![1, 2]);
    assert!(a.iter().all(|p| p.1.abs() < 0.5 && (p.2 - 100.0).abs() < 0.5), "{a:?}");
}

#[test]
fn a_fixed_box_keeps_the_atomic_placement() {
    let root = lay_measured(
        "<div id='c'><div id='f'></div><div id='a'></div></div>",
        &format!(
            "{MULTICOL} #f {{ height: 150px; }} \
             #a {{ position: fixed; top: 5px; left: 5px; width: 20px; height: 20px; }}"
        ),
        800.0,
    );
    let c = first_element_child(&root);
    let fixed: Vec<_> = c.children.iter().filter(|b| b.style.position == Position::Fixed).collect();
    assert_eq!(fixed.len(), 1);
    assert!((fixed[0].rect.y - 5.0).abs() < 0.5, "viewport-relative: {}", fixed[0].rect.y);
}
