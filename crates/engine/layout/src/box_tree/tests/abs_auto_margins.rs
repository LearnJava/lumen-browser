use lumen_core::geom::{Rect, Size};

// ── BUG-1282: auto-поля абсолютного бокса с двумя инсетами по оси ──

fn rect_of(css: &str) -> Rect {
    let doc = lumen_html_parser::parse(r#"<div id="p"><div id="c"></div></div>"#);
    let sheet = lumen_css_parser::parse(&format!(
        "body{{margin:0}}#p{{position:relative;width:100px;height:100px}}#c{{position:absolute;width:70px;height:70px;{css}}}"
    ));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "c").expect("нет #c").rect
}

#[test]
fn inset_zero_margin_auto_centers() {
    let r = rect_of("inset:0;margin:auto");
    assert_eq!((r.x, r.y), (15.0, 15.0), "{r:?}");
}

#[test]
fn horizontal_auto_margins_split_evenly() {
    let r = rect_of("left:0;right:0;margin:0 auto");
    assert_eq!((r.x, r.y), (15.0, 0.0), "{r:?}");
}

#[test]
fn single_auto_margin_takes_remainder() {
    let r = rect_of("left:0;right:0;margin-left:auto;margin-right:5px");
    assert_eq!(r.x, 25.0, "{r:?}");
}

#[test]
fn no_auto_margin_stays_at_start() {
    let r = rect_of("left:0;right:0;margin-left:3px");
    assert_eq!(r.x, 3.0, "{r:?}");
}
