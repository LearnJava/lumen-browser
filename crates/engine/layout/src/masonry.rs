// CSS Masonry layout (CSS Masonry Layout L1) — Phase 0 stub
// Implements waterfall grid algorithm: column-count columns, items fill by height.
// // CSS: masonry-auto-flow, align-tracks, justify-tracks

use crate::box_tree::LayoutBox;

/// Waterfall-grid masonry layout algorithm.
/// Distributes children across N columns, filling each column from top to bottom.
/// Each item goes into the column with minimum height so far.
///
/// # Parameters
/// - `container_w`: available width for all columns
/// - `gap`: space between items (CSS gap property)
/// - `children`: list of LayoutBoxes to place
/// - `column_count`: number of columns to create
///
/// # Returns
/// Total height of the layout and per-child position updates.
pub fn lay_out_masonry(
    container_w: f32,
    gap: f32,
    children: &mut [LayoutBox],
    column_count: usize,
) -> f32 {
    if children.is_empty() || column_count == 0 {
        return 0.0;
    }

    // Phase 0: track column heights
    let mut column_heights = vec![0.0; column_count];
    let column_width = (container_w - (gap * (column_count as f32 - 1.0))) / column_count as f32;

    // Waterfall algorithm: for each child, place in lowest column
    for child in children.iter_mut() {
        // Find column with minimum height
        let min_col = column_heights
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        // Set child inline position (column placement)
        child.rect.x = (min_col as f32) * (column_width + gap);
        child.rect.y = column_heights[min_col];

        // Child width = column width (Phase 0: no aspect-ratio handling)
        child.rect.width = column_width;

        // Advance column height by child height + gap
        column_heights[min_col] += child.rect.height + gap;
    }

    // Total height = max column height (minus final gap)
    let total_height = column_heights
        .iter()
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .copied()
        .unwrap_or(0.0);

    if total_height > gap {
        total_height - gap
    } else {
        0.0
    }
}
