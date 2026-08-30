//! Grid layout (`lay_out_grid`) — track distribution, line/named-area
//! resolution.
//!
//! Перенесено батчем SPLIT-BT6 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn grid_content_distribution`) без правок тел.

use super::*;

/// CSS Box Alignment L3 §5 — content distribution along one axis of a grid container.
///
/// Returns `(start_offset, extra_gap)`: how far the first track is pushed away from
/// the content-box start edge, and how much spacing to insert between every pair of
/// adjacent tracks on top of the `gap` property.
///
/// # Arguments
/// * `align` — the used `align-content` / `justify-content` value.
/// * `free` — leftover space after all tracks and their gaps.
/// * `n` — number of tracks on the axis.
///
/// With non-positive free space the axis overflows, and §5.3 replaces the
/// distribution with its fallback alignment — `space-between` → `start`,
/// `space-around` / `space-evenly` → `center` — after which the alignment is
/// resolved *unsafely*: `center` / `end` still shift the tracks back past the
/// content-box start edge (a negative offset), matching Edge. `safe` / `unsafe`
/// are not parsed, so the unsafe behaviour is unconditional.
///
/// `normal` / `stretch` always return `(0, 0)` — that pair is handled by the track
/// sizing pass, which hands the free space to the auto-sized tracks instead.
fn grid_content_distribution(align: AlignValue, free: f32, n: usize) -> (f32, f32) {
    if n == 0 {
        return (0.0, 0.0);
    }
    if free <= 0.0 {
        return match align {
            AlignValue::End => (free, 0.0),
            // `center` directly, plus the two distributions that fall back to it.
            AlignValue::Center | AlignValue::SpaceAround | AlignValue::SpaceEvenly => {
                (free / 2.0, 0.0)
            }
            // `start`, `space-between` (falls back to `start`), `normal`, `stretch`.
            _ => (0.0, 0.0),
        };
    }
    match align {
        AlignValue::End => (free, 0.0),
        AlignValue::Center => (free / 2.0, 0.0),
        AlignValue::SpaceBetween => {
            // A single track has no in-between gap — the spec falls back to `start`.
            if n <= 1 { (0.0, 0.0) } else { (0.0, free / (n - 1) as f32) }
        }
        AlignValue::SpaceAround => {
            let per = free / n as f32;
            (per / 2.0, per)
        }
        AlignValue::SpaceEvenly => {
            let per = free / (n + 1) as f32;
            (per, per)
        }
        _ => (0.0, 0.0),
    }
}

/// Size of the cell spanning tracks `t0..t1` (0-based, end-exclusive), measured from
/// the resolved track offsets.
///
/// Deriving the span from offsets rather than summing sizes + `gap` keeps spanning
/// items correct when `align-content` / `justify-content` injected extra spacing
/// between tracks (`space-between` and friends).
fn grid_track_span(offsets: &[f32], sizes: &[f32], t0: usize, t1: usize) -> f32 {
    let last = t1.max(t0 + 1) - 1;
    match (offsets.get(t0), offsets.get(last), sizes.get(last)) {
        (Some(&o0), Some(&o_last), Some(&s_last)) => (o_last + s_last - o0).max(0.0),
        _ => sizes.get(t0).copied().unwrap_or(0.0),
    }
}

/// CSS Grid Layout Level 1 — grid container layout.
///
/// Implements a Phase-0 subset of the grid layout algorithm (CSS Grid L1 §12):
///
/// - Explicit track lists (grid-template-columns / rows) with px, fr, auto.
/// - `repeat(N, size)` expansion.
/// - `minmax(min, max)` — min side used for sizing.
/// - Integer line numbers (positive only), `span N`, and `auto` placement.
/// - `grid-auto-flow: row | column` (no dense packing).
/// - `gap` / `column-gap` / `row-gap` between cells.
/// - `align-items` / `justify-items` within cells.
/// - `align-content` / `justify-content` (and the `place-content` shorthand)
///   distributing the container's free space between tracks — CSS Box Alignment
///   L3 §5 / CSS Grid L1 §12.3.
///
/// `definite_content_height` is the container's content-box block size when it is
/// definite (explicit `height`, box-sizing already applied), `None` when the height
/// is derived from the content. Only a definite height leaves block-axis free space
/// for `align-content` to distribute.
///
/// Returns the total content height of the grid.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_grid(
    children: &mut [LayoutBox],
    s: &ComputedStyle,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    definite_content_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let em = s.font_size;

    // CSS Grid L2 §9: If this grid was set up as a subgrid by its parent, read
    // the inherited track contexts that the parent set in the thread-locals.
    // We clear them immediately so our own children don't accidentally inherit them.
    let inherited_cols: Option<SubgridContext> = SUBGRID_COL_CTX.with(|c| c.borrow_mut().take());
    let inherited_rows: Option<SubgridContext> = SUBGRID_ROW_CTX.with(|c| c.borrow_mut().take());

    // Indices of actual items (non-Skip).
    let mut item_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();
    // CSS Grid §6: grid items are placed in "modified document order" — source order
    // reordered by the `order` property. A stable sort preserves source order among
    // items with equal `order`, so auto-placement honours `order` like Edge does.
    item_idxs.sort_by_key(|&i| children[i].style.order);

    if item_idxs.is_empty() {
        return 0.0;
    }

    // Gap between tracks.  When the axis is subgridded we use the parent's gap
    // (already baked into the offsets in SubgridContext); fall back to our own style.
    let col_gap = inherited_cols.as_ref()
        .map(|ctx| ctx.gap)
        .unwrap_or_else(|| s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0));
    let row_gap = inherited_rows.as_ref()
        .map(|ctx| ctx.gap)
        .unwrap_or_else(|| s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0));

    // CSS Grid L1 §7.2.3.4 — Phase 2: expand repeat(auto-fill|auto-fit, ...) at layout time.
    // If the style carried auto-repeat metadata, resolve the track count and build an expanded list.
    let auto_fill_col_tracks: Vec<GridTrackSize> =
        if let Some(ref rep) = s.grid_template_col_auto_repeat {
            let n = resolve_auto_fill_fit_count(content_width, &rep.tracks, col_gap).max(1);
            let mut tracks = Vec::with_capacity(n * rep.tracks.len());
            for _ in 0..n {
                tracks.extend_from_slice(&rep.tracks);
            }
            tracks
        } else {
            Vec::new()
        };
    let eff_col_template: &[GridTrackSize] = if s.grid_template_col_auto_repeat.is_some() {
        &auto_fill_col_tracks
    } else {
        &s.grid_template_columns
    };

    // CSS Masonry Layout (CSS Grid L3 §14) is not shipped by any stable browser —
    // Edge/Chrome treat `masonry` as an invalid track value and drop it, so the axis
    // falls back to `none` (a regular auto-sized grid). We match that ground truth:
    // strip the `masonry` sentinel from the effective track list on whichever axis
    // carries it, then fall through to the normal grid placement algorithm below.
    let col_is_masonry = eff_col_template.first() == Some(&GridTrackSize::Masonry);
    let row_is_masonry = s.grid_template_rows.first() == Some(&GridTrackSize::Masonry);
    let eff_col_template: &[GridTrackSize] = if col_is_masonry { &[] } else { eff_col_template };
    let eff_row_template: &[GridTrackSize] = if row_is_masonry { &[] } else { &s.grid_template_rows };

    // Determine explicit track counts.
    // Subgrid sentinel `[Subgrid]` is a single-element vec meaning "inherit all parent tracks";
    // for placement purposes use the number of inherited tracks (or 1 for auto-placement).
    let n_explicit_cols = if eff_col_template.first() == Some(&GridTrackSize::Subgrid) {
        inherited_cols.as_ref().map(|ctx| ctx.sizes.len()).unwrap_or(1).max(1)
    } else {
        eff_col_template.len().max(1)
    };

    // --- Step 1: Resolve placements for every item ---
    // placement: (col_start, col_end, row_start, row_end) all 1-based inclusive/exclusive.
    let mut placements: Vec<(u32, u32, u32, u32)> = vec![(0, 0, 0, 0); item_idxs.len()];

    let row_flow = !matches!(s.grid_auto_flow, GridAutoFlow::Column | GridAutoFlow::ColumnDense);

    // Pass 1: items with fully explicit placements.
    for (k, &i) in item_idxs.iter().enumerate() {
        let is = &children[i].style;

        // Resolve named area references first (grid-area: <name> shorthand or
        // individual grid-{row,column}-{start,end}: <name> values).
        let (named_cs, named_ce, named_rs, named_re) = {
            let has_named = matches!(&is.grid_column_start, GridLine::Named(_))
                || matches!(&is.grid_column_end, GridLine::Named(_))
                || matches!(&is.grid_row_start, GridLine::Named(_))
                || matches!(&is.grid_row_end, GridLine::Named(_));
            if has_named && !s.grid_template_areas.is_empty() {
                resolve_named_lines(
                    &is.grid_column_start,
                    &is.grid_column_end,
                    &is.grid_row_start,
                    &is.grid_row_end,
                    &s.grid_template_areas,
                )
            } else {
                (0, 0, 0, 0)
            }
        };

        // For each axis: use resolved named value if non-zero, else fall back to
        // the normal numeric/span resolver.
        let cs = if named_cs != 0 { named_cs } else { resolve_grid_line(&is.grid_column_start, n_explicit_cols as u32) };
        let ce = if named_ce != 0 { named_ce } else { resolve_grid_line_end(&is.grid_column_end, cs, n_explicit_cols as u32) };
        let rs = if named_rs != 0 { named_rs } else { resolve_grid_line(&is.grid_row_start, 0) };
        let re = if named_re != 0 { named_re } else { resolve_grid_line_end(&is.grid_row_end, rs, 0) };

        // `grid-column: span N` → start=Span(N), end=Auto → cs=0, ce=0.
        // resolve_grid_line returns 0 for Span-on-start, losing the count.
        // Recover the span so Pass 2 can use it for placement sizing.
        let ce = if ce == 0 {
            match &is.grid_column_start { GridLine::Span(n) => *n, _ => 0 }
        } else { ce };
        let re = if re == 0 {
            match &is.grid_row_start { GridLine::Span(n) => *n, _ => 0 }
        } else { re };

        if cs != 0 && rs != 0 {
            // Fully explicit: both axes known.
            placements[k] = (cs, ce, rs, re);
        } else if cs != 0 {
            // Column position fixed, row auto; preserve row-span if declared.
            placements[k] = (cs, ce, 0, re);
        } else if rs != 0 {
            // Row position fixed, column auto; preserve col-span if declared.
            placements[k] = (0, ce, rs, re);
        } else if ce > 0 || re > 0 {
            // Both axes auto but at least one span is declared (e.g. grid-column:span 2).
            // Store so pass-2 can recover the span via `end - 0 = span`.
            placements[k] = (0, ce, 0, re);
        }
        // All-auto no spans: stays (0,0,0,0) → span=1 in pass 2.
    }

    // Pass 2: auto-place remaining items — CSS Grid L1 §8.5 auto-placement algorithm.
    //
    // Two packing modes:
    //   Sparse (grid-auto-flow: row | column): cursor only moves forward.
    //   Dense  (grid-auto-flow: row dense | column dense): each item scans from
    //          (1,1) so it can fill gaps left by larger items.
    //
    // Occupancy HashSet replaces the O(k²) overlap scan from Pass 1 with O(1)
    // per-cell lookups.
    let dense = matches!(s.grid_auto_flow, GridAutoFlow::RowDense | GridAutoFlow::ColumnDense);
    let mut occupied: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    for &(cs, ce, rs, re) in &placements {
        if cs != 0 && rs != 0 {
            for r in rs..re {
                for c in cs..ce {
                    occupied.insert((c, r));
                }
            }
        }
    }

    let mut cursor_row: u32 = 1;
    let mut cursor_col: u32 = 1;

    for (k, _) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs != 0 && rs != 0 {
            continue; // explicitly placed
        }

        let col_span = if ce > cs { ce - cs } else { 1 };
        let row_span = if re > rs { re - rs } else { 1 };

        if row_flow {
            let fixed_cs = if cs != 0 { cs } else { 0 };
            let fixed_ce = if cs != 0 { ce } else { 0 };

            // Dense packing starts each scan from (1,1); sparse continues from cursor.
            let (mut scan_r, mut scan_c) = if dense { (1u32, 1u32) } else { (cursor_row, cursor_col) };

            // BUG-801: the column bound below must never be able to reject
            // EVERY scan position, or the loop has no exit. Two ways that
            // happened: an auto-placed item whose own `col_span` exceeds
            // `n_explicit_cols` (`grid-column: span 3` on a 2-column grid)
            // failed `fits` at every column, since the bound never grew past
            // the explicit track count — CSS Grid L1 §7.1 grows the implicit
            // grid to fit such an item rather than refusing it, so the bound
            // here does too. An item with an EXPLICIT column start beyond the
            // explicit grid (`grid-column: 9 / span 2` on 2 columns,
            // `fixed_cs != 0`) failed the same check at every row, since
            // `try_ce_val` is fixed and never changes — that placement is not
            // a search at all, so it is exempted from the bound entirely and
            // only occupancy still applies.
            let col_bound = (n_explicit_cols as u32).max(col_span);

            loop {
                let try_c   = if fixed_cs != 0 { fixed_cs } else { scan_c };
                let try_ce_val = if fixed_cs != 0 { fixed_ce } else { try_c + col_span };

                // Bounds: item must fit within the (possibly grid-grown) column count.
                let fits = fixed_cs != 0 || (try_ce_val - 1) <= col_bound;
                let cell_free = fits && (try_c..try_ce_val)
                    .all(|c| (scan_r..scan_r + row_span).all(|r| !occupied.contains(&(c, r))));

                if cell_free {
                    placements[k] = (try_c, try_ce_val, scan_r, scan_r + row_span);
                    for r in scan_r..scan_r + row_span {
                        for c in try_c..try_ce_val {
                            occupied.insert((c, r));
                        }
                    }
                    // Track highest placed row for grid-size calculation.
                    cursor_row = cursor_row.max(scan_r);
                    if !dense {
                        cursor_col = try_ce_val;
                        if cursor_col > n_explicit_cols as u32 {
                            cursor_col = 1;
                            cursor_row += 1;
                        }
                    }
                    break;
                }

                // Advance scan position.
                if fixed_cs != 0 {
                    scan_r += 1;
                    scan_c = 1;
                } else {
                    scan_c += 1;
                    if scan_c > n_explicit_cols as u32 {
                        scan_c = 1;
                        scan_r += 1;
                    }
                }
            }
        } else {
            // Column flow: fill top-to-bottom, wrap to next column.
            let n_explicit_rows = eff_row_template.len().max(1) as u32;
            let fixed_rs = if rs != 0 { rs } else { 0 };
            let fixed_re = if rs != 0 { re } else { 0 };

            let (mut scan_r, mut scan_c) = if dense { (1u32, 1u32) } else { (cursor_row, cursor_col) };

            // BUG-801, column-flow mirror of the row-flow fix above.
            let row_bound = n_explicit_rows.max(row_span);

            loop {
                let try_r      = if fixed_rs != 0 { fixed_rs } else { scan_r };
                let try_re_val = if fixed_rs != 0 { fixed_re } else { try_r + row_span };

                let fits = fixed_rs != 0 || (try_re_val - 1) <= row_bound;
                let cell_free = fits && (scan_c..scan_c + col_span)
                    .all(|c| (try_r..try_re_val).all(|r| !occupied.contains(&(c, r))));

                if cell_free {
                    placements[k] = (scan_c, scan_c + col_span, try_r, try_re_val);
                    for r in try_r..try_re_val {
                        for c in scan_c..scan_c + col_span {
                            occupied.insert((c, r));
                        }
                    }
                    cursor_col = cursor_col.max(scan_c);
                    if !dense {
                        cursor_row = try_re_val;
                        if cursor_row > n_explicit_rows {
                            cursor_row = 1;
                            cursor_col += 1;
                        }
                    }
                    break;
                }

                if fixed_rs != 0 {
                    scan_c += 1;
                    scan_r = 1;
                } else {
                    scan_r += 1;
                    if scan_r > n_explicit_rows {
                        scan_r = 1;
                        scan_c += 1;
                    }
                }
            }
        }
    }

    // --- Step 2: Determine total grid dimensions ---
    let n_cols = placements.iter().map(|&(_, ce, _, _)| ce.saturating_sub(1)).max().unwrap_or(1)
        .max(n_explicit_cols as u32);
    let n_rows = placements.iter().map(|&(_, _, _, re)| re.saturating_sub(1)).max().unwrap_or(1);

    // --- Step 3: Compute column widths ---
    // If the column axis is subgridded, use the inherited track sizes directly;
    // otherwise compute from the style as usual (CSS Grid L2 §9).
    let (col_widths, col_offsets) = if let Some(ref ctx) = inherited_cols {
        // Subgrid column axis: clip to n_cols (parent may span more tracks than
        // the explicit template; auto-place inside those tracks).
        let sizes: Vec<f32> = ctx.sizes.iter().take(n_cols as usize).cloned().collect();
        let offsets: Vec<f32> = ctx.offsets.iter().take(n_cols as usize).cloned().collect();
        (sizes, offsets)
    } else {
        // Normal grid: compute column widths from the style.
        let mut col_widths: Vec<f32> = (0..n_cols)
            .map(|c| {
                let ts = grid_track(c, eff_col_template, &s.grid_auto_columns);
                match ts {
                    GridTrackSize::Length(l) => l.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0),
                    GridTrackSize::Minmax(min, _) => min.resolve_fixed(em, content_width, viewport).unwrap_or(0.0),
                    // Subgrid sentinel without parent context — fall back to auto.
                    GridTrackSize::Subgrid => 0.0,
                    _ => 0.0, // fr / auto resolved later
                }
            })
            .collect();

        // Total gap between columns.
        let total_col_gap = if n_cols > 1 { col_gap * (n_cols - 1) as f32 } else { 0.0 };
        let fixed_col_total: f32 = col_widths.iter().sum::<f32>() + total_col_gap;
        let free_col = (content_width - fixed_col_total).max(0.0);

        // Distribute fr among column tracks.
        let total_fr: f32 = (0..n_cols)
            .map(|c| grid_track(c, eff_col_template, &s.grid_auto_columns).fr().unwrap_or(0.0))
            .sum();
        let auto_col_count = (0..n_cols)
            .filter(|&c| matches!(
                grid_track(c, eff_col_template, &s.grid_auto_columns),
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent
            ))
            .count();

        // For auto columns, divide remaining free space equally (after fr).
        let fr_width = if total_fr > 0.0 { free_col / total_fr } else { 0.0 };
        let auto_col_width = if auto_col_count > 0 && total_fr == 0.0 {
            free_col / auto_col_count as f32
        } else {
            0.0
        };

        for c in 0..n_cols {
            match grid_track(c, eff_col_template, &s.grid_auto_columns) {
                GridTrackSize::Fr(f) => col_widths[c as usize] = (f * fr_width).max(0.0),
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent => {
                    col_widths[c as usize] = auto_col_width;
                }
                _ => {}
            }
        }

        // CSS Box Alignment L3 §5 — `justify-content` distributes whatever inline-axis
        // space the tracks left over. `fr` / `auto` tracks already absorb it during
        // sizing above, so this only ever fires for a fixed-size track list.
        let used_col_total: f32 = col_widths.iter().sum::<f32>() + total_col_gap;
        let (jc_start, jc_extra) = grid_content_distribution(
            s.justify_content,
            content_width - used_col_total,
            n_cols as usize,
        );

        // Column start offsets.
        let mut col_offsets: Vec<f32> = Vec::with_capacity(n_cols as usize);
        let mut x_off = jc_start;
        for c in 0..n_cols {
            col_offsets.push(x_off);
            x_off += col_widths[c as usize]
                + if c < n_cols - 1 { col_gap + jc_extra } else { 0.0 };
        }

        (col_widths, col_offsets)
    };

    // --- Step 4: Layout items to measure row heights ---
    // If the row axis is subgridded, use inherited sizes; otherwise compute from style.
    let mut row_heights: Vec<f32> = if let Some(ref ctx) = inherited_rows {
        ctx.sizes.iter().take(n_rows as usize).cloned().collect()
    } else {
        (0..n_rows)
            .map(|r| {
                match grid_track(r, eff_row_template, &s.grid_auto_rows) {
                    GridTrackSize::Length(l) => l.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0),
                    GridTrackSize::Minmax(min, _) => min.resolve_fixed(em, content_width, viewport).unwrap_or(0.0),
                    GridTrackSize::Subgrid => 0.0,
                    _ => 0.0,
                }
            })
            .collect()
    };

    // Row offsets (computed from row_heights regardless of subgrid).
    // For subgrid row axis the offsets are inherited below in final pass.

    // BUG-341 S33: this probe pass and Step 5's final positioning pass below
    // always call `lay_out` with the exact same `(width, height=None)` for a
    // given non-subgrid item — `col_offsets`/`col_widths` are resolved once,
    // above this loop, and nothing between here and Step 5 touches them again,
    // so `cell_w` is bit-identical for both passes *by construction*, not just
    // "happens to match" the way S30-S32's general `(node, width, height)`
    // cache could only ever hope for. Stash each non-subgrid item's probe
    // result and reuse it directly in Step 5 instead of laying the subtree out
    // twice — the one real redundancy the S28-S32 general layout-result cache
    // slices ever found a case for, captured here with zero overhead on every
    // other box in the document (no thread-local `HashMap`, no per-call key,
    // nothing paid on a miss that never repeats — see `CV_AUTO_TOUCHED`'s doc
    // comment for why the general mechanism was removed instead of kept).
    //
    // Subgrid items are excluded: their own recursive `lay_out_grid` reads a
    // thread-local track context (`SubgridContextGuard`, set in both arms
    // below) that genuinely differs between this estimated-tracks probe and
    // Step 5's resolved-tracks final pass.
    let mut probe_reuse: Vec<Option<(f32, f32, LayoutBox)>> = vec![None; item_idxs.len()];

    // Layout each item in its cell to determine content height.
    for (k, &i) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs == 0 || rs == 0 {
            continue; // unplaced (should not happen after auto-placement)
        }
        let c0 = (cs - 1).min(n_cols - 1) as usize;
        let c1 = (ce - 1).min(n_cols) as usize;
        let cell_w: f32 = grid_track_span(&col_offsets, &col_widths, c0, c1);

        // For subgrid children: set the thread-local context before laying out.
        let child_col_subgrid = children[i].style.grid_template_columns.first()
            == Some(&GridTrackSize::Subgrid);
        let child_row_subgrid = children[i].style.grid_template_rows.first()
            == Some(&GridTrackSize::Subgrid);

        if child_col_subgrid || child_row_subgrid {
            // Build subgrid context slices from our resolved track sizes.
            let child_col_ctx = if child_col_subgrid && c1 > c0 {
                Some(SubgridContext::from_parent_tracks(&col_widths[c0..c1], col_gap))
            } else {
                None
            };
            let child_row_ctx = if child_row_subgrid {
                // Row heights not fully determined yet; pass current estimates.
                let r0 = (rs - 1).min(n_rows - 1) as usize;
                let re_eff = re.max(rs + 1);
                let r1 = (re_eff - 1).min(n_rows) as usize;
                if r1 > r0 {
                    Some(SubgridContext::from_parent_tracks(&row_heights[r0..r1], row_gap))
                } else {
                    None
                }
            } else {
                None
            };
            let _guard = SubgridContextGuard::set(child_col_ctx, child_row_ctx);
            lay_out(&mut children[i], content_x + col_offsets.get(c0).copied().unwrap_or(0.0), 0.0, cell_w, None, measurer, viewport, pcb, hp, false);
        } else {
            // Layout at temporary position (y=0) to get intrinsic height.
            let probe_x = content_x + col_offsets.get(c0).copied().unwrap_or(0.0);
            let probe_y = 0.0;
            let outer_cv_touched = CV_AUTO_TOUCHED.with(|c| c.replace(false));
            lay_out(&mut children[i], probe_x, probe_y, cell_w, None, measurer, viewport, pcb, hp, false);
            let touched_here = CV_AUTO_TOUCHED.with(|c| c.get());
            CV_AUTO_TOUCHED.with(|c| c.set(outer_cv_touched || touched_here));
            if !touched_here {
                probe_reuse[k] = Some((probe_x, probe_y, children[i].clone()));
            }
        }

        // Update auto row heights.
        let r0 = (rs - 1) as usize;
        if r0 < row_heights.len()
            && inherited_rows.is_none()
            && matches!(
                grid_track(r0 as u32, eff_row_template, &s.grid_auto_rows),
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent | GridTrackSize::Fr(_)
            )
        {
            let item_h = children[i].rect.height;
            if item_h > row_heights[r0] {
                row_heights[r0] = item_h;
            }
        }
    }

    // Resolve fr row heights (skip when row axis is subgridded — sizes are fixed).
    let total_row_gap = if n_rows > 1 { row_gap * (n_rows - 1) as f32 } else { 0.0 };
    if inherited_rows.is_none() {
        // CSS Grid L1 §11.7 — the free space available to flexible (`fr`) tracks is
        // the container's content size minus the base sizes of the OTHER tracks
        // only. `row_heights[r]` for an `fr` track was seeded from its content's
        // probed intrinsic height above (the fallback used when the container's
        // block size is indefinite) — that probed value is a floor for the final
        // `.max()` below, not a "fixed" size to subtract here. Counting it against
        // `definite_content_height` double-dips: a two-row `1fr 1fr` grid with
        // ~29px-tall cell content and a 220px definite height wrongly landed each
        // row at (220 - 29*2) / 2 ≈ 83px instead of 220 / 2 = 110px, leaving a
        // ~58px unaccounted gap at the bottom (found via TEST-62 BUG-277 triage).
        let fixed_row_total: f32 = (0..n_rows)
            .map(|r| {
                if grid_track(r, eff_row_template, &s.grid_auto_rows).fr().is_some() {
                    0.0
                } else {
                    row_heights[r as usize]
                }
            })
            .sum::<f32>()
            + total_row_gap;
        // If container has explicit height, distribute fr rows from it.
        let free_row = definite_content_height.map(|h| (h - fixed_row_total).max(0.0)).unwrap_or(0.0);
        let total_row_fr: f32 = (0..n_rows)
            .map(|r| grid_track(r, eff_row_template, &s.grid_auto_rows).fr().unwrap_or(0.0))
            .sum();
        if total_row_fr > 0.0 && free_row > 0.0 {
            let fr_h = free_row / total_row_fr;
            for r in 0..n_rows {
                if let Some(f) = grid_track(r, eff_row_template, &s.grid_auto_rows).fr() {
                    row_heights[r as usize] = (f * fr_h).max(row_heights[r as usize]);
                }
            }
        }

        // CSS Grid L1 §12.3 — `align-content: normal` behaves as `stretch` for a grid
        // container: the leftover block-axis space is shared equally between the
        // `auto`-sized rows. Only an explicitly sized container has leftover space.
        // Deferred: `minmax(_, auto)` rows do not participate — the track-sizing pass
        // above resolves them from their min side, not as auto.
        if matches!(s.align_content, AlignValue::Auto | AlignValue::Normal | AlignValue::Stretch) {
            let auto_rows: Vec<u32> = (0..n_rows)
                .filter(|&r| matches!(grid_track(r, eff_row_template, &s.grid_auto_rows), GridTrackSize::Auto))
                .collect();
            let used: f32 = row_heights.iter().sum::<f32>() + total_row_gap;
            let free = definite_content_height.map(|h| h - used).unwrap_or(0.0);
            if free > 0.0 && !auto_rows.is_empty() {
                let per = free / auto_rows.len() as f32;
                for r in auto_rows {
                    row_heights[r as usize] += per;
                }
            }
        }
    }

    // Row top offsets: if row axis is subgridded, use inherited offsets; else compute.
    let (row_offsets, y_off) = if let Some(ref ctx) = inherited_rows {
        let offsets: Vec<f32> = ctx.offsets.iter().take(n_rows as usize).cloned().collect();
        let total = ctx.total_size();
        (offsets, total)
    } else {
        // CSS Box Alignment L3 §5 — `align-content` distributes the block-axis free
        // space left over by the tracks (only ever non-zero for a definite height).
        let used_row_total: f32 = row_heights.iter().sum::<f32>() + total_row_gap;
        let (ac_start, ac_extra) = grid_content_distribution(
            s.align_content,
            definite_content_height.map(|h| h - used_row_total).unwrap_or(0.0),
            n_rows as usize,
        );

        let mut row_offsets: Vec<f32> = Vec::with_capacity(n_rows as usize);
        let mut y_off = ac_start;
        for r in 0..n_rows {
            row_offsets.push(y_off);
            y_off += row_heights[r as usize]
                + if r < n_rows - 1 { row_gap + ac_extra } else { 0.0 };
        }
        (row_offsets, y_off)
    };
    let mut y_off = y_off;

    // --- Step 5: Final positioning pass ---
    for (k, &i) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs == 0 || rs == 0 {
            // Unplaced — stack below grid content.
            lay_out(&mut children[i], content_x, content_y + y_off, content_width, None, measurer, viewport, pcb, hp, false);
            y_off += children[i].rect.height;
            continue;
        }
        let c0 = (cs - 1).min(n_cols - 1) as usize;
        let c1 = (ce - 1).min(n_cols) as usize;
        let r0 = (rs - 1).min(n_rows - 1) as usize;
        let r1 = (re - 1).min(n_rows) as usize;

        let cell_x = content_x + col_offsets.get(c0).copied().unwrap_or(0.0);
        let cell_y = content_y + row_offsets.get(r0).copied().unwrap_or(0.0);
        let cell_w: f32 = grid_track_span(&col_offsets, &col_widths, c0, c1);
        let cell_h: f32 = grid_track_span(&row_offsets, &row_heights, r0, r1);

        // Re-layout with final cell width. For subgrid children, restore the context.
        let child_col_subgrid = children[i].style.grid_template_columns.first()
            == Some(&GridTrackSize::Subgrid);
        let child_row_subgrid = children[i].style.grid_template_rows.first()
            == Some(&GridTrackSize::Subgrid);
        if child_col_subgrid || child_row_subgrid {
            let final_col_ctx = if child_col_subgrid && c1 > c0 {
                Some(SubgridContext::from_parent_tracks(&col_widths[c0..c1], col_gap))
            } else {
                None
            };
            let final_row_ctx = if child_row_subgrid && r1 > r0 {
                Some(SubgridContext::from_parent_tracks(&row_heights[r0..r1], row_gap))
            } else {
                None
            };
            let _guard = SubgridContextGuard::set(final_col_ctx, final_row_ctx);
            lay_out(&mut children[i], cell_x, cell_y, cell_w, None, measurer, viewport, pcb, hp, false);
        } else if let Some((probe_x, probe_y, mut reused)) = probe_reuse[k].take() {
            // BUG-341 S33: `cell_w` above was derived from the same
            // `col_offsets`/`col_widths`/`(c0, c1)` as the probe pass's, so
            // the subtree reused here already has the correct final size —
            // only its position needs to catch up to the resolved row offset.
            crate::incremental::translate_subtree(&mut reused, cell_x - probe_x, cell_y - probe_y);
            children[i] = reused;
        } else {
            // No usable probe: an unplaced-at-probe-time item can't reach
            // here (handled by the early-continue above), so this is a
            // subtree whose probe touched `content-visibility: auto` and was
            // refused for reuse (see `CV_AUTO_TOUCHED`'s doc comment).
            lay_out(&mut children[i], cell_x, cell_y, cell_w, None, measurer, viewport, pcb, hp, false);
        }

        let item = &mut children[i];
        let is = &item.style;
        let iem = is.font_size;
        let m_t = is.margin_top.resolve_or_zero(iem, content_width, viewport);
        let m_b = is.margin_bottom.resolve_or_zero(iem, content_width, viewport);
        let m_l = is.margin_left.resolve_or_zero(iem, content_width, viewport);
        let m_r = is.margin_right.resolve_or_zero(iem, content_width, viewport);

        // align-items (cross / block axis within cell).
        let align = if matches!(is.align_self, AlignValue::Auto) { s.align_items } else { is.align_self };
        let item_outer_h = item.rect.height + m_t + m_b;
        match align {
            AlignValue::End => {
                item.rect.y = cell_y + cell_h - item.rect.height - m_b;
            }
            AlignValue::Center => {
                item.rect.y = cell_y + (cell_h - item_outer_h) / 2.0 + m_t;
            }
            AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                // CSS Grid §11.2: `stretch` only grows items whose used block size is
                // `auto`; an explicit `height` is preserved (the item is top-aligned in
                // the cell, leaving free space below — like Edge).
                if is.height.is_none() && item.rect.height < cell_h - m_t - m_b {
                    item.rect.height = (cell_h - m_t - m_b).max(item.rect.height);
                }
                item.rect.y = cell_y + m_t;
            }
            _ => {
                item.rect.y = cell_y + m_t;
            }
        }

        // justify-items (inline axis within cell).
        let justify = if matches!(is.justify_self, AlignValue::Auto) { s.justify_items } else { is.justify_self };
        let item_outer_w = item.rect.width + m_l + m_r;
        match justify {
            AlignValue::End => {
                item.rect.x = cell_x + cell_w - item.rect.width - m_r;
            }
            AlignValue::Center => {
                item.rect.x = cell_x + (cell_w - item_outer_w) / 2.0 + m_l;
            }
            AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                item.rect.x = cell_x + m_l;
            }
            _ => {
                item.rect.x = cell_x + m_l;
            }
        }
    }

    y_off
}

/// CSS Grid Layout L3 §9 — Resolve `repeat(auto-fill|auto-fit, <track-list>)` count.
/// Returns the number of tracks to fill the available space when using auto-fill or auto-fit.
///
/// # Arguments
/// * `available_width` — CSS px width of the container content box.
/// * `track_sizes` — The track sizes inside the repeat(), e.g. `[minmax(100px, 1fr)]`.
/// * `gap` — Column gap in px.
/// * `auto_fit` — If true, resolve as auto-fit (collapse empty tracks); else auto-fill.
///
/// # Returns
/// The minimum number of tracks that fit in available space, with preference
/// for auto-fill (leave empty) over auto-fit (collapse).
pub fn resolve_auto_fill_fit_count(
    available_width: f32,
    track_sizes: &[GridTrackSize],
    gap: f32,
) -> usize {
    if track_sizes.is_empty() || available_width <= 0.0 {
        return 1; // At least one track
    }

    // Compute minimum track width: the min() sizing function of each track.
    // For minmax(min, max), use min. For auto/fr/max-content, use 0 as placeholder (content-sized).
    let mut track_min_width: f32 = 0.0;
    for track in track_sizes {
        let w = match track {
            GridTrackSize::Length(len) => {
                // Fixed length: use as-is (simplified: only px supported in this pass)
                len.resolve(1.0, Some(available_width), Size::new(1024.0, 768.0))
                    .unwrap_or(0.0)
            }
            GridTrackSize::Minmax(min, _max) => {
                // Use the min() part
                min.resolve_fixed(1.0, available_width, Size::new(1024.0, 768.0))
                    .unwrap_or(0.0)
            }
            GridTrackSize::FitContent(limit) => {
                // Use the limit as min sizing (simplified)
                limit.resolve_fixed(1.0, available_width, Size::new(1024.0, 768.0))
                    .unwrap_or(0.0)
            }
            // Auto, MinContent, MaxContent, Fr, Subgrid: no fixed minimum, use 0
            _ => 0.0,
        };
        track_min_width = track_min_width.max(w);
    }

    // Count tracks: (available_width + gap) / (track_min_width + gap), minimum 1.
    let gap_adjusted_available = available_width + gap;
    let track_plus_gap = track_min_width + gap;

    if track_plus_gap <= 0.0 {
        1
    } else {
        ((gap_adjusted_available / track_plus_gap).floor() as usize).max(1)
    }
}

/// Return the track size for track index `idx` (0-based) from a template list,
/// falling back to `auto_track` for implicit tracks beyond the template.
fn grid_track<'a>(idx: u32, template: &'a [GridTrackSize], auto_track: &'a GridTrackSize) -> &'a GridTrackSize {
    template.get(idx as usize).unwrap_or(auto_track)
}

/// Resolve a `GridLine` to a 1-based track number, or 0 if auto.
fn resolve_grid_line(line: &GridLine, n_tracks: u32) -> u32 {
    match line {
        GridLine::Auto | GridLine::Named(_) => 0,
        GridLine::Line(n) => {
            if *n > 0 {
                *n as u32
            } else if n_tracks > 0 {
                // Negative line numbers count from the end.
                (n_tracks as i32 + 1 + n).max(1) as u32
            } else {
                1
            }
        }
        GridLine::Span(_) => 0, // span on start — auto
    }
}

/// Resolve a grid-line end given start position and span.
fn resolve_grid_line_end(line: &GridLine, start: u32, n_tracks: u32) -> u32 {
    match line {
        GridLine::Auto | GridLine::Named(_) => {
            if start > 0 { start + 1 } else { 0 }
        }
        GridLine::Line(n) => {
            if *n > 0 {
                (*n as u32).max(start + 1)
            } else if n_tracks > 0 {
                let abs = (n_tracks as i32 + 1 + n).max(1) as u32;
                abs.max(start + 1)
            } else {
                start + 1
            }
        }
        GridLine::Span(n) => {
            // When start is known: end = start + span.
            // When start is auto (0): store span N directly so pass-2 placement
            // can use `re - rs = N - 0 = N` to recover the span count.
            if start > 0 { start + n } else { *n }
        }
    }
}

/// CSS Grid L1 §7.3 — locate a named area in `grid-template-areas`.
///
/// Returns `(row_start, row_end, col_start, col_end)` as 1-based exclusive
/// line numbers, or `None` if the name is not found. Handles rectangular
/// area shapes only (CSS Grid L1 requires areas to be rectangular).
fn find_named_area(areas: &[Vec<String>], name: &str) -> Option<(u32, u32, u32, u32)> {
    let mut row_start: Option<u32> = None;
    let mut row_end: Option<u32> = None;
    let mut col_start: Option<u32> = None;
    let mut col_end: Option<u32> = None;
    for (r, row) in areas.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if cell == name {
                let rs = (r + 1) as u32;
                let re = (r + 2) as u32;
                let cs = (c + 1) as u32;
                let ce = (c + 2) as u32;
                row_start = Some(row_start.map_or(rs, |v: u32| v.min(rs)));
                row_end   = Some(row_end.map_or(re,   |v: u32| v.max(re)));
                col_start = Some(col_start.map_or(cs, |v: u32| v.min(cs)));
                col_end   = Some(col_end.map_or(ce,   |v: u32| v.max(ce)));
            }
        }
    }
    Some((row_start?, row_end?, col_start?, col_end?))
}

/// Resolve named grid-line references for a single item against the
/// container's `grid-template-areas`. Returns `(col_start, col_end, row_start, row_end)`.
///
/// When all four placement properties are `Named(same_name)` (set by
/// `grid-area: <name>` shorthand), the area bounds are looked up once and
/// applied to all four axes. Mixed named/unnamed configurations fall back
/// to `Auto` (0) for any unresolved axis.
fn resolve_named_lines(
    col_start: &GridLine,
    col_end: &GridLine,
    row_start: &GridLine,
    row_end: &GridLine,
    areas: &[Vec<String>],
) -> (u32, u32, u32, u32) {
    // When grid-area: <name> sets all four to Named(name), resolve as one area.
    if let (
        GridLine::Named(n_cs),
        GridLine::Named(n_ce),
        GridLine::Named(n_rs),
        GridLine::Named(n_re),
    ) = (col_start, col_end, row_start, row_end)
        && n_cs == n_ce
        && n_ce == n_rs
        && n_rs == n_re
        && let Some((rs, re, cs, ce)) = find_named_area(areas, n_cs)
    {
        return (cs, ce, rs, re);
    }
    // Partial Named references: each axis resolved independently.
    let cs = if let GridLine::Named(n) = col_start {
        find_named_area(areas, n).map_or(0, |(_, _, cs, _)| cs)
    } else { 0 };
    let ce = if let GridLine::Named(n) = col_end {
        find_named_area(areas, n).map_or(0, |(_, _, _, ce)| ce)
    } else { 0 };
    let rs = if let GridLine::Named(n) = row_start {
        find_named_area(areas, n).map_or(0, |(rs, _, _, _)| rs)
    } else { 0 };
    let re = if let GridLine::Named(n) = row_end {
        find_named_area(areas, n).map_or(0, |(_, re, _, _)| re)
    } else { 0 };
    (cs, ce, rs, re)
}
