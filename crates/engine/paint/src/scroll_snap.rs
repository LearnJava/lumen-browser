//! CSS Scroll Snap L1 — snap-point finding for the page scroll container.
//!
//! Phase 0 scope: one scroll container = the document viewport. Nested
//! overflow-scroll containers are not yet tracked. For each element with
//! `scroll-snap-align != none`, we compute the Y scroll offset that would
//! place the element's block-start/center/end at the corresponding position
//! in the viewport, then return the candidate closest to `current_y`.
//!
//! CSS Scroll Snap L1 §5 insets (block axis): the target's snap area is its
//! border box outset by the target's own `scroll-margin`; the container's
//! snapport is the viewport inset by the container's `scroll-padding`. Both are
//! applied here — `scroll-margin-top`/`-bottom` from each snap target and
//! `scroll-padding-top`/`-bottom` from the root scroll container. The inline
//! (X) axis has no snap path yet, so the `-left`/`-right` values are inert.
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
    let (pad_top, pad_bottom) = container_block_padding(root);
    let mut candidates: Vec<f32> = Vec::new();
    collect_snap_y(root, viewport_h, pad_top, pad_bottom, &mut candidates);
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
    let (pad_top, pad_bottom) = container_block_padding(root);
    let mut candidates: Vec<f32> = Vec::new();
    collect_snap_y(root, viewport_h, pad_top, pad_bottom, &mut candidates);
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

/// CSS Scroll Snap L1 §5 — the block-axis `scroll-padding` of the document
/// scroll container, which insets the snapport start/end edges. Returns
/// `(scroll-padding-top, scroll-padding-bottom)` in CSS px. `auto` resolves to
/// `0` during parsing, so the stored value is directly usable here.
///
/// The `root` box is the anonymous document/viewport box; the viewport's
/// scroll-padding propagates from the root element (`:root` / `html`), which is
/// `root`'s first block-level child. Falls back to `root`'s own style when
/// there is no such child (e.g. a bare box passed directly in tests).
fn container_block_padding(root: &LayoutBox) -> (f32, f32) {
    let src = root
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::Block))
        .unwrap_or(root);
    (src.style.scroll_padding_top, src.style.scroll_padding_bottom)
}

fn collect_snap_y(b: &LayoutBox, viewport_h: f32, pad_top: f32, pad_bottom: f32, out: &mut Vec<f32>) {
    if matches!(b.kind, BoxKind::Skip) || b.style.display == Display::None {
        return;
    }
    // CSS Scroll Snap L1 §5: the snap area is the target's border box outset by
    // its own `scroll-margin` (block axis: top/bottom); the snapport is the
    // container viewport inset by `scroll-padding`. The alignment maps the snap
    // area's start/center/end onto the snapport's start/center/end.
    let area_start = b.rect.y - b.style.scroll_margin_top;
    let area_end = b.rect.y + b.rect.height + b.style.scroll_margin_bottom;
    match b.style.scroll_snap_align.block {
        ScrollSnapAlignKeyword::None => {}
        ScrollSnapAlignKeyword::Start => {
            // snapport start = offset + pad_top → offset = area_start - pad_top.
            out.push((area_start - pad_top).max(0.0));
        }
        ScrollSnapAlignKeyword::Center => {
            // snapport center = offset + (pad_top + viewport_h - pad_bottom)/2.
            let area_center = (area_start + area_end) * 0.5;
            let snapport_center = (pad_top + viewport_h - pad_bottom) * 0.5;
            out.push((area_center - snapport_center).max(0.0));
        }
        ScrollSnapAlignKeyword::End => {
            // snapport end = offset + viewport_h - pad_bottom → offset =
            // area_end - (viewport_h - pad_bottom).
            out.push((area_end - (viewport_h - pad_bottom)).max(0.0));
        }
    }
    for child in &b.children {
        collect_snap_y(child, viewport_h, pad_top, pad_bottom, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;
    use lumen_layout::layout;

    fn build(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        // Neutralise UA `body { margin: 8px }` (HTML Rendering §14.3.3, BUG-204)
        // so snap offsets are measured from the page origin.
        let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
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

    // ── scroll-margin / scroll-padding insets (CSS Scroll Snap L1 §5) ──────────

    #[test]
    fn start_snap_honors_scroll_margin_top() {
        // Target at y=300 with scroll-margin-top:40 → snap area starts at 260,
        // so start-alignment snaps to 260 (40px above the plain 300).
        let root = build(
            "<div class='sp'>x</div><div class='snap'>y</div>",
            "
                .sp { height: 300px; }
                .snap { height: 200px; scroll-margin-top: 40px; scroll-snap-align: start; }
            ",
        );
        let s = find_scroll_snap_y(&root, 260.0, 600.0).unwrap();
        assert!((s - 260.0).abs() < 1.0, "expected 260.0, got {s}");
    }

    #[test]
    fn end_snap_honors_scroll_margin_bottom() {
        // Target at y=600 h=200, scroll-margin-bottom:40 → area end = 840.
        // end-alignment: offset = 840 - (600 - 0) = 240 (40px past the plain 200).
        let root = build(
            "<div class='sp'>x</div><div class='snap'>y</div>",
            "
                .sp { height: 600px; }
                .snap { height: 200px; scroll-margin-bottom: 40px; scroll-snap-align: end; }
            ",
        );
        let s = find_scroll_snap_y(&root, 240.0, 600.0).unwrap();
        assert!((s - 240.0).abs() < 1.0, "expected 240.0, got {s}");
    }

    #[test]
    fn center_snap_honors_asymmetric_scroll_margin() {
        // y=300 h=200, margin-top:20 margin-bottom:60 → area [280, 560], center 420.
        // snapport center (no padding) = 300 → offset 120 (plain center would be 100).
        let root = build(
            "<div class='sp'>x</div><div class='snap'>y</div>",
            "
                .sp { height: 300px; }
                .snap { height: 200px; scroll-margin-top: 20px; scroll-margin-bottom: 60px;
                        scroll-snap-align: center; }
            ",
        );
        let s = find_scroll_snap_y(&root, 120.0, 600.0).unwrap();
        assert!((s - 120.0).abs() < 1.0, "expected 120.0, got {s}");
    }

    #[test]
    fn start_snap_honors_container_scroll_padding() {
        // scroll-padding-top:50 on the root element insets the snapport start,
        // so a start-aligned target at y=300 snaps to 250 (50px earlier).
        let root = build(
            "<div class='sp'>x</div><div class='snap'>y</div>",
            "
                html { scroll-padding-top: 50px; }
                .sp { height: 300px; }
                .snap { height: 200px; scroll-snap-align: start; }
            ",
        );
        let s = find_scroll_snap_y(&root, 250.0, 600.0).unwrap();
        assert!((s - 250.0).abs() < 1.0, "expected 250.0, got {s}");
    }

    #[test]
    fn start_snap_combines_margin_and_padding() {
        // area_start = 400 - 30 = 370; pad_top = 50 → offset = 320.
        let root = build(
            "<div class='sp'>x</div><div class='snap'>y</div>",
            "
                html { scroll-padding-top: 50px; }
                .sp { height: 400px; }
                .snap { height: 200px; scroll-margin-top: 30px; scroll-snap-align: start; }
            ",
        );
        let s = find_scroll_snap_y(&root, 320.0, 600.0).unwrap();
        assert!((s - 320.0).abs() < 1.0, "expected 320.0, got {s}");
    }
}
