//! GAP-LAYOUTSHIFT (BUG-809): Layout Instability L1 scoring of one relayout's
//! rect changes — impact region, distance fraction and the attributed
//! `entry.sources[]`.
//!
//! Moved verbatim out of `relayout.rs` (file-size gate, docs/lint-policy.md
//! §5.1); only module path and visibility differ.

/// GAP-LAYOUTSHIFT (BUG-809): the visible-area of one rect clipped to the
/// `[0, 0, vw, vh]` viewport — the building block "impact region" is made
/// of (Layout Instability L1 §3.1).
fn clip_area_to_viewport(rect: [f32; 4], vw: f32, vh: f32) -> f32 {
    let x0 = rect[0].max(0.0);
    let y0 = rect[1].max(0.0);
    let x1 = (rect[0] + rect[2]).min(vw);
    let y1 = (rect[1] + rect[3]).min(vh);
    (x1 - x0).max(0.0) * (y1 - y0).max(0.0)
}

/// GAP-LAYOUTSHIFT (BUG-809): score one relayout's rect changes the way
/// Layout Instability L1 §3.1 scores a frame — `impact_fraction ×
/// distance_fraction` — from a `prev`/`next` snapshot pair of
/// [`lumen_layout::collect_layout_rects`] output.
///
/// A node present in only one snapshot (entered/left the tree, e.g. through
/// `display: none` or a DOM mutation) is not a shift and is skipped — the
/// spec scores *moved* elements only. A node whose rect moved less than
/// half a CSS px is treated as unchanged (sub-pixel layout jitter, not a
/// visible shift).
///
/// **Approximation, not the spec's exact geometry**: real §3.1 unions the
/// old+new rects of every unstable element into one non-overlapping impact
/// region before dividing by the viewport area; this sums each element's own
/// clipped area instead, so a page whose several elements shift within
/// overlapping regions double-counts that overlap (`impact_fraction` is
/// clamped to 1.0, so it caps rather than diverges). Good enough to turn
/// "no delivery at all" into a real score for the common one-or-few-elements
/// shift case (`simple-block-movement.html`, `cls-shift` probe variants).
/// `sources` ranks the shifted nodes by their own clipped impact area,
/// largest first, capped at [`LAYOUT_SHIFT_MAX_SOURCES`] (§4.2's "at most
/// five largest") — an approximation of the spec's attribution list, not an
/// exact match, since the spec ranks by the *unioned* region a node
/// contributes rather than each node's standalone area.
pub(crate) fn compute_layout_shift_score(
    prev: &std::collections::HashMap<u32, [f32; 4]>,
    next: &std::collections::HashMap<u32, [f32; 4]>,
    viewport_w: f32,
    viewport_h: f32,
) -> LayoutShiftResult {
    if viewport_w <= 0.0 || viewport_h <= 0.0 {
        return LayoutShiftResult::default();
    }
    let mut impact_area = 0.0f64;
    let mut max_distance_frac = 0.0f64;
    let mut shifted: Vec<(LayoutShiftSource, f64)> = Vec::new();
    for (node, new_rect) in next {
        let Some(old_rect) = prev.get(node) else { continue };
        let dx = new_rect[0] - old_rect[0];
        let dy = new_rect[1] - old_rect[1];
        if dx.abs() <= 0.5 && dy.abs() <= 0.5 {
            continue;
        }
        let old_area = clip_area_to_viewport(*old_rect, viewport_w, viewport_h);
        let new_area = clip_area_to_viewport(*new_rect, viewport_w, viewport_h);
        let node_area = old_area.max(new_area) as f64;
        impact_area += node_area;
        shifted.push((
            LayoutShiftSource {
                node: *node,
                previous_rect: *old_rect,
                current_rect: *new_rect,
            },
            node_area,
        ));
        let dist_frac = dx.abs().max(dy.abs()) as f64 / viewport_w.max(viewport_h) as f64;
        if dist_frac > max_distance_frac {
            max_distance_frac = dist_frac;
        }
    }
    if impact_area <= 0.0 {
        return LayoutShiftResult::default();
    }
    let impact_fraction = (impact_area / (viewport_w as f64 * viewport_h as f64)).min(1.0);
    shifted.sort_by(|a, b| b.1.total_cmp(&a.1));
    shifted.truncate(LAYOUT_SHIFT_MAX_SOURCES);
    LayoutShiftResult {
        score: impact_fraction * max_distance_frac,
        sources: shifted.into_iter().map(|(source, _)| source).collect(),
    }
}

/// Layout Instability §4.2: "at most five" largest sources per entry.
const LAYOUT_SHIFT_MAX_SOURCES: usize = 5;

/// Result of [`compute_layout_shift_score`] — a CLS score plus the node ids
/// behind it, for `entry.sources[]` attribution.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct LayoutShiftResult {
    pub(crate) score: f64,
    pub(crate) sources: Vec<LayoutShiftSource>,
}

/// GAP-LAYOUTSHIFT срез 3 (BUG-809): one `entry.sources[]` attribution
/// entry — the node plus its pre-/post-shift border-box geometry
/// (`LayoutShiftAttribution.previousRect`/`currentRect`, L1 §4.2), both in
/// `collect_layout_rects`'s `[x, y, width, height]` layout-space form (not
/// yet clipped to the viewport — the JS side builds the `DOMRectReadOnly`).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct LayoutShiftSource {
    pub(crate) node: u32,
    pub(crate) previous_rect: [f32; 4],
    pub(crate) current_rect: [f32; 4],
}
