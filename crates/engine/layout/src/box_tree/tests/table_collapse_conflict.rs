// BUG-1532: border-collapse — разрешение конфликта рамок соседних ячеек (CSS 2.1 §17.6.2.1).

use crate::BorderStyle;

fn cells(html: &str, ids: &[&str]) -> Vec<std::sync::Arc<crate::ComputedStyle>> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = super::super::layout(&doc, &sheet, lumen_core::geom::Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| std::sync::Arc::clone(&super::find_by_id_all(&root, &doc, id).expect("нет ячейки").style))
        .collect()
}

#[test]
fn hidden_beats_wider_solid_on_both_cells() {
    let s = cells(
        r#"<table style="border-collapse:collapse"><tr><td id="a" style="border-right:5px hidden red"></td><td id="b" style="border-left:9px solid blue"></td></tr></table>"#,
        &["a", "b"],
    );
    assert_eq!((s[0].border_right_style, s[0].border_right_width), (BorderStyle::Hidden, 0.0));
    assert_eq!((s[1].border_left_style, s[1].border_left_width), (BorderStyle::Hidden, 0.0));
}

#[test]
fn wider_border_wins_over_stronger_style() {
    let s = cells(
        r#"<table style="border-collapse:collapse"><tr><td id="a" style="border-right:2px solid red"></td><td id="b" style="border-left:6px dotted blue"></td></tr></table>"#,
        &["a", "b"],
    );
    assert_eq!((s[0].border_right_style, s[0].border_right_width), (BorderStyle::Dotted, 6.0));
    assert_eq!((s[1].border_left_style, s[1].border_left_width), (BorderStyle::Dotted, 6.0));
}

#[test]
fn equal_width_style_decides_then_the_cell_before() {
    let s = cells(
        r#"<table style="border-collapse:collapse"><tr><td id="a" style="border-right:4px dashed red"></td><td id="b" style="border-left:4px solid blue"></td></tr>
        <tr><td id="c" style="border-right:4px solid red"></td><td id="d" style="border-left:4px solid blue"></td></tr></table>"#,
        &["a", "b", "c", "d"],
    );
    assert_eq!(s[0].border_right_style, BorderStyle::Solid);
    assert_eq!(s[0].border_right_color, crate::CssColor::Rgba(crate::style::Color { r: 0, g: 0, b: 255, a: 255 }));
    // A full tie goes to the left cell.
    assert_eq!(s[3].border_left_color, crate::CssColor::Rgba(crate::style::Color { r: 255, g: 0, b: 0, a: 255 }));
}

#[test]
fn table_border_strictly_winning_clears_the_outer_cell_side() {
    let s = cells(
        r#"<table style="border-collapse:collapse;border:5px solid green"><tr><td id="a" style="border:5px outset red"></td><td id="b" style="border:5px solid red"></td></tr></table>"#,
        &["a", "b"],
    );
    // outset loses to the table's solid border; a tie keeps the cell's own border.
    assert_eq!((s[0].border_bottom_style, s[0].border_bottom_width), (BorderStyle::None, 0.0));
    assert_eq!((s[1].border_bottom_style, s[1].border_bottom_width), (BorderStyle::Solid, 5.0));
}
