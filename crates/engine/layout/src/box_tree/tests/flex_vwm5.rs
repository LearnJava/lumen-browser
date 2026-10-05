use lumen_core::geom::{Rect, Size};

// ── FLEX-VWM-5: растяжение и baseline вертикальных items во flex ──
//
// Пустые `div` с явными размерами: числа считаются руками, шрифт не нужен.

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

const TWO_COLUMNS: &str = r#"<div id="k"><div id="a"><div id="ac"></div></div><div id="b"><div id="bc"></div></div></div>"#;
const TWO_COLUMNS_CSS: &str =
    "#k{display:flex;height:100px}#a,#b{writing-mode:vertical-rl}#ac{width:10px;height:20px}#bc{width:30px;height:20px}";

/// CSS Flexbox L1 §9.4 шаг 11: `width: auto` вертикального item'а во flex-ряду
/// вертикального режима растягивается до поперечного размера линии, а содержимое
/// остаётся у края начала блока — у `vertical-rl` справа.
#[test]
fn vertical_row_item_stretches_and_keeps_content_at_the_right() {
    let r = rects(TWO_COLUMNS, &format!("{TWO_COLUMNS_CSS}#k{{writing-mode:vertical-rl}}"), &["a", "ac"]);
    assert_eq!(r[0].width, 30.0, "{:?}", r[0]);
    assert_eq!(r[1].x + r[1].width, r[0].x + r[0].width, "{:?} в {:?}", r[1], r[0]);
}

#[test]
fn vertical_lr_row_item_keeps_content_at_the_left() {
    let r = rects(
        TWO_COLUMNS,
        &format!("{TWO_COLUMNS_CSS}#k{{writing-mode:vertical-lr}}#a,#b{{writing-mode:vertical-lr}}"),
        &["a", "ac"],
    );
    assert_eq!(r[0].width, 30.0, "{:?}", r[0]);
    assert_eq!(r[1].x, r[0].x, "{:?} в {:?}", r[1], r[0]);
}

#[test]
fn vertical_row_item_with_flex_start_is_not_stretched() {
    let r = rects(TWO_COLUMNS, &format!("{TWO_COLUMNS_CSS}#k{{writing-mode:vertical-rl;align-items:flex-start}}"), &["a"]);
    assert_eq!(r[0].width, 10.0, "{:?}", r[0]);
}

const VERTICAL_ITEM: &str = r#"<div id="k"><div id="i"><div id="c"></div></div></div>"#;
const VERTICAL_ITEM_CSS: &str = "#i{writing-mode:vertical-rl}#c{width:10px;height:30px}";

/// Writing Modes L3 §7.3.1: вертикальный item горизонтального ряда без
/// определённой высоты обтягивает содержимое, а не заполняет окно.
#[test]
fn vertical_item_of_auto_height_row_shrinks_to_content() {
    let r = rects(VERTICAL_ITEM, &format!("{VERTICAL_ITEM_CSS}#k{{display:flex}}"), &["i", "k"]);
    assert_eq!(r[0].height, 30.0, "{:?}", r[0]);
    assert_eq!(r[1].height, 30.0, "{:?}", r[1]);
}

/// В определённой высоте item без `stretch` тоже обтягивает содержимое,
/// а растянутый заполняет линию.
#[test]
fn vertical_item_fills_a_definite_row_only_when_stretched() {
    let css = format!("{VERTICAL_ITEM_CSS}#k{{display:flex;height:200px");
    let packed = rects(VERTICAL_ITEM, &format!("{css};align-items:flex-start}}"), &["i"]);
    assert_eq!(packed[0].height, 30.0, "{:?}", packed[0]);
    let stretched = rects(VERTICAL_ITEM, &format!("{css}}}"), &["i"]);
    assert_eq!(stretched[0].height, 200.0, "{:?}", stretched[0]);
}

/// Синтезированная базовая линия ортогонального item'а в горизонтальном
/// контейнере — нижний край border box и для `first` (CSS Align L3 §9.1).
#[test]
fn orthogonal_item_first_baseline_is_its_bottom_edge_in_a_horizontal_row() {
    let r = rects(
        r#"<div id="k"><div id="a"></div><div id="b"></div></div>"#,
        "#k{display:flex;align-items:baseline}#a{width:10px;height:10px}\
         #b{writing-mode:vertical-rl;width:40px;height:40px}",
        &["a", "b"],
    );
    assert_eq!(r[1].y, 0.0, "{:?}", r[1]);
    assert_eq!(r[0].y, 30.0, "{:?}", r[0]);
}

/// В вертикальном контейнере линия синтезируется по середине (центральная).
#[test]
fn orthogonal_item_synthesizes_a_central_baseline_in_a_vertical_container() {
    let r = rects(
        r#"<div id="k"><div id="a"></div><div id="b"></div></div>"#,
        "#k{display:flex;flex-direction:column;writing-mode:vertical-rl;align-items:last baseline;height:100px}\
         #a{writing-mode:horizontal-tb;width:20px;height:30px}#b{width:20px;height:10px}",
        &["a", "b"],
    );
    assert_eq!(r[0].y, 65.0, "{:?}", r[0]);
    assert_eq!(r[1].y, 90.0, "{:?}", r[1]);
}

const NESTED_ROW: &str = r#"<div id="k"><div id="c"><div id="f"><div id="x"></div><div id="y"></div><div id="z"></div></div></div></div>"#;
const NESTED_ROW_CSS: &str =
    "#k{display:flex;align-items:flex-start}#c{writing-mode:vertical-rl}#f{display:flex}#x,#y,#z{width:10px;height:40px}";

/// CSS Flexbox L1 §9.9.1: inline-размер flex-ряда вертикального режима —
/// сумма вкладов items (по оси y), а не самый длинный.
#[test]
fn vertical_flex_row_item_sums_its_items_inline_sizes() {
    let r = rects(NESTED_ROW, NESTED_ROW_CSS, &["c"]);
    assert_eq!(r[0].height, 120.0, "{:?}", r[0]);
}

/// `max-height` вертикального бокса ограничивает его inline-размер.
#[test]
fn vertical_flex_row_item_is_bounded_by_max_height() {
    let r = rects(NESTED_ROW, &format!("{NESTED_ROW_CSS}#f{{max-height:100px}}"), &["c"]);
    assert_eq!(r[0].height, 100.0, "{:?}", r[0]);
}

/// Auto-поля по поперечной оси отключают растяжение: item обтягивает
/// содержимое и центрируется в линии.
#[test]
fn vertical_item_with_auto_margins_centers_instead_of_filling() {
    let r = rects(
        NESTED_ROW,
        &format!("{NESTED_ROW_CSS}#k{{height:200px}}#c{{margin:auto 0}}"),
        &["c"],
    );
    assert_eq!(r[0].height, 120.0, "{:?}", r[0]);
    assert_eq!(r[0].y, 40.0, "{:?}", r[0]);
}
