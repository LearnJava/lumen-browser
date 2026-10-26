// BUG-1531: ширина таблицы не меньше суммы минимальных ширин колонок (CSS Tables 3 §3.2).

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

fn table_and_cells(html: &str) -> (f32, Vec<f32>) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0} table{border-spacing:0} td{padding:0}");
    let root = super::super::layout_measured(&doc, &sheet, lumen_core::geom::Size::new(800.0, 600.0), &Measurer);
    let table = super::find_by_id_all(&root, &doc, "t").expect("нет #t").rect;
    let cells = ["a", "b"]
        .iter()
        .filter_map(|id| super::find_by_id_all(&root, &doc, id))
        .map(|c| c.rect.width)
        .collect();
    (table.width, cells)
}

#[test]
fn table_width_raised_to_sum_of_fixed_cell_widths() {
    let (t, cells) = table_and_cells(
        r#"<table id="t" style="width:50px"><tr><td id="a" style="width:30px"></td><td id="b" style="width:30px"></td></tr></table>"#,
    );
    assert_eq!(t, 60.0);
    assert_eq!(cells, vec![30.0, 30.0]);
}

#[test]
fn table_width_raised_to_single_wide_cell() {
    let (t, _) = table_and_cells(
        r#"<table id="t" style="width:50px"><tr><td id="a" style="width:100px"></td></tr></table>"#,
    );
    assert_eq!(t, 100.0);
}

#[test]
fn table_width_raised_to_min_content_of_block() {
    let (t, cells) = table_and_cells(
        r#"<table id="t" style="width:1px"><tr><td id="a"><div style="width:50px;height:5px"></div></td></tr></table>"#,
    );
    assert_eq!(t, 50.0);
    assert_eq!(cells, vec![50.0]);
}

#[test]
fn fixed_layout_table_width_raised_to_sum_of_column_widths() {
    let (t, cells) = table_and_cells(
        r#"<table id="t" style="width:50px;table-layout:fixed"><tr><td id="a" style="width:30px"></td><td id="b" style="width:30px"></td></tr></table>"#,
    );
    assert_eq!(t, 60.0);
    assert_eq!(cells, vec![30.0, 30.0]);
}

#[test]
fn wider_table_width_is_kept() {
    let (t, cells) = table_and_cells(
        r#"<table id="t" style="width:200px"><tr><td id="a" style="width:30px"></td><td id="b" style="width:30px"></td></tr></table>"#,
    );
    assert_eq!(t, 200.0);
    assert_eq!(cells.len(), 2);
}

#[test]
fn percent_cell_widths_do_not_widen_table() {
    let (t, _) = table_and_cells(
        r#"<table id="t" style="width:100px"><tr><td id="a" style="width:60%"></td><td id="b" style="width:60%"></td></tr></table>"#,
    );
    assert_eq!(t, 100.0);
}

#[test]
fn table_width_raised_to_longest_word_of_text() {
    let (t, _) = table_and_cells(
        r#"<table id="t" style="width:1px"><tr><td id="a" style="font-size:10px">abcdefghij k</td></tr></table>"#,
    );
    assert_eq!(t, 50.0);
}

#[test]
fn shrink_to_fit_table_sums_columns() {
    for wrapper in [
        r#"<div style="float:left">"#,
        r#"<div style="display:inline-block">"#,
        r#"<div style="width:fit-content">"#,
        r#"<div style="position:absolute">"#,
    ] {
        let html = format!(
            r#"{wrapper}<table id="t"><tr><td id="a" style="width:50px"></td><td id="b" style="width:50px"></td></tr></table></div>"#
        );
        let (t, _) = table_and_cells(&html);
        assert_eq!(t, 100.0, "{wrapper}");
    }
}
