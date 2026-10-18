use lumen_core::geom::{Rect, Size};

// ── TABLE-VWM: таблица в вертикальном writing-mode ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}table{{border-spacing:0}}td{{padding:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

#[test]
fn probe() {
    let html = r#"<table id="t" style="writing-mode:vertical-lr"><tr id="r1"><td id="a"></td><td id="b"></td><td id="c"></td></tr><tr id="r2"><td id="d"></td><td id="e"></td><td id="f"></td></tr></table>"#;
    let r = rects(html, "td{width:20px;height:10px}", &["t", "r1", "a", "b", "c", "r2", "d", "e", "f"]);
    for x in &r { eprintln!("{x:?}"); }
}
