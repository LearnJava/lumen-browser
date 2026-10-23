//! Декларации с `var()` и недопустимым телом (BUG-1500, CSS Variables L1 §2.1/§3):
//! значение, не разбирающееся как `<declaration-value>`, отбрасывает декларацию
//! целиком (предыдущая остаётся в силе); EOF закрывает открытые конструкции.

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
const BLUE: (u8, u8, u8) = (0, 0, 255);

#[test]
fn custom_property_with_bad_var_fallback_is_dropped() {
    // variable-declaration-11/12/13: `!`, `;` и `!important` в fallback-е.
    for fb in ["!", ";", "!important"] {
        let css = format!("p {{ color: red; --a: green; --b: crimson; --a: var(--b,{fb}); color: var(--a) }}");
        assert_eq!(p_color(&css), GREEN, "fallback {fb:?}");
    }
}

#[test]
fn cdo_token_in_var_value_is_not_a_bang() {
    // variable-declaration-24 / variable-reference-17: `<!--` — один токен, не `!`.
    let css = "p { color: orange; --a: red; --b: crimson; --a: var(--b) <!--; color: var(--a) }";
    assert_eq!(p_color(css), BLUE);
    assert_eq!(p_color("p { color: crimson; --a: green; color: var(--a, <!--) }"), GREEN);
}

#[test]
fn var_function_name_is_case_insensitive() {
    // variable-reference-20.
    assert_eq!(p_color("p { color: red; --a: green; color: VAR(--a) }"), GREEN);
}

#[test]
fn custom_property_with_double_important_is_dropped() {
    // variable-declaration-23.
    let css = "p { color: orange; --a: green; --b: crimson; --a: var(--b) !important !important; color: var(--a) }";
    assert_eq!(p_color(css), GREEN);
}

#[test]
fn custom_property_with_stray_close_paren_is_dropped() {
    // variable-declaration-59: `--a: red)` — лишняя `)`, прежнее `--a` остаётся.
    assert_eq!(p_color("p { color: var(--a); --a: green; --a: red); }"), GREEN);
}

#[test]
fn braced_var_reference_in_regular_property_is_invalid_at_computed_time() {
    // variable-reference-18: `{ [ var() ] }` допустим при разборе, но подстановка
    // даёт `{ [ ... ] }`, которое цвет не принимает — свойство становится `unset`.
    assert_eq!(p_color("p { color: red; color: { [ var(--a) ] } }"), BLUE);
}

#[test]
fn unclosed_var_at_eof_is_closed() {
    // variable-reference-25…29: конец листа закрывает `var(`, строку и комментарий.
    assert_eq!(p_color("p { color: red; --a: green; color: var(--a"), GREEN);
    assert_eq!(p_color("p { color: red; --a: green; color: var(--a /* x"), GREEN);
    assert_eq!(p_color("p { color: red; --a: green; color: var(--a, \""), GREEN);
    assert_eq!(p_color("p { color: red; --a: green; color: var(--a, url(\""), GREEN);
}

#[test]
fn supports_rejects_invalid_var_declaration_values() {
    // variable-supports-09/10/17/21/23/30/41: условие `not (…)` над недопустимым
    // `var()` истинно, а `(color: var(--a))` — поддержано.
    for bad in [
        "(color: var(--a,!))",
        "(color: var(--a,!important))",
        "(color: var(--a) !important !important)",
        "(color: var(--a,;))",
        "(color: var(--a);)",
        "(color: var(1px))",
        "(--a: var(--b,!))",
    ] {
        let css = format!(
            "body {{ color: red }} @supports (color: var(--a)) and (not {bad}) {{ p {{ color: green }} }}"
        );
        let doc = lumen_html_parser::parse("<p>x</p>");
        let sheet = lumen_css_parser::parse(&css);
        let root = crate::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let c = crate::selector_query::find_box_by_selector(&root, &doc, "p").expect("p box").style.color;
        assert_eq!((c.r, c.g, c.b), GREEN, "@supports not {bad}");
    }
}
