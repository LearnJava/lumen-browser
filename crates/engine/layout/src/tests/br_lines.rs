//! BUG-1411: a line made only of `<br>` / NBSP takes a line box; leading and consecutive `<br>`
//! each open a line. Шрифт — `Fixed8`, `line-height: 20px`.

use super::*;

fn height_of(inner: &str) -> f32 {
    let root = lay_measured(&format!("<div>{inner}</div>"), "div { line-height: 20px; font-size: 16px; }", 400.0);
    let div = first_element_child(&root);
    div.rect.height
}

#[test]
fn br_height_matches_the_lines_it_opens() {
    // (содержимое `<div>`, число строк). Эталон — браузеры: `<br>` после текста только закрывает
    // его строку, `<br>` без текста перед ним — пустая строка.
    let cases: &[(&str, f32)] = &[
        ("<br>", 1.0),
        ("&nbsp;", 1.0),
        ("<br>D", 2.0),
        ("B<br><br>", 2.0),
        ("<br><br>", 2.0),
        ("<span><br></span>", 1.0),
        ("<br>&nbsp;", 2.0),
        ("B<br>", 1.0),
        ("B<br>C", 2.0),
        ("B <br> <br> C", 3.0),
        ("<br><br><br>", 3.0),
    ];
    for (inner, lines) in cases {
        assert_eq!(height_of(inner), lines * 20.0, "<div>{inner}</div>");
    }
}

#[test]
fn br_after_a_block_opens_its_own_line() {
    assert_eq!(height_of("<p style=\"margin:0\">a</p><br>"), 40.0);
}

#[test]
fn br_in_a_flex_container_adds_no_height_of_its_own() {
    let root = lay_measured("<div style=\"display:flex\"><br></div>", "div { line-height: 20px; }", 400.0);
    assert_eq!(first_element_child(&root).rect.height, 0.0);
}
