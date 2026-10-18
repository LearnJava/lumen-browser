use lumen_core::geom::{Rect, Size};

// ── TABLE-VWM: таблица в вертикальном writing-mode ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet =
        lumen_css_parser::parse(&format!("body{{margin:0}}table{{border-spacing:0}}td{{padding:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

fn xywh(r: &Rect) -> (f32, f32, f32, f32) {
    (r.x, r.y, r.width, r.height)
}

const GRID_3X2: &str = r#"<table id="t"><tr id="r1"><td id="a"></td><td id="b"></td><td id="c"></td></tr><tr id="r2"><td id="d"></td><td id="e"></td><td id="f"></td></tr></table>"#;

fn grid(wm: &str, extra: &str) -> Vec<Rect> {
    let css = format!("table{{writing-mode:{wm};{extra}}}td{{width:20px;height:10px}}");
    rects(GRID_3X2, &css, &["t", "r1", "r2", "a", "b", "c", "d", "e", "f"])
}

#[test]
fn vertical_lr_rows_stack_along_x_columns_along_y() {
    let r = grid("vertical-lr", "");
    assert_eq!(xywh(&r[0]), (0.0, 0.0, 40.0, 30.0), "table");
    assert_eq!(xywh(&r[1]), (0.0, 0.0, 20.0, 30.0), "row 1");
    assert_eq!(xywh(&r[2]), (20.0, 0.0, 20.0, 30.0), "row 2");
    assert_eq!(xywh(&r[3]), (0.0, 0.0, 20.0, 10.0), "a");
    assert_eq!(xywh(&r[4]), (0.0, 10.0, 20.0, 10.0), "b");
    assert_eq!(xywh(&r[5]), (0.0, 20.0, 20.0, 10.0), "c");
    assert_eq!(xywh(&r[6]), (20.0, 0.0, 20.0, 10.0), "d");
    assert_eq!(xywh(&r[8]), (20.0, 20.0, 20.0, 10.0), "f");
}

#[test]
fn vertical_rl_rows_run_right_to_left() {
    let r = grid("vertical-rl", "");
    assert_eq!(xywh(&r[0]), (0.0, 0.0, 40.0, 30.0), "table");
    assert_eq!(xywh(&r[1]), (20.0, 0.0, 20.0, 30.0), "row 1 справа");
    assert_eq!(xywh(&r[2]), (0.0, 0.0, 20.0, 30.0), "row 2 слева");
    assert_eq!(xywh(&r[3]), (20.0, 0.0, 20.0, 10.0), "a");
    assert_eq!(xywh(&r[6]), (0.0, 0.0, 20.0, 10.0), "d");
}

#[test]
fn rtl_reverses_columns_bottom_to_top() {
    let r = grid("vertical-lr", "direction:rtl");
    assert_eq!(xywh(&r[3]), (0.0, 20.0, 20.0, 10.0), "a внизу");
    assert_eq!(xywh(&r[4]), (0.0, 10.0, 20.0, 10.0), "b");
    assert_eq!(xywh(&r[5]), (0.0, 0.0, 20.0, 10.0), "c вверху");
}

#[test]
fn sideways_lr_reverses_columns() {
    let r = grid("sideways-lr", "");
    assert_eq!(xywh(&r[3]), (0.0, 20.0, 20.0, 10.0), "a внизу");
}

#[test]
fn border_spacing_is_physical() {
    // Первая длина — по x (между строками), вторая — по y (между столбцами).
    let r = grid("vertical-lr", "border-spacing:4px 2px");
    // По x: 3 щели по 4 + 2 строки по 20 = 52; по y: 4 щели по 2 + 3 столбца по 10 = 38.
    assert_eq!(xywh(&r[0]), (0.0, 0.0, 52.0, 38.0), "table");
    assert_eq!(xywh(&r[3]), (4.0, 2.0, 20.0, 10.0), "a");
    assert_eq!(xywh(&r[4]), (4.0, 14.0, 20.0, 10.0), "b");
    assert_eq!(xywh(&r[6]), (28.0, 2.0, 20.0, 10.0), "d");
}

#[test]
fn thead_first_tfoot_last() {
    let html = r#"<table id="t"><tfoot><tr><td id="f"></td></tr></tfoot><tbody><tr><td id="b"></td></tr></tbody><thead><tr><td id="h"></td></tr></thead></table>"#;
    let r = rects(html, "table{writing-mode:vertical-lr}td{width:10px;height:10px}", &["h", "b", "f"]);
    assert_eq!(r[0].x, 0.0, "thead слева");
    assert_eq!(r[1].x, 10.0, "tbody");
    assert_eq!(r[2].x, 20.0, "tfoot справа");
}

#[test]
fn colspan_and_rowspan_cover_tracks() {
    let html = r#"<table id="t"><tr><td id="a" rowspan="2"></td><td id="b" colspan="2"></td></tr><tr><td id="c"></td><td id="d"></td></tr></table>"#;
    let r = rects(html, "table{writing-mode:vertical-lr}td{width:10px;height:10px}", &["t", "a", "b", "c", "d"]);
    // Строка 1 — x 0..10, строка 2 — x 10..20; столбцы: 0..10 (a), 10..20, 20..30.
    assert_eq!(xywh(&r[0]), (0.0, 0.0, 20.0, 30.0), "table");
    assert_eq!(xywh(&r[1]), (0.0, 0.0, 20.0, 10.0), "a занимает обе строки в столбце 0");
    assert_eq!(xywh(&r[2]), (0.0, 10.0, 10.0, 20.0), "b занимает два столбца в строке 1");
    assert_eq!(xywh(&r[3]), (10.0, 10.0, 10.0, 10.0), "c");
    assert_eq!(xywh(&r[4]), (10.0, 20.0, 10.0, 10.0), "d");
}

#[test]
fn vertical_align_middle_works_along_block_axis() {
    let html = r#"<table><tr><td id="a"><div id="x" style="width:10px;height:5px"></div></td><td id="b" style="width:40px"></td></tr></table>"#;
    let css = "table{writing-mode:vertical-lr}td{height:10px}#a{vertical-align:middle}";
    let r = rects(html, css, &["a", "b", "x"]);
    assert_eq!(r[0].width, 40.0, "ячейка растянута на толщину строки");
    assert_eq!(r[2].x, 15.0, "содержимое по центру block-оси: (40 − 10) / 2");
}

#[test]
fn auto_table_shrinks_to_columns() {
    let r = grid("vertical-lr", "");
    assert!(r[0].height <= 30.0 + 1e-3, "таблица без height обтягивает столбцы, а не заполняет 600px");
}

#[test]
fn explicit_table_height_stretches_columns() {
    let r = grid("vertical-lr", "height:60px");
    assert_eq!(r[0].height, 60.0);
    assert_eq!(xywh(&r[3]), (0.0, 0.0, 20.0, 20.0), "a");
    assert_eq!(xywh(&r[4]), (0.0, 20.0, 20.0, 20.0), "b");
}

#[test]
fn explicit_table_width_stretches_rows() {
    let r = grid("vertical-lr", "width:80px");
    assert_eq!(r[0].width, 80.0);
    assert_eq!(xywh(&r[1]), (0.0, 0.0, 40.0, 30.0), "row 1");
    assert_eq!(xywh(&r[2]), (40.0, 0.0, 40.0, 30.0), "row 2");
}

#[test]
fn horizontal_table_is_unchanged() {
    let r = grid("horizontal-tb", "");
    assert_eq!(xywh(&r[0]), (0.0, 0.0, 60.0, 20.0), "table");
    assert_eq!(xywh(&r[3]), (0.0, 0.0, 20.0, 10.0), "a");
    assert_eq!(xywh(&r[4]), (20.0, 0.0, 20.0, 10.0), "b");
}

// ── горизонтальная таблица: rtl и ортогональная ячейка ──

#[test]
fn horizontal_rtl_table_reverses_columns() {
    let r = grid("horizontal-tb", "direction:rtl");
    assert_eq!(xywh(&r[0]), (0.0, 0.0, 60.0, 20.0), "table");
    assert_eq!(xywh(&r[3]), (40.0, 0.0, 20.0, 10.0), "a — крайний справа");
    assert_eq!(xywh(&r[4]), (20.0, 0.0, 20.0, 10.0), "b");
    assert_eq!(xywh(&r[5]), (0.0, 0.0, 20.0, 10.0), "c — крайний слева");
    assert_eq!(xywh(&r[6]), (40.0, 10.0, 20.0, 10.0), "d во второй строке");
}

#[test]
fn orthogonal_cell_keeps_explicit_column_width() {
    let r = rects(
        r#"<table id="t"><tr><td id="a">x</td><td id="b"></td></tr></table>"#,
        "#a{writing-mode:vertical-lr;width:30px;height:10px}#b{width:20px;height:10px}",
        &["t", "a", "b"],
    );
    assert_eq!(r[1].width, 30.0, "ширина ячейки — по `width`, не по содержимому");
    assert_eq!(r[2].x, 30.0, "вторая ячейка сдвинута на ширину первого столбца");
    assert_eq!(r[0].width, 50.0, "table");
}
