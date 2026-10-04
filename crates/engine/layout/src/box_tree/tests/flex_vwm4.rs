use lumen_core::geom::{Rect, Size};

// ── FLEX-VWM-4: растянутый flex-элемент — определённая высота для потомков ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

/// CSS Flexbox L1 §9.8: высота растянутого `align-self: stretch` элемента
/// определённая, поэтому `height: 100%` потомка резолвится от неё.
#[test]
fn percent_height_child_of_stretched_item_resolves() {
    let r = rects(
        r#"<div id="f"><div id="i"><div id="c"></div></div></div>"#,
        "#f{display:flex;height:100px}#i{flex:1;padding:5px 3px}#c{height:100%}",
        &["i", "c"],
    );
    assert_eq!(r[0].height, 100.0, "{:?}", r[0]);
    assert_eq!(r[1].height, 90.0, "{:?}", r[1]);
}

/// Элемент с `box-sizing: content-box`, margin и padding: растянутый размер
/// переводится в content-box, а x/ширина сохраняются.
#[test]
fn stretched_item_relayout_keeps_x_and_width() {
    let r = rects(
        r#"<div id="f"><div id="i"><div id="c"></div></div></div>"#,
        "#f{display:flex;width:200px;height:100px}\
         #i{width:120px;margin:4px 6px 3px 7px;padding:5px 9px;border:2px solid}\
         #c{height:100%}",
        &["i", "c"],
    );
    assert_eq!(r[0].x, 7.0, "{:?}", r[0]);
    assert_eq!(r[0].width, 120.0 + 18.0 + 4.0, "{:?}", r[0]);
    assert_eq!(r[0].height, 100.0 - 7.0, "{:?}", r[0]);
    assert_eq!(r[1].height, 100.0 - 7.0 - 14.0, "{:?}", r[1]);
}

/// Содержимое выше линии: растянутый элемент всё равно получает размер линии
/// (§9.4 шаг 11), если потомок просит процентную высоту.
#[test]
fn stretched_item_shrinks_to_the_line_when_percent_child_asks() {
    let r = rects(
        r#"<div id="f"><div id="i"><div id="c" style="height:100%"></div><div style="height:300px"></div></div></div>"#,
        "#f{display:flex;height:100px}",
        &["i"],
    );
    assert_eq!(r[0].height, 100.0, "{:?}", r[0]);
}
