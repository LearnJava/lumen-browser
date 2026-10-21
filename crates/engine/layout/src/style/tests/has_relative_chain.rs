//! BUG-1444: `:has()` с составным относительным селектором привязан к субъекту.

use super::*;
use lumen_core::geom::Size;

const VP: Size = Size { width: 800.0, height: 600.0 };

/// Совпадает ли `#s:has(<arg>)` на странице `markup` (цвет правила — зелёный).
fn has_matches(markup: &str, arg: &str) -> bool {
    let doc = lumen_html_parser::parse(markup);
    let sheet = lumen_css_parser::parse(&format!("#s:has({arg}) {{ color: rgb(0, 128, 0); }}"));
    let root = ComputedStyle::root();
    let s_id = doc.find_by_id("s").expect("#s");
    let st = compute_style(&doc, s_id, &sheet, &root, VP, false);
    (st.color.r, st.color.g, st.color.b) == (0, 128, 0)
}

const CHILD_PAGE: &str = r#"<div id="s"><div class="a"><div class="b"><span class="t"></span></div></div><i class="c"></i><i class="d"></i></div>"#;
const SIB_PAGE: &str = r#"<div class="e"><div id="s"></div><div class="a"><div class="b"><span class="t"></span></div></div><i class="c"></i><i class="d"></i></div>"#;

#[test]
fn child_chain_is_anchored_at_subject() {
    for (arg, want) in [
        ("> .a", true),
        ("> .a > .b", true),
        ("> .a .b", true),
        ("> .a > .t", false),
        ("> .b", false),
        ("> .b > .t", false),
        (".a > .b", true),
        (".b .t", true),
        (".b > .t", true),
        ("> .a ~ .c", true),
        ("> .a + .c", true),
        ("> .a + .d", false),
        ("> .c + .d", true),
    ] {
        assert_eq!(has_matches(CHILD_PAGE, arg), want, ":has({arg})");
    }
}

#[test]
fn subject_itself_is_not_part_of_inner_chain() {
    // `.a` в цепочке `.a > .b` обязан быть потомком субъекта, а не им самим.
    let page = r#"<div id="s" class="a"><div class="b"></div></div>"#;
    assert!(!has_matches(page, ".a > .b"));
    assert!(!has_matches(page, "> .a > .b"));
    assert!(has_matches(page, "> .b"));
}

#[test]
fn sibling_chains_are_anchored_at_subject() {
    for (arg, want) in [
        ("+ .a", true),
        ("+ .a + .c", true),
        ("+ .a ~ .d", true),
        ("+ .a > .b", true),
        ("+ .a .t", true),
        ("+ .a > .b > .t", true),
        ("+ .c", false),
        ("+ .b", false),
        ("+ .c + .d", false),
        ("~ .a", true),
        ("~ .a ~ .c", true),
        ("~ .a + .c", true),
        ("~ .c ~ .d", true),
        ("~ .a .t", true),
        ("~ .a > .b", true),
        ("~ .e", false),
        ("~ .a > .t", false),
    ] {
        assert_eq!(has_matches(SIB_PAGE, arg), want, ":has({arg})");
    }
}
