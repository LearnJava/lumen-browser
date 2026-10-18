//! CSS Flexbox L1 §12 "Fragmenting Flex Layout" / Fragmentation L3 — a flex container in a
//! multicol container is cut across the columns (`multicol_flow`): a column container between
//! and inside its items, a row container through the items of its lines, each in its own flow.

use crate::{first_element_child, lay_measured, LayoutBox};

/// `(column index by x, rect.y, rect.height)` of the fragments of a box nesting `depth` levels
/// below the multicol container; columns are 25px wide at `x = 0, 25, 50, 75`.
fn frags(root: &LayoutBox, depth: usize) -> Vec<(usize, f32, f32)> {
    let mut level: Vec<&LayoutBox> = first_element_child(root).children.iter().collect();
    for _ in 0..depth {
        level = level.into_iter().flat_map(|b| b.children.iter()).collect();
    }
    level.iter().map(|b| ((b.rect.x / 25.0).round() as usize, b.rect.y, b.rect.height)).collect()
}

const MULTICOL: &str = "#c { width: 100px; column-count: 4; column-gap: 0px; column-fill: auto; height: 100px; }";

#[test]
fn a_column_flex_container_is_cut_between_and_inside_its_items() {
    let root = lay_measured(
        "<div id='c'><div id='f'><div id='a'></div><div id='b'></div></div></div>",
        &format!(
            "{MULTICOL} #f {{ display: flex; flex-direction: column; height: 400px; }} \
             #a {{ height: 50px; }} #b {{ height: 350px; }}"
        ),
        800.0,
    );
    let f = frags(&root, 0);
    assert_eq!(f.len(), 4, "one fragment of #f per column: {f:?}");
    for (k, (col, _, h)) in f.iter().enumerate() {
        assert_eq!(*col, k);
        assert!((h - 100.0).abs() < 0.5, "column {k} is full: {h}");
    }
    let items = frags(&root, 1);
    assert_eq!(items.len(), 5, "#a once, #b in four pieces: {items:?}");
    assert!((items[0].2 - 50.0).abs() < 0.5 && (items[1].2 - 50.0).abs() < 0.5);
}

#[test]
fn an_empty_flex_container_is_cut_through_its_box() {
    let root = lay_measured(
        "<div id='c'><div class='f' style='height: 134px'></div><div class='f' style='height: 234px'></div></div>",
        &format!(
            "{MULTICOL} .f {{ display: flex; border: 3px solid green; padding: 5px; width: 9px; \
             box-sizing: content-box; }}"
        ),
        800.0,
    );
    let f = frags(&root, 0);
    assert!(f.len() >= 5, "two containers over several columns: {f:?}");
    assert!(f.iter().all(|(_, _, h)| *h <= 100.5), "no fragment exceeds the column: {f:?}");
}

#[test]
fn a_row_flex_container_is_cut_through_the_items_of_its_line() {
    let root = lay_measured(
        "<div id='c'><div id='f'><div id='a'></div><div id='b'></div></div></div>",
        &format!(
            "{MULTICOL} #f {{ display: flex; width: 20px; }} \
             #a {{ width: 10px; height: 150px; }} #b {{ width: 10px; height: 150px; }}"
        ),
        800.0,
    );
    let f = frags(&root, 0);
    assert_eq!(f.len(), 2, "two columns: {f:?}");
    assert!((f[0].2 - 100.0).abs() < 0.5 && (f[1].2 - 50.0).abs() < 0.5, "{f:?}");
    let items = frags(&root, 1);
    assert_eq!(items.len(), 4, "both items continue in the second column: {items:?}");
    assert!(items.iter().filter(|i| i.0 == 1).all(|i| (i.2 - 50.0).abs() < 0.5), "{items:?}");
}

#[test]
fn a_forced_break_before_a_column_flex_item_starts_a_column() {
    let root = lay_measured(
        "<div id='c'><div id='f'><div id='a'></div><div id='b'></div></div></div>",
        &format!(
            "{MULTICOL} #f {{ display: flex; flex-direction: column; }} \
             #a {{ height: 20px; }} #b {{ height: 20px; break-before: column; }}"
        ),
        800.0,
    );
    let f = frags(&root, 0);
    assert_eq!(f.len(), 2, "{f:?}");
    assert_eq!((f[0].0, f[1].0), (0, 1));
}

#[test]
fn a_reversed_column_flex_container_breaks_in_visual_order() {
    // `column-reverse`: #b is on top. 70 + 70 in 100px columns: the break falls between them.
    let root = lay_measured(
        "<div id='c'><div id='f'><div id='a'></div><div id='b'></div></div></div>",
        &format!(
            "{MULTICOL} #f {{ display: flex; flex-direction: column-reverse; height: 140px; }} \
             #a {{ height: 70px; break-inside: avoid; }} #b {{ height: 70px; }}"
        ),
        800.0,
    );
    let items = frags(&root, 1);
    assert_eq!(items.len(), 2, "{items:?}");
    assert_eq!((items[0].0, items[1].0), (0, 1), "{items:?}");
}

#[test]
fn a_flex_container_that_breaks_fills_the_rest_of_the_column() {
    // 20px filler, then a column flex container of 40px items with a 5px gap: the second item
    // does not fit under the first and starts the next column; the container's first fragment
    // reaches the bottom of its column.
    let root = lay_measured(
        "<div id='c'><div id='x'></div><div id='f'><div></div><div></div><div></div></div></div>",
        "#c { width: 100px; column-count: 2; column-gap: 0px; column-fill: auto; height: 100px; } \
         #x { height: 20px; } \
         #f { display: flex; flex-direction: column; row-gap: 5px; border: solid green; border-width: 15px 5px; } \
         #f > div { height: 40px; break-inside: avoid; }",
        800.0,
    );
    let c = first_element_child(&root);
    let f: Vec<&LayoutBox> = c.children.iter().filter(|b| b.rect.height > 25.0).collect();
    assert_eq!(f.len(), 2, "two fragments of #f");
    assert!((f[0].rect.y + f[0].rect.height - (c.rect.y + 100.0)).abs() < 0.5, "first fragment reaches the column bottom");
    assert!((f[1].rect.height - 100.0).abs() < 0.5, "second: 2 items, 5px gap, bottom border: {}", f[1].rect.height);
}

#[test]
fn a_margin_taller_than_a_column_is_cut_by_the_break() {
    let root = lay_measured(
        "<div id='c'><div id='f'></div></div>",
        "#c { width: 100px; column-count: 4; column-gap: 0px; column-fill: auto; height: 100px; } \
         #f { display: flex; width: 25px; height: 400px; margin-top: 200px; }",
        800.0,
    );
    let f = frags(&root, 0);
    assert_eq!(f.iter().map(|b| b.0).collect::<Vec<_>>(), vec![1, 2, 3, 4], "{f:?}");
}
