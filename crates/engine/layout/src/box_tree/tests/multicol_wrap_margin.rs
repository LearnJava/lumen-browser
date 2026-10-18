//! CSS Multicol L2 §4.4 `column-wrap: wrap` rows opened by forced breaks without a
//! `column-height` (WPT `column-wrap-no-constraints-002`) and CSS Multicol L1 §2 (a multicol
//! container is a BFC: its first child's `margin-top` stays inside).

use lumen_core::geom::Size;

use crate::box_tree::{BoxKind, LayoutBox};

fn lay(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    super::super::layout(&doc, &sheet, Size::new(800.0, 600.0))
}

fn find<'a>(b: &'a LayoutBox, pred: &dyn Fn(&LayoutBox) -> bool) -> Option<&'a LayoutBox> {
    if pred(b) {
        return Some(b);
    }
    b.children.iter().find_map(|c| find(c, pred))
}

fn leaves(b: &LayoutBox, out: &mut Vec<(f32, f32, f32)>) {
    if matches!(b.kind, BoxKind::Block) && b.children.iter().all(|c| matches!(c.kind, BoxKind::Skip)) && b.rect.height > 0.0 {
        out.push((b.rect.x, b.rect.y, b.rect.height));
    }
    for c in &b.children {
        leaves(c, out);
    }
}

const ITEMS: &str = "<div class=i style=\"height:20px\"></div><div class=i style=\"height:20px\"></div>\
    <div class=i style=\"height:50px\"></div><div class=i style=\"height:50px\"></div>\
    <div class=i style=\"height:10px\"></div><div class=i style=\"height:10px\"></div>";

fn rows(wrap: &str) -> (Vec<(f32, f32, f32)>, f32) {
    let root = lay(
        &format!("<div id=c>{ITEMS}</div>"),
        &format!(
            "body{{margin:0}} #c{{columns:2;column-gap:0;row-gap:10px;width:100px;{wrap}}} \
             .i{{break-after:column}}"
        ),
    );
    let mut v = Vec::new();
    leaves(&root, &mut v);
    let c = find(&root, &|b| b.style.column_count.is_some()).expect("container");
    (v, c.rect.height)
}

#[test]
fn explicit_wrap_opens_a_row_per_pair_of_forced_columns() {
    let (v, h) = rows("column-wrap:wrap");
    let want = [
        (0.0, 0.0, 20.0), (50.0, 0.0, 20.0),
        (0.0, 30.0, 50.0), (50.0, 30.0, 50.0),
        (0.0, 90.0, 10.0), (50.0, 90.0, 10.0),
    ];
    assert_eq!(v, want, "{v:?}");
    assert_eq!(h, 100.0);
}

#[test]
fn auto_wrap_without_a_column_height_keeps_one_row_of_overflow_columns() {
    let (v, _) = rows("");
    assert!(v.iter().all(|b| b.1 == 0.0), "{v:?}");
}

#[test]
fn first_child_margin_top_stays_inside_the_multicol_container() {
    let root = lay(
        "<div id=c><div class=m style=\"height:10px\"></div></div>",
        "body{margin:0} #c{columns:1;background:red} .m{margin-top:30px}",
    );
    let c = find(&root, &|b| b.style.column_count.is_some()).expect("container");
    assert_eq!(c.rect.y, 0.0, "container must not be shifted by the child's margin");
    assert_eq!(c.rect.height, 40.0);
}
