use lumen_core::geom::{Rect, Size};

// ── GRID-ABSPOS: абсолютно позиционированные дети grid ──
//
// Контейнер-CB: область по `grid-*-start/end`, `auto` — край padding box. Статическая позиция —
// начало области (не content box), `justify-self`/`align-self` выравнивают внутри неё. Ожидания
// выведены из CSS Grid L1 §9 и WPT `css-grid/abspos` (`grid-positioned-items-*`).

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

const HTML: &str = r#"<div id="g"><div id="i"></div><div id="p"></div></div>"#;
// Padding 10px: content box начинается в (10, 10), padding box — (0, 0) 130×90.
const BASE: &str = "#g{display:grid;position:relative;padding:10px;width:110px;height:70px;\
                    grid-template-columns:40px 60px;grid-template-rows:20px 30px;gap:5px 10px}\
                    #i{width:10px;height:10px}#p{position:absolute}";

/// Без `grid-*` область — весь padding box: `width/height: 100%` заполняют его.
#[test]
fn auto_lines_fill_padding_box() {
    let r = rects(HTML, &format!("{BASE}#p{{width:100%;height:100%}}"), &["p"]);
    assert_rect(r[0], 0.0, 0.0, 130.0, 90.0);
}

/// Статическая позиция без `grid-*` — начало padding box, а не content box.
#[test]
fn static_position_is_padding_box_origin() {
    let r = rects(HTML, &format!("{BASE}#p{{width:20px;height:20px}}"), &["p"]);
    assert_rect(r[0], 0.0, 0.0, 20.0, 20.0);
}

/// Заданные линии: область второго столбца и первой строки; зазоры на краях не входят.
#[test]
fn explicit_lines_give_area() {
    let css = format!("{BASE}#p{{grid-column:2 / 3;grid-row:1 / 2;width:100%;height:100%}}");
    let r = rects(HTML, &css, &["p"]);
    // Столбец 2: x = 10 + 40 + 10 = 60, ширина 60; строка 1: y = 10, высота 20.
    assert_rect(r[0], 60.0, 10.0, 60.0, 20.0);
}

/// `auto / N` — старт на краю padding box (не `N-1`); `N / auto` — конец на краю padding box.
#[test]
fn half_auto_lines_extend_to_padding_edge() {
    let css = format!("{BASE}#p{{grid-column:auto / 2;grid-row:2 / auto;width:100%;height:100%}}");
    let r = rects(HTML, &css, &["p"]);
    // Столбцы: от левого края padding box (0) до конца столбца 1 (10 + 40 = 50);
    // строки: от начала строки 2 (10 + 20 + 5 = 35) до нижнего края padding box (90).
    assert_rect(r[0], 0.0, 35.0, 50.0, 55.0);
}

/// Линия за неявной сеткой — `auto`.
#[test]
fn nonexistent_line_is_auto() {
    let css = format!("{BASE}#p{{grid-column:2 / 9;width:100%;height:20px}}");
    let r = rects(HTML, &css, &["p"]);
    assert_rect(r[0], 60.0, 0.0, 70.0, 20.0);
}

/// `justify-self` / `align-self` выравнивают статическую позицию внутри области.
#[test]
fn static_position_aligned_in_area() {
    let css = format!(
        "{BASE}#p{{grid-column:2 / 3;grid-row:2 / 3;width:20px;height:10px;justify-self:end;align-self:center}}"
    );
    let r = rects(HTML, &css, &["p"]);
    // Область: x 60..120, y 35..65.
    assert_rect(r[0], 100.0, 45.0, 20.0, 10.0);
}

/// Значения `justify-items`/`align-items` контейнера действуют на `auto`.
#[test]
fn container_items_alignment_applies() {
    let css = format!("{BASE}#g{{justify-items:center;align-items:end}}#p{{width:20px;height:10px}}");
    let r = rects(HTML, &css, &["p"]);
    // Область — padding box 130×90.
    assert_rect(r[0], 55.0, 80.0, 20.0, 10.0);
}

/// Ось с заданной вставкой не выравнивается: `left` фиксирует x от края области.
#[test]
fn inset_axis_is_not_aligned() {
    let css = format!("{BASE}#p{{grid-column:2;left:5px;width:20px;height:10px;justify-self:end}}");
    let r = rects(HTML, &css, &["p"]);
    assert_rect(r[0], 65.0, 0.0, 20.0, 10.0);
}

/// Контейнер не CB (нет `position`): область не применяется, позиция — начало content box.
#[test]
fn non_positioned_container_uses_content_box() {
    let css = format!("{BASE}#g{{position:static}}#p{{grid-column:2;width:20px;height:10px}}");
    let r = rects(HTML, &css, &["p"]);
    assert_rect(r[0], 10.0, 10.0, 20.0, 10.0);
}

/// `direction: rtl`: столбец 1 — справа, статическая позиция — правый край области.
#[test]
fn rtl_area_and_static_position() {
    let css = format!("{BASE}#g{{direction:rtl}}#p{{grid-column:1 / 2;width:20px;height:10px}}");
    let r = rects(HTML, &css, &["p"]);
    // Столбец 1 справа: x 80..120, бокс у правого края области; строка auto — от верха padding box.
    assert_rect(r[0], 100.0, 0.0, 20.0, 10.0);
}

/// Вертикальный режим: столбцы идут вниз, строки — справа налево (`vertical-rl`).
#[test]
fn vertical_rl_area() {
    let css = "#g{display:grid;position:relative;writing-mode:vertical-rl;width:100px;height:100px;\
               grid-template-columns:30px 40px;grid-template-rows:20px 30px}\
               #i{width:5px;height:5px}#p{position:absolute;grid-column:2 / 3;grid-row:1 / 2;width:100%;height:100%}";
    let r = rects(HTML, css, &["p"]);
    // Строка 1 — правый край (x 80..100), столбец 2 — y 30..70.
    assert_rect(r[0], 80.0, 30.0, 20.0, 40.0);
}
