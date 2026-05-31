//! Scroll-discard: evict decoded CPU-side images that are far outside the
//! current viewport (ADR-008 §10E.4).
//!
//! After every scroll or layout update the shell calls
//! [`discard_offscreen_images`].  Any image whose layout box is outside the
//! viewport ± `LOOKAHEAD_SCREENS` (defined in `lumen_layout::gate_image_requests`)
//! is removed from the CPU decode cache.  The GPU texture in the renderer is not
//! touched — only RAM is freed.  If the image scrolls back into view it will be
//! re-decoded from disk on the next lazy or full page load.

use std::collections::HashSet;

use lumen_core::geom::Size;
use lumen_dom::NodeId;
use lumen_image::{ImageDecodeCache, ImageKey};
use lumen_layout::{gate_image_requests, BoxKind, LayoutBox};

/// Drop CPU-decoded images for all `BoxKind::Image` boxes that are NOT in the
/// set returned by [`gate_image_requests`].
///
/// Does nothing if `cache` is empty or `root` has no image boxes.
pub fn discard_offscreen_images(
    cache: &mut ImageDecodeCache,
    root: &LayoutBox,
    viewport: Size,
    scroll_x: f32,
    scroll_y: f32,
) {
    if cache.is_empty() {
        return;
    }

    let visible: HashSet<NodeId> = gate_image_requests(root, viewport, scroll_x, scroll_y);

    let mut srcs: Vec<String> = Vec::new();
    collect_offscreen_srcs(root, &visible, &mut srcs);

    for src in srcs {
        cache.remove(&ImageKey::new(src));
    }
}

/// Recursively collect `src` strings for image boxes whose node is NOT in `visible`.
fn collect_offscreen_srcs(node: &LayoutBox, visible: &HashSet<NodeId>, out: &mut Vec<String>) {
    if let BoxKind::Image { src, .. } = &node.kind
        && !visible.contains(&node.node)
    {
        out.push(src.clone());
    }
    for child in &node.children {
        collect_offscreen_srcs(child, visible, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;
    use lumen_image::{Image, PixelFormat};

    const VP: Size = Size { width: 800.0, height: 600.0 };

    fn make_image(w: u32, h: u32) -> Image {
        Image {
            width: w,
            height: h,
            format: PixelFormat::Rgba8,
            data: vec![0u8; w as usize * h as usize * 4],
            icc_profile: None,
        }
    }

    fn parse_layout(html: &str, css: &str) -> (lumen_dom::Document, LayoutBox) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = lumen_layout::box_tree::layout(&doc, &sheet, VP);
        (doc, root)
    }

    // ── discard ──────────────────────────────────────────────────────────────

    #[test]
    fn offscreen_image_removed_from_cache() {
        // Spacer 2000px → image at y ≈ 2000, outside 3-screen gate (1800px).
        let css = "div{display:block;height:2000px} img{display:block;width:50px;height:50px}";
        let html = r#"<div></div><img src="far.png">"#;
        let (_, root) = parse_layout(html, css);

        let mut cache = ImageDecodeCache::new();
        cache.insert(ImageKey::new("far.png"), make_image(50, 50));
        assert!(cache.contains(&ImageKey::new("far.png")));

        discard_offscreen_images(&mut cache, &root, VP, 0.0, 0.0);

        assert!(
            !cache.contains(&ImageKey::new("far.png")),
            "image beyond gate must be evicted"
        );
    }

    #[test]
    fn visible_image_stays_in_cache() {
        // Image at y ≈ 0 — in viewport.
        let html = r#"<img src="near.png" style="display:block;width:50px;height:50px">"#;
        let (_, root) = parse_layout(html, "");

        let mut cache = ImageDecodeCache::new();
        cache.insert(ImageKey::new("near.png"), make_image(50, 50));

        discard_offscreen_images(&mut cache, &root, VP, 0.0, 0.0);

        assert!(cache.contains(&ImageKey::new("near.png")), "visible image must remain");
    }

    #[test]
    fn only_far_image_evicted_near_kept() {
        // img1 at y ≈ 0 (visible), spacer 2500px, img2 at y ≈ 2500 (offscreen).
        let css = "div{display:block;height:2500px} img{display:block;width:50px;height:50px}";
        let html = r#"<img src="near.png"><div></div><img src="far.png">"#;
        let (_, root) = parse_layout(html, css);

        let mut cache = ImageDecodeCache::new();
        cache.insert(ImageKey::new("near.png"), make_image(50, 50));
        cache.insert(ImageKey::new("far.png"), make_image(50, 50));

        discard_offscreen_images(&mut cache, &root, VP, 0.0, 0.0);

        assert!(cache.contains(&ImageKey::new("near.png")), "near image stays");
        assert!(!cache.contains(&ImageKey::new("far.png")), "far image evicted");
    }

    #[test]
    fn empty_cache_noop() {
        let (_, root) = parse_layout("<p>no images</p>", "");
        let mut cache = ImageDecodeCache::new();
        // Must not panic on empty cache.
        discard_offscreen_images(&mut cache, &root, VP, 0.0, 0.0);
        assert!(cache.is_empty());
    }

    #[test]
    fn no_images_in_layout_noop() {
        let (_, root) = parse_layout("<p>text only</p>", "");
        let mut cache = ImageDecodeCache::new();
        cache.insert(ImageKey::new("bg.png"), make_image(10, 10));
        // Layout has no image boxes; key "bg.png" is unknown to gating — stays.
        discard_offscreen_images(&mut cache, &root, VP, 0.0, 0.0);
        assert!(cache.contains(&ImageKey::new("bg.png")), "unknown bg key untouched");
    }

    // ── remove ────────────────────────────────────────────────────────────────

    #[test]
    fn remove_updates_used_bytes() {
        let mut cache = ImageDecodeCache::new();
        cache.insert(ImageKey::new("a.png"), make_image(100, 100));
        let before = cache.used_bytes();
        assert!(before > 0);
        cache.remove(&ImageKey::new("a.png"));
        assert_eq!(cache.used_bytes(), 0);
    }

    #[test]
    fn remove_absent_key_returns_false() {
        let mut cache = ImageDecodeCache::new();
        assert!(!cache.remove(&ImageKey::new("ghost.png")));
    }
}
