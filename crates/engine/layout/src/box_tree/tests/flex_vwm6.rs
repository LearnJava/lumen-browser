use lumen_core::geom::{Rect, Size};

// ── FLEX-VWM-6: ширина контейнера вокруг ортогонального элемента ──
//
// Пустые `span` с явными размерами и `canvas` без текста: шрифт не нужен.

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

const COLUMNS: &str = r#"<div id="t"><div id="k"><div id="v"><span></span><span></span></div></div></div>"#;
const COLUMNS_CSS: &str = "span{display:inline-block;inline-size:100px;block-size:50px}\
    #t{width:100px;line-height:0}#k{display:inline-flex;height:100%}#v{writing-mode:vertical-rl}";

/// Два пролёта по 100px в колонке высотой 100px — две колонки по 50px: ширина
/// `inline-flex` — 100px, а не 50px одной колонки, и `flex-shrink` не ужимает
/// элемент до min-content (`css-flexbox/dynamic-orthogonal-flex-item`).
#[test]
fn inline_flex_is_as_wide_as_the_columns_of_its_orthogonal_item() {
    let r = rects(COLUMNS, &format!("{COLUMNS_CSS}#t{{height:100px}}"), &["k", "v"]);
    assert_eq!(r[0].width, 100.0, "{:?}", r[0]);
    assert_eq!(r[1].width, 100.0, "{:?}", r[1]);
    assert_eq!(r[1].height, 100.0, "{:?}", r[1]);
}

/// Высота контейнера меняется — меняется число колонок и ширина контейнера.
#[test]
fn inline_flex_width_follows_the_container_height() {
    let r = rects(COLUMNS, &format!("{COLUMNS_CSS}#t{{height:200px}}"), &["k", "v"]);
    assert_eq!(r[0].width, 50.0, "{:?}", r[0]);
    assert_eq!(r[1].width, 50.0, "{:?}", r[1]);
}

const CANVAS: &str = r#"<div id="k"><div id="i"><canvas id="c" width="10" height="10" style="height:100%"></canvas></div></div>"#;
const CANVAS_CSS: &str = "#k{display:inline-flex;height:100px}";

/// Растянутый flex-элемент определён (Flexbox §9.8): `height: 100%` у `canvas`
/// в нём — 100px, ширина — высота через соотношение сторон, а не 10px битмапа
/// (`css-flexbox/aspect-ratio-intrinsic-size-001`).
#[test]
fn canvas_with_percent_height_in_a_stretched_item_is_a_square() {
    let r = rects(CANVAS, CANVAS_CSS, &["k", "i", "c"]);
    assert_eq!((r[2].width, r[2].height), (100.0, 100.0), "{:?}", r[2]);
    assert_eq!(r[1].width, 100.0, "{:?}", r[1]);
    assert_eq!(r[0].width, 100.0, "{:?}", r[0]);
}

/// Авторская высота `canvas` в пикселях: ширина — по соотношению, а не 10px
/// от атрибута `width`.
#[test]
fn canvas_with_authored_height_takes_its_width_from_the_ratio() {
    let r = rects(
        r#"<canvas id="c" width="10" height="10" style="height:100px"></canvas>"#,
        "",
        &["c"],
    );
    assert_eq!((r[0].width, r[0].height), (100.0, 100.0), "{:?}", r[0]);
}

/// Авто-минимум ортогонального элемента — его ширина по пробе (две колонки), а не
/// одна колонка: в контейнере уже содержимого `flex-shrink` не ужимает ниже неё.
#[test]
fn orthogonal_item_is_not_shrunk_below_its_columns() {
    let r = rects(
        r#"<div id="k"><div id="v"><span></span><span></span></div></div>"#,
        "span{display:inline-block;inline-size:100px;block-size:50px}\
         #k{display:flex;width:60px;height:100px}#v{writing-mode:vertical-rl}",
        &["v"],
    );
    assert_eq!(r[0].width, 100.0, "{:?}", r[0]);
}
