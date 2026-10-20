use lumen_core::geom::{Rect, Size};

// ── проценты margin/padding grid-item'а: база — inline-размер grid-области (BUG-1310) ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

const ONE_ITEM: &str = r#"<div id="g"><div id="a"></div></div>"#;
const GRID: &str = "#g{display:grid;width:300px;grid-template-columns:200px}";

#[test]
fn percent_margin_left_resolves_against_grid_area_not_container() {
    // 30 % от области 200 = 60 (а не 90 от контейнера 300).
    let css = format!("{GRID}#a{{width:50px;height:10px;margin-left:30%}}");
    let r = rects(ONE_ITEM, &css, &["a"]);
    assert!((r[0].x - 60.0).abs() < 0.01 && r[0].width == 50.0, "{r:?}");
}

#[test]
fn calc_margin_with_percent_uses_grid_area() {
    let css = format!("{GRID}#a{{width:50px;height:10px;margin-left:calc(10px + 30%)}}");
    let r = rects(ONE_ITEM, &css, &["a"]);
    assert!((r[0].x - 70.0).abs() < 0.01, "{r:?}");
}

#[test]
fn percent_vertical_margin_and_padding_use_area_width() {
    // Вертикальные поля тоже от ширины области: margin-top 10 % = 20, padding 10 % = 20 со всех сторон.
    let css = format!("{GRID}#a{{margin-top:10%;padding:10%;box-sizing:border-box;height:100px}}");
    let r = rects(ONE_ITEM, &css, &["a"]);
    assert!((r[0].y - 20.0).abs() < 0.01 && r[0].width == 200.0, "{r:?}");
}

#[test]
fn percent_padding_spanning_two_columns_uses_whole_area() {
    let html = r#"<div id="g"><div id="a"><div id="c"></div></div></div>"#;
    let css = "#g{display:grid;width:300px;grid-template-columns:100px 200px}\
               #a{grid-column:1/3;padding-left:50%}#c{height:5px}";
    let r = rects(html, css, &["a", "c"]);
    assert_eq!((r[0].width, r[1].x, r[1].width), (300.0, 150.0, 150.0), "{r:?}");
}

#[test]
fn percent_margin_of_item_in_auto_track_uses_resolved_track() {
    let html = r#"<div id="g"><div id="a"></div></div>"#;
    let css = "#g{display:grid;width:300px;grid-template-columns:auto}#a{width:50px;height:10px;margin-left:10%}";
    let r = rects(html, css, &["a"]);
    assert!((r[0].x - 30.0).abs() < 0.01, "{r:?}");
}

#[test]
fn percent_padding_resolves_against_grid_area() {
    // padding-left 10 % от области 200 = 20, а не 30 от контейнера 300: потомок сдвинут на 20.
    let html = r#"<div id="g"><div id="a"><div id="c"></div></div></div>"#;
    let css = format!("{GRID}#a{{padding-left:10%}}#c{{height:5px}}");
    let r = rects(html, &css, &["a", "c"]);
    assert!((r[1].x - 20.0).abs() < 0.01 && (r[1].width - 180.0).abs() < 0.01, "{r:?}");
}
