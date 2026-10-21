//! `@container scroll-state(…)` — разбор `container-type`, оценка условий, `scrollable`.

use crate::box_tree::LayoutBox;
use crate::selector_query::find_box_by_selector;
use crate::style::ContainerType;
use lumen_core::geom::Size;

fn layout_tree(html: &str, css: &str) -> (lumen_dom::Document, LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let tree = crate::layout(&doc, &sheet, Size::new(1024.0, 600.0));
    (doc, tree)
}

fn custom(tree: &LayoutBox, doc: &lumen_dom::Document, sel: &str, name: &str) -> String {
    let b = find_box_by_selector(tree, doc, sel).expect("box");
    b.style.custom_props.get(name).cloned().unwrap_or_default()
}

fn container_type_of(css: &str) -> (ContainerType, bool) {
    let (doc, tree) = layout_tree(r#"<div id="c"></div>"#, css);
    let b = find_box_by_selector(&tree, &doc, "#c").expect("box");
    (b.style.container_type, b.style.container_scroll_state)
}

#[test]
fn container_type_scroll_state_grammar() {
    assert_eq!(container_type_of("#c{container-type:scroll-state}"), (ContainerType::Normal, true));
    assert_eq!(container_type_of("#c{container-type:size scroll-state}"), (ContainerType::Size, true));
    assert_eq!(container_type_of("#c{container-type:scroll-state inline-size}"), (ContainerType::InlineSize, true));
    assert_eq!(container_type_of("#c{container-type:inline-size}"), (ContainerType::InlineSize, false));
    // Недопустимое значение объявление не меняет.
    assert_eq!(container_type_of("#c{container-type:size;container-type:scroll-state scroll-state}"), (ContainerType::Size, false));
    assert_eq!(container_type_of("#c{container-type:normal scroll-state}"), (ContainerType::Normal, false));
    assert_eq!(container_type_of("#c{container-type:size inline-size}"), (ContainerType::Normal, false));
    assert_eq!(container_type_of("#c{container:n / scroll-state}"), (ContainerType::Normal, true));
}

const SCROLLER: &str = "#s { container-type: scroll-state; overflow: auto; width: 100px; height: 50px }
     #t { --top: no; --bottom: no; --none: no; --any: no }
     @container scroll-state(scrollable: top) { #t { --top: yes } }
     @container scroll-state(scrollable: bottom) { #t { --bottom: yes } }
     @container scroll-state(scrollable: none) { #t { --none: yes } }
     @container scroll-state(scrollable) { #t { --any: yes } }";

#[test]
fn scrollable_bottom_when_content_overflows() {
    let (doc, tree) = layout_tree(
        r#"<div id="s"><div id="t"></div><div style="height:300px"></div></div>"#,
        SCROLLER,
    );
    assert_eq!(custom(&tree, &doc, "#t", "--bottom"), "yes");
    assert_eq!(custom(&tree, &doc, "#t", "--top"), "no");
    assert_eq!(custom(&tree, &doc, "#t", "--none"), "no");
    assert_eq!(custom(&tree, &doc, "#t", "--any"), "yes");
}

#[test]
fn not_scrollable_without_overflow_or_user_scrolling() {
    let (doc, tree) = layout_tree(r#"<div id="s"><div id="t"></div></div>"#, SCROLLER);
    assert_eq!(custom(&tree, &doc, "#t", "--none"), "yes");
    assert_eq!(custom(&tree, &doc, "#t", "--any"), "no");
    // `overflow: clip` и `hidden` пользователь не прокручивает.
    for ov in ["clip", "hidden"] {
        let css = format!("{SCROLLER} #s {{ overflow: {ov} }}");
        let (doc, tree) = layout_tree(
            r#"<div id="s"><div id="t"></div><div style="height:300px"></div></div>"#,
            &css,
        );
        assert_eq!(custom(&tree, &doc, "#t", "--none"), "yes", "overflow: {ov}");
        assert_eq!(custom(&tree, &doc, "#t", "--bottom"), "no", "overflow: {ov}");
    }
}

#[test]
fn unknown_condition_and_its_negation_both_fail() {
    let css = "#s { container-type: scroll-state; width: 100px }
         #t { --m: no }
         @container (scrollable: left) or (not (scrollable: left)) { #t { --m: yes } }
         @container scroll-state(scrollable: auto) or (not (scroll-state(scrollable: auto))) { #t { --m: yes } }";
    let (doc, tree) = layout_tree(r#"<div id="s"><div id="t"></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--m"), "no");
}

#[test]
fn known_condition_or_its_negation_applies() {
    let css = "#s { container-type: scroll-state; width: 100px }
         #t { --m: no }
         @container (scroll-state(scrollable)) or (not (scroll-state(scrollable))) { #t { --m: yes } }";
    let (doc, tree) = layout_tree(r#"<div id="s"><div id="t"></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--m"), "yes");
}

#[test]
fn query_picks_container_by_kind_through_the_chain() {
    // Размерный запрос минует ближайший scroll-state контейнер и берёт внешний size.
    let css = "#o { container-type: inline-size; width: 400px }
         #s { container-type: scroll-state }
         #t { --w: no }
         @container (min-width: 300px) { #t { --w: yes } }";
    let (doc, tree) = layout_tree(r#"<div id="o"><div id="s"><div id="t"></div></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--w"), "yes");
}

#[test]
fn named_query_skips_unnamed_nearer_container() {
    let css = "#o { container: outer / inline-size; width: 400px }
         #s { container-type: inline-size; width: 100px }
         #t { --w: no }
         @container outer (min-width: 300px) { #t { --w: yes } }";
    let (doc, tree) = layout_tree(r#"<div id="o"><div id="s"><div id="t"></div></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--w"), "yes");
}
