use lumen_core::geom::{Rect, Size};

// ── §10.3.4: авто-горизонтальные маргины block-level replaced с определённой шириной ──
//
// Живой случай — портал nextera: логотип-шарики `display:block; width:180px;
// margin:0 auto 14px` стоял у левого края обёртки 820: `!is_replaced` в общем
// §10.3.3-решателе исключал replaced целиком, а §10.3.4 при определённой ширине
// решает авто-маргины так же (оба auto → равные половины, один auto → весь
// остаток). Авто-ширина replaced по-прежнему означает авто-маргины = 0 — такие
// случаи сюда не доходят (ограничитель определённой ширины).

fn rect_of(canvas_style: &str, extra_css: &str) -> Rect {
    let html = format!(
        r#"<div id="fx"><canvas id="c" width="10" height="10" style="{canvas_style}"></canvas></div>"#
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}#fx{{width:800px;height:200px}}{extra_css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "c").expect("нет #c").rect
}

#[test]
fn replaced_block_with_both_auto_margins_centers() {
    let r = rect_of("display:block;width:180px;margin:0 auto", "");
    assert_eq!(r.width, 180.0);
    assert_eq!(r.x, 310.0);
}

#[test]
fn replaced_block_with_left_auto_margin_takes_the_remainder() {
    let r = rect_of("display:block;width:180px;margin-left:auto", "");
    assert_eq!(r.width, 180.0);
    assert_eq!(r.x, 620.0);
}

#[test]
fn replaced_block_with_right_auto_margin_stays_at_start() {
    let r = rect_of("display:block;width:180px;margin-right:auto", "");
    assert_eq!(r.width, 180.0);
    assert_eq!(r.x, 0.0);
}

#[test]
fn plain_block_with_both_auto_margins_centers_as_before() {
    // Контроль соседнего пути (§10.3.3): обычный блок с теми же маргинами —
    // поведение не изменилось.
    let html = r#"<div id="fx"><div id="d" style="width:180px;height:20px;margin:0 auto"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}#fx{width:800px;height:200px}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let r = super::find_by_id_all(&root, &doc, "d").expect("нет #d").rect;
    assert_eq!(r.width, 180.0);
    assert_eq!(r.x, 310.0);
}
