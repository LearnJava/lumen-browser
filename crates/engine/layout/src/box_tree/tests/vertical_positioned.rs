use lumen_core::geom::{Rect, Size};

// ── BUG-1276: `position: absolute` / `relative` внутри вертикального `writing-mode` ──

fn rects(css: &str, ids: &[&str]) -> Vec<Rect> {
    let html = r#"<div id="cb"><div id="a"></div><div id="b"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

const CB: &str = "#cb{position:relative;writing-mode:vertical-lr;width:320px;height:320px}\
                  #a{position:absolute;width:50px;height:50px}";

fn a_rect(inset: &str) -> Rect {
    rects(&format!("{CB}#a{{{inset}}}"), &["a"])[0]
}

#[test]
fn absolute_insets_apply_in_vertical_cb() {
    let r = a_rect("top:10px");
    assert_eq!((r.x, r.y), (0.0, 10.0), "{r:?}");
    let r = a_rect("bottom:10px");
    assert_eq!((r.x, r.y), (0.0, 260.0), "{r:?}");
    let r = a_rect("left:10px");
    assert_eq!((r.x, r.y), (10.0, 0.0), "{r:?}");
    let r = a_rect("right:10px");
    assert_eq!((r.x, r.y), (260.0, 0.0), "{r:?}");
}

#[test]
fn top_and_bottom_define_the_height() {
    let r = rects(
        "#cb{position:relative;writing-mode:vertical-lr;width:320px;height:320px}\
         #a{position:absolute;width:50px;top:10px;bottom:20px}",
        &["a"],
    )[0];
    assert_eq!((r.y, r.height), (10.0, 290.0), "{r:?}");
}

#[test]
fn absolute_child_takes_no_room_in_the_flow() {
    let r = rects(&format!("{CB}#a{{top:5px}}#b{{width:30px;height:30px}}"), &["b"])[0];
    assert_eq!(r.x, 0.0, "{r:?}");
}

#[test]
fn relative_offset_applies_in_vertical_flow() {
    let r = rects(
        "#cb{writing-mode:vertical-lr;width:320px;height:320px}\
         #a{width:50px;height:50px}#b{position:relative;left:20px;top:10px;width:30px;height:30px}",
        &["b"],
    )[0];
    assert_eq!((r.x, r.y), (70.0, 10.0), "{r:?}");
}
