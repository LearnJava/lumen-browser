//! CSS Scroll Snap L1 — snap-point finding for the page scroll container.
//!
//! Phase 0 scope: one scroll container = the document viewport. Nested
//! overflow-scroll containers are not yet tracked. For each element with
//! `scroll-snap-align != none`, we compute the Y scroll offset that would
//! place the element's block-start/center/end at the corresponding position
//! in the viewport, then return the candidate closest to `current_y`.
//!
//! Wire-up note for P3 (lumen-shell): call `find_scroll_snap_y` after a
//! scroll gesture ends (WheelEvent phase == Ended, or keyboard scroll settle).
//! Use `scroll-snap-type.strictness` on the *container* to decide whether to
//! snap unconditionally (`Mandatory`) or only within proximity (~30% vh).
//!
//! Example shell integration:
//! ```ignore
//! if let Some(snap_y) = lumen_paint::find_scroll_snap_y(
//!     &self.layout_box, self.scroll_y, self.viewport_height_css()
//! ) {
//!     self.start_smooth_scroll(snap_y);
//! }
//! ```

use lumen_layout::{BoxKind, Display, LayoutBox, ScrollSnapAlignKeyword};

/// CSS Scroll Snap L1 — returns the Y scroll offset to snap to, or `None`
/// if no snap targets exist in `root`.
///
/// `current_y` — current page scroll offset in CSS px.
/// `viewport_h` — viewport height in CSS px.
///
/// The returned value is clamped to `[0, +∞)` but NOT to max-scroll; the
/// caller should clamp to `max_scroll()` after receiving the result.
pub fn find_scroll_snap_y(root: &LayoutBox, current_y: f32, viewport_h: f32) -> Option<f32> {
    let mut candidates: Vec<f32> = Vec::new();
    collect_snap_y(root, viewport_h, &mut candidates);
    if candidates.is_empty() {
        return None;
    }
    candidates
        .into_iter()
        .min_by(|a, b| {
            (a - current_y)
                .abs()
                .partial_cmp(&(b - current_y).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// CSS Scroll Snap L1 — same as [`find_scroll_snap_y`] but restricts candidates
/// to within `proximity_fraction * viewport_h` of `current_y`.
///
/// Implements `scroll-snap-type: proximity` behaviour. For `mandatory`, call
/// [`find_scroll_snap_y`] directly (no proximity filter).
pub fn find_scroll_snap_y_proximity(
    root: &LayoutBox,
    current_y: f32,
    viewport_h: f32,
    proximity_fraction: f32,
) -> Option<f32> {
    let threshold = viewport_h * proximity_fraction;
    let mut candidates: Vec<f32> = Vec::new();
    collect_snap_y(root, viewport_h, &mut candidates);
    candidates
        .into_iter()
        .filter(|&c| (c - current_y).abs() <= threshold)
        .min_by(|a, b| {
            (a - current_y)
                .abs()
                .partial_cmp(&(b - current_y).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn collect_snap_y(b: &LayoutBox, viewport_h: f32, out: &mut Vec<f32>) {
    if matches!(b.kind, BoxKind::Skip) || b.style.display == Display::None {
        return;
    }
    match b.style.scroll_snap_align.block {
        ScrollSnapAlignKeyword::None => {}
        ScrollSnapAlignKeyword::Start => {
            out.push(b.rect.y.max(0.0));
        }
        ScrollSnapAlignKeyword::Center => {
            let snap = b.rect.y - (viewport_h - b.rect.height) * 0.5;
            out.push(snap.max(0.0));
        }
        ScrollSnapAlignKeyword::End => {
            let snap = b.rect.y - (viewport_h - b.rect.height);
            out.push(snap.max(0.0));
        }
    }
    for child in &b.children {
        collect_snap_y(child, viewport_h, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;
    use lumen_layout::layout;

    fn build(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        layout(&doc, &sheet, Size::new(800.0, 600.0))
    }

    #[test]
    fn no_snap_targets_returns_none() {
        let root = build("<div>x</div>", "div { height: 200px; }");
        assert!(find_scroll_snap_y(&root, 0.0, 600.0).is_none());
    }

    #[test]
    fn snap_start_returns_element_top() {
        let root = build(
            "<div class='snap'>x</div>",
            ".snap { height: 200px; scroll-snap-align: start; }",
        );
        let snap = find_scroll_snap_y(&root, 50.0, 600.0);
        // div starts at y=0 → snap_y = 0.
        assert!(snap.is_some());
        assert!((snap.unwrap()).abs() < 1.0, "snap to y=0 for start-aligned element at top");
    }

    #[test]
    fn snap_center_returns_centered_offset() {
        // div height=200, viewport=600 → center snap = 0 - (600-200)*0.5 = -200 → clamped to 0.
        let root = build(
            "<div class='snap'>x</div>",
            ".snap { height: 200px; scroll-snap-align: center; }",
        );
        let snap = find_scroll_snap_y(&root, 0.0, 600.0);
        assert!(snap.is_some());
        assert!((snap.unwrap()).abs() < 1.0, "small element at top: center snap clamped to 0");
    }

    #[test]
    fn snap_chooses_closest_to_current() {
        // Two snap targets at y=0 and y=600. With current_y=400, closest = 600.
        let root = build(
            r#"<div class='a'>x</div><div class='b'>y</div>"#,
            "
                .a { height: 600px; scroll-snap-align: start; }
                .b { height: 600px; scroll-snap-align: start; }
            ",
        );
        let snap = find_scroll_snap_y(&root, 400.0, 600.0);
        assert!(snap.is_some());
        let s = snap.unwrap();
        // Snap targets: a at y=0, b at y=600. Closest to 400 is 600.
        assert!((s - 600.0).abs() < 1.0, "expected 600.0, got {s}");
    }

    #[test]
    fn proximity_snap_filters_by_threshold() {
        // Two snaps at y=0 and y=600. current_y=50, threshold=30%(600)=180px.
        // Snap at 0 is within threshold (50px away), snap at 600 is not (550px).
        let root = build(
            r#"<div class='a'>x</div><div class='b'>y</div>"#,
            "
                .a { height: 600px; scroll-snap-align: start; }
                .b { height: 600px; scroll-snap-align: start; }
            ",
        );
        let snap = find_scroll_snap_y_proximity(&root, 50.0, 600.0, 0.3);
        assert!(snap.is_some());
        let s = snap.unwrap();
        assert!((s).abs() < 1.0, "only snap at 0 is within proximity; got {s}");
    }

    #[test]
    fn proximity_snap_returns_none_when_no_candidate_in_range() {
        let root = build(
            "<div class='snap'>x</div>",
            ".snap { height: 200px; scroll-snap-align: start; }",
        );
        // current_y=500, snap at 0, threshold=30%(600)=180. Distance=500 > 180 → None.
        let snap = find_scroll_snap_y_proximity(&root, 500.0, 600.0, 0.3);
        assert!(snap.is_none());
    }
}
