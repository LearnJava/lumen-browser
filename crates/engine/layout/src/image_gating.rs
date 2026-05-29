//! Viewport-gating for image decoding (task 10E.3).
//!
//! Filters [`crate::ImageRequest`] lists so that the shell only starts
//! decoding images whose layout bounding box falls within the current viewport
//! extended by [`LOOKAHEAD_SCREENS`] screen-heights in both vertical directions
//! and by the same factor horizontally.
//!
//! # Integration
//!
//! After `layout()` and `collect_image_requests()`, call [`gate_image_requests`]
//! and pass the returned [`NodeId`] set to the shell's image-load scheduler.
//! The scheduler skips any [`crate::ImageRequest`] whose `node_id` is not in
//! the set, deferring it until the next scroll event triggers a re-gate.

use std::collections::HashSet;

use lumen_core::geom::Size;
use lumen_dom::NodeId;

use crate::box_tree::{BoxKind, LayoutBox};

/// How many additional viewport-heights to include above and below (and left/right
/// of) the current viewport when deciding which images to decode eagerly.
const LOOKAHEAD_SCREENS: f32 = 2.0;

/// Returns the set of [`NodeId`]s for `BoxKind::Image` boxes whose bounding
/// rectangle intersects the *gated viewport*:
///
/// ```text
/// y_min = scroll_y − LOOKAHEAD_SCREENS × viewport.height
/// y_max = scroll_y + (1 + LOOKAHEAD_SCREENS) × viewport.height
/// x_min = scroll_x − LOOKAHEAD_SCREENS × viewport.width
/// x_max = scroll_x + (1 + LOOKAHEAD_SCREENS) × viewport.width
/// ```
///
/// Rects in the layout tree are in document coordinates (origin = document
/// top-left). `scroll_x`/`scroll_y` are the current scroll offsets in CSS px.
///
/// Pass the returned set to the image-load scheduler: decode only those
/// [`crate::ImageRequest`]s whose `node_id` appears in the set.
#[must_use]
pub fn gate_image_requests(
    root: &LayoutBox,
    viewport: Size,
    scroll_x: f32,
    scroll_y: f32,
) -> HashSet<NodeId> {
    let vw = viewport.width;
    let vh = viewport.height;
    let gate_x_min = scroll_x - LOOKAHEAD_SCREENS * vw;
    let gate_x_max = scroll_x + vw * (1.0 + LOOKAHEAD_SCREENS);
    let gate_y_min = scroll_y - LOOKAHEAD_SCREENS * vh;
    let gate_y_max = scroll_y + vh * (1.0 + LOOKAHEAD_SCREENS);

    let mut visible = HashSet::new();
    collect_visible(root, gate_x_min, gate_x_max, gate_y_min, gate_y_max, &mut visible);
    visible
}

fn collect_visible(
    node: &LayoutBox,
    gate_x_min: f32,
    gate_x_max: f32,
    gate_y_min: f32,
    gate_y_max: f32,
    out: &mut HashSet<NodeId>,
) {
    if matches!(node.kind, BoxKind::Image { .. }) {
        let r = node.rect;
        // AABB intersection: rect overlaps gate region on both axes.
        let in_x = r.x < gate_x_max && r.right() > gate_x_min;
        let in_y = r.y < gate_y_max && r.bottom() > gate_y_min;
        if in_x && in_y {
            out.insert(node.node);
        }
    }
    for child in &node.children {
        collect_visible(child, gate_x_min, gate_x_max, gate_y_min, gate_y_max, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;

    const VP: Size = Size { width: 800.0, height: 600.0 };

    /// Parse HTML + CSS and return (Document, full layout tree).
    fn parse_layout(html: &str, css: &str) -> (lumen_dom::Document, LayoutBox) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = crate::box_tree::layout(&doc, &sheet, VP);
        (doc, root)
    }

    /// Node IDs of all images collected from the document.
    fn all_image_ids(doc: &lumen_dom::Document) -> Vec<NodeId> {
        crate::box_tree::collect_image_requests(doc, VP)
            .into_iter()
            .map(|r| r.node_id)
            .collect()
    }

    // ── gating inclusion ────────────────────────────────────────────────────

    #[test]
    fn image_at_top_is_included() {
        // Single image at y ≈ 0 should be within viewport.
        let (doc, root) = parse_layout(
            r#"<img src="a.png" style="display:block;width:100px;height:100px">"#,
            "",
        );
        let ids = gate_image_requests(&root, VP, 0.0, 0.0);
        let all = all_image_ids(&doc);
        assert!(!all.is_empty(), "no images parsed");
        assert!(ids.contains(&all[0]), "top image must be gated in");
    }

    #[test]
    fn image_within_lookahead_below_included() {
        // Image at y ≈ 1100px is within 2 × 600 = 1200px lookahead.
        let css = "div{display:block;height:1100px} img{display:block;width:50px;height:50px}";
        let html = r#"<div></div><img src="b.png">"#;
        let (doc, root) = parse_layout(html, css);
        let ids = gate_image_requests(&root, VP, 0.0, 0.0);
        let all = all_image_ids(&doc);
        assert!(!all.is_empty());
        assert!(ids.contains(&all[0]), "image at ~1100px must be within 2-screen lookahead");
    }

    // ── gating exclusion ────────────────────────────────────────────────────

    #[test]
    fn image_beyond_lookahead_excluded() {
        // Spacer 2000px → image lands at y ≈ 2000. Gate max = 0 + 600*3 = 1800.
        let css = "div{display:block;height:2000px} img{display:block;width:50px;height:50px}";
        let html = r#"<div></div><img src="c.png">"#;
        let (doc, root) = parse_layout(html, css);
        let ids = gate_image_requests(&root, VP, 0.0, 0.0);
        let all = all_image_ids(&doc);
        assert!(!all.is_empty());
        assert!(!ids.contains(&all[0]), "image at ~2000px must be outside 2-screen lookahead");
    }

    #[test]
    fn image_far_above_after_scroll_excluded() {
        // Image at y ≈ 0, scroll_y = 5000. Gate top = 5000 - 1200 = 3800 → image excluded.
        let (doc, root) = parse_layout(
            r#"<img src="d.png" style="display:block;width:100px;height:100px">"#,
            "",
        );
        let ids = gate_image_requests(&root, VP, 0.0, 5000.0);
        let all = all_image_ids(&doc);
        assert!(!all.is_empty());
        assert!(!ids.contains(&all[0]), "image far above current scroll must be excluded");
    }

    // ── scroll-adjusted inclusion ────────────────────────────────────────────

    #[test]
    fn image_above_included_within_lookahead() {
        // Image at y ≈ 0; scroll to 700 (just over one screen). Image is 700px above
        // viewport top; gate_y_min = 700 - 1200 = -500 → image included.
        let (doc, root) = parse_layout(
            r#"<img src="e.png" style="display:block;width:100px;height:100px">"#,
            "",
        );
        let ids = gate_image_requests(&root, VP, 0.0, 700.0);
        let all = all_image_ids(&doc);
        assert!(!all.is_empty());
        assert!(ids.contains(&all[0]), "image one screen above scroll must be included");
    }

    // ── multiple images ──────────────────────────────────────────────────────

    #[test]
    fn only_nearby_images_gated_in() {
        // img1 at y ≈ 0 (included), spacer 2500px, img2 at y ≈ 2500 (excluded).
        let css = "div{display:block;height:2500px} img{display:block;width:50px;height:50px}";
        let html = r#"<img src="f1.png"><div></div><img src="f2.png">"#;
        let (doc, root) = parse_layout(html, css);
        let ids = gate_image_requests(&root, VP, 0.0, 0.0);
        let all = all_image_ids(&doc);
        assert_eq!(all.len(), 2, "expected two images");
        assert!(ids.contains(&all[0]), "first image must be included");
        assert!(!ids.contains(&all[1]), "second image must be excluded");
    }

    // ── no images ────────────────────────────────────────────────────────────

    #[test]
    fn empty_set_when_no_images() {
        let (_doc, root) = parse_layout("<p>hello</p>", "");
        let ids = gate_image_requests(&root, VP, 0.0, 0.0);
        assert!(ids.is_empty());
    }
}
