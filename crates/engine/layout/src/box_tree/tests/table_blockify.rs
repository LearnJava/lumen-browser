use lumen_core::geom::{Rect, Size};

// BUG-1357: CSS 2.1 §9.7 — float / abspos на table-* блокифицируются.

fn rect(child_css: &str) -> Rect {
    let html = format!(
        r#"<div style="display:table;position:relative;width:300px"><div id="t" style="{child_css}"><div style="width:50px;height:50px"></div></div></div>"#
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "t").expect("нет #t").rect
}

#[test]
fn abspos_row_group_is_block() {
    let r = rect("display:table-row-group;position:absolute;width:40px;height:20px;left:0;top:0");
    assert_eq!((r.width, r.height), (40.0, 20.0), "{r:?}");
}

#[test]
fn abspos_row_is_block() {
    let r = rect("display:table-row;position:absolute;width:40px;height:20px;left:0;top:0");
    assert_eq!((r.width, r.height), (40.0, 20.0), "{r:?}");
}

#[test]
fn abspos_cell_is_block() {
    let r = rect("display:table-cell;position:absolute;width:40px;height:20px;left:0;top:0");
    assert_eq!((r.width, r.height), (40.0, 20.0), "{r:?}");
}

#[test]
fn floated_row_group_is_shrink_to_fit_right() {
    let html = r#"<div style="width:300px"><div id="t" style="display:table-row-group;float:right;width:40px;height:20px"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let r = super::find_by_id_all(&root, &doc, "t").expect("нет #t").rect;
    assert_eq!((r.x, r.width, r.height), (260.0, 40.0, 20.0), "{r:?}");
}
