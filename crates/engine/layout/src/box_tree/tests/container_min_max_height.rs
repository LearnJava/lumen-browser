//! BUG-1314: CSS 2.1 §10.4 / Sizing L3 §5 — the height of a flex / grid container is clamped
//! to `[min-height, max-height]` (and the logical `*-block-size`), as a block's is.

use lumen_core::geom::Size;

use super::super::{layout, BoxKind, LayoutBox};
use crate::style::Display;

/// Height of the container `.c` for `display` and the extra declarations `decl`; the only
/// child is 10px tall.
fn container_height(display: &str, decl: &str) -> f32 {
    let css = format!("body {{ margin: 0 }} .c {{ display: {display}; {decl} }} .i {{ height: 10px }}");
    let doc = lumen_html_parser::parse("<div class='c'><div class='i'></div></div>");
    let sheet = lumen_css_parser::parse(&css);
    let root = layout(&doc, &sheet, Size::new(400.0, 400.0));
    fn find(b: &LayoutBox, d: Display) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::Block) && b.style.display == d && b.children.len() == 1 {
            return Some(b);
        }
        b.children.iter().find_map(|c| find(c, d))
    }
    let d = match display {
        "grid" => Display::Grid,
        _ => Display::Flex,
    };
    find(&root, d).expect("container").rect.height
}

#[test]
fn min_height_raises_container() {
    for d in ["grid", "flex"] {
        assert_eq!(container_height(d, "min-height: 50px"), 50.0, "{d}");
        assert_eq!(container_height(d, "height: 20px; min-height: 50px"), 50.0, "{d}");
        assert_eq!(container_height(d, "min-block-size: 100px"), 100.0, "{d}");
    }
}

#[test]
fn max_height_caps_container() {
    for d in ["grid", "flex"] {
        assert_eq!(container_height(d, "height: 200px; max-height: 50px"), 50.0, "{d}");
        assert_eq!(container_height(d, "height: 200px; max-block-size: 60px"), 60.0, "{d}");
    }
}

#[test]
fn box_sizing_and_padding_are_respected() {
    for d in ["grid", "flex"] {
        assert_eq!(container_height(d, "min-height: 50px; padding: 5px"), 60.0, "{d}");
        assert_eq!(container_height(d, "min-height: 50px; padding: 5px; box-sizing: border-box"), 50.0, "{d}");
    }
}

#[test]
fn min_wins_over_max() {
    for d in ["grid", "flex"] {
        assert_eq!(container_height(d, "min-height: 80px; max-height: 30px"), 80.0, "{d}");
    }
}

#[test]
fn unconstrained_height_is_unchanged() {
    for d in ["grid", "flex"] {
        assert_eq!(container_height(d, ""), 10.0, "{d}");
        assert_eq!(container_height(d, "min-height: 5px; max-height: 500px"), 10.0, "{d}");
    }
}
