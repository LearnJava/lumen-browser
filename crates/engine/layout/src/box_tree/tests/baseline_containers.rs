use lumen_core::geom::{Rect, Size};

// ── базовая линия grid / multicol / line-clamp (CSS Grid §6, Align §9.1) ──
//
// Без метрик шрифта: базовую линию блока задаёт пустой `inline-block` — она у
// него на нижней кромке margin box (CSS 2.1 §10.8.1).

fn span(h: u32) -> String {
    format!(r#"<span style="display:inline-block;width:10px;height:{h}px"></span>"#)
}

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

#[test]
fn grid_row_baseline_group_aligns_items_on_the_shared_baseline() {
    // Подъёмы 20 и 40: общая линия на 40 от верха строки, строка — 40.
    let html = format!(r#"<div id="g"><div id="a">{}</div><div id="b">{}</div></div>"#, span(20), span(40));
    let r = rects(&html, "#g{display:grid;grid-template-columns:50px 50px;align-items:baseline}", &["g", "a", "b"]);
    assert_eq!((r[1].y, r[2].y, r[0].height), (20.0, 0.0, 40.0), "{r:?}");
}

#[test]
fn grid_baseline_group_row_covers_ascent_plus_descent() {
    // a: подъём 20, спуск 30; b: подъём 40. Строка — 40 + 30.
    let html = format!(
        r#"<div id="g"><div id="a" style="padding-bottom:30px">{}</div><div id="b">{}</div></div>"#,
        span(20),
        span(40)
    );
    let r = rects(&html, "#g{display:grid;grid-template-columns:50px 50px;align-items:baseline}", &["g"]);
    assert_eq!(r[0].height, 70.0, "{r:?}");
}

#[test]
fn grid_last_baseline_group_sits_on_the_row_end() {
    let html = format!(r#"<div id="g"><div id="a">{}</div><div id="b">{}</div></div>"#, span(20), span(40));
    let r = rects(
        &html,
        "#g{display:grid;grid-template:100px / 50px 50px;align-items:last baseline}",
        &["a", "b"],
    );
    assert_eq!((r[0].y, r[1].y), (80.0, 60.0), "{r:?}");
}

#[test]
fn grid_first_baseline_comes_from_the_first_row_group() {
    let html = format!(
        r#"<div id="c"><div id="x">{}</div><div id="g"><div>{}</div><div>{}</div></div></div>"#,
        span(15),
        span(20),
        span(40)
    );
    let css = "#c{display:flex;align-items:first baseline}#g{display:grid;grid-template-columns:50px 50px;align-items:baseline}";
    let r = rects(&html, css, &["x", "g"]);
    // Линия группы — 40 от верха сетки; у x — 15.
    assert_eq!((r[0].y, r[1].y), (25.0, 0.0), "{r:?}");
}

#[test]
fn grid_without_baseline_participants_uses_first_item_of_first_row() {
    let html = format!(
        r#"<div id="c"><div id="x">{}</div><div id="g" style="padding-top:5px"><div>{}</div><div>{}</div></div></div>"#,
        span(15),
        span(20),
        span(40)
    );
    let css = "#c{display:flex;align-items:first baseline}#g{display:grid;grid-template-columns:50px 50px;align-items:start}";
    let r = rects(&html, css, &["x"]);
    // Первый item: 5 (padding) + 20.
    assert_eq!(r[0].y, 10.0, "{r:?}");
}

#[test]
fn grid_last_baseline_comes_from_the_last_row() {
    let html = format!(
        r#"<div id="c"><div id="x">{}</div><div id="g"><div>{}</div><div>{}</div></div></div>"#,
        span(15),
        span(20),
        span(40)
    );
    let css = "#c{display:flex;align-items:last baseline}#g{display:grid;grid-template:30px 50px / 50px;align-items:start}";
    let r = rects(&html, css, &["x"]);
    // Последняя строка начинается на 30, её item — подъём 40: линия 70; у x — 15.
    assert_eq!(r[0].y, 55.0, "{r:?}");
}

#[test]
fn multicol_baseline_follows_column_spanners() {
    let html = format!(
        r#"<div id="c"><div id="x">{}</div><div id="m"><div style="column-span:all">{}</div><div>{}</div></div></div>"#,
        span(15),
        span(30),
        span(10)
    );
    let css = "#m{columns:2}";
    let first = rects(&html, &format!("#c{{display:flex;align-items:first baseline}}{css}"), &["x"]);
    // Первая линия — у спаннера в начале: 30; у x — 15.
    assert_eq!(first[0].y, 15.0, "{first:?}");
    let last = rects(&html, &format!("#c{{display:flex;align-items:last baseline}}{css}"), &["x"]);
    // Последний сегмент — колонки под спаннером (30): 30 + 10; у x — 15.
    assert_eq!(last[0].y, 25.0, "{last:?}");
}

#[test]
fn line_clamp_last_baseline_is_the_last_visible_line() {
    let rows = format!("{0}<br>{0}<br>{0}<br>{0}", span(10));
    let html = format!(r#"<div id="c"><div id="x">{}</div><div id="k">{rows}</div></div>"#, span(5));
    let css = "#c{display:flex;align-items:last baseline;line-height:0}\
               #k{display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:2;overflow:hidden}";
    let r = rects(&html, css, &["x"]);
    // Видимы две строки по 10: последняя линия — 20; у x — 5.
    assert_eq!(r[0].y, 15.0, "{r:?}");
}

fn wspan(w: u32) -> String {
    format!(r#"<span style="display:inline-block;width:{w}px;height:10px"></span>"#)
}

#[test]
fn vertical_items_align_on_their_central_baselines_in_a_column_flex() {
    // Базовая линия вертикального блока — середина строки по горизонтали: у a
    // 30 (padding) + 10, у b — 5; группа `first baseline` бокса `vertical-lr`
    // тянется к левому краю, наибольший подъём 40.
    let html = format!(
        r#"<div id="c"><div id="a" style="writing-mode:vertical-lr;padding-left:30px;line-height:0">{}</div><div id="b" style="writing-mode:vertical-lr;line-height:0">{}</div></div>"#,
        wspan(20),
        wspan(10)
    );
    let r = rects(&html, "#c{display:flex;flex-direction:column;align-items:baseline;width:200px}", &["a", "b"]);
    assert_eq!((r[0].x, r[1].x), (0.0, 35.0), "{r:?}");
}

#[test]
fn first_baseline_of_vertical_rl_shares_a_group_with_last_baseline_of_vertical_lr() {
    // `first` тянется к началу блока самого бокса: справа у `vertical-rl`; `last` у
    // `vertical-lr` — тоже справа. Общая линия: правый край минус наибольший спуск.
    let html = format!(
        r#"<div id="c"><div id="a" style="writing-mode:vertical-rl;align-self:first baseline;line-height:0">{}</div><div id="b" style="writing-mode:vertical-lr;align-self:last baseline;line-height:0">{}</div></div>"#,
        wspan(20),
        wspan(40)
    );
    let r = rects(&html, "#c{display:flex;flex-direction:column;width:200px}", &["a", "b"]);
    assert_eq!((r[0].x, r[1].x), (170.0, 160.0), "{r:?}");
}

#[test]
fn vertical_flex_row_baseline_alignment_uses_the_horizontal_cross_axis() {
    let html = format!(
        r#"<div id="c"><div id="a" style="padding-left:6px;line-height:0">{}</div><div id="b" style="line-height:0">{}</div></div>"#,
        wspan(20),
        wspan(40)
    );
    let r = rects(
        &html,
        "#c{display:flex;writing-mode:vertical-lr;align-items:baseline;height:200px}",
        &["a", "b"],
    );
    // a: 6 + 10 = 16, b: 20 — группа у левого края, подъём 20.
    assert_eq!((r[0].x, r[1].x), (4.0, 0.0), "{r:?}");
}
