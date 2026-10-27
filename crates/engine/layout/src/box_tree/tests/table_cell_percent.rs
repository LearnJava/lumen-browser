use lumen_core::geom::{Rect, Size};

// ── процентная высота потомка ячейки от высоты строки/таблицы (BUG-1534) ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}table{{border-spacing:0}}td{{padding:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

#[test]
fn child_percent_height_resolves_against_row_height() {
    let html = r#"<table><tr style="height:100px"><td id="c"><div id="d" style="height:50%"></div></td></tr></table>"#;
    let r = rects(html, "", &["c", "d"]);
    assert_eq!((r[0].height, r[1].height), (100.0, 50.0), "{r:?}");
}

#[test]
fn child_percent_height_of_display_table_row_cell() {
    let html = r#"<div style="display:table"><div style="display:table-row;height:100px"><div id="c" style="display:table-cell"><div id="d" style="height:100%"></div></div></div></div>"#;
    let r = rects(html, "", &["c", "d"]);
    assert_eq!((r[0].height, r[1].height), (100.0, 100.0), "{r:?}");
}

#[test]
fn child_percent_height_resolves_against_cell_height_still() {
    let html = r#"<table><tr><td id="c" style="height:100px"><div id="d" style="height:50%"></div></td></tr></table>"#;
    let r = rects(html, "", &["c", "d"]);
    assert_eq!((r[0].height, r[1].height), (100.0, 50.0), "{r:?}");
}

#[test]
fn row_height_percent_child_respects_cell_padding() {
    let html = r#"<table><tr style="height:100px"><td id="c" style="padding:10px"><div id="d" style="height:50%"></div></td></tr></table>"#;
    let r = rects(html, "", &["c", "d"]);
    assert_eq!((r[0].height, r[1].height), (100.0, 40.0), "{r:?}");
}

#[test]
fn row_height_smaller_than_content_does_not_clip_the_cell() {
    let html = r#"<table><tr style="height:50px"><td id="c"><div id="d" style="height:200px"></div></td></tr></table>"#;
    let r = rects(html, "", &["c", "d"]);
    assert_eq!((r[0].height, r[1].height), (200.0, 200.0), "{r:?}");
}
