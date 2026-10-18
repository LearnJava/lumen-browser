use lumen_core::geom::{Rect, Size};

// ── GRID-FR-ROWS: гибкие строки при неопределённой высоте контейнера (CSS Grid L1 §11.7) ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

/// WPT `flex-sizing-rows-indefinite-height-002`: `repeat(2, minmax(0, 0.4fr))`, item (100px) во
/// второй строке. Первый проход: размер `fr` = 100 → строки по 40px, высота 80px; второй проход
/// по этой высоте: 80 / max(0.8, 1) = 80 → строки по 32px.
#[test]
fn fractional_minmax_rows_are_resolved_in_two_passes() {
    let r = rects(
        r#"<div id="g"><div id="a"></div></div>"#,
        "#g{display:grid;width:100px;grid-template-rows:repeat(2,minmax(0,0.4fr))}\
         #a{grid-row:2/span 1;width:100px;height:100px}",
        &["g", "a"],
    );
    assert_eq!(r[0].height, 64.0, "{:?}", r[0]);
    assert_eq!(r[1].y, 32.0, "{:?}", r[1]);
}

/// `minmax(0, 1fr)` не растёт по содержимому как `auto`-минимум, но размер `fr` берёт у item'а.
#[test]
fn minmax_zero_fr_row_takes_the_item_height() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="b"></div></div>"#,
        "#g{display:grid;grid-template-rows:minmax(0,1fr) minmax(0,1fr)}\
         #a{height:30px}#b{height:50px}",
        &["g", "a", "b"],
    );
    assert_eq!(r[0].height, 100.0, "{:?}", r[0]);
    assert_eq!(r[2].y, 50.0, "{:?}", r[2]);
}

/// Пропорции `fr` сохраняются: `minmax(0, 2fr) minmax(0, 1fr)` и item'ы 40px / 40px — размер `fr`
/// 40 (большее из 40/2 и 40), строки 80 и 40.
#[test]
fn fr_ratio_is_kept_between_minmax_rows() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="b"></div></div>"#,
        "#g{display:grid;grid-template-rows:minmax(0,2fr) minmax(0,1fr)}\
         #a{height:40px}#b{height:40px}",
        &["g", "b"],
    );
    assert_eq!(r[0].height, 120.0, "{:?}", r[0]);
    assert_eq!(r[1].y, 80.0, "{:?}", r[1]);
}

/// Строка с фиксированной высотой не входит в гибкую долю, а пролёт её вычитает.
#[test]
fn spanning_item_subtracts_fixed_rows() {
    let r = rects(
        r#"<div id="g"><div id="a"></div></div>"#,
        "#g{display:grid;grid-template-rows:20px minmax(0,1fr)}#a{grid-row:1/span 2;height:100px}",
        &["g"],
    );
    assert_eq!(r[0].height, 100.0, "{:?}", r[0]);
}

/// WPT `flex-sizing-rows-indefinite-height`: `inline-grid` с `minmax(0, .5fr)` по обеим осям и
/// item 200×200. Ширина по max-content: 0.5 × 200 = 100; при этой ширине колонка 100 / max(0.5, 1)
/// × 0.5 = 50, строка — так же. Область 50×50, abspos 200% = 100×100 целиком под зелёным квадратом.
#[test]
fn half_fr_inline_grid_shrinks_abspos_area() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="p"></div></div>"#,
        "#g{display:inline-grid;position:relative;grid-template-columns:minmax(0,.5fr);\
         grid-template-rows:minmax(0,.5fr)}\
         #a{width:200px;height:200px}\
         #p{grid-area:1/1/2/2;position:absolute;width:200%;height:200%}",
        &["g", "p"],
    );
    assert_eq!((r[0].width, r[0].height), (100.0, 50.0), "{:?}", r[0]);
    assert_eq!((r[1].width, r[1].height), (100.0, 100.0), "{:?}", r[1]);
}

/// Max-content `inline-grid` с `1fr 2fr` по items 100px: размер `fr` = 100, колонки 100 и 200.
#[test]
fn inline_grid_fr_columns_keep_their_ratio() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="b"></div></div>"#,
        "#g{display:inline-grid;grid-template-columns:1fr 2fr}#a,#b{width:100px;height:10px}",
        &["g", "b"],
    );
    assert_eq!(r[0].width, 300.0, "{:?}", r[0]);
    assert_eq!(r[1].x, 100.0, "{:?}", r[1]);
}
