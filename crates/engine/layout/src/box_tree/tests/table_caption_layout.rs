//! CSS Tables L2 §17.4 `caption-side` и §17.5.2 `table-layout` — раскладка
//! `<caption>` над/под сеткой строк и fixed-алгоритм ширин колонок.

use lumen_core::geom::Size;

use super::super::{BoxKind, LayoutBox};
use crate::style::{CaptionSide, TableLayout};

fn lay(body: &str, css: &str) -> LayoutBox {
    let html = format!("<html><head><style>{css}</style></head><body>{body}</body></html>");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(css);
    super::super::layout(&doc, &sheet, Size::new(800.0, 600.0))
}

fn find<'a>(b: &'a LayoutBox, pred: &dyn Fn(&LayoutBox) -> bool) -> Option<&'a LayoutBox> {
    if pred(b) {
        return Some(b);
    }
    b.children.iter().find_map(|c| find(c, pred))
}

fn table(root: &LayoutBox) -> &LayoutBox {
    find(root, &|b| matches!(b.kind, BoxKind::Table)).expect("table")
}

fn caption(t: &LayoutBox) -> &LayoutBox {
    t.children
        .iter()
        .find(|c| matches!(c.style.display, crate::style::Display::TableCaption))
        .expect("caption")
}

fn first_row(t: &LayoutBox) -> &LayoutBox {
    find(t, &|b| matches!(b.kind, BoxKind::TableRow)).expect("row")
}

const CELL: &str = "<tr><td style=\"width:100px;height:30px\">a</td></tr>";

#[test]
fn caption_side_default_top_places_caption_above_rows() {
    let root = lay(
        &format!("<table><caption style=\"height:20px\">c</caption>{CELL}</table>"),
        "body{margin:0}caption{padding:0}",
    );
    let t = table(&root);
    let cap = caption(t);
    let row = first_row(t);
    assert_eq!(cap.style.caption_side, CaptionSide::Top);
    assert!(cap.rect.y <= row.rect.y - 20.0 + 0.5, "caption {:?} row {:?}", cap.rect, row.rect);
    assert!((cap.rect.height - 20.0).abs() < 0.5);
    assert!(cap.rect.width > 99.0, "caption spans the table width, got {}", cap.rect.width);
}

#[test]
fn caption_side_bottom_places_caption_below_rows() {
    let root = lay(
        &format!("<table><caption style=\"height:20px\">c</caption>{CELL}</table>"),
        "body{margin:0}caption{caption-side:bottom;padding:0}",
    );
    let t = table(&root);
    let cap = caption(t);
    let row = first_row(t);
    assert_eq!(cap.style.caption_side, CaptionSide::Bottom);
    assert!(cap.rect.y >= row.rect.y + row.rect.height - 0.5, "caption {:?} row {:?}", cap.rect, row.rect);
    // Таблица охватывает и строки, и подпись.
    assert!(t.rect.y + t.rect.height >= cap.rect.y + cap.rect.height - 0.5);
}

#[test]
fn caption_adds_to_table_height_for_both_sides() {
    let plain = lay(&format!("<table>{CELL}</table>"), "body{margin:0}");
    let h0 = table(&plain).rect.height;
    for side in ["top", "bottom"] {
        let root = lay(
            &format!("<table><caption style=\"height:20px\">c</caption>{CELL}</table>"),
            &format!("body{{margin:0}}caption{{caption-side:{side};padding:0}}"),
        );
        let h = table(&root).rect.height;
        assert!((h - h0 - 20.0).abs() < 0.5, "{side}: {h} vs {h0}");
    }
}

#[test]
fn caption_side_is_inherited_and_keywords_parse() {
    assert_eq!(CaptionSide::parse("bottom"), Some(CaptionSide::Bottom));
    assert_eq!(CaptionSide::parse("left"), None);
    let root = lay(
        &format!("<table><caption>c</caption>{CELL}</table>"),
        "table{caption-side:bottom}",
    );
    assert_eq!(caption(table(&root)).style.caption_side, CaptionSide::Bottom);
}

#[test]
fn table_layout_keywords_and_not_inherited() {
    assert_eq!(TableLayout::parse("fixed"), Some(TableLayout::Fixed));
    assert_eq!(TableLayout::parse("auto"), Some(TableLayout::Auto));
    assert_eq!(TableLayout::parse("x"), None);
    let root = lay(
        &format!("<div class=\"d\"><table>{CELL}</table></div>"),
        ".d{table-layout:fixed}",
    );
    assert_eq!(table(&root).style.table_layout, TableLayout::Auto);
}

fn cell_widths(t: &LayoutBox) -> Vec<f32> {
    first_row(t)
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .map(|c| c.rect.width)
        .collect()
}

#[test]
fn fixed_layout_ignores_content_width() {
    let body = "<table><tr><td>averyveryverylongunbreakablewordthatwouldwidenacolumn</td><td></td></tr></table>";
    let fixed = lay(body, "body{margin:0}table{width:300px;table-layout:fixed;border-spacing:0}td{padding:0}");
    let w = cell_widths(table(&fixed));
    assert_eq!(w.len(), 2);
    assert!((w[0] - 150.0).abs() < 0.5 && (w[1] - 150.0).abs() < 0.5, "{w:?}");
}

#[test]
fn fixed_layout_uses_first_row_widths_only() {
    let body = "<table><tr><td style=\"width:50px\">a</td><td></td></tr>\
                <tr><td style=\"width:250px\">b</td><td></td></tr></table>";
    let fixed = lay(body, "body{margin:0}table{width:300px;table-layout:fixed;border-spacing:0}td{padding:0}");
    let w = cell_widths(table(&fixed));
    assert!((w[0] - 50.0).abs() < 0.5 && (w[1] - 250.0).abs() < 0.5, "{w:?}");
}

#[test]
fn fixed_layout_with_auto_width_falls_back_to_auto() {
    let body = "<table><tr><td style=\"width:50px\">a</td><td style=\"width:70px\"></td></tr></table>";
    let fixed = lay(body, "body{margin:0}table{table-layout:fixed;border-spacing:0}td{padding:0}");
    let w = cell_widths(table(&fixed));
    assert!((w[0] - 50.0).abs() < 0.5 && (w[1] - 70.0).abs() < 0.5, "{w:?}");
}
