//! Debug-only geometry/style invariants for the layout tree (DEVX-8a,
//! `docs/tasks/p1-introspection-track.md`).
//!
//! Every check here is a `debug_assert!` and compiles to nothing outside
//! `cfg(debug_assertions)` builds — zero cost in `release`/`dev-release`.
//! A firing assertion is a real engine bug: file it in `BUGS.md` and fix the
//! root cause. Do not loosen a check to make a firing assertion go away — see
//! the DEVX-8a "trap" note in the task doc.

use crate::box_tree::{BoxKind, LayoutBox};
use crate::style::{Display, Overflow, Position};

/// In-flow children may sit fractions of a px outside the parent's border box
/// from sub-pixel rounding in independent length resolutions (e.g. percentage
/// widths rounded on both the parent and the child). Not a tolerance for real
/// overflow — CSS 2.1 layout rounds in `f32`, not exactly.
const GEOMETRY_EPSILON: f32 = 0.5;

/// Walks `root`'s subtree checking two geometry invariants (DEVX-8a):
///
/// - No `NaN`/`±Infinity` in any box's rect or scroll offset.
/// - A block-level, in-flow (`position: static`), **auto-width** child's
///   border box stays horizontally inside its parent's border box, when the
///   parent's `overflow-x` is `visible`. `width: auto` is the one case CSS
///   2.1 §10.3.3 pins to an exact formula (`margin + border + padding +
///   width = containing block width`), so a static auto-width child escaping
///   it without a positioning/overflow/negative-margin escape hatch is
///   always an engine bug. An **explicit** `width` is deliberately excluded:
///   CSS never clamps it to the containing block (an author-specified
///   `width: 300px` inside a `200px` parent is normal, unclamped CSS, found
///   failing this check on ordinary intrinsic-sizing tests before this
///   exclusion was added). The vertical axis is not checked at all — a box's
///   height routinely exceeds its container's specified height
///   (`line-height` taller than a fixed-height box, `auto`-height content
///   overflowing a fixed ancestor, …), which is exactly what `overflow:
///   visible` means, not a bug. `InlineRun`/text boxes are excluded for the
///   same reason.
///
/// Further exclusions found empirically (`cargo test -p lumen-layout` run
/// against this check before they were added): the parent must itself be a
/// plain `BoxKind::Block` with `display: block` — `flex`/`grid`/table/form-
/// control containers use their own sizing algorithms (flex automatic
/// minimum size, grid auto-fill with too few tracks, native `<select>`
/// popup internals, …) where a static auto-width child legitimately doesn't
/// fill the container. The child must not carry an SVG group transform
/// (`<g>` and friends are positioned in SVG's own coordinate system via
/// `svg_group_transform`, not resolved against a CSS containing block, and
/// are left at `Rect::ZERO` by the box builder until paint applies the
/// transform).
pub(crate) fn check_geometry(root: &LayoutBox) {
    check_finite(root);
    check_containment(root);
}

fn check_finite(b: &LayoutBox) {
    debug_assert!(
        b.rect.x.is_finite() && b.rect.y.is_finite() && b.rect.width.is_finite() && b.rect.height.is_finite(),
        "DEVX-8a: non-finite layout rect on node={:?} kind={:?} rect={:?}",
        b.node,
        b.kind,
        b.rect
    );
    debug_assert!(
        b.scroll_x.is_finite() && b.scroll_y.is_finite(),
        "DEVX-8a: non-finite scroll offset on node={:?} scroll=({}, {})",
        b.node,
        b.scroll_x,
        b.scroll_y
    );
    for child in &b.children {
        check_finite(child);
    }
}

fn check_containment(b: &LayoutBox) {
    let parent_is_plain_block =
        matches!(b.kind, BoxKind::Block) && b.style.display == Display::Block;
    let parent_unclipped_x = b.style.overflow_x == Overflow::Visible;
    for child in &b.children {
        if parent_is_plain_block
            && parent_unclipped_x
            && child.style.position == Position::Static
            && child.style.width.is_none()
            && child.svg_group_transform.is_none()
            && matches!(child.kind, BoxKind::Block)
        {
            debug_assert!(
                child.rect.x + GEOMETRY_EPSILON >= b.rect.x
                    && child.rect.right() <= b.rect.right() + GEOMETRY_EPSILON,
                "DEVX-8a: in-flow block child escapes parent border box horizontally, \
                 without overflow/positioning: parent node={:?} rect={:?}; child node={:?} rect={:?}",
                b.node,
                b.rect,
                child.node,
                child.rect
            );
        }
        check_containment(child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::box_tree::layout;
    use lumen_core::geom::Size;

    fn layout_html(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        layout(&doc, &sheet, Size::new(800.0, 600.0))
    }

    #[test]
    fn plain_block_flow_has_finite_geometry() {
        let root = layout_html(
            "<html><body><div><p>hello</p></div></body></html>",
            "div { width: 200px; } p { margin: 4px; }",
        );
        check_geometry(&root);
    }

    #[test]
    fn overflow_hidden_child_is_exempt_from_containment() {
        // A child wider than its overflow:hidden parent is normal CSS, not a bug —
        // clipping happens in paint, the box geometry itself is unclipped.
        let root = layout_html(
            "<html><body><div class=\"c\"><span class=\"w\">wide</span></div></body></html>",
            ".c { width: 50px; overflow: hidden; } .w { width: 500px; display: block; }",
        );
        check_geometry(&root);
    }

    #[test]
    fn absolutely_positioned_child_is_exempt_from_containment() {
        let root = layout_html(
            "<html><body><div class=\"c\"><div class=\"p\">x</div></div></body></html>",
            ".c { width: 50px; height: 50px; position: relative; } \
             .p { position: absolute; left: 200px; top: 200px; width: 10px; height: 10px; }",
        );
        check_geometry(&root);
    }
}
