use lumen_core::geom::{Rect, Size};

// ── BUG-1351: shrink-to-fit ширина учитывает горизонтальные margin блочных детей ──

fn rect_of(css: &str, id: &str) -> Rect {
    let html = r#"<div id="o"><div id="c"></div></div>"#;
    let css = format!("body{{margin:0}}{css}");
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect
}

#[test]
fn absolute_container_includes_child_margins() {
    let css = "#o{position:absolute}#c{margin:0 40px;width:10px;height:10px}";
    assert_eq!(rect_of(css, "o").width, 90.0);
}

#[test]
fn float_container_includes_child_margins() {
    let css = "#o{float:left}#c{margin:0 40px;width:10px;height:10px}";
    assert_eq!(rect_of(css, "o").width, 90.0);
}

// ── BUG-1484: процентный горизонтальный padding abspos-бокса при width:auto ──

#[test]
fn absolute_auto_width_resolves_percent_horizontal_padding() {
    let html = r#"<div id="cb"><div id="a"></div></div>"#;
    let css = "body{margin:0}#cb{position:relative;width:50px;height:100px}\
               #a{position:absolute;left:50px;top:0;height:100px;padding-left:50%;padding-right:50%}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("нет #a").rect;
    assert_eq!(a.width, 50.0);
}
