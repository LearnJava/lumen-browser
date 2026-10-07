//! Grid layout (`lay_out_grid`) — track distribution, line/named-area
//! resolution.
//!
//! Перенесено батчем SPLIT-BT6 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn grid_content_distribution`) без правок тел.

use super::*;
use crate::style::{GridRepeat, RepeatCount};

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
pub(super) fn grid_content_distribution(align: AlignValue, free: f32, n: usize) -> (f32, f32) {
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

/// CSS Grid L1 §7.2.3.2 — number of repetitions of an auto `repeat()` that fit `avail`
/// (the container's definite content size on the axis; `None` → the list repeats once).
///
/// Every track counts at its definite minimum size (`minmax(100px, 1fr)` → 100px), the
/// tracks around the repeat (`rep.before` / `rep.after`) and the gutters between all
/// tracks take their space first; at least one repetition is always produced.
fn auto_repeat_count(rep: &GridRepeat, avail: Option<f32>, gap: f32, em: f32, viewport: Size) -> usize {
    let Some(avail) = avail.filter(|a| *a > 0.0) else {
        return 1;
    };
    let min_px = |t: &GridTrackSize| match t {
        GridTrackSize::FitContent(limit) => limit.resolve_fixed(em, avail, viewport).unwrap_or(0.0),
        other => other.resolve_fixed(em, avail, viewport).unwrap_or(0.0).max(0.0),
    };
    let fixed_sum: f32 = rep.before.iter().chain(&rep.after).map(min_px).sum();
    let fixed_n = rep.before.len() + rep.after.len();
    let unit_sum: f32 = rep.tracks.iter().map(min_px).sum();
    let unit_n = rep.tracks.len();
    // k · (unit_sum + unit_n · gap) ≤ avail − fixed_sum − (fixed_n − 1) · gap
    let per_unit = unit_sum + unit_n as f32 * gap;
    if per_unit <= 0.0 {
        return 1;
    }
    let room = avail - fixed_sum - (fixed_n as f32 - 1.0) * gap;
    ((room / per_unit).floor().max(1.0)) as usize
}

/// The track list with the auto `repeat()` expanded `count` times between the tracks
/// written around it.
fn expand_auto_repeat(rep: &GridRepeat, count: usize) -> Vec<GridTrackSize> {
    let mut out = Vec::with_capacity(rep.before.len() + rep.after.len() + count * rep.tracks.len());
    out.extend_from_slice(&rep.before);
    for _ in 0..count {
        out.extend_from_slice(&rep.tracks);
    }
    out.extend_from_slice(&rep.after);
    out
}

/// CSS Grid L1 §7.2.3.2 — `repeat(auto-fit, …)`: tracks produced by the repeat that hold no
/// item are collapsed (sized `0`, the gutters on both sides merge into one). Returns the
/// track list with those tracks replaced by `0px` and the per-track collapsed flags; for
/// any other template (no `auto-fit`) the flags are all `false` and the list is unchanged.
///
/// `used(t)` — whether some item covers 0-based track `t`.
fn collapse_auto_fit(
    template: &[GridTrackSize],
    rep: Option<&GridRepeat>,
    count: usize,
    used: impl Fn(usize) -> bool,
) -> (Vec<GridTrackSize>, Vec<bool>) {
    let mut collapsed = vec![false; template.len()];
    let mut tracks = template.to_vec();
    if let Some(rep) = rep.filter(|r| r.count == RepeatCount::AutoFit) {
        let first = rep.before.len();
        for t in first..(first + count * rep.tracks.len()).min(template.len()) {
            if !used(t) {
                collapsed[t] = true;
                tracks[t] = GridTrackSize::Length(Length::Px(0.0));
            }
        }
    }
    (tracks, collapsed)
}

/// Start offsets of tracks sized `sizes` along one axis: one `gap` between consecutive
/// non-collapsed tracks, so the gutters on both sides of a collapsed track (or run of
/// them) merge into a single one and no gutter is left at the container edges. `start` is
/// where the first track begins; the content-distribution extra is added per gutter.
pub(super) fn track_offsets(sizes: &[f32], collapsed: &[bool], gap: f32, extra: f32, start: f32) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(sizes.len());
    let mut pos = start;
    let mut any_visible = false;
    for (t, &size) in sizes.iter().enumerate() {
        if collapsed.get(t).copied().unwrap_or(false) {
            offsets.push(pos);
            continue;
        }
        if any_visible {
            pos += gap + extra;
        }
        offsets.push(pos);
        pos += size;
        any_visible = true;
    }
    offsets
}

/// Number of gutters on an axis: one between each pair of consecutive non-collapsed tracks.
pub(super) fn gutter_count(collapsed: &[bool], n: usize) -> usize {
    let visible = (0..n).filter(|&t| !collapsed.get(t).copied().unwrap_or(false)).count();
    visible.saturating_sub(1)
}

/// Size of the cell spanning tracks `t0..t1` (0-based, end-exclusive), measured from
/// the resolved track offsets.
///
/// Deriving the span from offsets rather than summing sizes + `gap` keeps spanning
/// items correct when `align-content` / `justify-content` injected extra spacing
/// between tracks (`space-between` and friends).
pub(super) fn grid_track_span(offsets: &[f32], sizes: &[f32], t0: usize, t1: usize) -> f32 {
    let last = t1.max(t0 + 1) - 1;
    match (offsets.get(t0), offsets.get(last), sizes.get(last)) {
        (Some(&o0), Some(&o_last), Some(&s_last)) => (o_last + s_last - o0).max(0.0),
        _ => sizes.get(t0).copied().unwrap_or(0.0),
    }
}

/// CSS Grid Layout Level 1 — grid container layout, loop-entry construction.
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
/// `vertical` (GRID-VWM): `Some` for a container in a vertical `writing-mode`. Everything here
/// then stays logical — `content_width` is the inline extent (the physical content height),
/// `definite_content_height` the definite block extent (the physical content width), the
/// `col_*` vectors run along physical y and the `row_*` ones along x; the physical mapping is
/// done by `grid_vertical` at the item boundary. Only the `min-/max-content` column sizing
/// reads it here (inline-axis margins and intrinsic height instead of width).
///
/// `definite_content_height` is the container's content-box block size when it is
/// definite (explicit `height`, box-sizing already applied), `None` when the height
/// is derived from the content. Only a definite height leaves block-axis free space
/// for `align-content` to distribute.
///
/// LAYOUT-2 срез 4: Steps 1–3 below (placement resolution, column-track sizing)
/// never call `lay_out` on a child — they run natively here, same as flex's
/// Step 1–3 precompute in `build_flex_init`. Steps 4–5 (the per-item probe and
/// final-placement passes, CSS Grid L1 §12.3/§11.2 — the two loops that call
/// `lay_out`/`dispatch_box` on each item and then read its `.rect` back) are
/// the non-tail-recursive part this slice targets; they are captured into the
/// returned [`super::grid_trampoline::GridInit`] instead of running here, and
/// `grid_trampoline::run` drives them (and every further grid-container
/// descendant it meets, incl. subgrid) on an explicit heap stack. Returns
/// `None` when there are no items — the caller uses the same zero-height
/// epilogue `grid_trampoline::finish_container_height` uses for a populated
/// container, so the empty case does not need its own copy of that logic.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_grid_init(
    children: &[LayoutBox],
    s: &Arc<ComputedStyle>,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    definite_content_height: Option<f32>,
    viewport: Size,
    pcb: Rect,
    em: f32,
    available_height: Option<f32>,
    padding_top: f32,
    padding_bottom: f32,
    size_contained: bool,
    is_positioned: bool,
    own_pcb: Rect,
    measurer: Option<&dyn TextMeasurer>,
    vertical: Option<super::grid_vertical::VGridGeom>,
) -> Option<Box<super::grid_trampoline::GridInit>> {
    use super::grid_trampoline::GridInit;

    // CSS Grid L2 §9: If this grid was set up as a subgrid by its parent, read
    // the inherited track contexts that the parent set in the thread-locals.
    // We clear them immediately so our own children don't accidentally inherit them.
    let inherited_cols: Option<SubgridContext> = SUBGRID_COL_CTX.with(|c| c.borrow_mut().take());
    let inherited_rows: Option<SubgridContext> = SUBGRID_ROW_CTX.with(|c| c.borrow_mut().take());

    // Indices of actual items (non-Skip). CSS Grid L1 §9.1: an absolutely-positioned
    // child does not take part in grid layout — it is not a grid item and is laid out
    // afterwards against its containing block (`grid_trampoline::lay_out_abs`).
    let mut item_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip)
            && !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|(i, _)| i)
        .collect();
    // CSS Grid §6: grid items are placed in "modified document order" — source order
    // reordered by the `order` property. A stable sort preserves source order among
    // items with equal `order`, so auto-placement honours `order` like Edge does.
    item_idxs.sort_by_key(|&i| children[i].style.order);

    // CSS Grid L1 §7.1: a grid container with no items still has its explicit
    // tracks (`grid-template-*` of fixed lengths) — they give it a size and
    // gaps, and `column-rule`/`row-rule` are painted in those gaps. Only a
    // container with nothing to size (no explicit template, or a subgrid whose
    // tracks come from the parent) takes the zero-height shortcut.
    if item_idxs.is_empty()
        && (inherited_cols.is_some()
            || inherited_rows.is_some()
            || (s.grid_template_columns.is_empty() && s.grid_template_rows.is_empty())
            || s.grid_template_col_auto_repeat.is_some()
            || s.grid_template_row_auto_repeat.is_some()
            || s.grid_template_columns.first() == Some(&GridTrackSize::Subgrid)
            || s.grid_template_rows.first() == Some(&GridTrackSize::Subgrid))
    {
        return None;
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
    // CSS Grid L1 §7.2.3.2: the repeat is expanded between the fixed tracks written around it;
    // `auto-fit` tracks that end up empty are collapsed after placement (below).
    let col_repeat_count = s
        .grid_template_col_auto_repeat
        .as_ref()
        .map(|rep| auto_repeat_count(rep, Some(content_width), col_gap, em, viewport));
    let auto_fill_col_tracks: Vec<GridTrackSize> = match (&s.grid_template_col_auto_repeat, col_repeat_count) {
        (Some(rep), Some(n)) => expand_auto_repeat(rep, n),
        _ => Vec::new(),
    };
    let eff_col_template: &[GridTrackSize] = if s.grid_template_col_auto_repeat.is_some() {
        &auto_fill_col_tracks
    } else {
        &s.grid_template_columns
    };
    // Rows repeat against the container's definite content height; an indefinite one repeats once.
    let row_repeat_count = s
        .grid_template_row_auto_repeat
        .as_ref()
        .map(|rep| auto_repeat_count(rep, definite_content_height, row_gap, em, viewport));
    let auto_fill_row_tracks: Vec<GridTrackSize> = match (&s.grid_template_row_auto_repeat, row_repeat_count) {
        (Some(rep), Some(n)) => expand_auto_repeat(rep, n),
        _ => Vec::new(),
    };
    let src_row_template: &[GridTrackSize] = if s.grid_template_row_auto_repeat.is_some() {
        &auto_fill_row_tracks
    } else {
        &s.grid_template_rows
    };

    // CSS Masonry Layout (CSS Grid L3 §14) is not shipped by any stable browser —
    // Edge/Chrome treat `masonry` as an invalid track value and drop it, so the axis
    // falls back to `none` (a regular auto-sized grid). We match that ground truth:
    // strip the `masonry` sentinel from the effective track list on whichever axis
    // carries it, then fall through to the normal grid placement algorithm below.
    let col_is_masonry = eff_col_template.first() == Some(&GridTrackSize::Masonry);
    let row_is_masonry = src_row_template.first() == Some(&GridTrackSize::Masonry);
    let eff_col_template: &[GridTrackSize] = if col_is_masonry { &[] } else { eff_col_template };
    let eff_row_template: &[GridTrackSize] = if row_is_masonry { &[] } else { src_row_template };

    // Determine explicit track counts.
    // Subgrid sentinel `[Subgrid]` is a single-element vec meaning "inherit all parent tracks";
    // for placement purposes use the number of inherited tracks (or 1 for auto-placement).
    let n_explicit_cols = if eff_col_template.first() == Some(&GridTrackSize::Subgrid) {
        inherited_cols.as_ref().map(|ctx| ctx.sizes.len()).unwrap_or(1).max(1)
    } else {
        eff_col_template.len().max(1)
    };

    // Явная сетка для разрешения линий: треки шаблона или строки/столбцы
    // `grid-template-areas` (что больше — CSS Grid L1 §7.1).
    let areas_cols = s.grid_template_areas.first().map_or(0, Vec::len);
    let areas_rows = s.grid_template_areas.len();
    let subgrid_cols = eff_col_template.first() == Some(&GridTrackSize::Subgrid);
    let subgrid_rows = eff_row_template.first() == Some(&GridTrackSize::Subgrid);
    let col_axis = GridAxis {
        n_tracks: if subgrid_cols { n_explicit_cols as u32 } else { eff_col_template.len().max(areas_cols) as u32 },
        names: if subgrid_cols { &[] } else { &s.grid_template_col_line_names },
        areas: &s.grid_template_areas,
        is_col: true,
    };
    let row_axis = GridAxis {
        n_tracks: if subgrid_rows {
            inherited_rows.as_ref().map(|ctx| ctx.sizes.len()).unwrap_or(1) as u32
        } else {
            eff_row_template.len().max(areas_rows) as u32
        },
        names: if subgrid_rows { &[] } else { &s.grid_template_row_line_names },
        areas: &s.grid_template_areas,
        is_col: false,
    };

    // --- Step 1: Resolve placements for every item ---
    // placement: (col_start, col_end, row_start, row_end) all 1-based inclusive/exclusive.
    let mut placements: Vec<(u32, u32, u32, u32)> = vec![(0, 0, 0, 0); item_idxs.len()];

    let row_flow = !matches!(s.grid_auto_flow, GridAutoFlow::Column | GridAutoFlow::ColumnDense);

    // Pass 1: items with fully explicit placements.
    for (k, &i) in item_idxs.iter().enumerate() {
        let is = &children[i].style;

        // CSS Grid L1 §8.3: каждая ось разрешается независимо — номера линий
        // (в т.ч. отрицательные), `span`, имена линий и неявные линии областей.
        // Результат оси — `(start, end)`; `start == 0` — позиция авто, тогда
        // `end` несёт span (0 — span 1).
        let (cs, ce) = resolve_grid_axis(&is.grid_column_start, &is.grid_column_end, &col_axis);
        let (rs, re) = resolve_grid_axis(&is.grid_row_start, &is.grid_row_end, &row_axis);

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
            // CSS Grid L1 §8.5 шаг 2: элемент с определённой строкой и авто-столбцом
            // остаётся в своей строке и ищет первый свободный столбец (а не уезжает
            // за курсором).
            let fixed_rs = rs;

            // Dense packing starts each scan from (1,1); sparse continues from cursor.
            let (mut scan_r, mut scan_c) = if fixed_rs != 0 {
                (fixed_rs, 1u32)
            } else if dense {
                (1u32, 1u32)
            } else {
                (cursor_row, cursor_col)
            };

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
                let fits = fixed_cs != 0 || fixed_rs != 0 || (try_ce_val - 1) <= col_bound;
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
                    if fixed_rs != 0 {
                        // Элемент закреплён за строкой — курсор авто-размещения не двигается.
                        break;
                    }
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
                if fixed_rs != 0 {
                    scan_c += 1;
                } else if fixed_cs != 0 {
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
    let mut n_rows = placements.iter().map(|&(_, _, _, re)| re.saturating_sub(1)).max().unwrap_or(1);
    // Every explicit row is a track even when no item reaches it (§7.1, like the
    // columns above): trailing empty rows keep their size and the gaps before them.
    n_rows = n_rows.max(eff_row_template.len() as u32);

    // CSS Grid L1 §7.2.3.2: `repeat(auto-fit, …)` tracks that hold no item collapse to zero
    // (the gutters on both sides merge into one). Placement above used the full expanded list.
    let covers = |track: usize, axis_col: bool| {
        placements.iter().any(|&(cs, ce, rs, re)| {
            let (a, b) = if axis_col { (cs, ce) } else { (rs, re) };
            a != 0 && (a as usize - 1) <= track && track < (b.max(a + 1) as usize - 1)
        })
    };
    let (col_tracks_collapsed, col_collapsed) = collapse_auto_fit(
        eff_col_template,
        s.grid_template_col_auto_repeat.as_ref(),
        col_repeat_count.unwrap_or(0),
        |t| covers(t, true),
    );
    let eff_col_template: &[GridTrackSize] = &col_tracks_collapsed;
    let (row_tracks_collapsed, row_collapsed) = collapse_auto_fit(
        eff_row_template,
        s.grid_template_row_auto_repeat.as_ref(),
        row_repeat_count.unwrap_or(0),
        |t| covers(t, false),
    );
    let eff_row_template: &[GridTrackSize] = &row_tracks_collapsed;
    // Collapsed tracks have no gutters: `col_gutters` / `row_gutters` count the real ones.
    let col_gutters = gutter_count(&col_collapsed, n_cols as usize);

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

        // CSS Grid L1 §11.5 — a `min-content` / `max-content` track is as wide as the
        // largest contribution of the items that sit in it alone (spanning items
        // contribute to several tracks and are left to the free-space pass).
        for (c, col_w) in col_widths.iter_mut().enumerate() {
            let kind = grid_track(c as u32, eff_col_template, &s.grid_auto_columns);
            let is_max = match kind {
                GridTrackSize::MaxContent => true,
                GridTrackSize::MinContent => false,
                _ => continue,
            };
            let mut w = 0.0_f32;
            for (k, &i) in item_idxs.iter().enumerate() {
                let (cs, ce, _, _) = placements[k];
                if cs as usize != c + 1 || ce.saturating_sub(cs) > 1 {
                    continue;
                }
                let item = &children[i];
                let cem = item.style.font_size;
                // Вертикальный режим: колонка — inline-ось, то есть физическая высота.
                let (ml, mr, outer) = if vertical.is_some() {
                    (
                        item.style.margin_top.resolve_or_zero(cem, 0.0, viewport),
                        item.style.margin_bottom.resolve_or_zero(cem, 0.0, viewport),
                        max_content_outer_height(item, measurer, viewport),
                    )
                } else {
                    (
                        item.style.margin_left.resolve_or_zero(cem, 0.0, viewport),
                        item.style.margin_right.resolve_or_zero(cem, 0.0, viewport),
                        if is_max {
                            max_content_outer_width(item, measurer, viewport)
                        } else {
                            min_content_outer_width(item, measurer, viewport)
                        },
                    )
                };
                w = w.max(outer + ml + mr);
            }
            *col_w = w;
        }

        // Total gap between columns.
        let total_col_gap = col_gap * col_gutters as f32;
        let fixed_col_total: f32 = col_widths.iter().sum::<f32>() + total_col_gap;
        let free_col = (content_width - fixed_col_total).max(0.0);

        // Distribute fr among column tracks.
        // Flexible tracks are `Nfr` and `minmax(<min>, Nfr)` (BUG-1217): the latter keeps its
        // fixed minimum as a floor and takes its share of what the other tracks leave.
        let flex: Vec<Option<f32>> = (0..n_cols)
            .map(|c| grid_track(c, eff_col_template, &s.grid_auto_columns).flex_factor())
            .collect();
        let total_fr: f32 = flex.iter().map(|f| f.unwrap_or(0.0)).sum();
        let (flex_fr, frozen) = find_fr_size(&flex, &col_widths, content_width, total_col_gap);
        let auto_col_count = (0..n_cols)
            .filter(|&c| matches!(grid_track(c, eff_col_template, &s.grid_auto_columns), GridTrackSize::Auto))
            .count();

        // For auto columns, divide remaining free space equally (after fr).
        let fr_width = flex_fr;
        let auto_col_width = if auto_col_count > 0 && total_fr == 0.0 {
            free_col / auto_col_count as f32
        } else {
            0.0
        };

        for c in 0..n_cols {
            match grid_track(c, eff_col_template, &s.grid_auto_columns) {
                GridTrackSize::Fr(f) => col_widths[c as usize] = (f * fr_width).max(0.0),
                GridTrackSize::Minmax(..) if flex[c as usize].is_some() => {
                    if !frozen[c as usize] {
                        col_widths[c as usize] = (flex[c as usize].unwrap_or(0.0) * fr_width).max(col_widths[c as usize]);
                    }
                }
                GridTrackSize::Auto => {
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
            col_gutters + 1,
        );

        // Column start offsets (a collapsed track takes no gutter).
        let col_offsets = track_offsets(&col_widths, &col_collapsed, col_gap, jc_extra, jc_start);

        (col_widths, col_offsets)
    };

    // Initial row sizes (CSS Grid L1 §12.3 track-sizing base, before auto-row
    // content growth). If the row axis is subgridded, use inherited sizes;
    // otherwise compute from style. LAYOUT-2 срез 4: this stays native — no
    // `lay_out` call — grown auto/fr sizes are resolved by the trampoline's
    // `grid_trampoline::finish_probe_pass` once every item's probe height is in.
    let row_heights: Vec<f32> = if let Some(ref ctx) = inherited_rows {
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

    // BUG-341 S33: the probe pass (`grid_trampoline`'s Probe phase) and the
    // final positioning pass (its Final phase) always call `lay_out` with the
    // exact same `(width, height=None)` for a given non-subgrid item —
    // `col_offsets`/`col_widths` are resolved once, right above, and nothing
    // between here and the final pass touches them again, so `cell_w` is
    // bit-identical for both passes *by construction*, not just "happens to
    // match" the way S30-S32's general `(node, width, height)` cache could
    // only ever hope for. `grid_trampoline` stashes each non-subgrid item's
    // probe result and reuses it directly in the final pass instead of laying
    // the subtree out twice — the one real redundancy the S28-S32 general
    // layout-result cache slices ever found a case for, captured here with
    // zero overhead on every other box in the document (no thread-local
    // `HashMap`, no per-call key, nothing paid on a miss that never repeats —
    // see `CV_AUTO_TOUCHED`'s doc comment for why the general mechanism was
    // removed instead of kept).
    //
    // Subgrid items are excluded: their own recursive grid layout reads a
    // thread-local track context (`SubgridContextGuard`, set in both phases)
    // that genuinely differs between this estimated-tracks probe and the
    // final pass's resolved-tracks pass.
    let probe_reuse: Vec<Option<(f32, f32, LayoutBox)>> = vec![None; item_idxs.len()];
    let item_baselines = vec![None; item_idxs.len()];
    let row_groups = vec![(0.0_f32, 0.0_f32); n_rows as usize];

    Some(Box::new(GridInit {
        item_idxs,
        placements,
        n_cols,
        n_rows,
        col_widths,
        col_offsets,
        eff_row_template: eff_row_template.to_vec(),
        row_collapsed,
        inherited_rows,
        row_heights,
        row_offsets: Vec::new(),
        y_off: 0.0,
        content_x,
        content_y,
        content_width,
        definite_content_height,
        col_gap,
        row_gap,
        s: Arc::clone(s),
        children_pcb: pcb,
        em,
        available_height,
        padding_top,
        padding_bottom,
        size_contained,
        is_positioned,
        own_pcb,
        probe_reuse,
        item_baselines,
        row_first_group: row_groups.clone(),
        row_last_group: row_groups,
        vertical,
    }))
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
pub(super) fn grid_track<'a>(idx: u32, template: &'a [GridTrackSize], auto_track: &'a GridTrackSize) -> &'a GridTrackSize {
    template.get(idx as usize).unwrap_or(auto_track)
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

/// Явная сетка одной оси для разрешения `<grid-line>` (CSS Grid L1 §8.3).
struct GridAxis<'a> {
    /// Число треков явной сетки (линий — на одну больше).
    n_tracks: u32,
    /// Имена линий: индекс `i` — линия номер `i + 1`.
    names: &'a [Vec<String>],
    /// `grid-template-areas` — источник неявных линий `<area>-start/-end`.
    areas: &'a [Vec<String>],
    /// `true` — ось столбцов.
    is_col: bool,
}

impl GridAxis<'_> {
    /// Номер последней линии явной сетки.
    fn last_line(&self) -> u32 {
        self.n_tracks + 1
    }

    /// Все линии с именем `name` (по возрастанию, без дублей): именованные
    /// линии `grid-template-*` и неявные `<area>-start` / `<area>-end`.
    fn lines_named(&self, name: &str) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for (i, group) in self.names.iter().enumerate() {
            if group.iter().any(|n| n == name) {
                out.push(i as u32 + 1);
            }
        }
        let implicit = name
            .strip_suffix("-start")
            .map(|a| (a, true))
            .or_else(|| name.strip_suffix("-end").map(|a| (a, false)));
        if let Some((area, is_start)) = implicit
            && let Some((rs, re, cs, ce)) = find_named_area(self.areas, area)
        {
            let (start, end) = if self.is_col { (cs, ce) } else { (rs, re) };
            out.push(if is_start { start } else { end });
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// `<integer> <custom-ident>` — N-я линия с именем (отрицательная — с конца).
    /// Не хватает линий — неявные линии за явной сеткой считаются имеющими это
    /// имя (§8.3), в нужную сторону от явной сетки.
    fn nth_named(&self, name: &str, n: i32) -> u32 {
        let lines = self.lines_named(name);
        let k = lines.len() as i32;
        if n > 0 {
            if n <= k {
                lines[n as usize - 1]
            } else {
                self.last_line() + (n - k) as u32
            }
        } else if -n <= k {
            lines[(k + n) as usize]
        } else {
            1
        }
    }

    /// `<custom-ident>` на стороне start/end: сначала `<ident>-start|-end`
    /// (область), затем линия с именем `<ident>`, иначе `1 <ident>`.
    fn named_edge(&self, name: &str, is_start: bool) -> u32 {
        let suffixed = format!("{name}{}", if is_start { "-start" } else { "-end" });
        if let Some(&l) = self.lines_named(&suffixed).first() {
            return l;
        }
        self.nth_named(name, 1)
    }

    /// Номер линии для `Line(n)`: положительный как есть, отрицательный — с
    /// конца явной сетки (`-1` — последняя линия). Не меньше 1.
    fn numbered(&self, n: i32) -> u32 {
        if n > 0 {
            n as u32
        } else {
            (self.last_line() as i32 + 1 + n).max(1) as u32
        }
    }

    /// N-я линия `name` строго после `from` (поиск вперёд, для `span N name`).
    fn span_forward(&self, name: &str, from: u32, n: u32) -> u32 {
        let after: Vec<u32> = self.lines_named(name).into_iter().filter(|&l| l > from).collect();
        let k = after.len() as u32;
        if n <= k {
            after[n as usize - 1]
        } else {
            from.max(self.last_line()) + (n - k)
        }
    }

    /// N-я линия `name` строго до `from` (поиск назад); нет — 1.
    fn span_backward(&self, name: &str, from: u32, n: u32) -> u32 {
        let before: Vec<u32> = self.lines_named(name).into_iter().filter(|&l| l < from).collect();
        let k = before.len() as u32;
        if n <= k { before[(k - n) as usize] } else { 1 }
    }
}

/// Одна сторона `<grid-line>` после разрешения имён.
enum Edge {
    /// Позиция авто.
    Auto,
    /// Определённая линия.
    Line(u32),
    /// `span N` с необязательным именем линии-границы.
    Span(u32, Option<String>),
}

fn resolve_edge(line: &GridLine, axis: &GridAxis, is_start: bool) -> Edge {
    match line {
        GridLine::Auto => Edge::Auto,
        GridLine::Line(n) => Edge::Line(axis.numbered(*n)),
        GridLine::Span(n) => Edge::Span(*n, None),
        GridLine::Named(name) => Edge::Line(axis.named_edge(name, is_start)),
        GridLine::NamedLine(name, n) => Edge::Line(axis.nth_named(name, *n)),
        GridLine::SpanNamed(name, n) => Edge::Span(*n, Some(name.clone())),
    }
}

/// CSS Grid L1 §8.3.1 — разрешает пару `start`/`end` одной оси.
///
/// Возвращает `(start, end)` — номера линий, 1-based. `start == 0` означает
/// авто-позицию: тогда `end` — число занимаемых треков (0 — один).
fn resolve_grid_axis(start: &GridLine, end: &GridLine, axis: &GridAxis) -> (u32, u32) {
    let s = resolve_edge(start, axis, true);
    let mut e = resolve_edge(end, axis, false);
    // Два `span` — end отбрасывается (§8.3.1).
    if matches!(s, Edge::Span(..)) && matches!(e, Edge::Span(..)) {
        e = Edge::Auto;
    }
    match (s, e) {
        (Edge::Line(a), Edge::Line(b)) => match a.cmp(&b) {
            std::cmp::Ordering::Less => (a, b),
            std::cmp::Ordering::Greater => (b, a), // меняются местами
            std::cmp::Ordering::Equal => (a, a + 1),
        },
        (Edge::Line(a), Edge::Span(n, name)) => {
            let b = match name {
                Some(name) => axis.span_forward(&name, a, n),
                None => a + n,
            };
            (a, b)
        }
        (Edge::Line(a), Edge::Auto) => (a, a + 1),
        (Edge::Span(n, name), Edge::Line(b)) => {
            let a = match name {
                Some(name) => axis.span_backward(&name, b, n),
                None => b.saturating_sub(n).max(1),
            };
            (a, b.max(a + 1))
        }
        // Без противоположной линии именованный span считается по числу.
        (Edge::Span(n, _), Edge::Auto) => (0, n),
        (Edge::Auto, Edge::Line(b)) => {
            let a = b.saturating_sub(1).max(1);
            (a, b.max(a + 1))
        }
        (Edge::Auto, Edge::Span(n, _)) => (0, n),
        (Edge::Auto, Edge::Auto) => (0, 0),
        (Edge::Span(..), Edge::Span(..)) => unreachable!("end span dropped above"),
    }
}

/// CSS Grid L1 §12.7.1 "find the size of an fr": the fr size left once the non-flexible tracks
/// and gaps are taken out, freezing flexible tracks whose share falls below their base size
/// (`minmax(<min>, Nfr)`) and redistributing until stable. Returns the size and the frozen set.
/// Kept out of line: inlined into `lay_out_grid` it deepens the frame of a deep grid chain.
#[inline(never)]
fn find_fr_size(flex: &[Option<f32>], base: &[f32], content_width: f32, total_gap: f32) -> (f32, Vec<bool>) {
    let mut frozen = vec![false; flex.len()];
    let nonflex_total: f32 =
        base.iter().zip(flex).filter(|(_, f)| f.is_none()).map(|(w, _)| *w).sum::<f32>() + total_gap;
    let mut fr = 0.0_f32;
    loop {
        let frozen_total: f32 = (0..flex.len()).filter(|&c| frozen[c]).map(|c| base[c]).sum();
        let live_fr: f32 = (0..flex.len()).filter(|&c| !frozen[c]).map(|c| flex[c].unwrap_or(0.0)).sum();
        if live_fr <= 0.0 {
            break;
        }
        fr = (content_width - nonflex_total - frozen_total).max(0.0) / live_fr;
        let mut changed = false;
        for c in 0..flex.len() {
            if let Some(f) = flex[c]
                && !frozen[c]
                && f * fr < base[c]
            {
                frozen[c] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    (fr, frozen)
}
