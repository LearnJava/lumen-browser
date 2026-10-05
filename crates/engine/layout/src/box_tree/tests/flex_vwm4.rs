use lumen_core::geom::{Rect, Size};

// ── FLEX-VWM-4: растянутый flex-элемент — определённая высота для потомков ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

/// CSS Flexbox L1 §9.8: высота растянутого `align-self: stretch` элемента
/// определённая, поэтому `height: 100%` потомка резолвится от неё.
#[test]
fn percent_height_child_of_stretched_item_resolves() {
    let r = rects(
        r#"<div id="f"><div id="i"><div id="c"></div></div></div>"#,
        "#f{display:flex;height:100px}#i{flex:1;padding:5px 3px}#c{height:100%}",
        &["i", "c"],
    );
    assert_eq!(r[0].height, 100.0, "{:?}", r[0]);
    assert_eq!(r[1].height, 90.0, "{:?}", r[1]);
}

/// Элемент с `box-sizing: content-box`, margin и padding: растянутый размер
/// переводится в content-box, а x/ширина сохраняются.
#[test]
fn stretched_item_relayout_keeps_x_and_width() {
    let r = rects(
        r#"<div id="f"><div id="i"><div id="c"></div></div></div>"#,
        "#f{display:flex;width:200px;height:100px}\
         #i{width:120px;margin:4px 6px 3px 7px;padding:5px 9px;border:2px solid}\
         #c{height:100%}",
        &["i", "c"],
    );
    assert_eq!(r[0].x, 7.0, "{:?}", r[0]);
    assert_eq!(r[0].width, 120.0 + 18.0 + 4.0, "{:?}", r[0]);
    assert_eq!(r[0].height, 100.0 - 7.0, "{:?}", r[0]);
    assert_eq!(r[1].height, 100.0 - 7.0 - 14.0, "{:?}", r[1]);
}

/// Содержимое выше линии: растянутый элемент всё равно получает размер линии
/// (§9.4 шаг 11), если потомок просит процентную высоту.
#[test]
fn stretched_item_shrinks_to_the_line_when_percent_child_asks() {
    let r = rects(
        r#"<div id="f"><div id="i"><div id="c" style="height:100%"></div><div style="height:300px"></div></div></div>"#,
        "#f{display:flex;height:100px}",
        &["i"],
    );
    assert_eq!(r[0].height, 100.0, "{:?}", r[0]);
}

/// CSS Tables L3 «used min width of table»: таблица-элемент не уже своего
/// min-content, даже при `min-width: 0` в контейнере нулевой ширины.
#[test]
fn table_flex_item_is_not_narrower_than_its_min_content() {
    let r = rects(
        r#"<div id="f"><div id="t"><div id="c"><div style="width:100px;height:10px"></div></div></div></div>"#,
        "#f{display:flex;width:0}\
         #t{display:table;min-width:0;width:50px}#c{display:table-cell}",
        &["t"],
    );
    assert_eq!(r[0].width, 100.0, "{:?}", r[0]);
}

/// Writing Modes L3 §7.3.1: ортогональный блок в родителе с неопределённой
/// высотой обтягивает содержимое по inline-оси, а не заполняет вьюпорт.
#[test]
fn orthogonal_block_in_auto_height_parent_shrinks_to_content() {
    let r = rects(
        r#"<div id="p"><div id="v"><div style="height:70px"></div></div></div>"#,
        "#v{writing-mode:vertical-lr}",
        &["v"],
    );
    assert_eq!(r[0].height, 70.0, "{:?}", r[0]);
}

/// …но корневой элемент заполняет начальный содержащий блок.
#[test]
fn vertical_root_element_still_fills_the_viewport() {
    let doc = lumen_html_parser::parse("<div id=\"v\"></div>");
    let sheet = lumen_css_parser::parse("html{writing-mode:vertical-lr}body{margin:0}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let html = root.children.iter().find(|c| !matches!(c.kind, super::super::BoxKind::Skip)).unwrap();
    assert_eq!(html.rect.height, 600.0, "{:?}", html.rect);
}

/// Вертикальный flex-контейнер блочного уровня в родителе с auto-высотой:
/// высота — размер содержимого по главной оси, а не вьюпорт (`flex-aspect-ratio-img-vert-lr`).
#[test]
fn vertical_flex_container_in_auto_height_parent_is_content_tall() {
    let r = rects(
        r#"<div id="f"><div style="height:40px"></div><div style="height:60px"></div></div>"#,
        "#f{display:flex;writing-mode:vertical-lr}",
        &["f"],
    );
    assert_eq!(r[0].height, 100.0, "{:?}", r[0]);
}

// ── вложенный row-flex в растянутом flex-item'е ──────────────────────────────

#[test]
fn nested_row_flex_children_stretch_to_the_stretched_item() {
    // Внешний row-flex высотой 10px; внутренний flex-item растягивается до неё, и его
    // пустой ребёнок (`align-self: stretch`) тоже — Flexbox §9.4 step 11: растянутый
    // размер определённый (`css-gaps/grid/subgrid/subgrid-gap-decorations-013-ref`).
    let r = rects(
        r#"<div id="o"><div id="m"><div id="i"></div></div><div id="c"></div></div>"#,
        "#o{display:flex;height:10px;width:140px} #m{display:flex;width:80px} #i{width:20px} #c{width:20px}",
        &["m", "i"],
    );
    assert_eq!(r[0].height, 10.0);
    assert_eq!(r[1].height, 10.0);
}
