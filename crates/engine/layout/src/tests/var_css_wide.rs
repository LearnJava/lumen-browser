//! CSS-wide ключевые слова у пользовательских свойств и запасное значение
//! `var()` при guaranteed-invalid / цикле (BUG-1503, CSS Cascade 4 §7.3,
//! CSS Variables L1 §3).

use lumen_core::geom::Size;

/// Цвет `<p>` после `css` (на странице `<p>` внутри `<body>`).
fn p_color(css: &str) -> (u8, u8, u8) {
    let doc = lumen_html_parser::parse("<p>x</p>");
    let sheet = lumen_css_parser::parse(css);
    let root = crate::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let c = crate::selector_query::find_box_by_selector(&root, &doc, "p").expect("p box").style.color;
    (c.r, c.g, c.b)
}

const GREEN: (u8, u8, u8) = (0, 128, 0);
const BLACK: (u8, u8, u8) = (0, 0, 0);

#[test]
fn initial_makes_fallback_apply() {
    assert_eq!(p_color("p { --a: initial; color: var(--a, green) }"), GREEN);
    assert_eq!(p_color("body { --a: crimson } p { --a: initial; color: var(--a, green) }"), GREEN);
}

#[test]
fn inherit_and_unset_at_root_are_guaranteed_invalid() {
    assert_eq!(p_color("body { --a: inherit; color: var(--a, green) }"), GREEN);
    assert_eq!(p_color("body { --a: unset; color: var(--a, green) }"), GREEN);
}

#[test]
fn inherit_and_unset_take_parent_value() {
    assert_eq!(p_color("body { --a: green } p { --a: inherit; color: var(--a) }"), GREEN);
    assert_eq!(p_color("body { --a: green } p { --a: unset; color: var(--a) }"), GREEN);
}

#[test]
fn cycle_uses_fallback() {
    assert_eq!(p_color("p { --a: var(--b); --b: var(--a); color: var(--a, green) }"), GREEN);
    assert_eq!(p_color("p { --a: crimson; --a: var(--a); color: var(--a, green) }"), GREEN);
}

#[test]
fn fallback_inside_cycle_does_not_rescue_it() {
    assert_eq!(
        p_color("p { --a: var(--b, crimson); --b: var(--a, crimson); color: var(--a, green) }"),
        GREEN
    );
}

#[test]
fn property_depending_on_cycle_is_invalid_too() {
    assert_eq!(
        p_color("p { --a: var(--b); --b: var(--a); --d: var(--a); color: var(--d, green) }"),
        GREEN
    );
}

#[test]
fn unresolvable_reference_makes_fallback_apply() {
    assert_eq!(p_color("p { --a: var(--missing); color: var(--a, green) }"), GREEN);
}

#[test]
fn plain_chain_and_no_fallback_unchanged() {
    assert_eq!(p_color("p { --a: green; --b: var(--a); color: var(--b, crimson) }"), GREEN);
    assert_eq!(p_color("p { --a: initial; color: var(--a) }"), BLACK);
}
