use lumen_core::geom::{Rect, Size};

// ── LAYOUT-FIXED-CB: fixed под предком, создающим containing block ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

/// Опорная линия: абсолютный потомок через статический промежуточный блок
/// считается от padding box позиционированного предка.
#[test]
fn abs_through_static_block_uses_positioned_ancestor() {
    let r = rects(
        r#"<div style="height:50px"></div><div id="p"><div id="m"><div id="c"></div></div></div>"#,
        "#p{position:relative;margin-left:30px;height:200px;width:300px}#c{position:absolute;right:0;bottom:0;width:10px;height:10px}",
        &["c"],
    );
    assert_eq!((r[0].x, r[0].y), (320.0, 240.0), "{:?}", r[0]);
}

/// css-transforms-1 §2: `position: fixed` под `transform` — CB это padding box предка.
#[test]
fn fixed_under_transform_uses_ancestor_padding_box() {
    let r = rects(
        r#"<div style="height:50px"></div><div id="p"><div id="m"><div id="c"></div></div></div>"#,
        "#p{transform:scale(1);margin-left:30px;height:200px;width:300px}#c{position:fixed;right:0;bottom:0;width:10px;height:10px}",
        &["c"],
    );
    assert_eq!((r[0].x, r[0].y), (320.0, 240.0), "{:?}", r[0]);
}

/// Прямой потомок: `top`/`left` от padding box предка с рамкой.
#[test]
fn fixed_direct_child_of_filter_ancestor() {
    let r = rects(
        r#"<div id="p"><div id="c"></div></div>"#,
        "#p{filter:blur(0px);margin:20px;border:5px solid;height:100px;width:200px}#c{position:fixed;top:10px;left:15px;width:10px;height:10px}",
        &["c"],
    );
    assert_eq!((r[0].x, r[0].y), (40.0, 35.0), "{:?}", r[0]);
}

/// Без предка-CB fixed остаётся относительно вьюпорта.
#[test]
fn fixed_without_ancestor_stays_viewport_relative() {
    let r = rects(
        r#"<div style="height:50px"></div><div id="p"><div id="c"></div></div>"#,
        "#p{margin-left:30px;height:200px}#c{position:fixed;right:0;bottom:0;width:10px;height:10px}",
        &["c"],
    );
    assert_eq!((r[0].x, r[0].y), (790.0, 590.0), "{:?}", r[0]);
}

/// `left: 0; right: 0` без ширины растягивает fixed до padding box предка, а не вьюпорта.
#[test]
fn fixed_stretch_uses_ancestor_width() {
    let r = rects(
        r#"<div id="p"><div id="m"><div id="c"></div></div></div>"#,
        "#p{transform:translate(0,0);margin-left:30px;width:300px;height:100px;padding:0 10px}#c{position:fixed;left:0;right:0;top:0;height:5px}",
        &["c"],
    );
    assert_eq!((r[0].x, r[0].y, r[0].width, r[0].height), (30.0, 0.0, 320.0, 5.0), "{:?}", r[0]);
}

/// `top`/`bottom` без высоты — высота из зазора padding box предка через статическую обёртку.
#[test]
fn abs_top_bottom_fill_through_wrapper() {
    let r = rects(
        r#"<div style="height:20px"></div><div id="p"><div id="m"><div id="c"></div></div></div>"#,
        "#p{position:relative;height:200px;border-top:4px solid}#c{position:absolute;top:10px;bottom:30px;left:0;width:5px}",
        &["c"],
    );
    assert_eq!((r[0].y, r[0].height), (34.0, 160.0), "{:?}", r[0]);
}

/// css-transforms-1 §2: transform делает предка containing block и для absolute.
#[test]
fn abs_under_transformed_static_ancestor() {
    let r = rects(
        r#"<div style="height:20px"></div><div id="p"><div id="c"></div></div>"#,
        "#p{transform:rotate(0deg);margin-left:40px;height:100px;width:200px}#c{position:absolute;right:0;bottom:0;width:10px;height:10px}",
        &["c"],
    );
    assert_eq!((r[0].x, r[0].y), (230.0, 110.0), "{:?}", r[0]);
}

/// fixed в flex-контейнере с `filter`, через обёртку, и вложенный fixed в absolute внутри.
#[test]
fn fixed_under_filtered_flex_container() {
    let r = rects(
        r#"<div style="height:20px"></div><div id="p"><div id="m"><div id="c"></div></div></div>"#,
        "#p{display:flex;filter:opacity(1);margin-left:30px;width:300px;height:100px}#c{position:fixed;right:0;bottom:0;width:10px;height:10px}",
        &["c"],
    );
    assert_eq!((r[0].x, r[0].y), (320.0, 110.0), "{:?}", r[0]);
}

/// Ближайший предок с transform — CB, внешние не учитываются; relative без transform — не CB для fixed.
#[test]
fn nearest_fixed_cb_wins() {
    let r = rects(
        r#"<div id="o"><div id="r"><div id="t"><div id="c"></div></div></div></div>"#,
        "#o{transform:scale(1);width:500px;height:500px}#r{position:relative;margin:50px;height:300px}#t{transform:scale(1);margin:10px;width:100px;height:100px}#c{position:fixed;left:5px;top:5px;width:1px;height:1px}",
        &["t", "c"],
    );
    assert_eq!((r[1].x, r[1].y), (r[0].x + 5.0, r[0].y + 5.0), "{:?} {:?}", r[0], r[1]);
}

/// Не-transform relative-предок не захватывает fixed: он остаётся относительно вьюпорта.
#[test]
fn positioned_ancestor_does_not_capture_fixed() {
    let r = rects(
        r#"<div id="r"><div id="c"></div></div>"#,
        "#r{position:relative;margin:50px;height:300px}#c{position:fixed;left:5px;bottom:5px;width:1px;height:1px}",
        &["c"],
    );
    assert_eq!((r[0].x, r[0].y), (5.0, 594.0), "{:?}", r[0]);
}

/// `collect_layout_shift_rects`: fixed под transform скроллится с предком, а значит попадает в отчёт.
#[test]
fn fixed_under_transform_is_not_scroll_pinned() {
    let html = r#"<div id="p"><div id="c"></div></div><div id="f"></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        "body{margin:0}#p{transform:scale(1);height:50px}#c{position:fixed;width:5px;height:5px}#f{position:fixed;width:5px;height:5px}",
    );
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let shifts = crate::collect_layout_shift_rects(&root);
    let id = |name: &str| super::find_by_id_all(&root, &doc, name).expect(name).node.index() as u32;
    assert!(shifts.contains_key(&id("c")), "fixed под transform должен учитываться");
    assert!(!shifts.contains_key(&id("f")), "fixed под вьюпортом исключается");
}
