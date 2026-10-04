use lumen_core::geom::{Rect, Size};

// ── vertical-align ячеек таблицы (CSS 2.1 §17.5.3) и базовая линия таблицы ──
//
// Без метрик шрифта: базовую линию ячейки задаёт пустой `inline-block` — она у
// него на нижней кромке margin box (CSS 2.1 §10.8.1).

fn span(h: u32) -> String {
    format!(r#"<span style="display:inline-block;width:10px;height:{h}px"></span>"#)
}

/// Прямоугольники боксов с заданными `id`, в порядке запроса.
fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}table{{border-spacing:0}}td{{padding:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

fn block(h: u32, id: &str) -> String {
    format!(r#"<div id="{id}" style="height:{h}px"></div>"#)
}

#[test]
fn cell_content_follows_vertical_align_top_middle_bottom() {
    let html = format!(
        r#"<table><tr><td style="height:100px;vertical-align:top">{}</td><td style="vertical-align:middle">{}</td><td style="vertical-align:bottom">{}</td></tr></table>"#,
        block(20, "t"),
        block(20, "m"),
        block(20, "b"),
    );
    let r = rects(&html, "", &["t", "m", "b"]);
    assert_eq!((r[0].y, r[1].y, r[2].y), (0.0, 40.0, 80.0), "{r:?}");
}

#[test]
fn cell_defaults_to_middle_and_valign_attribute_overrides() {
    let html = format!(
        r#"<table><tr><td style="height:100px">{}</td><td valign="bottom">{}</td><td valign="TOP">{}</td></tr></table>"#,
        block(20, "d"),
        block(20, "b"),
        block(20, "t"),
    );
    let r = rects(&html, "", &["d", "b", "t"]);
    assert_eq!((r[0].y, r[1].y, r[2].y), (40.0, 80.0, 0.0), "{r:?}");
}

#[test]
fn row_valign_is_inherited_by_cells() {
    let html = format!(
        r#"<table><tr valign="bottom"><td style="height:100px">{}</td></tr></table>"#,
        block(20, "b"),
    );
    assert_eq!(rects(&html, "", &["b"])[0].y, 80.0);
}

#[test]
fn baseline_cells_share_baseline_and_row_covers_ascent_plus_descent() {
    // a: подъём 20, спуск 30 (padding-bottom); b: подъём 40. Общая линия — на 40,
    // высота строки — 40 + 30, а не max высот (50).
    let html = format!(
        r#"<table id="t"><tr id="r"><td id="a" style="vertical-align:baseline;padding-bottom:30px">{}</td><td id="b" style="vertical-align:baseline">{}</td></tr></table>"#,
        span(20),
        span(40)
    );
    let r = rects(&html, "", &["r", "a", "b"]);
    assert_eq!(r[0].height, 70.0, "строка: {:?}", r[0]);
    assert_eq!((r[1].height, r[2].height), (70.0, 70.0), "ячейки заполняют строку");
}

#[test]
fn baseline_cell_content_is_pushed_down_to_the_shared_baseline() {
    let html = format!(
        r#"<table><tr><td style="vertical-align:baseline"><div id="a">{}</div></td><td style="vertical-align:baseline"><div id="b">{}</div></td></tr></table>"#,
        span(20),
        span(40)
    );
    let r = rects(&html, "", &["a", "b"]);
    assert_eq!((r[0].y, r[1].y), (20.0, 0.0), "{r:?}");
}

#[test]
fn rowspan_cell_aligns_content_over_all_spanned_rows() {
    let html = format!(
        r#"<table><tr><td rowspan="2" style="vertical-align:bottom">{}</td><td style="height:30px"></td></tr><tr><td style="height:70px"></td></tr></table>"#,
        block(20, "s"),
    );
    assert_eq!(rects(&html, "", &["s"])[0].y, 80.0);
}

#[test]
fn explicit_row_height_stretches_cells() {
    let html = format!(
        r#"<table><tr style="height:60px"><td id="c">{}</td></tr></table>"#,
        block(20, "m"),
    );
    let r = rects(&html, "", &["c", "m"]);
    assert_eq!((r[0].height, r[1].y), (60.0, 20.0), "{r:?}");
}

#[test]
fn table_baseline_is_first_row_baseline_and_ignores_caption() {
    // Элемент flex-строки с `align-items: baseline` садится на базовую линию
    // таблицы: первая строка — подъём 40 (высокая ячейка), подпись не считается.
    let html = format!(
        r#"<div id="c"><div id="x">{}</div><table id="t"><caption style="height:15px"></caption><tr><td style="vertical-align:baseline">{}</td><td style="vertical-align:baseline">{}</td></tr></table></div>"#,
        span(25),
        span(40),
        span(10)
    );
    let r = rects(&html, "#c{display:flex;align-items:first baseline;width:300px}", &["x", "t"]);
    // Базовая линия таблицы: 15 (подпись) + 40; у x — 25 → x.y = 55 - 25 = 30.
    assert_eq!((r[0].y, r[1].y), (30.0, 0.0), "{r:?}");
}

#[test]
fn last_baseline_of_table_is_the_baseline_of_its_last_row() {
    let html = format!(
        r#"<div id="c"><div id="x">{}</div><table id="t"><tr><td style="vertical-align:baseline;height:50px">{}</td></tr><tr><td style="vertical-align:baseline">{}</td></tr></table></div>"#,
        span(10),
        span(20),
        span(30)
    );
    let r = rects(&html, "#c{display:flex;align-items:last baseline;width:300px}", &["x", "t"]);
    // Последняя строка начинается на 50, её базовая линия — 30: 80; у x — 10.
    assert_eq!((r[0].y, r[1].y), (70.0, 0.0), "{r:?}");
}
