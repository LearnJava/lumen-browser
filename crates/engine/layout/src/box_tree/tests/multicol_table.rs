//! CSS Fragmentation L3 §4 / CSS Tables L3 — a table cut across the columns of a multicol
//! container between its rows (`multicol_flow`).

use crate::{BoxKind, LayoutBox, first_element_child, lay_measured};

const COLS: &str = "#c { width: 300px; column-count: 3; column-gap: 0px; column-fill: auto; height: 100px; } \
    table { border-spacing: 0; width: 100%; } td { padding: 0; } ";

fn rows_of(b: &LayoutBox, out: &mut Vec<(f32, f32)>) {
    if matches!(b.kind, BoxKind::TableRow) {
        out.push((b.rect.y, b.rect.height));
    }
    for c in &b.children {
        rows_of(c, out);
    }
}

/// Per table fragment: its `x`, height and the `(y, height)` of its rows.
fn fragments(c: &LayoutBox) -> Vec<(f32, f32, Vec<(f32, f32)>)> {
    c.children
        .iter()
        .filter(|f| matches!(f.kind, BoxKind::Table))
        .map(|f| {
            let mut rows = Vec::new();
            rows_of(f, &mut rows);
            (f.rect.x, f.rect.height, rows)
        })
        .collect()
}

fn lay(body: &str, css: &str) -> LayoutBox {
    let root = lay_measured(&format!("<div id='c'>{body}</div>"), &format!("{COLS}{css}"), 800.0);
    first_element_child(&root).clone()
}

#[test]
fn two_rows_fill_two_columns() {
    let c = lay(
        "<table><tbody><tr><td style='height:100px'>a</td></tr><tr><td style='height:100px'>b</td></tr></tbody></table>",
        "",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 2, "one table fragment per column: {f:?}");
    assert!(f[0].0 < 1.0 && (f[1].0 - 100.0).abs() < 1.0, "x: {} {}", f[0].0, f[1].0);
    assert_eq!(f[0].2.len(), 1);
    assert_eq!(f[1].2.len(), 1);
    assert!((f[0].1 - 100.0).abs() < 0.5 && (f[1].1 - 100.0).abs() < 0.5, "heights: {} {}", f[0].1, f[1].1);
    assert!(f[1].2[0].0.abs() < 0.5, "second fragment starts at the top of its column: {}", f[1].2[0].0);
}

#[test]
fn a_table_that_fits_stays_whole() {
    let c = lay(
        "<table><tbody><tr><td style='height:30px'>a</td></tr><tr><td style='height:30px'>b</td></tr></tbody></table>",
        "",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].2.len(), 2);
}

#[test]
fn break_before_on_a_row_starts_a_new_column() {
    let c = lay(
        "<table><tbody><tr><td style='height:20px'>a</td></tr>\
         <tr style='break-before:column'><td style='height:20px'>b</td></tr>\
         <tr><td style='height:20px'>c</td></tr></tbody></table>",
        "",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 2, "{f:?}");
    assert_eq!(f[0].2.len(), 1);
    assert_eq!(f[1].2.len(), 2);
}

#[test]
fn break_inside_avoid_rows_are_not_cut_but_moved() {
    // Three 40px rows in 100px columns: the third does not fit and moves whole.
    let c = lay(
        "<table><tbody><tr><td style='height:40px'>a</td></tr><tr><td style='height:40px'>b</td></tr>\
         <tr><td style='height:40px'>c</td></tr></tbody></table>",
        "tr { break-inside: avoid; }",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 2, "{f:?}");
    assert_eq!(f[0].2.len(), 2);
    assert_eq!(f[1].2.len(), 1);
}

#[test]
fn the_header_group_repeats_in_every_fragment() {
    let c = lay(
        "<table><thead><tr><td style='height:20px'>h</td></tr></thead><tbody>\
         <tr><td style='height:40px'>a</td></tr><tr><td style='height:40px'>b</td></tr>\
         <tr><td style='height:40px'>c</td></tr><tr><td style='height:40px'>d</td></tr></tbody></table>",
        "tr { break-inside: avoid; }",
    );
    let f = fragments(&c);
    assert!(f.len() >= 2, "{f:?}");
    for (i, frag) in f.iter().enumerate() {
        assert!((frag.2[0].1 - 20.0).abs() < 0.5, "fragment {i} starts with the 20px header: {frag:?}");
        assert!(frag.2[0].0.abs() < 0.5, "header at the column top: {frag:?}");
    }
    assert_eq!(f[0].2.len(), 3, "header + two rows (20+40+40): {f:?}");
}

#[test]
fn the_footer_group_repeats_at_the_bottom_of_all_but_the_last_fragment() {
    let c = lay(
        "<table><tbody><tr><td style='height:40px'>a</td></tr><tr><td style='height:40px'>b</td></tr>         <tr><td style='height:40px'>c</td></tr></tbody><tfoot><tr><td style='height:20px'>f</td></tr></tfoot></table>",
        "tr { break-inside: avoid; }",
    );
    let f = fragments(&c);
    assert!(f.len() >= 2, "{f:?}");
    let (first, last) = (&f[0].2, &f.last().unwrap().2);
    assert_eq!(first.len(), 3, "two body rows and the repeated footer: {f:?}");
    assert!((first[2].0 - 80.0).abs() < 0.5 && (first[2].1 - 20.0).abs() < 0.5, "footer closes the first fragment: {f:?}");
    assert!((last.last().unwrap().1 - 20.0).abs() < 0.5, "the real footer ends the last one: {f:?}");
}

#[test]
fn a_cell_spanning_rows_is_not_cut() {
    // The rowspan cell is 200px tall over two 100px rows: no break between the rows.
    let c = lay(
        "<table><tbody><tr><td rowspan='2' style='height:200px'>a</td><td style='height:100px'>b</td></tr>\
         <tr><td style='height:100px'>c</td></tr></tbody></table>",
        "",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 1, "rows tied by a rowspan stay in one fragment: {f:?}");
    assert_eq!(f[0].2.len(), 2);
}

#[test]
fn a_tall_row_with_text_is_cut_inside_its_cells() {
    // Six lines of text (~19px each) in a 100px column: the row continues in the next column.
    let c = lay(
        "<table><tbody><tr><td>1<br>2<br>3<br>4<br>5<br>6<br>7<br>8</td></tr></tbody></table>",
        "",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 2, "{f:?}");
    assert_eq!(f[0].2.len(), 1);
    assert_eq!(f[1].2.len(), 1);
}

#[test]
fn spacing_at_the_break_is_dropped() {
    let c = lay(
        "<table style='border-spacing: 10px'><tbody><tr><td style='height:60px'>a</td></tr>\
         <tr><td style='height:60px'>b</td></tr></tbody></table>",
        "",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 2, "{f:?}");
    assert!((f[1].2[0].0 - 10.0).abs() < 0.5 || f[1].2[0].0 < 0.5, "no spacing above the first row of the second fragment: {f:?}");
}

#[test]
fn col_boxes_follow_the_fragments() {
    let c = lay(
        "<table><colgroup><col style='width:50px'></colgroup><tbody>\
         <tr><td style='height:100px'>a</td></tr><tr><td style='height:100px'>b</td></tr></tbody></table>",
        "",
    );
    let tables: Vec<&LayoutBox> = c.children.iter().filter(|f| matches!(f.kind, BoxKind::Table)).collect();
    assert_eq!(tables.len(), 2);
    for t in tables {
        let col = t.children.iter().find(|k| matches!(k.style.display, crate::Display::TableColumnGroup));
        if let Some(col) = col {
            assert!(col.rect.height <= 100.5, "colgroup clipped to the fragment: {}", col.rect.height);
        }
    }
}
