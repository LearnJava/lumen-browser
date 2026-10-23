//! `@container style(…)` — любой элемент является style-контейнером (BUG-1496):
//! без `container-type`, по имени, по родителю, на `:root`, на бокс-less предке.

use crate::box_tree::LayoutBox;
use crate::selector_query::find_box_by_selector;
use lumen_core::geom::Size;

fn layout_tree(html: &str, css: &str) -> (lumen_dom::Document, LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let tree = crate::layout(&doc, &sheet, Size::new(1024.0, 600.0));
    (doc, tree)
}

/// `#t`'s `--hit` after `@container <query> { #t { --hit: yes } }`.
fn hit(html: &str, css: &str, query: &str) -> bool {
    let css = format!("#t {{ --hit: no }} {css} @container {query} {{ #t {{ --hit: yes }} }}");
    let (doc, tree) = layout_tree(html, &css);
    let b = find_box_by_selector(&tree, &doc, "#t").expect("box");
    b.style.custom_props.get("--hit").map(String::as_str) == Some("yes")
}

#[test]
fn container_without_container_type() {
    let html = r#"<div class="c"><div id="t"></div></div>"#;
    assert!(hit(html, ".c { --x: y }", "style(--x: y)"));
    assert!(!hit(html, ".c { --x: z }", "style(--x: y)"));
}

#[test]
fn named_container_without_container_type() {
    let html = r#"<div class="c"><div id="t"></div></div>"#;
    assert!(hit(html, ".c { container-name: n; --x: y }", "n style(--x: y)"));
    assert!(!hit(html, ".c { container-name: m; --x: y }", "n style(--x: y)"));
}

#[test]
fn parent_with_inline_custom_property() {
    assert!(hit(r#"<div style="--x:y"><div id="t"></div></div>"#, "", "style(--x: y)"));
}

#[test]
fn root_element_is_a_container() {
    assert!(hit(r#"<div><div id="t"></div></div>"#, ":root { --x: y }", "style(--x: y)"));
}

#[test]
fn unnamed_query_takes_the_nearest_ancestor() {
    // The parent resets `--x`, the grandparent still has it: the nearest one answers.
    let html = r#"<div class="a"><div class="b"><div id="t"></div></div></div>"#;
    assert!(!hit(html, ".a { --x: y } .b { --x: z }", "style(--x: y)"));
    assert!(hit(html, ".a { --x: z } .b { --x: y }", "style(--x: y)"));
}

#[test]
fn named_query_skips_unnamed_ancestors() {
    let html = r#"<div class="a"><div class="b"><div id="t"></div></div></div>"#;
    assert!(hit(html, ".a { container-name: n; --x: y } .b { --x: z }", "n style(--x: y)"));
}

#[test]
fn size_container_keeps_working_with_style_queries_in_the_sheet() {
    let html = r#"<div class="c"><div class="m"><div id="t"></div></div></div>"#;
    let css = ".c { container-type: inline-size; width: 300px; --x: y } .m { --x: z }";
    assert!(hit(html, css, "(min-width: 100px)"));
    assert!(!hit(html, css, "(min-width: 400px)"));
    // One query container answers every feature of a query: the nearest size one here.
    assert!(hit(html, ".c { container-type: inline-size; width: 300px; --x: y } .m { --x: z }", "(min-width: 100px) and style(--x: y)"));
    assert!(!hit(html, ".c { container-type: inline-size; width: 300px } .m { --x: y }", "(min-width: 100px) and style(--x: y)"));
}

#[test]
fn standard_property_of_a_plain_parent() {
    let html = r#"<div class="c"><div id="t"></div></div>"#;
    assert!(hit(html, ".c { color: red }", "style(color: red)"));
    assert!(!hit(html, ".c { color: blue }", "style(color: red)"));
}

#[test]
fn boxless_inline_parent_is_a_container() {
    let html = r#"<p><span class="c"><b id="t" style="display:block">x</b></span></p>"#;
    assert!(hit(html, ".c { --x: y }", "style(--x: y)"));
}

#[test]
fn custom_property_set_by_container_rule_is_seen_by_the_next_level() {
    let html = r#"<div class="c"><div class="m"><div id="t"></div></div></div>"#;
    let css = ".c { --x: y } @container style(--x: y) { .m { --seen: 1 } }";
    assert!(hit(html, css, "style(--seen: 1)"));
}

#[test]
fn query_value_substitutes_var_against_the_container() {
    let html = r#"<div class="c"><div id="t"></div></div>"#;
    let css = ".c { --foo: baz; --bar: baz }";
    assert!(hit(html, css, "style(--foo: var(--bar))"));
    assert!(!hit(html, css, "style(--foo: var(--unknown))"));
    assert!(hit(html, css, "not style(--foo: var(--unknown))"));
    assert!(hit(html, css, "style(--foo: var(--unknown, baz))"));
    assert!(!hit(html, css, "style(--foo: var(--unknown, nomatch))"));
    // An unresolvable reference is guaranteed-invalid: equal to an unset property.
    assert!(hit(html, css, "style(--nothing: var(--unknown))"));
    // A property that refers to itself.
    assert!(hit(html, ".c { --foo: bar }", "style(--foo: var(--foo))"));
}

#[test]
fn important_in_the_query_value_is_ignored() {
    let html = r#"<div class="c"><div id="t"></div></div>"#;
    assert!(hit(html, ".c { --foo: bar }", "style(--foo: bar !important)"));
    assert!(!hit(html, ".c { --foo: baz }", "style(--foo: bar !important)"));
}
