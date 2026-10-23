//! Имя пользовательского свойства с CSS-экранированием (BUG-1502, CSS Syntax L3
//! §4.3.7): `--\30` и `--0` — одно свойство, `--\d800` и `--\fffd` — тоже
//! (суррогат заменяется на U+FFFD), как в объявлении, так и в `var()`.

use lumen_core::geom::Size;

/// Цвет `<p>` под `body { color: blue }` после `css`.
fn p_color(css: &str) -> (u8, u8, u8) {
    let doc = lumen_html_parser::parse("<p>x</p>");
    let sheet = lumen_css_parser::parse(&format!("body {{ color: blue }} {css}"));
    let root = crate::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let c = crate::selector_query::find_box_by_selector(&root, &doc, "p").expect("p box").style.color;
    (c.r, c.g, c.b)
}

const GREEN: (u8, u8, u8) = (0, 128, 0);

#[test]
fn escaped_digit_name_matches_plain_in_reference() {
    assert_eq!(p_color(r"p { --0: green; color: var(--\30) }"), GREEN);
}

#[test]
fn escaped_digit_name_matches_in_declaration_and_reference() {
    assert_eq!(p_color(r"p { --\30: green; color: var(--\30) }"), GREEN);
    assert_eq!(p_color(r"p { --\30: green; color: var(--0) }"), GREEN);
}

#[test]
fn surrogate_escape_is_replacement_character() {
    assert_eq!(p_color(r"p { --\d800: green; color: var(--\fffd) }"), GREEN);
    assert_eq!(p_color(r"p { --\d800: green; color: var(--\d800) }"), GREEN);
}

#[test]
fn escape_with_space_terminator_inside_name() {
    assert_eq!(p_color(r"p { --a\27 d: green; color: var(--a\27 d) }"), GREEN);
    assert_eq!(p_color(r"p { --a\27 d: green; color: var(--a\000027d) }"), GREEN);
    assert_eq!(p_color(r"p { --a-长-name-that-might-be-longer-than-you\27 d-normally-use: green; color: var(--a-长-name-that-might-be-longer-than-you\27 d-normally-use) }"), GREEN);
}

#[test]
fn raw_non_ascii_name_still_works() {
    assert_eq!(p_color("p { --длинное-имя: green; color: var(--длинное-имя) }"), GREEN);
}
