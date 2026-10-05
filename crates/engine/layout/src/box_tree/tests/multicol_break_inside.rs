//! CSS Fragmentation L3 §3.1 — `break-inside: avoid` keeps a box whole in one column of a
//! multicol container (WPT `css/css-gaps/multicol/multicol-gap-decorations-015/016`: six 20px
//! boxes in four columns; a box that is cut across two columns draws a rule piece that the
//! reference does not have).

use lumen_core::geom::Size;

use crate::box_tree::{BoxKind, LayoutBox};

fn lay(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    super::super::layout(&doc, &sheet, Size::new(800.0, 600.0))
}

/// The children of the first box that has at least `n` of them.
fn children_of_wide(b: &LayoutBox, n: usize) -> Option<&Vec<LayoutBox>> {
    if b.children.len() >= n {
        return Some(&b.children);
    }
    b.children.iter().find_map(|c| children_of_wide(c, n))
}

/// `(x, y, height)` of the six `.b` boxes (or their fragments) of a 4-column container.
fn boxes(avoid: &str) -> Vec<(f32, f32, f32)> {
    let root = lay(
        "<div id=\"c\"><div class=\"b\"></div><div class=\"b\"></div><div class=\"b\"></div>\
         <div class=\"b\"></div><div class=\"b\"></div><div class=\"b\"></div></div>",
        &format!(
            "body{{margin:0}} #c{{columns:4;column-gap:0;width:400px}} \
             .b{{height:20px;background:cyan;{avoid}}}"
        ),
    );
    let kids = children_of_wide(&root, 6).expect("multicol container");
    kids.iter()
        .filter(|c| matches!(c.kind, BoxKind::Block) && c.rect.height > 0.0)
        .map(|c| (c.rect.x, c.rect.y, c.rect.height))
        .collect()
}

#[test]
fn a_box_without_the_property_is_cut_across_columns() {
    // 120px balanced over four columns is 30px each: the second and fourth boxes straddle a cut.
    let v = boxes("");
    assert!(v.len() > 6, "{v:?}");
    assert!(v.iter().any(|b| b.2 < 20.0), "{v:?}");
}

#[test]
fn break_inside_avoid_keeps_every_box_whole() {
    let v = boxes("break-inside:avoid");
    assert_eq!(v.len(), 6, "{v:?}");
    assert!(v.iter().all(|b| b.2 == 20.0), "{v:?}");
}

#[test]
fn break_inside_avoid_column_is_the_same_as_avoid() {
    let v = boxes("break-inside:avoid-column");
    assert_eq!(v.len(), 6, "{v:?}");
    assert!(v.iter().all(|b| b.2 == 20.0), "{v:?}");
}
