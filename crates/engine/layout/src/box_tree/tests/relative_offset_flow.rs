use lumen_core::geom::{Rect, Size};

// ── BUG-1240: position: relative не двигает соседей и не растит родителя ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

#[test]
fn relative_top_does_not_shift_following_siblings() {
    let r = rects(
        r#"<div id="a" style="height:50px"></div><div id="b" style="height:20px;position:relative;top:7px"></div><div id="c" style="height:30px"></div>"#,
        "",
        &["a", "b", "c"],
    );
    assert_eq!((r[0].y, r[1].y, r[2].y), (0.0, 57.0, 70.0), "{r:?}");
}

#[test]
fn relative_child_does_not_grow_parent() {
    let r = rects(
        r#"<div id="p" style="position:relative"><div style="position:relative;top:50px;height:30px"></div></div>"#,
        "",
        &["p"],
    );
    assert_eq!(r[0].height, 30.0, "{r:?}");
}
