use lumen_core::geom::{Rect, Size};

// ── align-items/align-self: baseline | last baseline (CSS Flexbox §8.5, Align §9) ──
//
// Раскладка без измерителя шрифтов: базовую линию элемента задаёт пустой
// `inline-block` — она у него на нижней кромке margin box (CSS 2.1 §10.8.1),
// так что числа в тестах считаются руками, без метрик шрифта.

/// `<span>` высотой `h` внутри блока: базовая линия блока — на `h` от его верха.
fn span(h: u32) -> String {
    format!(r#"<span style="display:inline-block;width:10px;height:{h}px"></span>"#)
}

/// Раскладывает `html` с таблицей `css`, возвращает прямоугольники боксов с
/// заданными `id`, в порядке запроса.
fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

#[test]
fn baseline_aligns_row_items_on_shared_baseline() {
    // Базовые линии 20 и 40: общая линия — на 40 от верха строки, элемент с
    // меньшим подъёмом опускается на разницу (§9.4 шаг 8).
    let html = format!(
        r#"<div id="c"><div id="a">{}</div><div id="b">{}</div></div>"#,
        span(20),
        span(40)
    );
    let r = rects(&html, "#c{display:flex;align-items:baseline;width:300px}", &["c", "a", "b"]);
    assert_eq!(r[1].y, 20.0, "a: {:?}", r[1]);
    assert_eq!(r[2].y, 0.0, "b: {:?}", r[2]);
    assert_eq!(r[0].height, 40.0, "линия вмещает обе: {:?}", r[0]);
}

#[test]
fn baseline_group_extent_counts_descent_below_the_baseline() {
    // a: подъём 20, спуск 30 (padding-bottom); b: подъём 40, спуск 0. Размер
    // линии — max подъём + max спуск = 40 + 30, а не max высот (50).
    let html = format!(
        r#"<div id="c"><div id="a" style="padding-bottom:30px">{}</div><div id="b">{}</div></div>"#,
        span(20),
        span(40)
    );
    let r = rects(&html, "#c{display:flex;align-items:baseline;width:300px}", &["c", "a", "b"]);
    assert_eq!(r[0].height, 70.0, "высота линии: {:?}", r[0]);
    assert_eq!(r[1].y, 20.0, "a: {:?}", r[1]);
    assert_eq!(r[2].y, 0.0, "b: {:?}", r[2]);
}

#[test]
fn last_baseline_group_is_flush_with_cross_end() {
    // Контейнер выше линии: `baseline` прижимает группу к верху, `last baseline`
    // — к низу (§8.5). a: подъём 50 (padding-top), спуск 0; b: подъём 10, спуск 30.
    let items = format!(
        r#"<div id="a" style="padding-top:30px">{}</div><div id="b" style="padding-bottom:30px">{}</div>"#,
        span(20),
        span(10)
    );
    let html = format!(r#"<div id="c">{items}</div>"#);
    let first = rects(&html, "#c{display:flex;align-items:baseline;width:300px;height:200px}", &["a", "b"]);
    assert_eq!((first[0].y, first[1].y), (0.0, 40.0), "first baseline: {first:?}");
    let last = rects(&html, "#c{display:flex;align-items:last baseline;width:300px;height:200px}", &["a", "b"]);
    assert_eq!((last[0].y, last[1].y), (120.0, 160.0), "last baseline: {last:?}");
}

#[test]
fn align_self_baseline_mixes_with_other_alignments() {
    // Только помеченный `align-self: baseline` участвует в группе; остальной
    // элемент выровнен по `center` и в группу не входит.
    let html = format!(
        r#"<div id="c"><div id="a">{}</div><div id="b" style="align-self:baseline">{}</div><div id="d" style="height:10px;align-self:center"></div></div>"#,
        span(20),
        span(40)
    );
    let r = rects(&html, "#c{display:flex;align-items:flex-start;width:300px;height:100px}", &["a", "b", "d"]);
    assert_eq!(r[0].y, 0.0, "a (flex-start): {:?}", r[0]);
    assert_eq!(r[1].y, 0.0, "b — единственный в группе: {:?}", r[1]);
    assert_eq!(r[2].y, 45.0, "d (center): {:?}", r[2]);
}

#[test]
fn flex_line_cross_size_includes_item_margins() {
    // Размер линии — внешний (с полями) поперечный размер элемента (§9.4 шаг 7):
    // контейнер с auto-высотой обязан вмещать margin-top/bottom элемента.
    let r = rects(
        r#"<div id="c"><div id="a"></div></div>"#,
        "#c{display:flex;width:300px} #a{width:50px;height:10px;margin:20px 0 30px}",
        &["c", "a"],
    );
    assert_eq!(r[0].height, 60.0, "контейнер: {:?}", r[0]);
    // Относительно контейнера: его собственный `y` двигает схлопывание поля
    // первого flex-элемента с предком (отдельный дефект `establishes_bfc`).
    assert_eq!(r[1].y - r[0].y, 20.0, "элемент: {:?}", r[1]);
}

#[test]
fn baseline_alignment_respects_item_margins() {
    // Базовая линия отсчитывается от верха margin box: margin-top сдвигает её вниз.
    let html = format!(
        r#"<div id="c"><div id="a" style="margin-top:15px">{}</div><div id="b">{}</div></div>"#,
        span(20),
        span(30)
    );
    let r = rects(&html, "#c{display:flex;align-items:baseline;width:300px}", &["c", "a", "b"]);
    // a: подъём 15 + 20 = 35, b: 30 → линия 35; a.y = margin-top, b опущен на 5
    // (относительно контейнера — см. комментарий в тесте про поля).
    assert_eq!(r[1].y - r[0].y, 15.0, "a: {:?}", r[1]);
    assert_eq!(r[2].y - r[0].y, 5.0, "b: {:?}", r[2]);
}

#[test]
fn auto_cross_margin_takes_item_out_of_baseline_group() {
    // §8.1: `margin-top: auto` приоритетнее baseline-выравнивания.
    let html = format!(
        r#"<div id="c"><div id="a" style="margin-top:auto">{}</div><div id="b">{}</div></div>"#,
        span(20),
        span(40)
    );
    let r = rects(&html, "#c{display:flex;align-items:baseline;width:300px;height:100px}", &["a", "b"]);
    assert_eq!(r[0].y, 80.0, "a прижат к низу авто-полем: {:?}", r[0]);
    assert_eq!(r[1].y, 0.0, "b: {:?}", r[1]);
}

#[test]
fn nested_flex_exports_first_item_baseline() {
    // §8.5: у вложенного flex-контейнера без baseline-выровненных детей базовая
    // линия — линия его первого (визуально верхнего-левого) элемента.
    let html = format!(
        r#"<div id="c"><div id="a">{}</div><div id="n" style="display:flex"><div>{}</div><div>{}</div></div></div>"#,
        span(40),
        span(10),
        span(30)
    );
    let r = rects(&html, "#c{display:flex;align-items:baseline;width:300px}", &["a", "n"]);
    // Базовая линия вложенного — 10 (первый элемент), у a — 40: n опущен на 30.
    assert_eq!(r[0].y, 0.0, "a: {:?}", r[0]);
    assert_eq!(r[1].y, 30.0, "n: {:?}", r[1]);
}

#[test]
fn nested_flex_row_reverse_exports_visually_first_item_baseline() {
    // В `row-reverse` визуально первый (левый) элемент — последний в DOM.
    let html = format!(
        r#"<div id="c"><div id="a">{}</div><div id="n" style="display:flex;flex-direction:row-reverse"><div>{}</div><div>{}</div></div></div>"#,
        span(40),
        span(10),
        span(30)
    );
    let r = rects(&html, "#c{display:flex;align-items:baseline;width:300px}", &["a", "n"]);
    // Левый элемент — второй по DOM, его базовая линия 30: n опущен на 10.
    assert_eq!(r[1].y, 10.0, "n: {:?}", r[1]);
}

#[test]
fn nested_flex_baseline_aligned_children_define_container_baseline() {
    // Если на первой линии есть выровненные по базовой линии элементы, базовая
    // линия контейнера — их общая линия, а не линия первого элемента.
    let html = format!(
        r#"<div id="c"><div id="a">{}</div><div id="n" style="display:flex;align-items:baseline"><div>{}</div><div>{}</div></div></div>"#,
        span(40),
        span(10),
        span(30)
    );
    let r = rects(&html, "#c{display:flex;align-items:baseline;width:300px}", &["a", "n"]);
    // Внутри n общая линия — 30 (подъём второго); у a — 40: n опущен на 10.
    assert_eq!(r[1].y, 10.0, "n: {:?}", r[1]);
}

#[test]
fn scroll_container_baseline_is_clamped_to_border_box() {
    // Переполнение не двигает базовую линию контейнера прокрутки: линия
    // содержимого (120) обрезается до нижней кромки border box (30).
    let html = format!(
        r#"<div id="c"><div id="a" style="overflow:hidden;height:30px"><div style="margin-top:100px">{}</div></div><div id="b">{}</div></div>"#,
        span(20),
        span(10)
    );
    let r = rects(&html, "#c{display:flex;align-items:baseline;width:300px}", &["a", "b"]);
    assert_eq!(r[0].y, 0.0, "a: {:?}", r[0]);
    assert_eq!(r[1].y, 20.0, "b (линия a = 30, у b — 10): {:?}", r[1]);
}

#[test]
fn replaced_item_synthesizes_baseline_from_bottom_edge() {
    // У пустого блока своей базовой линии нет: она синтезируется по нижней
    // кромке border box (Align §9.1) — блок высотой 25 встаёт низом на линию.
    let html = format!(
        r#"<div id="c"><div id="a" style="height:25px"></div><div id="b">{}</div></div>"#,
        span(40)
    );
    let r = rects(&html, "#c{display:flex;align-items:baseline;width:300px}", &["a", "b"]);
    assert_eq!(r[0].y, 15.0, "a: {:?}", r[0]);
    assert_eq!(r[1].y, 0.0, "b: {:?}", r[1]);
}

#[test]
fn column_baseline_falls_back_to_start_and_does_not_stretch() {
    // Во flex-колонке поперечная ось — строчная, базовой группы нет: `baseline`
    // работает как `start` (без растяжения), `last baseline` — как `end`.
    let html = r#"<div id="c"><div id="a" style="width:60px;height:10px"></div><div id="b" style="height:10px"><span style="display:inline-block;width:40px;height:10px"></span></div></div>"#;
    let first = rects(html, "#c{display:flex;flex-direction:column;align-items:baseline;width:200px}", &["a", "b"]);
    assert_eq!(first[0].x, 0.0, "a: {:?}", first[0]);
    assert_eq!(first[1].width, 40.0, "b не растянут: {:?}", first[1]);
    let last = rects(html, "#c{display:flex;flex-direction:column;align-items:last baseline;width:200px}", &["a", "b"]);
    assert_eq!(last[0].x, 140.0, "a прижат к концу: {:?}", last[0]);
    assert_eq!(last[1].x, 160.0, "b прижат к концу: {:?}", last[1]);
}

#[test]
fn align_value_last_baseline_round_trips_through_computed_style() {
    let html = r#"<div id="c"></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("#c{display:flex;align-items:last baseline;align-self:first baseline}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let c = super::find_by_id_all(&root, &doc, "c").expect("c");
    assert_eq!(c.style.align_items, crate::style::AlignValue::LastBaseline);
    assert_eq!(c.style.align_self, crate::style::AlignValue::Baseline);
}

#[test]
fn place_shorthands_keep_two_word_baseline_as_one_token() {
    use crate::style::AlignValue;
    let html = r#"<div id="c"><div id="d"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        "#c{display:flex;place-items:last baseline start} #d{place-self:first baseline}",
    );
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let c = super::find_by_id_all(&root, &doc, "c").expect("c");
    assert_eq!(c.style.align_items, AlignValue::LastBaseline);
    assert_eq!(c.style.justify_items, AlignValue::Start);
    let d = super::find_by_id_all(&root, &doc, "d").expect("d");
    assert_eq!(d.style.align_self, AlignValue::Baseline);
    assert_eq!(d.style.justify_self, AlignValue::Baseline);
}
