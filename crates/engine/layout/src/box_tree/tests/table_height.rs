use lumen_core::geom::{Rect, Size};

// ── высота таблицы делится по строкам (CSS 2.1 §17.5.3, TABLE-HEIGHT) ──

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
fn single_row_takes_the_whole_table_height_and_centers_its_content() {
    let html = format!(r#"<table id="t" style="height:200px"><tr id="r"><td id="c">{}</td></tr></table>"#, block(20, "b"));
    let r = rects(&html, "", &["t", "r", "c", "b"]);
    assert_eq!(r[0].height, 200.0, "{r:?}");
    assert_eq!((r[1].y, r[1].height), (0.0, 200.0), "строка: {:?}", r[1]);
    assert_eq!(r[2].height, 200.0, "ячейка: {:?}", r[2]);
    assert_eq!(r[3].y, 90.0, "vertical-align: middle по умолчанию: {:?}", r[3]);
}

#[test]
fn extra_height_is_shared_in_proportion_to_row_heights() {
    // Строки 20 и 60, излишек 100 → 20 + 25 и 60 + 75.
    let html = format!(
        r#"<table id="t" style="height:180px"><tr id="r1"><td style="vertical-align:top">{}</td></tr><tr id="r2"><td id="c2" style="vertical-align:top">{}</td></tr></table>"#,
        block(20, "a"),
        block(60, "b"),
    );
    let r = rects(&html, "", &["t", "r1", "r2", "c2", "b"]);
    assert_eq!(r[0].height, 180.0);
    assert_eq!((r[1].y, r[1].height), (0.0, 45.0), "{r:?}");
    assert_eq!((r[2].y, r[2].height), (45.0, 135.0), "{r:?}");
    assert_eq!((r[3].y, r[3].height), (45.0, 135.0), "ячейка второй строки едет вместе со строкой");
    assert_eq!(r[4].y, 45.0, "содержимое сверху");
}

#[test]
fn table_is_never_shorter_than_its_rows() {
    let html = format!(r#"<table id="t" style="height:30px"><tr id="r"><td>{}</td></tr></table>"#, block(80, "b"));
    let r = rects(&html, "", &["t", "r"]);
    assert_eq!((r[0].height, r[1].height), (80.0, 80.0), "{r:?}");
}

#[test]
fn min_height_stretches_the_rows_like_height() {
    let html = format!(r#"<table id="t" style="min-height:120px"><tr id="r"><td>{}</td></tr></table>"#, block(20, "b"));
    let r = rects(&html, "", &["t", "r"]);
    assert_eq!((r[0].height, r[1].height), (120.0, 120.0), "{r:?}");
}

#[test]
fn only_auto_height_rows_take_the_extra() {
    let html = format!(
        r#"<table id="t" style="height:200px"><tr id="r1" style="height:40px"><td>{}</td></tr><tr id="r2"><td>{}</td></tr></table>"#,
        block(10, "a"),
        block(10, "b"),
    );
    let r = rects(&html, "", &["r1", "r2"]);
    assert_eq!((r[0].y, r[0].height), (0.0, 40.0), "строка с height не растёт: {r:?}");
    assert_eq!((r[1].y, r[1].height), (40.0, 160.0), "{r:?}");
}

#[test]
fn border_spacing_stays_between_stretched_rows() {
    let html = format!(
        r#"<table id="t" style="height:100px;border-spacing:10px"><tr id="r1"><td>{}</td></tr><tr id="r2"><td>{}</td></tr></table>"#,
        block(10, "a"),
        block(10, "b"),
    );
    let r = rects(&html, "", &["t", "r1", "r2"]);
    assert_eq!(r[0].height, 100.0);
    // 100 = 10 + r1 + 10 + r2 + 10, строки поровну.
    assert_eq!((r[1].y, r[1].height), (10.0, 35.0), "{r:?}");
    assert_eq!((r[2].y, r[2].height), (55.0, 35.0), "{r:?}");
}

#[test]
fn row_groups_move_and_grow_with_their_rows() {
    let html = format!(
        r#"<table id="t" style="height:200px"><thead id="h"><tr id="r1"><td style="vertical-align:top">{}</td></tr></thead><tbody id="b"><tr id="r2"><td id="c2" style="vertical-align:bottom">{}</td></tr></tbody></table>"#,
        block(30, "a"),
        block(30, "x"),
    );
    let r = rects(&html, "", &["h", "b", "r1", "r2", "x", "t"]);
    assert_eq!(r[5].height, 200.0);
    assert_eq!((r[0].y, r[0].height), (0.0, 100.0), "{r:?}");
    assert_eq!((r[1].y, r[1].height), (100.0, 100.0), "{r:?}");
    assert_eq!((r[2].y, r[3].y), (0.0, 100.0), "{r:?}");
    assert_eq!(r[4].y, 170.0, "bottom: низ ячейки второй строки — 200");
}

#[test]
fn rowspan_cell_reaches_the_bottom_of_its_stretched_rows() {
    let html = format!(
        r#"<table id="t" style="height:200px"><tr id="r1"><td id="s" rowspan="2" style="vertical-align:top">{}</td><td>{}</td></tr><tr id="r2"><td>{}</td></tr></table>"#,
        block(10, "a"),
        block(10, "b"),
        block(10, "c"),
    );
    let r = rects(&html, "", &["s", "r2"]);
    assert_eq!((r[1].y, r[1].height), (100.0, 100.0), "{r:?}");
    assert_eq!((r[0].y, r[0].height), (0.0, 200.0), "{r:?}");
}

#[test]
fn bottom_caption_follows_the_stretched_rows() {
    let html = format!(
        r#"<table id="t" style="height:200px"><caption id="cap" style="caption-side:bottom"><div style="height:20px"></div></caption><tr id="r"><td>{}</td></tr></table>"#,
        block(10, "a"),
    );
    let r = rects(&html, "", &["t", "r", "cap"]);
    assert_eq!(r[0].height, 200.0);
    assert_eq!((r[1].y, r[1].height), (0.0, 180.0), "{r:?}");
    assert_eq!(r[2].y, 180.0, "{r:?}");
}

#[test]
fn collapsed_borders_table_stretches_too() {
    let html = format!(r#"<table id="t" style="height:150px;border-collapse:collapse"><tr id="r"><td>{}</td></tr></table>"#, block(10, "a"));
    let r = rects(&html, "", &["t", "r"]);
    assert_eq!((r[0].height, r[1].height), (150.0, 150.0), "{r:?}");
}

#[test]
fn height_attribute_is_distributed_as_well() {
    let html = format!(r#"<table id="t" height="160"><tr id="r"><td>{}</td></tr></table>"#, block(10, "a"));
    let r = rects(&html, "", &["t", "r"]);
    assert_eq!((r[0].height, r[1].height), (160.0, 160.0), "{r:?}");
}

#[test]
fn table_without_columns_keeps_the_specified_height() {
    // CSS Tables L3 §3.1 шаг 3B: сетка 0×1 — таблица пуста, строки в высоту не входят.
    let html = r#"<table id="t" style="height:50px"><tr style="height:100px"></tr></table>"#;
    assert_eq!(rects(html, "", &["t"])[0].height, 50.0);
}
