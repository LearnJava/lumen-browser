//! CSS Grid L2 §9 — what the items of a row-`subgrid` ask of the parent's row tracks.
//!
//! The parent's probe pass used to take the subgrid box as one item: its whole height (every
//! subgrid row plus the subgrid's own gaps) grew the *first* row it spans, so
//! `grid-template-rows: repeat(5, 1fr)` with a subgrid over rows 2–4 and `row-gap: 20px` came out
//! with a 40px second row (`css-gaps/.../subgrid-gap-decorations-fragmentation-020`: the
//! reference has 20px rows). Here every item of the subgrid is placed in the parent's rows and
//! contributes its own outer height, plus the «extra margin» of the subgrid (L2 §9: half of
//! `own gap - parent gap` next to every inner gutter, and the subgrid's own margin/border/padding
//! at its first/last row).

use super::grid::{grid_item_indices, place_grid_items, GridAxis};
use super::*;

/// One item of a row subgrid: the parent row it starts in, the rows it spans and the outer block
/// size it asks for (px, with the subgrid's extra margin).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RowContribution {
    pub(super) row: usize,
    pub(super) span: usize,
    pub(super) size: f32,
}

/// Is `b` a grid container whose row axis is `subgrid`?
pub(super) fn is_row_subgrid(b: &LayoutBox) -> bool {
    matches!(b.style.display, Display::Grid | Display::InlineGrid)
        && b.style.grid_template_rows.first() == Some(&GridTrackSize::Subgrid)
}

/// Margin + border + padding of `b` on its (top, bottom) edges.
fn block_edges(b: &LayoutBox, viewport: Size) -> (f32, f32) {
    let s = &b.style;
    let em = s.font_size;
    (
        s.margin_top.resolve_or_zero(em, 0.0, viewport)
            + s.border_top_width
            + s.padding_top.resolve_or_zero(em, 0.0, viewport),
        s.margin_bottom.resolve_or_zero(em, 0.0, viewport)
            + s.border_bottom_width
            + s.padding_bottom.resolve_or_zero(em, 0.0, viewport),
    )
}

/// Contributions of the items of the laid-out row subgrid `sub`, whose first track is parent row
/// `base_row`. `gap` is the gutter its tracks sit in (the parent's, or that of the enclosing
/// subgrid), `(edge_start, edge_end)` the extra margin of the enclosing subgrids on its first/last
/// row. `None` when the subgrid kept no used tracks (nothing to place the items by).
pub(super) fn collect_row_contributions(
    sub: &LayoutBox,
    base_row: usize,
    gap: f32,
    (edge_start, edge_end): (f32, f32),
    viewport: Size,
    percent_base: Option<f32>,
    out: &mut Vec<RowContribution>,
) -> Option<()> {
    let used = sub.used_grid_tracks.as_deref()?;
    let n_rows = used.rows.len();
    if n_rows == 0 {
        return None;
    }
    let st = &sub.style;
    let em = st.font_size;
    let own_gap = (!st.row_gap_normal).then(|| st.row_gap.resolve(em, percent_base, viewport).unwrap_or(0.0).max(0.0));
    let half = own_gap.map_or(0.0, |g| (g - gap) / 2.0);
    let (own_start, own_end) = block_edges(sub, viewport);
    let idxs = grid_item_indices(&sub.children, super::fieldset::rendered_legend_idx_of(sub));
    let col_sub = st.grid_template_columns.first() == Some(&GridTrackSize::Subgrid);
    let col_axis = GridAxis {
        n_tracks: used.cols.len() as u32,
        names: &used.col_names,
        areas: &st.grid_template_areas,
        is_col: true,
        clamp: col_sub,
    };
    let row_axis = GridAxis {
        n_tracks: n_rows as u32,
        names: &used.row_names,
        areas: &st.grid_template_areas,
        is_col: false,
        clamp: true,
    };
    let placements = place_grid_items(&sub.children, &idxs, st, used.cols.len(), n_rows, &col_axis, &row_axis);
    for (k, &i) in idxs.iter().enumerate() {
        let item = &sub.children[i];
        let (_, _, rs, re) = placements[k];
        if rs == 0 {
            continue;
        }
        let start = (rs as usize - 1).min(n_rows - 1);
        let span = (re.saturating_sub(rs) as usize).max(1);
        let end = (start + span).min(n_rows);
        let lead = if start == 0 { edge_start + own_start } else { half };
        let trail = if end >= n_rows { edge_end + own_end } else { half };
        if is_row_subgrid(item)
            && collect_row_contributions(
                item,
                base_row + start,
                own_gap.unwrap_or(gap),
                (lead, trail),
                viewport,
                percent_base,
                out,
            )
            .is_some()
        {
            continue;
        }
        let ie = item.style.font_size;
        let size = item.rect.height
            + item.style.margin_top.resolve_or_zero(ie, 0.0, viewport)
            + item.style.margin_bottom.resolve_or_zero(ie, 0.0, viewport)
            + lead
            + trail;
        out.push(RowContribution { row: base_row + start, span: end - start, size });
    }
    Some(())
}
