//! CSS Masonry Layout — CSS Grid L3 §14 algorithm module.
//!
//! Implements the waterfall (masonry) placement algorithm: items are distributed
//! across N tracks, each item placed in the track with the minimum running height.
//! This is the "greedy" variant of the spec; the optional optimizing variant
//! (`masonry-auto-flow: ordered`) is a future extension.
//!
//! Integration: `box_tree::lay_out_grid` detects `GridTrackSize::Masonry` on either
//! axis and dispatches to the inline masonry path. This module exposes the standalone
//! algorithm for unit testing and potential reuse.
//!
//! P4 handoff:
//! - `masonry-auto-flow` in `ComputedStyle` — controls placement order
//!   (`definite-first | next | ordered`) per CSS Masonry Layout §9.
//! - `align-tracks` / `justify-tracks` — alignment of tracks in the grid axis.

use crate::box_tree::LayoutBox;

/// Greedy waterfall masonry placement algorithm (CSS Grid L3 §14).
///
/// Distributes `children` across `track_count` tracks, placing each child in the
/// track with the minimum running height. Children must already have their
/// intrinsic `rect.height` set (from a prior `lay_out` pass).
///
/// # Parameters
/// - `container_w`: total available width for all tracks including gaps.
/// - `gap`: space between tracks (column-gap for column-masonry).
/// - `children`: mutable slice of all boxes (all are placed, none skipped).
/// - `track_count`: number of parallel tracks.
///
/// # Returns
/// Total height of the masonry container (max track height minus trailing gap).
pub fn lay_out_masonry(
    container_w: f32,
    gap: f32,
    children: &mut [LayoutBox],
    track_count: usize,
) -> f32 {
    if children.is_empty() || track_count == 0 {
        return 0.0;
    }

    let total_gap = gap * track_count.saturating_sub(1) as f32;
    let track_w = ((container_w - total_gap) / track_count as f32).max(0.0);
    let mut track_heights = vec![0.0_f32; track_count];

    for child in children.iter_mut() {
        let min_col = min_track_idx(&track_heights);

        child.rect.x = min_col as f32 * (track_w + gap);
        child.rect.y = track_heights[min_col];
        child.rect.width = track_w;
        track_heights[min_col] += child.rect.height + gap;
    }

    let total_h = track_heights.iter().cloned().fold(0.0_f32, f32::max);
    (total_h - gap).max(0.0)
}

/// Returns the index of the track with the minimum running height.
///
/// Used by `lay_out_masonry` and the inline masonry dispatch in `lay_out_grid`.
/// Returns 0 for an empty slice.
pub fn min_track_idx(track_heights: &[f32]) -> usize {
    track_heights
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        box_tree::{BoxKind, LayoutBox},
        style::ComputedStyle,
    };
    use lumen_core::geom::Rect;
    use lumen_dom::NodeId;

    fn make_box(height: f32) -> LayoutBox {
        LayoutBox {
            node: NodeId::from_index(0),
            rect: Rect::new(0.0, 0.0, 100.0, height),
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: vec![],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        }
    }

    #[test]
    fn masonry_empty_is_zero() {
        let mut children: Vec<LayoutBox> = vec![];
        let h = lay_out_masonry(300.0, 10.0, &mut children, 3);
        assert_eq!(h, 0.0);
    }

    #[test]
    fn masonry_single_item() {
        let mut children = vec![make_box(80.0)];
        let h = lay_out_masonry(300.0, 10.0, &mut children, 3);
        // Item placed in track 0 at (0, 0).
        assert_eq!(children[0].rect.x, 0.0);
        assert_eq!(children[0].rect.y, 0.0);
        // Height = item_height (no trailing gap subtracted beyond item itself).
        assert_eq!(h, 80.0);
    }

    #[test]
    fn masonry_three_equal_items_distributed() {
        // 3 items of 100px each, 3 columns, 0 gap → one item per column.
        let mut children = vec![make_box(100.0), make_box(100.0), make_box(100.0)];
        let h = lay_out_masonry(300.0, 0.0, &mut children, 3);
        // Each item should land in a different column (x = 0, 100, 200).
        assert_eq!(children[0].rect.x, 0.0);
        assert_eq!(children[1].rect.x, 100.0);
        assert_eq!(children[2].rect.x, 200.0);
        // All column heights equal → total height = 100.
        assert_eq!(h, 100.0);
    }

    #[test]
    fn masonry_short_item_reuses_shorter_track() {
        // 2 tracks: item0→track0 (height 200), item1→track1 (height 50).
        // item2 should go to track1 (shorter).
        let mut children = vec![make_box(200.0), make_box(50.0), make_box(30.0)];
        lay_out_masonry(200.0, 0.0, &mut children, 2);
        assert_eq!(children[0].rect.x, 0.0);   // track 0
        assert_eq!(children[1].rect.x, 100.0); // track 1
        // Item 2 → track 1 (y = 50, shorter than track 0 at 200).
        assert_eq!(children[2].rect.x, 100.0);
        assert_eq!(children[2].rect.y, 50.0);
    }

    #[test]
    fn masonry_track_widths_computed_with_gap() {
        // 600px container, 3 tracks, 10px gap → track_w = (600 - 20) / 3 ≈ 193.33
        let mut children = vec![make_box(50.0)];
        lay_out_masonry(600.0, 10.0, &mut children, 3);
        let expected_w = (600.0_f32 - 20.0) / 3.0;
        assert!((children[0].rect.width - expected_w).abs() < 0.01);
    }

    #[test]
    fn min_track_idx_picks_shortest() {
        let heights = [100.0_f32, 50.0, 75.0];
        assert_eq!(min_track_idx(&heights), 1);
    }

    #[test]
    fn min_track_idx_empty_returns_zero() {
        assert_eq!(min_track_idx(&[]), 0);
    }
}
