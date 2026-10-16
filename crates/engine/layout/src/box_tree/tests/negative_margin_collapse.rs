use lumen_core::geom::{Rect, Size};

// ── BUG-1365: отрицательное поле блока с блочными детьми не теряется при схлопывании (CSS 2.1 §8.3.1) ──

fn rects(body: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let css = format!("body{{margin:0}}{css}");
    let doc = lumen_html_parser::parse(body);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

const FIRST: &str = "#a{height:100px}#b{height:20px}";

#[test]
fn negative_top_margin_survives_block_child() {
    let r = rects(
        r#"<div id="a"></div><div style="margin-top:-60px"><div id="b"></div></div>"#,
        FIRST,
        &["b"],
    );
    assert_eq!(r[0].y, 40.0, "{:?}", r[0]);
}

#[test]
fn negative_top_margin_on_nested_child_escapes() {
    let r = rects(
        r#"<div id="a"></div><div><div id="b" style="margin-top:-60px"></div></div>"#,
        FIRST,
        &["b"],
    );
    assert_eq!(r[0].y, 40.0, "{:?}", r[0]);
}

#[test]
fn negative_top_margin_adds_to_previous_positive_bottom() {
    let r = rects(
        r#"<div id="a" style="margin-bottom:30px"></div><div style="margin-top:-60px"><div id="b"></div></div>"#,
        FIRST,
        &["b"],
    );
    assert_eq!(r[0].y, 70.0, "{:?}", r[0]);
}

#[test]
fn negative_bottom_margin_survives_block_child() {
    let r = rects(
        r#"<div id="a"></div><div style="margin-bottom:-60px"><div id="b"></div></div><div id="c" style="height:10px"></div>"#,
        FIRST,
        &["b", "c"],
    );
    assert_eq!(r[0].y, 100.0, "{:?}", r[0]);
    assert_eq!(r[1].y, 60.0, "{:?}", r[1]);
}

#[test]
fn negative_bottom_margin_on_last_child_escapes_without_shrinking_parent() {
    let r = rects(
        r#"<div id="a"></div><div id="w"><div id="b" style="margin-bottom:-60px"></div></div><div id="c" style="height:10px"></div>"#,
        FIRST,
        &["w", "c"],
    );
    assert_eq!(r[0].height, 20.0, "{:?}", r[0]);
    assert_eq!(r[1].y, 60.0, "{:?}", r[1]);
}

#[test]
fn padding_keeps_negative_margin_apart_from_child() {
    let r = rects(
        r#"<div id="a"></div><div style="margin-top:-60px;padding-top:1px"><div id="b"></div></div>"#,
        FIRST,
        &["b"],
    );
    assert_eq!(r[0].y, 41.0, "{:?}", r[0]);
}

#[test]
fn mixed_chain_sums_largest_positive_and_most_negative() {
    // 20 и -50 в одной цепочке первых детей: 20 + (-50) = -30.
    let r = rects(
        r#"<div id="a"></div><div style="margin-top:20px"><div style="margin-top:-50px"><div id="b"></div></div></div>"#,
        FIRST,
        &["b"],
    );
    assert_eq!(r[0].y, 70.0, "{:?}", r[0]);
}
