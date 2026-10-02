//! CSS Scroll Snap L1 — snap-point finding for the page scroll container.
//!
//! Phase 0 scope: one scroll container = the document viewport. Nested
//! overflow-scroll containers are not yet tracked. For each element with
//! `scroll-snap-align != none`, we compute the scroll offset (Y for the block
//! axis, X for the inline axis) that would place the element's start/center/end
//! at the corresponding position in the viewport, then return the candidate
//! closest to the current offset.
//!
//! CSS Scroll Snap L1 §5 insets (block axis): the target's snap area is its
//! border box outset by the target's own `scroll-margin`; the container's
//! snapport is the viewport inset by the container's `scroll-padding`. Both are
//! applied here — `scroll-margin-top`/`-bottom` from each snap target and
//! `scroll-padding-top`/`-bottom` from the root scroll container. The inline
//! (X) axis mirrors it: [`find_scroll_snap_x`] / [`find_scroll_snap_x_proximity`]
//! read `scroll-snap-align`'s inline keyword, the target's
//! `scroll-margin-left`/`-right` and the root's `scroll-padding-left`/`-right`
//! (physical `horizontal-tb` mapping: inline-start = left, inline-end = right).
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

/// Snap axis handled by the shared candidate collector.
#[derive(Clone, Copy)]
enum Axis {
    /// Block axis → Y scroll offset (`scroll-snap-align` block keyword,
    /// `scroll-margin-top/bottom`, `scroll-padding-top/bottom`).
    Block,
    /// Inline axis → X scroll offset (`scroll-snap-align` inline keyword,
    /// `scroll-margin-left/right`, `scroll-padding-left/right`).
    Inline,
}

/// Snapport insets `(start, end)` of the document scroll container on an axis.
type Padding = (f32, f32);

/// CSS Scroll Snap L1 — returns the Y scroll offset to snap to, or `None`
/// if no snap targets exist in `root`.
///
/// `current_y` — current page scroll offset in CSS px.
/// `viewport_h` — viewport height in CSS px.
///
/// The returned value is clamped to `[0, +∞)` but NOT to max-scroll; the
/// caller should clamp to `max_scroll()` after receiving the result.
pub fn find_scroll_snap_y(root: &LayoutBox, current_y: f32, viewport_h: f32) -> Option<f32> {
    nearest(root, Axis::Block, current_y, viewport_h, None)
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
    nearest(root, Axis::Block, current_y, viewport_h, Some(viewport_h * proximity_fraction))
}

/// CSS Scroll Snap L1 — inline-axis counterpart of [`find_scroll_snap_y`]:
/// returns the X scroll offset to snap to, or `None` if no target aligns on
/// the inline axis.
///
/// `current_x` — current horizontal scroll offset in CSS px.
/// `viewport_w` — viewport width in CSS px.
///
/// Like the Y variant, the value is clamped to `[0, +∞)` but not to max-scroll.
pub fn find_scroll_snap_x(root: &LayoutBox, current_x: f32, viewport_w: f32) -> Option<f32> {
    nearest(root, Axis::Inline, current_x, viewport_w, None)
}

/// CSS Scroll Snap L1 — [`find_scroll_snap_x`] restricted to candidates within
/// `proximity_fraction * viewport_w` of `current_x` (`scroll-snap-type: x proximity`).
pub fn find_scroll_snap_x_proximity(
    root: &LayoutBox,
    current_x: f32,
    viewport_w: f32,
    proximity_fraction: f32,
) -> Option<f32> {
    nearest(root, Axis::Inline, current_x, viewport_w, Some(viewport_w * proximity_fraction))
}

/// Candidate offset closest to `current` on `axis`; with `threshold = Some(t)`
/// only candidates within `t` of `current` qualify.
fn nearest(
    root: &LayoutBox,
    axis: Axis,
    current: f32,
    viewport: f32,
    threshold: Option<f32>,
) -> Option<f32> {
    let padding = container_padding(root, axis);
    let mut candidates: Vec<f32> = Vec::new();
    collect_snap(root, axis, viewport, padding, &mut candidates);
    candidates
        .into_iter()
        .filter(|&c| threshold.is_none_or(|t| (c - current).abs() <= t))
        .min_by(|a, b| {
            (a - current)
                .abs()
                .partial_cmp(&(b - current).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// CSS Scroll Snap L1 §5 — the `scroll-padding` of the document scroll
/// container on `axis`, which insets the snapport start/end edges. Returns
/// `(start, end)` in CSS px: top/bottom for the block axis, left/right for the
/// inline axis. `auto` resolves to `0` during parsing, so the stored value is
/// directly usable here.
///
/// The `root` box is the anonymous document/viewport box; the viewport's
/// scroll-padding propagates from the root element (`:root` / `html`), which is
/// `root`'s first block-level child. Falls back to `root`'s own style when
/// there is no such child (e.g. a bare box passed directly in tests).
fn container_padding(root: &LayoutBox, axis: Axis) -> Padding {
    let src = root
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::Block))
        .unwrap_or(root);
    match axis {
        Axis::Block => (src.style.scroll_padding_top, src.style.scroll_padding_bottom),
        Axis::Inline => (src.style.scroll_padding_left, src.style.scroll_padding_right),
    }
}

fn collect_snap(b: &LayoutBox, axis: Axis, viewport: f32, pad: Padding, out: &mut Vec<f32>) {
    if matches!(b.kind, BoxKind::Skip) || b.style.display == Display::None {
        return;
    }
    // CSS Scroll Snap L1 §5: the snap area is the target's border box outset by
    // its own `scroll-margin`; the snapport is the container viewport inset by
    // `scroll-padding`. The alignment maps the snap area's start/center/end
    // onto the snapport's start/center/end.
    let (pos, size, margin_start, margin_end, keyword) = match axis {
        Axis::Block => (
            b.rect.y,
            b.rect.height,
            b.style.scroll_margin_top,
            b.style.scroll_margin_bottom,
            b.style.scroll_snap_align.block,
        ),
        Axis::Inline => (
            b.rect.x,
            b.rect.width,
            b.style.scroll_margin_left,
            b.style.scroll_margin_right,
            b.style.scroll_snap_align.inline,
        ),
    };
    let (pad_start, pad_end) = pad;
    let area_start = pos - margin_start;
    let area_end = pos + size + margin_end;
    match keyword {
        ScrollSnapAlignKeyword::None => {}
        ScrollSnapAlignKeyword::Start => {
            // snapport start = offset + pad_start → offset = area_start - pad_start.
            out.push((area_start - pad_start).max(0.0));
        }
        ScrollSnapAlignKeyword::Center => {
            // snapport center = offset + (pad_start + viewport - pad_end)/2.
            let area_center = (area_start + area_end) * 0.5;
            let snapport_center = (pad_start + viewport - pad_end) * 0.5;
            out.push((area_center - snapport_center).max(0.0));
        }
        ScrollSnapAlignKeyword::End => {
            // snapport end = offset + viewport - pad_end → offset =
            // area_end - (viewport - pad_end).
            out.push((area_end - (viewport - pad_end)).max(0.0));
        }
    }
    for child in &b.children {
        collect_snap(child, axis, viewport, pad, out);
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

    // ── inline (X) axis: scroll-margin-left/right, scroll-padding-left/right ──

    #[test]
    fn x_no_snap_targets_returns_none() {
        let root = build("<div>x</div>", "div { height: 200px; }");
        assert!(find_scroll_snap_x(&root, 0.0, 800.0).is_none());
    }

    #[test]
    fn x_snap_start_returns_element_left() {
        let root = build(
            "<div class='sp'>x</div><div class='snap'>y</div>",
            "
                .sp, .snap { display: inline-block; vertical-align: top; }
                .sp { width: 300px; height: 20px; }
                .snap { width: 200px; height: 20px; scroll-snap-align: start; }
            ",
        );
        let s = find_scroll_snap_x(&root, 290.0, 800.0).unwrap();
        assert!((s - 300.0).abs() < 1.0, "expected 300.0, got {s}");
    }

    #[test]
    fn x_start_snap_honors_scroll_margin_left() {
        // Target at x=300, scroll-margin-left:40 → area starts at 260.
        let root = build(
            "<div class='sp'>x</div><div class='snap'>y</div>",
            "
                .sp, .snap { display: inline-block; vertical-align: top; }
                .sp { width: 300px; height: 20px; }
                .snap { width: 200px; height: 20px; scroll-margin-left: 40px;
                        scroll-snap-align: start; }
            ",
        );
        let s = find_scroll_snap_x(&root, 260.0, 800.0).unwrap();
        assert!((s - 260.0).abs() < 1.0, "expected 260.0, got {s}");
    }

    #[test]
    fn x_end_snap_honors_scroll_margin_right() {
        // Target x=700 w=200 → right edge 900, margin-right:40 → area end 940.
        // end-alignment: offset = 940 - 800 = 140.
        let root = build(
            "<div class='sp'>x</div><div class='snap'>y</div>",
            "
                body { width: 2000px; }
                .sp, .snap { display: inline-block; vertical-align: top; }
                .sp { width: 700px; height: 20px; }
                .snap { width: 200px; height: 20px; scroll-margin-right: 40px;
                        scroll-snap-align: end; }
            ",
        );
        let s = find_scroll_snap_x(&root, 140.0, 800.0).unwrap();
        assert!((s - 140.0).abs() < 1.0, "expected 140.0, got {s}");
    }

    #[test]
    fn x_start_snap_honors_container_scroll_padding_left() {
        // scroll-padding-left:50 on the root insets the snapport start: 300 → 250.
        let root = build(
            "<div class='sp'>x</div><div class='snap'>y</div>",
            "
                html { scroll-padding-left: 50px; }
                .sp, .snap { display: inline-block; vertical-align: top; }
                .sp { width: 300px; height: 20px; }
                .snap { width: 200px; height: 20px; scroll-snap-align: start; }
            ",
        );
        let s = find_scroll_snap_x(&root, 250.0, 800.0).unwrap();
        assert!((s - 250.0).abs() < 1.0, "expected 250.0, got {s}");
    }

    #[test]
    fn x_center_snap_honors_asymmetric_margin_and_padding() {
        // x=300 w=200, margin-left:20 margin-right:60 → area [280, 560], center 420.
        // padding-left:10 padding-right:30 → snapport center = (10 + 800 - 30)/2 = 390.
        // offset = 420 - 390 = 30.
        let root = build(
            "<div class='sp'>x</div><div class='snap'>y</div>",
            "
                html { scroll-padding-left: 10px; scroll-padding-right: 30px; }
                .sp, .snap { display: inline-block; vertical-align: top; }
                .sp { width: 300px; height: 20px; }
                .snap { width: 200px; height: 20px; scroll-margin-left: 20px;
                        scroll-margin-right: 60px; scroll-snap-align: center; }
            ",
        );
        let s = find_scroll_snap_x(&root, 30.0, 800.0).unwrap();
        assert!((s - 30.0).abs() < 1.0, "expected 30.0, got {s}");
    }

    #[test]
    fn x_ignores_block_only_alignment() {
        // `scroll-snap-align: start none` snaps on block only → no X candidate.
        let root = build(
            "<div class='snap'>y</div>",
            ".snap { height: 20px; scroll-snap-align: start none; }",
        );
        assert!(find_scroll_snap_x(&root, 0.0, 800.0).is_none());
        assert!(find_scroll_snap_y(&root, 0.0, 600.0).is_some());
    }

    #[test]
    fn x_proximity_filters_by_threshold() {
        let root = build(
            "<div class='a'>x</div><div class='b'>y</div>",
            "
                .a, .b { display: inline-block; vertical-align: top; width: 600px;
                         height: 20px; scroll-snap-align: start; }
            ",
        );
        // Snaps at x=0 and x=600; current=50, threshold=30%·800=240.
        let s = find_scroll_snap_x_proximity(&root, 50.0, 800.0, 0.3).unwrap();
        assert!(s.abs() < 1.0, "only snap at 0 is in range; got {s}");
        assert!(find_scroll_snap_x_proximity(&root, 300.0, 800.0, 0.1).is_none());
    }
}
