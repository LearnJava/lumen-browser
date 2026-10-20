use lumen_core::geom::Size;

// ── grid-item с авторской шириной, растянутый по block-оси, сохраняет padding/border (BUG-1414) ──

fn size(css: &str) -> (f32, f32) {
    let html = r#"<div class="g"><div id="a" class="a"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let r = super::find_by_id_all(&root, &doc, "a").expect("нет #a").rect;
    (r.width, r.height)
}

#[test]
fn width_plus_padding_survives_row_stretch() {
    let css = ".g{display:grid;grid-template-columns:100px;grid-template-rows:10px}.a{width:60px;padding-left:6px}";
    assert_eq!(size(css), (66.0, 10.0));
}

#[test]
fn width_plus_border_survives_row_stretch() {
    let css = ".g{display:grid;grid-template-columns:100px;grid-template-rows:10px}.a{width:60px;border-left:2px solid}";
    assert_eq!(size(css), (62.0, 10.0));
}

#[test]
fn explicit_height_plus_padding_survives_width_stretch() {
    let css = ".g{display:grid;grid-template-columns:100px;justify-items:stretch}.a{height:20px;padding-top:4px;aspect-ratio:2/1}";
    let (w, h) = size(css);
    assert_eq!(h, 24.0);
    assert!(w > 0.0);
}

#[test]
fn without_row_height_width_is_unchanged() {
    let css = ".g{display:grid;grid-template-columns:100px}.a{width:60px;padding-left:6px}";
    assert_eq!(size(css).0, 66.0);
}
