use lumen_core::geom::{Rect, Size};

// BUG-1284: `white-space: nowrap` запрещает перенос между атомарными inline-боксами.

fn rects(html: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

fn row(display: &str, container_style: &str) -> String {
    let item = |id: &str| {
        format!(r#"<div id="{id}" style="display:{display};width:40px;height:20px;margin:3px">x</div>"#)
    };
    format!(
        r#"<div style="width:100px;{container_style}">{}{}{}</div>"#,
        item("a"), item("b"), item("c")
    )
}

#[test]
fn inline_flex_wraps_without_nowrap() {
    let r = rects(&row("inline-flex", ""), &["a", "b", "c"]);
    assert_eq!(r[0].y, r[1].y, "{r:?}");
    assert!(r[2].y > r[0].y, "{r:?}");
}

#[test]
fn inline_flex_stays_on_one_line_under_nowrap() {
    let r = rects(&row("inline-flex", "white-space:nowrap"), &["a", "b", "c"]);
    assert_eq!((r[0].y, r[1].y, r[2].y), (r[0].y, r[0].y, r[0].y), "{r:?}");
    assert!(r[2].x > r[1].x, "{r:?}");
}

#[test]
fn inline_block_stays_on_one_line_under_nowrap() {
    let r = rects(&row("inline-block", "white-space:nowrap"), &["a", "b", "c"]);
    assert_eq!((r[1].y, r[2].y), (r[0].y, r[0].y), "{r:?}");
}

#[test]
fn text_wrap_mode_nowrap_also_holds_the_row() {
    let r = rects(&row("inline-block", "text-wrap-mode:nowrap"), &["a", "b", "c"]);
    assert_eq!((r[1].y, r[2].y), (r[0].y, r[0].y), "{r:?}");
}
