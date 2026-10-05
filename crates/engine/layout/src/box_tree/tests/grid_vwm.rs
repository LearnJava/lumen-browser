use lumen_core::geom::{Rect, Size};

// ── GRID-VWM: grid в вертикальном `writing-mode` ──
//
// Столбцы идут по физической оси y (inline), строки — по x (block): справа налево у
// `vertical-rl`, слева направо у `vertical-lr`. Ожидания сверены с Edge (headless,
// пиксельный diff страниц с теми же правилами — 0,00 %).

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

fn assert_rect(r: Rect, x: f32, y: f32, w: f32, h: f32) {
    assert_eq!((r.x, r.y, r.width, r.height), (x, y, w, h), "{r:?}");
}

const THREE: &str = r#"<div id="g"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;

/// `vertical-rl`: столбцы сверху вниз, строки справа налево; ширина контейнера — сумма строк.
#[test]
fn vertical_rl_columns_run_down_rows_run_left() {
    let r = rects(
        THREE,
        "#g{display:grid;writing-mode:vertical-rl;grid-template-columns:50px 70px;\
         grid-template-rows:30px 40px;height:200px}",
        &["g", "a", "b", "c"],
    );
    assert_rect(r[0], 0.0, 0.0, 70.0, 200.0);
    assert_rect(r[1], 40.0, 0.0, 30.0, 50.0);
    assert_rect(r[2], 40.0, 50.0, 30.0, 70.0);
    assert_rect(r[3], 0.0, 0.0, 40.0, 50.0);
}

/// `vertical-lr`: строки слева направо.
#[test]
fn vertical_lr_rows_run_right() {
    let r = rects(
        THREE,
        "#g{display:grid;writing-mode:vertical-lr;grid-template-columns:50px 70px;\
         grid-template-rows:30px 40px;height:200px}",
        &["a", "b", "c"],
    );
    assert_rect(r[0], 0.0, 0.0, 30.0, 50.0);
    assert_rect(r[1], 0.0, 50.0, 30.0, 70.0);
    assert_rect(r[2], 30.0, 0.0, 40.0, 50.0);
}

/// `direction: rtl` разворачивает inline-ось: первый столбец — у нижнего края.
#[test]
fn rtl_direction_reverses_columns_bottom_up() {
    let r = rects(
        THREE,
        "#g{display:grid;writing-mode:vertical-lr;direction:rtl;grid-template-columns:50px 70px;\
         grid-template-rows:30px 40px;height:200px}",
        &["a", "b"],
    );
    assert_rect(r[0], 0.0, 150.0, 30.0, 50.0);
    assert_rect(r[1], 0.0, 80.0, 30.0, 70.0);
}

/// `align-self` идёт по block-оси (физический x), `justify-self` — по inline (y): у `vertical-rl`
/// конец block-оси слева.
#[test]
fn self_alignment_uses_container_axes() {
    let r = rects(
        r#"<div id="g"><div id="a"></div></div>"#,
        "#g{display:grid;writing-mode:vertical-rl;grid-template-columns:100px;grid-template-rows:60px;height:100px}\
         #a{width:10px;height:20px;align-self:end;justify-self:center}",
        &["a"],
    );
    assert_rect(r[0], 0.0, 40.0, 10.0, 20.0);
}

/// Авто-поля забирают свободное место и отменяют растяжение.
#[test]
fn auto_margins_absorb_free_space() {
    let r = rects(
        r#"<div id="g"><div id="a"></div></div>"#,
        "#g{display:grid;writing-mode:vertical-lr;grid-template-columns:100px;grid-template-rows:60px;height:100px}\
         #a{width:10px;height:20px;margin:auto}",
        &["a"],
    );
    assert_rect(r[0], 25.0, 40.0, 10.0, 20.0);
}

/// `inline-grid` обтягивает дорожки столбцов по высоте (CSS Grid L1 §11.5).
#[test]
fn inline_grid_height_follows_column_tracks() {
    let r = rects(
        r#"<div id="g"><div id="a"></div></div>"#,
        "#g{display:inline-grid;writing-mode:vertical-rl;grid-template-columns:50px 70px;column-gap:10px;\
         grid-template-rows:30px}",
        &["g"],
    );
    assert_eq!((r[0].width, r[0].height), (30.0, 130.0), "{:?}", r[0]);
}

/// Блочный grid в горизонтальном родителе: `height: auto` — по дорожкам, а не по вьюпорту
/// (Writing Modes L3 §7.3.1).
#[test]
fn orthogonal_block_grid_shrinks_to_its_columns() {
    let r = rects(
        r#"<div><div id="g"><div id="a"></div></div></div>"#,
        "#g{display:grid;writing-mode:vertical-lr;grid-template-columns:100px 200px;grid-template-rows:30px}",
        &["g"],
    );
    assert_eq!((r[0].width, r[0].height), (30.0, 300.0), "{:?}", r[0]);
}

/// Baseline-группа строки по оси x: центральные базовые линии элементов разной ширины
/// совпадают, группа прижата к началу block-оси (справа у `vertical-rl`).
#[test]
fn baseline_group_aligns_on_the_x_axis() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="b"></div></div>"#,
        "#g{display:grid;writing-mode:vertical-rl;grid-template-columns:50px 50px;align-items:baseline}\
         #a{width:20px;height:10px}#b{width:40px;height:10px}",
        &["g", "a", "b"],
    );
    assert_eq!(r[0].width, 40.0, "{:?}", r[0]);
    assert_eq!(r[2].x, 0.0, "{:?}", r[2]);
    assert_eq!(r[1].x, 10.0, "{:?}", r[1]);
}

/// Выравнивание в горизонтальной сетке переносит элемент вместе с поддеревом
/// (раньше потомки оставались у начала ячейки).
#[test]
fn horizontal_alignment_moves_the_item_subtree() {
    let r = rects(
        r#"<div id="g"><div id="a"><div id="c"></div></div></div>"#,
        "#g{display:grid;grid-template-columns:200px;grid-template-rows:100px;align-items:end;justify-items:end}\
         #a{width:50px}#c{height:20px}",
        &["a", "c"],
    );
    assert_rect(r[0], 150.0, 80.0, 50.0, 20.0);
    assert_rect(r[1], 150.0, 80.0, 50.0, 20.0);
}

/// `justify-items`, отличный от `stretch`, обтягивает элемент по содержимому.
#[test]
fn justify_items_center_shrinks_auto_width_item() {
    let r = rects(
        r#"<div id="g"><div id="a"><div id="c"></div></div></div>"#,
        "#g{display:grid;grid-template-columns:200px;justify-items:center}#c{width:40px;height:10px}",
        &["a"],
    );
    assert_rect(r[0], 80.0, 0.0, 40.0, 10.0);
}

/// Поля и растяжение: растянутый элемент не уезжает на удвоенное поле.
#[test]
fn stretched_item_with_margins_stays_in_its_cell() {
    let r = rects(
        r#"<div id="g"><div id="a"></div></div>"#,
        "#g{display:grid;grid-template-columns:200px;grid-template-rows:100px;width:200px}#a{margin:10px}",
        &["g", "a"],
    );
    // Поле элемента сейчас схлопывается с контейнером (BUG-1262), поэтому смотрим от его верха.
    assert_rect(r[1], 10.0, r[0].y + 10.0, 180.0, 80.0);
}

/// `position: relative` не теряется при выравнивании в ячейке.
#[test]
fn relative_offset_survives_cell_alignment() {
    let r = rects(
        r#"<div id="g"><div id="a"></div></div>"#,
        "#g{display:grid;grid-template-columns:100px;grid-template-rows:50px}\
         #a{position:relative;left:10px;top:9px;height:20px}",
        &["a"],
    );
    assert_rect(r[0], 10.0, 9.0, 100.0, 20.0);
}

/// `min-width` растягивает контейнер по block-оси, строки `vertical-rl` остаются прижатыми к
/// его правому краю (а не к сумме строк).
#[test]
fn min_width_keeps_rows_at_the_right_edge() {
    let r = rects(
        r#"<div id="g"><div id="a"></div><div id="b"></div></div>"#,
        "#g{display:grid;writing-mode:vertical-rl;grid-template-columns:40px;grid-template-rows:50px 50px;         min-width:300px;height:100px}",
        &["g", "a", "b"],
    );
    assert_eq!(r[0].width, 300.0, "{:?}", r[0]);
    assert_rect(r[1], 250.0, 0.0, 50.0, 40.0);
    assert_rect(r[2], 200.0, 0.0, 50.0, 40.0);
}
