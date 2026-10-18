use lumen_core::geom::{Rect, Size};

// ── GRID-RTL: `direction: rtl` в горизонтальной сетке ──
//
// Первый столбец у правого края content box, `start`/`end` разворачиваются, `left`/`right`
// остаются физическими. Ожидания выведены из CSS Grid L1 §7 и Box Alignment L3 §6 (в Edge
// те же правила дают те же координаты).

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

fn assert_rect(r: Rect, x: f32, y: f32, w: f32, h: f32) {
    assert_eq!((r.x, r.y, r.width, r.height), (x, y, w, h), "{r:?}");
}

const THREE: &str = r#"<div id="g"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
const BASE: &str = "#g{display:grid;direction:rtl;width:300px;grid-template-columns:50px 70px;\
                    grid-template-rows:30px 40px}";

/// Первый столбец — у правого края, следующий — левее.
#[test]
fn rtl_columns_run_right_to_left() {
    let r = rects(THREE, BASE, &["a", "b", "c"]);
    assert_rect(r[0], 250.0, 0.0, 50.0, 30.0);
    assert_rect(r[1], 180.0, 0.0, 70.0, 30.0);
    assert_rect(r[2], 250.0, 30.0, 50.0, 40.0);
}

/// `justify-content: left` — физическая сторона: дорожки прижаты к левому краю, порядок прежний.
#[test]
fn rtl_justify_content_left_is_physical() {
    let css = format!("{BASE}#g{{justify-content:left}}");
    let r = rects(THREE, &css, &["a", "b"]);
    assert_rect(r[0], 70.0, 0.0, 50.0, 30.0);
    assert_rect(r[1], 0.0, 0.0, 70.0, 30.0);
}

/// `justify-content: start` в rtl — правый край; `end` — левый.
#[test]
fn rtl_justify_content_start_end_are_logical() {
    let start = rects(THREE, &format!("{BASE}#g{{justify-content:start}}"), &["a"]);
    assert_rect(start[0], 250.0, 0.0, 50.0, 30.0);
    let end = rects(THREE, &format!("{BASE}#g{{justify-content:end}}"), &["a", "b"]);
    assert_rect(end[0], 70.0, 0.0, 50.0, 30.0);
    assert_rect(end[1], 0.0, 0.0, 70.0, 30.0);
}

/// `justify-self: start | end` — по inline-оси (справа налево), `left | right` — физические.
#[test]
fn rtl_justify_self_start_end_left_right() {
    let css = format!(
        "{BASE}#g>div{{width:20px}}#a{{justify-self:start}}#b{{justify-self:end}}#c{{justify-self:left}}"
    );
    let r = rects(THREE, &css, &["a", "b", "c"]);
    assert_rect(r[0], 280.0, 0.0, 20.0, 30.0);
    assert_rect(r[1], 180.0, 0.0, 20.0, 30.0);
    assert_rect(r[2], 250.0, 30.0, 20.0, 40.0);
    let right = rects(THREE, &format!("{css}#c{{justify-self:right}}"), &["c"]);
    assert_rect(right[0], 280.0, 30.0, 20.0, 40.0);
}

/// Авто-поля забирают свободное место с учётом направления: `margin-left: auto` прижимает к правому краю.
#[test]
fn rtl_auto_margin_left_pushes_item_right() {
    let r = rects(THREE, &format!("{BASE}#a{{width:20px;margin-left:auto}}"), &["a"]);
    assert_rect(r[0], 280.0, 0.0, 20.0, 30.0);
}

/// `position: relative` с `left` и `right` одновременно: в rtl побеждает `right` (CSS 2.1 §9.4.3);
/// проценты `top` считаются от высоты области.
#[test]
fn rtl_relative_offset_right_wins_and_top_percent_uses_height() {
    let css = format!("{BASE}#a{{position:relative;left:10px;right:20px;top:10%}}");
    let r = rects(THREE, &css, &["a"]);
    assert_rect(r[0], 230.0, 3.0, 50.0, 30.0);
}

/// Статическая позиция абсолютного потомка — у правого края content box.
#[test]
fn rtl_abspos_static_position_at_right_edge() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="p"></div></div>"#,
        &format!("{BASE}#g{{position:relative}}#p{{position:absolute;width:20px;height:10px}}"),
        &["p"],
    );
    assert_eq!(r[0].x, 280.0, "{:?}", r[0]);
}
