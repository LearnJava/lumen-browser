//! Единицы `cqw`/`cqh`/`cqi`/`cqb`/`cqmin`/`cqmax` (CSS Conditional 5 §container-lengths, BUG-1499):
//! разрешаются без `@container` в листе, в `padding`/`margin`, а вне контейнера — как small viewport.

use crate::box_tree::LayoutBox;
use crate::selector_query::find_box_by_selector;
use lumen_core::geom::Size;

fn tree(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = crate::layout(&doc, &sheet, Size::new(1024.0, 768.0));
    find_box_by_selector(&root, &doc, "#t").expect("box #t").clone()
}

const CONTAINER: &str = ".c { container-type: size; width: 200px; height: 100px; }";

#[test]
fn units_resolve_without_container_rule() {
    let t = tree(r#"<div class="c"><div id="t"></div></div>"#, &format!("{CONTAINER} #t {{ width: 10cqw; height: 10cqh; }}"));
    assert!((t.rect.width - 20.0).abs() < 0.5, "width {}", t.rect.width);
    assert!((t.rect.height - 10.0).abs() < 0.5, "height {}", t.rect.height);
}

#[test]
fn units_resolve_with_container_rule() {
    let css = format!("{CONTAINER} #t {{ width: 10cqw; height: 10cqh; }} @container (min-width: 1px) {{ #t {{ color: red }} }}");
    let t = tree(r#"<div class="c"><div id="t"></div></div>"#, &css);
    assert!((t.rect.width - 20.0).abs() < 0.5, "width {}", t.rect.width);
    assert!((t.rect.height - 10.0).abs() < 0.5, "height {}", t.rect.height);
}

#[test]
fn padding_and_margin_resolve_against_container() {
    for rule in [true, false] {
        let at = if rule { "@container (min-width: 1px) { #t { color: red } }" } else { "" };
        let css = format!("{CONTAINER} #t {{ padding-left: 10cqw; margin-left: 10cqh; width: 50px; height: 5px; }} {at}");
        let t = tree(r#"<div class="c"><div id="t"></div></div>"#, &css);
        assert!((t.rect.width - 70.0).abs() < 0.5, "rule={rule} width {}", t.rect.width);
        // 8px body margin + 10cqh of the 100px container.
        assert!((t.rect.x - 18.0).abs() < 0.5, "rule={rule} x {}", t.rect.x);
    }
}

#[test]
fn units_without_container_fall_back_to_small_viewport() {
    let t = tree(r#"<div id="t"></div>"#, "#t { width: 10cqw; height: 10cqh; }");
    assert!((t.rect.width - 102.4).abs() < 0.5, "width {}", t.rect.width);
    assert!((t.rect.height - 76.8).abs() < 0.5, "height {}", t.rect.height);
}

#[test]
fn inline_size_container_block_units_fall_back_to_viewport() {
    let css = ".c { container-type: inline-size; width: 200px; } #t { width: 10cqw; height: 10cqh; }";
    let t = tree(r#"<div class="c"><div id="t"></div></div>"#, css);
    assert!((t.rect.width - 20.0).abs() < 0.5, "width {}", t.rect.width);
    assert!((t.rect.height - 76.8).abs() < 0.5, "height {}", t.rect.height);
}

#[test]
fn computed_padding_and_width_resolve_against_container() {
    for rule in [true, false] {
        let at = if rule { "@container (min-width: 1px) { #t { color: red } }" } else { "" };
        let css = format!("{CONTAINER} #t {{ padding-left: 10cqi; width: 10cqw; height: 10cqb; }} {at}");
        let doc = lumen_html_parser::parse(r#"<div class="c"><div id="t"></div></div>"#);
        let sheet = lumen_css_parser::parse(&css);
        let vp = Size::new(1024.0, 768.0);
        let root = crate::layout(&doc, &sheet, vp);
        let nid = crate::find_first_dom_node_by_selector(&doc, "#t").expect("#t").index() as u32;
        let styles = crate::collect_computed_styles(&root, &doc, None, vp);
        let m = &styles[&nid];
        let prop = |n: &str| m.get(n).map_or("<absent>", String::as_str);
        assert_eq!(prop("padding-left"), "20px", "rule={rule}");
        assert_eq!(prop("width"), "20px", "rule={rule}");
        assert_eq!(prop("height"), "10px", "rule={rule}");
    }
}

/// `container-units-basic.html`: an `inline-size` container inside a `size` one — `cqi` is the
/// inner one's width, `cqb` comes from the outer one (the inner does not contain its block size).
#[test]
fn nested_inline_size_container_takes_block_axis_from_outer_size_container() {
    let html = r#"<div class="inline outer"><div class="size outer"><div class="inline inner"><div id="t"></div></div></div></div>"#;
    let css = ".inline { container-type: inline-size; } .size { container-type: size; }
        .inline.outer { width: 500px; } .size.outer { height: 400px; } .inline.inner { width: 300px; }
        #t { padding-left: 10cqi; width: 10cqb; height: 10cqmin; }";
    for rule in [true, false] {
        let at = if rule { "@container (min-width: 1px) { #t { color: red } }" } else { "" };
        let css = format!("{css} {at}");
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(&css);
        let vp = Size::new(1024.0, 768.0);
        let root = crate::layout(&doc, &sheet, vp);
        let t = find_box_by_selector(&root, &doc, "#t").expect("box #t");
        assert!((t.rect.width - 70.0).abs() < 0.5, "rule={rule} width {} (30 padding + 40)", t.rect.width);
        assert!((t.rect.height - 30.0).abs() < 0.5, "rule={rule} height {}", t.rect.height);
        let nid = crate::find_first_dom_node_by_selector(&doc, "#t").expect("#t").index() as u32;
        let styles = crate::collect_computed_styles(&root, &doc, None, vp);
        let m = &styles[&nid];
        assert_eq!(m.get("padding-left").map(String::as_str), Some("30px"), "rule={rule}");
        assert_eq!(m.get("width").map(String::as_str), Some("40px"), "rule={rule}");
    }
}
