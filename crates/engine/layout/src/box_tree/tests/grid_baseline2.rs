use lumen_core::geom::{Rect, Size};

// ── остаток baseline в grid (GRID-BASELINE-2) ──
//
// Без метрик шрифта: базовую линию блока задаёт пустой `inline-block` (нижняя кромка
// margin box, CSS 2.1 §10.8.1).

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

#[test]
fn auto_row_covers_item_margins() {
    // CSS Grid L1 §11.5: вклад item'а в авто-строку — его margin box (40 + 10 + 7).
    let r = rects(
        r#"<div id="g"><div id="a"></div></div>"#,
        "#g{display:grid}#a{height:40px;margin:10px 0 7px}",
        &["g", "a"],
    );
    // Позиции — относительно контейнера: его собственное поле схлопывается с `body` (BUG-1262).
    assert_eq!((r[0].height, r[1].y - r[0].y), (57.0, 10.0), "{r:?}");
}

#[test]
fn auto_row_takes_the_tallest_margin_box() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="b"></div></div>"#,
        "#g{display:grid;grid-template-columns:50px 50px}#a{height:40px}#b{height:40px;margin-top:20px}",
        &["g"],
    );
    assert_eq!(r[0].height, 60.0, "{r:?}");
}

#[test]
fn inline_grid_with_column_auto_flow_is_as_wide_as_all_its_columns() {
    // `grid-auto-flow: column` порождает по неявному столбцу на item: ширина — сумма, а не
    // самый широкий item (WPT `grid-container-baseline-001`).
    let r = rects(
        r#"<div id="g"><div style="width:150px;height:10px"></div><div style="width:75px;height:10px"></div><div style="width:100px;height:10px"></div></div>"#,
        "#g{display:inline-grid;grid-auto-flow:column}",
        &["g"],
    );
    assert_eq!(r[0].width, 325.0, "{r:?}");
}

#[test]
fn border_width_without_border_style_takes_no_room() {
    // CSS Backgrounds L3 §4.2: `border-style: none` → вычисленная ширина 0.
    let r = rects(
        r#"<div id="a">x</div><div id="b">x</div>"#,
        "#a{border-width:10px 5px 20px;width:30px;height:20px}#b{border:10px solid #000;width:30px;height:20px}",
        &["a", "b"],
    );
    assert_eq!((r[0].width, r[0].height), (30.0, 20.0), "{r:?}");
    assert_eq!((r[1].width, r[1].height), (50.0, 40.0), "{r:?}");
}

#[test]
fn grid_baseline_skips_an_empty_first_row() {
    // Первая строка пуста: линия контейнера — от первой строки, где есть items
    // (WPT `grid-baseline-004`: «items in the second row are evaluated»). Пустой item
    // сетки синтезирует линию по нижней кромке: 50 (строка 1) + 50 (строка 2).
    let html = r#"<div id="w" style="display:flex;align-items:baseline"><div id="x" style="width:10px;height:10px"></div><div id="g"><div id="a"></div></div></div>"#;
    let css = "#g{display:inline-grid;grid-template-rows:50px 50px;grid-template-columns:50px}#a{grid-row:2}";
    let r = rects(html, css, &["x", "g"]);
    // Линия сетки — на 100 от её верха (нижняя кромка item'а во 2-й строке); `x` встаёт на неё.
    assert_eq!(r[0].y, r[1].y + 90.0, "{r:?}");
}

#[test]
fn justify_baseline_aligns_orthogonal_items_on_one_vertical_line() {
    // Горизонтальная сетка, один столбец 200px; items `vertical-lr` с пустым содержимым —
    // центральная линия по середине ширины (20 и 10). `first` — общая линия у левого края:
    // a.x = 0, b.x = 10.
    let html = r#"<div id="g"><div id="a"></div><div id="b"></div></div>"#;
    let css = "#g{display:grid;grid-template-columns:200px;justify-items:first baseline}\
               #a,#b{writing-mode:vertical-lr;height:10px}#a{width:40px}#b{width:20px}";
    let r = rects(html, css, &["g", "a", "b"]);
    assert_eq!((r[1].x - r[0].x, r[2].x - r[0].x), (0.0, 10.0), "{r:?}");
    // `last` — общая линия у правого края: спуски 20 и 10 → линия на 180.
    let css = css.replace("first baseline", "last baseline");
    let r = rects(html, &css, &["g", "a", "b"]);
    assert_eq!((r[1].x - r[0].x, r[2].x - r[0].x), (160.0, 170.0), "{r:?}");
}

#[test]
fn opposite_block_flow_swaps_the_baseline_group_in_a_vertical_grid() {
    // `first baseline` бокса `vertical-rl` в сетке `vertical-lr` лежит у правого края и делит
    // группу конца строки с `last baseline` бокса `vertical-lr` (Align L3 §9.3).
    let html = r#"<div id="g"><div id="a"></div><div id="b"></div></div>"#;
    let css = "#g{display:grid;writing-mode:vertical-lr;grid-auto-flow:column;grid-template-rows:100px;\
               align-items:last baseline}\
               #a{writing-mode:vertical-rl;align-self:first baseline;width:60px;height:10px}\
               #b{writing-mode:vertical-lr;width:20px;height:10px}";
    let r = rects(html, css, &["g", "a", "b"]);
    // Спуски 30 и 10 → общая линия на 100 − 30 = 70.
    assert_eq!((r[1].x - r[0].x, r[2].x - r[0].x), (40.0, 60.0), "{r:?}");
}

#[test]
fn orthogonal_item_shrinks_to_content_unless_stretched() {
    let html = r#"<div id="g"><div id="a"><div style="height:10px;width:5px"></div></div></div>"#;
    let css = "#g{display:grid;grid-template-rows:120px;grid-template-columns:50px}#a{writing-mode:vertical-lr}";
    let stretched = rects(html, css, &["a"]);
    assert_eq!(stretched[0].height, 120.0, "{stretched:?}");
    let aligned = rects(html, &format!("{css}#g{{align-items:start}}"), &["a"]);
    assert_eq!(aligned[0].height, 10.0, "{aligned:?}");
}
