use lumen_core::geom::{Rect, Size};

// ── LAYOUT-VFLOAT: схлопывание margin по block-оси вертикального потока (CSS 2.1 §8.3.1) ──
//
// Block-ось — физический `x`: для `vertical-lr` block-start — левый margin, для
// `vertical-rl` — правый. Числа считаются руками.

fn rects(mode: &str, body: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let html = format!(r#"<div id="w">{body}</div>"#);
    let css = format!(
        "body{{margin:0}}#w{{writing-mode:{mode};height:100px;width:200px}}div div{{height:100px}}{css}"
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

#[test]
fn vertical_lr_first_child_margin_collapses_with_the_parents() {
    let r = rects(
        "vertical-lr",
        r#"<div id="p"><div id="c"></div></div>"#,
        "#p{margin-left:20px}#c{margin-left:30px;width:10px}",
        &["p", "c"],
    );
    assert_eq!(r[1].x, 30.0, "max(20, 30), а не сумма: {:?}", r[1]);
    assert_eq!(r[0].x, 30.0, "родитель сдвинут на схлопнутый margin: {:?}", r[0]);
    assert_eq!(r[0].width, 10.0, "{:?}", r[0]);
}

#[test]
fn vertical_lr_parent_padding_blocks_the_first_child_collapse() {
    let r = rects(
        "vertical-lr",
        r#"<div id="p"><div id="c"></div></div>"#,
        "#p{margin-left:20px;padding-left:1px}#c{margin-left:30px;width:10px}",
        &["p", "c"],
    );
    assert_eq!(r[0].x, 20.0, "{:?}", r[0]);
    assert_eq!(r[1].x, 51.0, "20 + 1 + 30: {:?}", r[1]);
}

#[test]
fn vertical_lr_last_child_margin_escapes_and_collapses_with_the_next_sibling() {
    let r = rects(
        "vertical-lr",
        r#"<div id="p"><div id="c"></div></div><div id="n"></div>"#,
        "#c{margin-right:30px;width:10px}#n{margin-left:10px;width:10px}",
        &["p", "n"],
    );
    assert_eq!(r[0].width, 10.0, "margin потомка не закрывает коробку: {:?}", r[0]);
    assert_eq!(r[1].x, 40.0, "10 + max(30, 10): {:?}", r[1]);
}

#[test]
fn vertical_lr_margins_collapse_through_an_empty_block() {
    let r = rects(
        "vertical-lr",
        r#"<div id="a"></div><div id="e"></div><div id="b"></div>"#,
        "#a{width:10px;margin-right:20px}#e{margin-left:5px;margin-right:30px}#b{margin-left:10px;width:10px}",
        &["a", "b"],
    );
    assert_eq!(r[1].x, 40.0, "10 + max(20, 5, 30, 10): {:?}", r[1]);
}

#[test]
fn vertical_rl_sibling_margins_collapse_on_the_mirrored_sides() {
    // vertical-rl: block-start — правая сторона, block-end — левая.
    let r = rects(
        "vertical-rl",
        r#"<div id="a"></div><div id="b"></div>"#,
        "#a{width:10px;margin-left:20px}#b{width:10px;margin-right:5px}",
        &["a", "b"],
    );
    assert_eq!(r[0].x, 190.0, "первый — у правого края: {:?}", r[0]);
    assert_eq!(r[1].x, 160.0, "190 − max(20, 5) − 10: {:?}", r[1]);
}

#[test]
fn vertical_rl_first_child_margin_collapses_with_the_parents() {
    let r = rects(
        "vertical-rl",
        r#"<div id="p"><div id="c"></div></div>"#,
        "#p{margin-right:20px}#c{margin-right:30px;width:10px}",
        &["p", "c"],
    );
    assert_eq!(r[1].x, 160.0, "200 − max(20, 30) − 10: {:?}", r[1]);
    assert_eq!(r[0].width, 10.0, "{:?}", r[0]);
}
