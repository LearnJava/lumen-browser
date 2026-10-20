use lumen_core::geom::{Rect, Size};

// ── BUG-1308: `%` в строках и row-gap считаются от высоты контейнера, не от ширины ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

/// CSS Grid L1 §7.2.1: `60%` при неопределённой высоте — `auto`, строка по содержимому (15px),
/// а не 60 % ширины (120px).
#[test]
fn percent_row_is_auto_when_height_is_indefinite() {
    let r = rects(
        r#"<div id="g"><div id="a"></div></div>"#,
        "#g{display:grid;width:200px;grid-template-rows:60%}#a{height:15px}",
        &["g"],
    );
    assert_eq!(r[0].height, 15.0, "{:?}", r[0]);
}

/// `minmax(%, %)` и `grid-auto-rows: %` — то же самое.
#[test]
fn percent_minmax_and_auto_rows_are_auto_when_height_is_indefinite() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="b"></div></div>"#,
        "#g{display:grid;width:200px;grid-template-rows:minmax(50%,50%);grid-auto-rows:70%}\
         #a{height:15px}#b{height:20px}",
        &["g", "b"],
    );
    assert_eq!(r[0].height, 35.0, "{:?}", r[0]);
    assert_eq!(r[1].y, 15.0, "{:?}", r[1]);
}

/// Определённая высота: `%` от высоты, а не от ширины.
#[test]
fn percent_row_resolves_against_definite_height() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="b"></div></div>"#,
        "#g{display:grid;width:200px;height:400px;grid-template-rows:25% 50%;align-content:start}\
         #a{height:10px}#b{height:10px}",
        &["b"],
    );
    assert_eq!(r[0].y, 100.0, "{:?}", r[0]);
}

/// §8.3: процентный `row-gap` при неопределённой высоте — 0 (раньше брался от ширины, 20px).
#[test]
fn percent_row_gap_is_zero_when_height_is_indefinite() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="b"></div></div>"#,
        "#g{display:grid;width:200px;gap:10%;grid-template-rows:90px 90px}",
        &["g", "b"],
    );
    assert_eq!(r[1].y, 90.0, "{:?}", r[1]);
    assert_eq!(r[0].height, 180.0, "{:?}", r[0]);
}

/// С определённой высотой 400px `row-gap: 10%` — 40px; `column-gap: 10%` по-прежнему от ширины.
#[test]
fn percent_gaps_resolve_against_their_own_axis() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="b"></div><div id="c"></div><div id="d"></div></div>"#,
        "#g{display:grid;width:200px;height:400px;gap:10%;grid-template-columns:50px 50px;grid-template-rows:90px 90px;\
         align-content:start;justify-content:start}",
        &["d"],
    );
    assert_eq!(r[0].y, 130.0, "{:?}", r[0]);
    assert_eq!(r[0].x, 70.0, "{:?}", r[0]);
}
