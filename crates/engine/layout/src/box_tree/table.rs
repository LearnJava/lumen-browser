//! CSS 2.1 §17 table layout mode: row layout with colspan/rowspan, collapsed
//! and separated border models, and content/explicit column-width resolution.
//!
//! Перенесено батчем SPLIT-BT9 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn lay_out_table_row` до конца файла, перед `mod tests`) без
//! правок тел.

use super::*;

/// CSS 2.1 §17.5 — table row layout with colspan/rowspan support.
///
/// Algorithm:
/// 1. Map each cell to its starting column (skipping rowspan-occupied columns).
/// 2. Determine cell width: sum of spanned `col_widths` columns, or explicit CSS width.
/// 3. Place cells horizontally; use column-position x when `col_widths` is present.
/// 4. Normalise heights: non-rowspan cells all get the max row height.
///    Rowspan cells keep their laid-out height; `lay_out_table` fixes them after all rows.
/// 5. Register new rowspan occupancy in `rowspan_map` (caller decrements after the row).
///
/// Returns content height (without the row's own padding/border).
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_table_row(
    b: &mut LayoutBox,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    col_widths: Option<&[f32]>,
    // None for standalone <tr> outside <table>; caller must call decrement_rowspan_map after return.
    rowspan_map: Option<&mut Vec<u32>>,
    // Horizontal gap between adjacent cells (from table's border-spacing-h). 0.0 for standalone rows.
    h_spacing: f32,
    // CSS 2.1 §17.6.2 collapsed border model: absolute x of each column's cell border-box left
    // edge (length = n_cols). When present, cells are positioned here so adjacent borders overlap
    // by the collapsed grid-line width instead of being spaced by `h_spacing`.
    collapse_col_x: Option<&[f32]>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let cell_idxs: Vec<usize> = b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

    let n = cell_idxs.len();
    if n == 0 {
        return 0.0;
    }

    // Step 1 + 2: map cells to (col_start, cell_width).
    // `cell_cols[j]` = (starting column index, border-box width to allocate).
    let cell_cols: Vec<(usize, f32)> = if let Some(cw) = col_widths {
        // Pre-computed table-wide column widths are authoritative.
        // Skip columns occupied by rowspan cells from prior rows.
        let empty: Vec<u32> = Vec::new();
        let rsmap: &[u32] = rowspan_map
            .as_deref()
            .map(|v: &Vec<u32>| v.as_slice())
            .unwrap_or(empty.as_slice());
        let mut col_pos = 0usize;
        let mut result = Vec::with_capacity(n);
        for &i in &cell_idxs {
            while col_pos < rsmap.len() && rsmap[col_pos] > 0 {
                col_pos += 1;
            }
            let span = b.children[i].col_span.max(1) as usize;
            let w: f32 = (col_pos..col_pos + span)
                .map(|c| cw.get(c).copied().unwrap_or(0.0))
                .sum();
            result.push((col_pos, w));
            col_pos += span;
        }
        result
    } else {
        // No pre-computed widths: derive from each cell's explicit CSS width.
        let mut explicit_w: Vec<Option<f32>> = Vec::with_capacity(n);
        let mut total_explicit = 0.0_f32;
        let mut auto_count: usize = 0;
        for &i in &cell_idxs {
            let c = &b.children[i];
            let em = c.style.font_size;
            if let Some(w_len) = &c.style.width
                && let Some(w) = w_len.resolve(em, Some(content_width), viewport)
            {
                let border_w = match c.style.box_sizing {
                    BoxSizing::ContentBox => {
                        let pl = c.style.padding_left.resolve_or_zero(em, content_width, viewport);
                        let pr = c.style.padding_right.resolve_or_zero(em, content_width, viewport);
                        w + pl + pr + c.style.border_left_width + c.style.border_right_width
                    }
                    BoxSizing::BorderBox => w,
                };
                explicit_w.push(Some(border_w));
                total_explicit += border_w;
                continue;
            }
            explicit_w.push(None);
            auto_count += 1;
        }
        let auto_share = if auto_count > 0 {
            ((content_width - total_explicit) / auto_count as f32).max(0.0)
        } else {
            0.0
        };
        // Standalone row: sequential column assignment (cell j → column j).
        (0..n)
            .map(|j| (j, explicit_w[j].unwrap_or(auto_share)))
            .collect()
    };

    // Step 3: lay out each cell at its column x position.
    // When col_widths is present, the column width is authoritative — clear the cell's CSS
    // `width` temporarily so lay_out uses `avail` as the final width.
    let use_global = col_widths.is_some();
    for (j, &i) in cell_idxs.iter().enumerate() {
        let (col_start, avail) = cell_cols[j];
        let cell_x = if let Some(cx) = collapse_col_x {
            // Collapsed border model: each column has a precomputed absolute x at which its
            // cells' border-box starts; adjacent cells overlap by the shared grid-line border.
            cx.get(col_start).copied().unwrap_or(content_x)
        } else if use_global {
            // Exact x from column positions, accounting for h_spacing slots.
            // Cell at col_start k: content_x + (k+1)*h_spacing + sum(col_widths[0..k]).
            content_x
                + (col_start + 1) as f32 * h_spacing
                + (0..col_start)
                    .map(|c| col_widths.and_then(|cw| cw.get(c)).copied().unwrap_or(0.0))
                    .sum::<f32>()
        } else {
            // Standalone row: use prior cell's right edge.
            if j == 0 {
                content_x
            } else {
                let prev_i = cell_idxs[j - 1];
                let c = &b.children[prev_i];
                let c_em = c.style.font_size;
                let mr = c.style.margin_right.resolve_or_zero(c_em, content_width, viewport);
                c.rect.x + c.rect.width + mr
            }
        };
        let saved_width =
            if use_global { Arc::make_mut(&mut b.children[i].style).width.take() } else { None };
        lay_out(
            &mut b.children[i],
            cell_x,
            content_y,
            avail,
            None,
            measurer,
            viewport,
            pcb,
            hp,
            false,
        );
        if use_global {
            Arc::make_mut(&mut b.children[i].style).width = saved_width;
        }
    }

    // Register rowspan occupancy. Value = row_span (not row_span-1) because the caller
    // calls decrement_rowspan_map after this row, leaving row_span-1 remaining rows occupied.
    if let Some(rsmap) = rowspan_map {
        for (j, &i) in cell_idxs.iter().enumerate() {
            if b.children[i].row_span > 1 {
                let (col_start, _) = cell_cols[j];
                let span = b.children[i].col_span.max(1) as usize;
                let end_col = col_start + span;
                if end_col > rsmap.len() {
                    rsmap.resize(end_col, 0);
                }
                let rs = b.children[i].row_span;
                for v in rsmap.iter_mut().skip(col_start).take(span) {
                    if *v < rs {
                        *v = rs;
                    }
                }
            }
        }
    }

    // Step 4: normalise heights — non-rowspan cells all become the max row height.
    // Rowspan > 1 cells keep their own height; lay_out_table patches them later.
    let row_h = cell_idxs
        .iter()
        .filter(|&&i| b.children[i].row_span == 1)
        .map(|&i| b.children[i].rect.height)
        .fold(0.0_f32, f32::max);
    for &i in &cell_idxs {
        if b.children[i].row_span == 1 {
            b.children[i].rect.height = row_h;
        }
    }

    row_h
}

/// CSS 2.1 §17.6.2 — collapsed vertical border width at each column grid line for a table.
///
/// Returns a `Vec<f32>` of length `n_cols + 1`. Index `k` (1..n_cols) is the shared border
/// width between column `k-1` and column `k`: the maximum, over every row, of the right border
/// of the cell in column `k-1` and the left border of the cell in column `k`. Indices `0` and
/// `n_cols` (the outer edges) are left at `0.0` — outer cells are snapped onto the table border
/// by the caller, so their grid-line width is handled there. Cells are mapped to columns by
/// sequential order (colspan/rowspan are not accounted for; collapse overlap is exact only for
/// simple uniform grids, which is the common case).
fn collapse_v_edges(b: &LayoutBox, n_cols: usize) -> Vec<f32> {
    let mut edges = vec![0.0_f32; n_cols + 1];
    let mut visit = |row: &LayoutBox| {
        let cells: Vec<&LayoutBox> = row
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        for col in 1..cells.len().min(n_cols) {
            let edge = cells[col].style.border_left_width.max(cells[col - 1].style.border_right_width);
            edges[col] = edges[col].max(edge);
        }
    };
    for child in &b.children {
        match &child.kind {
            BoxKind::TableRow => visit(child),
            BoxKind::TableRowGroup => {
                for row in &child.children {
                    if matches!(row.kind, BoxKind::TableRow) {
                        visit(row);
                    }
                }
            }
            _ => {}
        }
    }
    edges
}

/// CSS 2.1 §17.6.2 — representative collapsed horizontal (row-to-row) border width.
///
/// Returns the maximum top/bottom border width across all cells in the table. Used as a uniform
/// row overlap in collapse mode: consecutive rows are pulled together by this amount so their
/// shared horizontal grid line renders as one border instead of two stacked ones. Uniform (rather
/// than per-row-pair) is exact when row borders are consistent — the common case.
fn collapse_max_cross_border(b: &LayoutBox) -> f32 {
    let mut max_b = 0.0_f32;
    let mut visit = |row: &LayoutBox| {
        for cell in row.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)) {
            max_b = max_b.max(cell.style.border_top_width).max(cell.style.border_bottom_width);
        }
    };
    for child in &b.children {
        match &child.kind {
            BoxKind::TableRow => visit(child),
            BoxKind::TableRowGroup => {
                for row in &child.children {
                    if matches!(row.kind, BoxKind::TableRow) {
                        visit(row);
                    }
                }
            }
            _ => {}
        }
    }
    max_b
}

/// CSS 2.1 §17 — table layout with colspan/rowspan support.
///
/// Pass 1: compute column widths (span-aware), lay out rows top-to-bottom while tracking
/// rowspan occupancy and collecting spanning cells.
/// Pass 2: fix spanning cell heights — each rowspan cell's height is extended to cover
/// the bottom edge of its last spanned row.
///
/// Returns content height.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_table(
    b: &mut LayoutBox,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    // CSS Tables L2 §17.6: collapse mode zeroes out border-spacing.
    let collapse = matches!(b.style.border_collapse, BorderCollapse::Collapse);
    let (h_spacing, v_spacing) = match b.style.border_collapse {
        BorderCollapse::Collapse => (0.0, 0.0),
        BorderCollapse::Separate => (b.style.border_spacing_h, b.style.border_spacing_v),
    };

    let col_widths = compute_table_col_widths(b, content_width, viewport, measurer);

    // CSS 2.1 §17.6.2 — collapsing border model. Adjacent cell borders (and the table's own
    // border with the outer cells) share a single grid line whose width is the larger of the
    // two meeting borders. We model this by positioning columns so neighbouring cells overlap
    // by that collapsed width, and by snapping the outer cells onto the table border (so a 2px
    // table border + 2px cell border render as one 2px line, not 4px). Width/colour conflict
    // resolution is approximated by max-width (sufficient for same-style same-colour grids).
    let n_cols = col_widths.len();
    let (collapse_col_x, collapse_v_overlap, collapse_width) = if collapse && n_cols > 0 {
        let v_edges = collapse_v_edges(b, n_cols);
        // base_x = table border-box left edge: outer cell borders coincide with the table border.
        let base_x = b.rect.x;
        let mut col_x = Vec::with_capacity(n_cols);
        col_x.push(base_x);
        for k in 1..n_cols {
            let prev = col_x[k - 1] + col_widths[k - 1] - v_edges[k];
            col_x.push(prev);
        }
        let total_w = (col_x[n_cols - 1] + col_widths[n_cols - 1] - base_x).max(0.0);
        (Some(col_x), collapse_max_cross_border(b), total_w)
    } else {
        (None, 0.0, 0.0)
    };
    let collapse_col_x_ref = collapse_col_x.as_deref();

    // First row starts after the top outer v_spacing slot; in collapse mode the first row's top
    // border coincides with the table's top border (start at the table border-box top edge).
    let mut cur_y = if collapse { b.rect.y } else { content_y + v_spacing };
    let mut rowspan_map: Vec<u32> = Vec::new();

    // flat_row_rects[k] = (y, height) for the k-th row in DOM order (across all groups).
    let mut flat_row_rects: Vec<(f32, f32)> = Vec::new();

    // Spanning cells that need height post-fix:
    // (group: Option<usize>, row_in_group: usize, child_idx: usize, start_flat: usize, span: u32)
    let mut span_fixes: Vec<(Option<usize>, usize, usize, usize, u32)> = Vec::new();

    let n = b.children.len();
    for i in 0..n {
        match b.children[i].kind {
            BoxKind::TableRow => {
                let c_em = b.children[i].style.font_size;
                let c_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                let row_y = cur_y + c_mt;
                b.children[i].rect.x = content_x;
                b.children[i].rect.y = row_y;
                b.children[i].rect.width = content_width;
                let flat_idx = flat_row_rects.len();
                let row_h = lay_out_table_row(
                    &mut b.children[i],
                    content_x, row_y, content_width,
                    Some(&col_widths),
                    Some(&mut rowspan_map),
                    h_spacing,
                    collapse_col_x_ref,
                    measurer, viewport, pcb, hp,
                );
                let row_style_h = {
                    let s = &b.children[i].style;
                    if let Some(h_len) = &s.height
                        && let Some(h) = h_len.resolve(s.font_size, None, viewport)
                    {
                        let pt = s.padding_top.resolve_or_zero(s.font_size, content_width, viewport);
                        let pb = s.padding_bottom.resolve_or_zero(s.font_size, content_width, viewport);
                        match s.box_sizing {
                            BoxSizing::ContentBox => (h + pt + pb + s.border_top_width + s.border_bottom_width).max(0.0),
                            BoxSizing::BorderBox => h.max(pt + pb + s.border_top_width + s.border_bottom_width),
                        }
                    } else {
                        let pt = b.children[i].style.padding_top.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                        let pb = b.children[i].style.padding_bottom.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                        row_h + pt + pb + b.children[i].style.border_top_width + b.children[i].style.border_bottom_width
                    }
                };
                b.children[i].rect.height = row_style_h;
                flat_row_rects.push((b.children[i].rect.y, row_style_h));
                // Collect spanning cells for post-fix.
                for (ci, child) in b.children[i].children.iter().enumerate() {
                    if !matches!(child.kind, BoxKind::Skip) && child.row_span > 1 {
                        span_fixes.push((None, i, ci, flat_idx, child.row_span));
                    }
                }
                let c_mb = b.children[i].style.margin_bottom.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                // Add v_spacing gap after each row (outer bottom slot included); in collapse mode
                // pull the next row up by the shared horizontal grid-line border instead.
                // CSS: border-spacing
                cur_y = b.children[i].rect.y + b.children[i].rect.height + c_mb + v_spacing - collapse_v_overlap;
                decrement_rowspan_map(&mut rowspan_map);
            }
            BoxKind::TableRowGroup => {
                let group_em = b.children[i].style.font_size;
                let g_mt = b.children[i].style.margin_top.resolve_or_zero(group_em, content_width, viewport);
                let group_y = cur_y + g_mt;
                b.children[i].rect.x = content_x;
                b.children[i].rect.y = group_y;
                b.children[i].rect.width = content_width;
                let mut row_y = group_y;
                let n_rows = b.children[i].children.len();
                for r in 0..n_rows {
                    if !matches!(b.children[i].children[r].kind, BoxKind::TableRow) {
                        continue;
                    }
                    let flat_idx = flat_row_rects.len();
                    let r_em = b.children[i].children[r].style.font_size;
                    let r_mt = b.children[i].children[r].style.margin_top.resolve_or_zero(r_em, content_width, viewport);
                    b.children[i].children[r].rect.x = content_x;
                    b.children[i].children[r].rect.y = row_y + r_mt;
                    b.children[i].children[r].rect.width = content_width;
                    let row_h = lay_out_table_row(
                        &mut b.children[i].children[r],
                        content_x, row_y + r_mt, content_width,
                        Some(&col_widths),
                        Some(&mut rowspan_map),
                        h_spacing,
                        collapse_col_x_ref,
                        measurer, viewport, pcb, hp,
                    );
                    let r_pt = b.children[i].children[r].style.padding_top.resolve_or_zero(r_em, content_width, viewport);
                    let r_pb = b.children[i].children[r].style.padding_bottom.resolve_or_zero(r_em, content_width, viewport);
                    let r_bor = b.children[i].children[r].style.border_top_width + b.children[i].children[r].style.border_bottom_width;
                    let row_style_h = row_h + r_pt + r_pb + r_bor;
                    b.children[i].children[r].rect.height = row_style_h;
                    flat_row_rects.push((b.children[i].children[r].rect.y, row_style_h));
                    // Collect spanning cells for post-fix.
                    for (ci, child) in b.children[i].children[r].children.iter().enumerate() {
                        if !matches!(child.kind, BoxKind::Skip) && child.row_span > 1 {
                            span_fixes.push((Some(i), r, ci, flat_idx, child.row_span));
                        }
                    }
                    let r_mb = b.children[i].children[r].style.margin_bottom.resolve_or_zero(r_em, content_width, viewport);
                    // CSS: border-spacing — collapse mode pulls rows together by the shared border.
                    row_y = b.children[i].children[r].rect.y + b.children[i].children[r].rect.height + r_mb + v_spacing - collapse_v_overlap;
                    decrement_rowspan_map(&mut rowspan_map);
                }
                let g_pt = b.children[i].style.padding_top.resolve_or_zero(group_em, content_width, viewport);
                let g_pb = b.children[i].style.padding_bottom.resolve_or_zero(group_em, content_width, viewport);
                let g_bor = b.children[i].style.border_top_width + b.children[i].style.border_bottom_width;
                b.children[i].rect.height = (row_y - group_y) + g_pt + g_pb + g_bor;
                let g_mb = b.children[i].style.margin_bottom.resolve_or_zero(group_em, content_width, viewport);
                cur_y = b.children[i].rect.y + b.children[i].rect.height + g_mb;
            }
            _ => {}
        }
    }

    // Pass 2: fix rowspan cell heights.
    // Each spanning cell's height is extended to reach the bottom of its last spanned row.
    for (group, row, child_idx, start_flat, span) in span_fixes {
        let end_flat = (start_flat + span as usize).min(flat_row_rects.len());
        if end_flat == 0 {
            continue;
        }
        let (last_y, last_h) = flat_row_rects[end_flat - 1];
        let target_bottom = last_y + last_h;
        let cell = match group {
            None => &mut b.children[row].children[child_idx],
            Some(g) => &mut b.children[g].children[row].children[child_idx],
        };
        let new_h = (target_bottom - cell.rect.y).max(cell.rect.height);
        cell.rect.height = new_h;
    }

    // CSS 2.1 §17.6.2 — collapsing model: the table border-box coincides with the outer cells'
    // shared borders. Snap the table width to the overlapped grid and the height to the bottom
    // edge of the last row (which already includes the collapsed top/bottom borders). The caller
    // skips its own height computation in collapse mode, so set it here for every collapse table
    // (an empty table with no rows collapses to a zero-height border-box).
    if collapse {
        if b.style.width.is_none() && n_cols > 0 {
            b.rect.width = collapse_width;
        }
        if b.style.height.is_none() {
            b.rect.height = flat_row_rects
                .last()
                .map(|&(last_y, last_h)| (last_y + last_h - b.rect.y).max(0.0))
                .unwrap_or(0.0);
        }
    }

    (cur_y - content_y).max(0.0)
}

/// Scans `row`'s cells and updates `col_explicit` with per-column explicit border-box
/// widths. Colspan cells distribute their width evenly across spanned columns.
/// Rowspan cells register occupancy in `rowspan_map` for subsequent rows.
/// Caller must call `decrement_rowspan_map` after processing each row.
fn scan_row_explicit_widths(
    row: &LayoutBox,
    col_explicit: &mut Vec<Option<f32>>,
    rowspan_map: &mut Vec<u32>,
    content_width: f32,
    viewport: Size,
) {
    let cells: Vec<_> = row
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();

    let mut col_pos = 0usize;
    for cell in &cells {
        // Skip columns occupied by rowspan cells from prior rows.
        while col_pos < rowspan_map.len() && rowspan_map[col_pos] > 0 {
            col_pos += 1;
        }

        let span = cell.col_span.max(1) as usize;
        let em = cell.style.font_size;
        let w_border = if let Some(w_len) = &cell.style.width
            && let Some(w) = w_len.resolve(em, Some(content_width), viewport)
        {
            let bw = match cell.style.box_sizing {
                BoxSizing::ContentBox => {
                    let pl = cell.style.padding_left.resolve_or_zero(em, content_width, viewport);
                    let pr = cell.style.padding_right.resolve_or_zero(em, content_width, viewport);
                    w + pl + pr + cell.style.border_left_width + cell.style.border_right_width
                }
                BoxSizing::BorderBox => w,
            };
            Some(bw)
        } else {
            None
        };

        let end_col = col_pos + span;
        if end_col > col_explicit.len() {
            col_explicit.resize(end_col, None);
        }
        // Distribute the cell's explicit width evenly across its spanned columns.
        if let Some(total_w) = w_border {
            let per_col = total_w / span as f32;
            for slot in col_explicit.iter_mut().skip(col_pos).take(span) {
                *slot = Some(match *slot {
                    Some(existing) => existing.max(per_col),
                    None => per_col,
                });
            }
        }

        // Register rowspan occupancy. Value = row_span (decrement_rowspan_map brings it to
        // row_span-1 after this row, meaning row_span-1 subsequent rows remain occupied).
        if cell.row_span > 1 {
            if end_col > rowspan_map.len() {
                rowspan_map.resize(end_col, 0);
            }
            let rs = cell.row_span;
            for v in rowspan_map.iter_mut().skip(col_pos).take(span) {
                if *v < rs {
                    *v = rs;
                }
            }
        }

        col_pos = end_col;
    }
}

/// Decrements each entry in `rowspan_map` by 1 (clamped to 0). Call after each row.
fn decrement_rowspan_map(map: &mut [u32]) {
    for v in map.iter_mut() {
        *v = v.saturating_sub(1);
    }
}

/// CSS 2.1 §17.5.2 — minimum (shrink-to-fit) content width for a table box.
///
/// Returns `sum(explicit_column_widths) + (n_cols + 1) * border_spacing_h`.
/// Cells without an explicit CSS width contribute 0 (effectively auto/min-content).
/// Used to shrink `display:table` boxes that have no explicit CSS `width`.
pub(crate) fn table_intrinsic_content_width(b: &LayoutBox, viewport: Size) -> f32 {
    let h_spacing = b.style.border_spacing_h;
    let mut col_explicit: Vec<Option<f32>> = Vec::new();
    let mut rowspan_map: Vec<u32> = Vec::new();
    for child in &b.children {
        match &child.kind {
            BoxKind::TableRow => {
                scan_row_explicit_widths(child, &mut col_explicit, &mut rowspan_map, 0.0, viewport);
                decrement_rowspan_map(&mut rowspan_map);
            }
            BoxKind::TableRowGroup => {
                for row in &child.children {
                    if matches!(row.kind, BoxKind::TableRow) {
                        scan_row_explicit_widths(row, &mut col_explicit, &mut rowspan_map, 0.0, viewport);
                        decrement_rowspan_map(&mut rowspan_map);
                    }
                }
            }
            _ => {}
        }
    }
    let n_cols = col_explicit.len();
    if n_cols == 0 {
        return 0.0;
    }
    let total_explicit: f32 = col_explicit.iter().filter_map(|w| *w).sum();
    total_explicit + (n_cols + 1) as f32 * h_spacing
}

/// CSS 2.1 §17.5.2 — min-content and max-content widths for a slice of boxes.
///
/// Traverses block containers recursively. Block-level items stack vertically —
/// the container's min/max is the max of its children's widths. `InlineRun`
/// items accumulate segments left-to-right for max-content and take the widest
/// whitespace-separated token for min-content.
///
/// Returns `(min_content_width, max_content_width)` as content-box widths
/// (the caller must add the container's own padding + border).
fn box_min_max_content_w(boxes: &[LayoutBox], m: &dyn TextMeasurer, vp: Size) -> (f32, f32) {
    let mut min_w = 0.0f32;
    let mut max_w = 0.0f32;
    for b in boxes {
        let (bmin, bmax) = match &b.kind {
            BoxKind::InlineRun { segments, .. } => {
                let mut line_w = 0.0f32;
                let mut run_max = 0.0f32;
                let mut run_min = 0.0f32;
                for seg in segments {
                    if seg.forced_break {
                        run_max = run_max.max(line_w);
                        line_w = 0.0;
                        continue;
                    }
                    let fs = seg.style.font_size;
                    let ls = seg.style.letter_spacing;
                    if seg.img_src.is_some() {
                        let w = seg.pre_space + seg.img_width + seg.post_space;
                        line_w += w;
                        run_min = run_min.max(w);
                    } else {
                        let fams = &seg.style.font_family;
                        line_w += seg.pre_space
                            + measure_text_w_families(&seg.text, fs, ls, 0.0, fams, m)
                            + seg.post_space;
                        for word in seg.text.split_ascii_whitespace() {
                            run_min = run_min.max(
                                seg.pre_space
                                    + measure_text_w_families(word, fs, ls, 0.0, fams, m)
                                    + seg.post_space,
                            );
                        }
                    }
                }
                run_max = run_max.max(line_w);
                (run_min, run_max)
            }
            BoxKind::Block | BoxKind::FlowRoot | BoxKind::InlineBlockRow => {
                let em = b.style.font_size;
                let pl = b.style.padding_left.resolve_or_zero(em, 0.0, vp);
                let pr = b.style.padding_right.resolve_or_zero(em, 0.0, vp);
                let bw = b.style.border_left_width + b.style.border_right_width;
                let (cmin, cmax) = box_min_max_content_w(&b.children, m, vp);
                (cmin + pl + pr + bw, cmax + pl + pr + bw)
            }
            BoxKind::Skip
            | BoxKind::TableRow
            | BoxKind::TableRowGroup
            | BoxKind::InlineSpace
            | BoxKind::Marker { .. }
            | BoxKind::Contents => (0.0, 0.0),
            // Replaced elements (Image, FormControl, Video, …): use explicit width if set.
            _ => {
                let em = b.style.font_size;
                if let Some(wl) = &b.style.width
                    && let Some(w) = wl.resolve(em, None, vp)
                    && w > 0.0
                {
                    (w, w)
                } else {
                    (0.0, 0.0)
                }
            }
        };
        min_w = min_w.max(bmin);
        max_w = max_w.max(bmax);
    }
    (min_w, max_w)
}

/// Returns `(min_content_border_box, max_content_border_box)` for a single table cell,
/// including the cell's own horizontal padding and border.
fn cell_min_max_border_box_w(cell: &LayoutBox, m: &dyn TextMeasurer, vp: Size) -> (f32, f32) {
    let em = cell.style.font_size;
    let pl = cell.style.padding_left.resolve_or_zero(em, 0.0, vp);
    let pr = cell.style.padding_right.resolve_or_zero(em, 0.0, vp);
    let bw = cell.style.border_left_width + cell.style.border_right_width;
    let horiz = pl + pr + bw;
    let (cmin, cmax) = box_min_max_content_w(&cell.children, m, vp);
    (cmin + horiz, cmax + horiz)
}

/// Scans `row`'s cells and updates `col_min`/`col_max` with per-column content-based widths.
/// Colspan cells distribute their content width evenly across the spanned columns.
/// Rowspan occupancy is tracked in `rowspan_map` (same semantics as `scan_row_explicit_widths`).
fn scan_row_content_widths(
    row: &LayoutBox,
    col_min: &mut Vec<f32>,
    col_max: &mut Vec<f32>,
    rowspan_map: &mut Vec<u32>,
    m: &dyn TextMeasurer,
    vp: Size,
) {
    let mut col_pos = 0usize;
    for cell in row.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)) {
        while col_pos < rowspan_map.len() && rowspan_map[col_pos] > 0 {
            col_pos += 1;
        }
        let span = cell.col_span.max(1) as usize;
        let end_col = col_pos + span;
        if end_col > col_min.len() {
            col_min.resize(end_col, 0.0);
            col_max.resize(end_col, 0.0);
        }
        if end_col > rowspan_map.len() {
            rowspan_map.resize(end_col, 0);
        }
        let (cmin, cmax) = cell_min_max_border_box_w(cell, m, vp);
        let per_min = cmin / span as f32;
        let per_max = cmax / span as f32;
        for i in col_pos..end_col {
            col_min[i] = col_min[i].max(per_min);
            col_max[i] = col_max[i].max(per_max);
        }
        if cell.row_span > 1 {
            let rs = cell.row_span;
            for v in rowspan_map.iter_mut().skip(col_pos).take(span) {
                if *v < rs {
                    *v = rs;
                }
            }
        }
        col_pos = end_col;
    }
}

/// Computes per-column widths for a `BoxKind::Table` element by scanning all rows
/// (direct and inside `TableRowGroup` children). Colspan/rowspan-aware: cells with
/// `colspan > 1` distribute their width across columns; `rowspan > 1` cells block
/// subsequent rows from reusing those columns. Returns a `Vec<f32>` of border-box
/// widths, one per column.
///
/// When `measurer` is provided, uses CSS 2.1 §17.5.2 content-based auto sizing:
/// each auto column gets at least its min-content width, with the remaining space
/// distributed proportionally to max-content widths. Without a measurer, falls back
/// to equal distribution among auto columns.
///
/// In Separate border mode, `(n_cols + 1) * h_spacing` is reserved for inter-cell and
/// outer gaps before distributing the remaining width among auto-width columns.
/// CSS: border-spacing — P4 wires h_spacing from ComputedStyle.border_spacing_h
fn compute_table_col_widths(
    b: &LayoutBox,
    content_width: f32,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
) -> Vec<f32> {
    let h_spacing = match b.style.border_collapse {
        BorderCollapse::Collapse => 0.0,
        BorderCollapse::Separate => b.style.border_spacing_h,
    };

    let mut col_explicit: Vec<Option<f32>> = Vec::new();
    let mut rowspan_map: Vec<u32> = Vec::new();

    for child in &b.children {
        match &child.kind {
            BoxKind::TableRow => {
                scan_row_explicit_widths(child, &mut col_explicit, &mut rowspan_map, content_width, viewport);
                decrement_rowspan_map(&mut rowspan_map);
            }
            BoxKind::TableRowGroup => {
                for row in &child.children {
                    if matches!(row.kind, BoxKind::TableRow) {
                        scan_row_explicit_widths(row, &mut col_explicit, &mut rowspan_map, content_width, viewport);
                        decrement_rowspan_map(&mut rowspan_map);
                    }
                }
            }
            _ => {}
        }
    }

    let n_cols = col_explicit.len();
    if n_cols == 0 {
        return Vec::new();
    }

    // Subtract spacing slots from available width before distributing to auto columns.
    let total_h_spacing = (n_cols + 1) as f32 * h_spacing;
    let total_explicit: f32 = col_explicit.iter().filter_map(|w| *w).sum();
    let available = (content_width - total_h_spacing - total_explicit).max(0.0);
    let auto_count = col_explicit.iter().filter(|w| w.is_none()).count();

    if auto_count == 0 {
        return col_explicit.iter().map(|w| w.unwrap_or(0.0)).collect();
    }

    // CSS 2.1 §17.5.2: content-based auto column sizing when a text measurer is available.
    if let Some(m) = measurer {
        let mut col_min = vec![0.0f32; n_cols];
        let mut col_max = vec![0.0f32; n_cols];
        let mut rs_map: Vec<u32> = Vec::new();
        for child in &b.children {
            match &child.kind {
                BoxKind::TableRow => {
                    scan_row_content_widths(child, &mut col_min, &mut col_max, &mut rs_map, m, viewport);
                    decrement_rowspan_map(&mut rs_map);
                }
                BoxKind::TableRowGroup => {
                    for row in &child.children {
                        if matches!(row.kind, BoxKind::TableRow) {
                            scan_row_content_widths(row, &mut col_min, &mut col_max, &mut rs_map, m, viewport);
                            decrement_rowspan_map(&mut rs_map);
                        }
                    }
                }
                _ => {}
            }
        }

        let auto_min_total: f32 = (0..n_cols)
            .filter(|&i| col_explicit[i].is_none())
            .map(|i| col_min[i])
            .sum();
        // Use col_max as the proportional weight; clamp at col_min so weight is always ≥ min.
        let total_weight: f32 = (0..n_cols)
            .filter(|&i| col_explicit[i].is_none())
            .map(|i| col_max[i].max(col_min[i]))
            .sum();

        return (0..n_cols)
            .map(|i| {
                col_explicit[i].unwrap_or_else(|| {
                    if auto_min_total >= available {
                        // Not enough space for min-content: distribute proportionally to min.
                        if auto_min_total > 0.0 {
                            (available * col_min[i] / auto_min_total).max(0.0)
                        } else {
                            available / auto_count as f32
                        }
                    } else {
                        // Enough for min; distribute extra proportionally to max-content weight.
                        let extra = available - auto_min_total;
                        let weight = col_max[i].max(col_min[i]);
                        col_min[i]
                            + if total_weight > 0.0 {
                                extra * weight / total_weight
                            } else {
                                extra / auto_count as f32
                            }
                    }
                })
            })
            .collect();
    }

    // Fallback without measurer: equal distribution.
    let auto_share = (available / auto_count as f32).max(0.0);
    col_explicit.iter().map(|w| w.unwrap_or(auto_share)).collect()
}
