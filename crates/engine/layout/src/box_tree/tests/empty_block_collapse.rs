use lumen_core::geom::{Rect, Size};

// ── BUG-1350: пустой блок высоты 0 схлопывает верхнее и нижнее поля (CSS 2.1 §8.3.1) ──

fn rects(body: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let html = format!(r#"<div id="w">{body}</div>"#);
    let css = format!("body{{margin:0}}{css}");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

#[test]
fn empty_block_margins_merge_with_neighbours() {
    let r = rects(
        r#"<div id="a"></div><div id="e"></div><div id="c"></div>"#,
        "#a{height:10px;margin-bottom:20px}#e{height:0;margin:20px 0}#c{height:10px;margin-top:20px}",
        &["a", "e", "c"],
    );
    assert_eq!(r[1].y, 30.0, "{:?}", r[1]);
    assert_eq!(r[2].y, 30.0, "{:?}", r[2]);
}

#[test]
fn two_empty_blocks_in_a_row_merge() {
    let r = rects(
        r#"<div id="a"></div><div id="e"></div><div id="f"></div><div id="c"></div>"#,
        "#a{height:10px;margin-bottom:20px}#e,#f{height:0;margin:20px 0}#c{height:10px;margin-top:20px}",
        &["c"],
    );
    assert_eq!(r[0].y, 30.0, "{:?}", r[0]);
}
