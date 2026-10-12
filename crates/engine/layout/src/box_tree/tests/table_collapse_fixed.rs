// BUG-1358: border-collapse + table-layout:fixed — соседние ячейки делят линию сетки.

#[test]
fn fixed_collapse_wide_side_borders_keep_grid() {
    let html = r#"<table style="border-collapse:collapse;table-layout:fixed;width:400px"><tr><td id="a" style="padding:0 24px"></td><td id="b" style="padding:0 24px;border-left:36px solid orange;border-right:36px solid orange;width:80px"></td><td id="c" style="padding:0 24px"></td></tr></table>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = super::super::layout(&doc, &sheet, lumen_core::geom::Size::new(800.0, 600.0));
    let b = super::find_by_id_all(&root, &doc, "b").expect("нет #b").rect;
    let c = super::find_by_id_all(&root, &doc, "c").expect("нет #c").rect;
    assert_eq!((b.x, b.width), (100.0, 200.0), "{b:?}");
    assert_eq!(c.x + c.width, 400.0, "{c:?}");
}
