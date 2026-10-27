use lumen_core::geom::{Rect, Size};

// ── `visibility: collapse` на строке / группе строк (CSS Tables 3, BUG-1533) ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}table{{border-spacing:0}}td{{padding:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

fn row(id: &str, style: &str, h: u32) -> String {
    format!(r#"<tr id="{id}" style="{style}"><td id="{id}c"><div style="height:{h}px;width:40px"></div></td></tr>"#)
}

#[test]
fn collapsed_middle_row_is_removed_from_table_height() {
    let html = format!(
        r#"<table id="t">{}{}{}</table>"#,
        row("a", "", 30),
        row("b", "visibility:collapse", 30),
        row("c", "", 30),
    );
    let r = rects(&html, "", &["t", "a", "b", "c"]);
    assert_eq!(r[0].height, 60.0, "таблица: {r:?}");
    assert_eq!(r[2].height, 0.0, "схлопнутая строка: {r:?}");
    assert_eq!(r[3].y, 30.0, "третья строка встаёт сразу за первой: {r:?}");
}

#[test]
fn collapsed_row_group_zeroes_visible_rows_inside() {
    let html = format!(
        r#"<table id="t"><tbody id="g" style="visibility:collapse">{}{}</tbody><tbody>{}</tbody></table>"#,
        row("a", "visibility:visible", 30),
        row("b", "visibility:visible", 30),
        row("c", "", 30),
    );
    let r = rects(&html, "", &["t", "g", "a", "b", "c"]);
    assert_eq!(r[0].height, 30.0, "таблица: {r:?}");
    assert_eq!((r[1].height, r[2].height, r[3].height), (0.0, 0.0, 0.0), "{r:?}");
    assert_eq!(r[4].y, 0.0, "{r:?}");
}

#[test]
fn collapsed_row_drops_its_border_spacing_slot() {
    // border-spacing 10: обычная таблица 3 строки = 4 щели; схлопнутая строка убирает свою щель.
    let html = format!(
        r#"<table id="t" style="border-spacing:10px">{}{}</table>"#,
        row("a", "visibility:collapse", 30),
        row("b", "", 80),
    );
    let r = rects(&html, "table{border-spacing:10px}", &["t", "b"]);
    assert_eq!(r[0].height, 100.0, "10 + 80 + 10: {r:?}");
    assert_eq!(r[1].y, 10.0, "{r:?}");
}

#[test]
fn rowspan_cell_ending_in_a_collapsed_row_stops_at_the_last_visible_row() {
    let html = r#"<table id="t" style="border-spacing:10px 100px"><tr><td id="s" rowspan="2" style="width:100px"></td></tr><tr style="visibility:collapse"></tr></table>"#;
    let r = rects(html, "table{border-spacing:50px 100px}", &["t", "s"]);
    assert_eq!(r[1].height, 0.0, "{r:?}");
    assert_eq!(r[0].height, 200.0, "{r:?}");
}

#[test]
fn collapsed_row_gets_no_share_of_extra_table_height() {
    let html = format!(
        r#"<table id="t" style="height:100px">{}{}</table>"#,
        row("a", "", 20),
        row("b", "visibility:collapse", 20),
    );
    let r = rects(&html, "", &["t", "a", "b"]);
    assert_eq!(r[1].height, 100.0, "{r:?}");
    assert_eq!(r[2].height, 0.0, "{r:?}");
}
