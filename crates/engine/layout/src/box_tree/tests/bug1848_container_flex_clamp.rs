//! BUG-1848: the container-query second pass re-laid out a query container's children as a
//! block stack, so a `flex: 1; min-height: 0; overflow-y: auto` scroller inside a
//! `container-type: inline-size` flex column grew to its content height
//! (portal.nextera.ru: `.messages-area` 15286 px inside an 845 px viewport; the wheel then
//! fell through to the root scroller and slid the whole window away). The main pass already
//! produced the correct flex geometry — the second pass must keep it for containers whose
//! children are laid out by their own algorithm, and keep the bespoke block stack only for
//! block-flow containers.

use lumen_core::geom::Size;

use super::super::{layout, LayoutBox};
use crate::style::Overflow;

const HTML: &str = "<body>\
<div class='screen'>\
<div class='cf'>\
<div class='col'>\
<div class='msg'></div><div class='msg'></div><div class='msg'></div><div class='msg'></div><div class='msg'></div>\
</div>\
<div class='composer'></div>\
</div>\
</div>\
</body>";

const BASE_CSS: &str = "body { margin: 0 } \
.screen { display: flex; flex-direction: column; height: 600px; overflow: hidden } \
.cf { flex: 1 1 0px; min-height: 0; overflow: hidden; display: flex; flex-direction: column } \
.col { flex: 1 1 0px; min-height: 0; overflow-y: auto; display: flex; flex-direction: column } \
.msg { height: 150px } \
.composer { height: 40px }";

/// Search that reports the parent box of the scroller — `.col`, the page's only
/// `overflow-y: auto` box, and `.cf` its flex-column parent.
fn scroller_with_parent(b: &LayoutBox) -> Option<(&LayoutBox, &LayoutBox)> {
    for child in &b.children {
        if child.style.overflow_y == Overflow::Auto {
            return Some((child, b));
        }
        if let Some(found) = scroller_with_parent(child) {
            return Some(found);
        }
    }
    None
}

/// Height of `.col` and y of `.composer` (the last child of `.cf`).
fn measure(css: &str) -> (f32, f32) {
    let root = layout(
        &lumen_html_parser::parse(HTML),
        &lumen_css_parser::parse(css),
        Size::new(800.0, 600.0),
    );
    let (col, parent) = scroller_with_parent(&root).expect("scroller .col");
    let col_h = col.rect.height;
    let composer_y = parent.children.last().map(|c| c.rect.y).expect("composer");
    (col_h, composer_y)
}

#[test]
fn container_rule_keeps_flex_clamp() {
    let css = BASE_CSS.to_string()
        + ".cf { container: pp / inline-size } \
            @container pp (min-width: 100px) { .msg { background: #def } }";
    let (col_h, composer_y) = measure(&css);
    assert_eq!(col_h, 560.0, "col must stay clamped to 600-40, not grow to content 750");
    assert_eq!(composer_y, 560.0, "composer must sit right below the clamped col");
}

#[test]
fn container_without_rules_keeps_flex_clamp() {
    let css = BASE_CSS.to_string() + ".cf { container: pp / inline-size }";
    let (col_h, composer_y) = measure(&css);
    assert_eq!(col_h, 560.0, "{col_h}");
    assert_eq!(composer_y, 560.0, "{composer_y}");
}

#[test]
fn size_container_rule_keeps_flex_clamp() {
    let css = BASE_CSS.to_string()
        + ".cf { container: pp / size } \
            @container pp (min-width: 100px) { .msg { background: #def } }";
    let (col_h, composer_y) = measure(&css);
    assert_eq!(col_h, 560.0, "{col_h}");
    assert_eq!(composer_y, 560.0, "{composer_y}");
}

#[test]
fn block_container_still_stacks_children() {
    // Block-flow containers keep the bespoke stacking pass: `.col` is a plain auto-height
    // block here (750 px of content), so the composer must stack below the content.
    let css = BASE_CSS.to_string()
        + ".cf { display: block; height: 600px; overflow: hidden; container: pp / inline-size } \
            .col { display: block; flex: none } \
            @container pp (min-width: 100px) { .msg { background: #def } }";
    let (col_h, composer_y) = measure(&css);
    assert_eq!(col_h, 750.0, "{col_h}");
    assert_eq!(composer_y, 750.0, "{composer_y}");
}
