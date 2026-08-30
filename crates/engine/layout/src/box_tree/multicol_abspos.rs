//! Multi-column layout (`lay_out_multicol_children`) and absolutely/fixed
//! positioned box placement (`lay_out_abs_children`) — two small, unrelated
//! layout modes that shared the tail of `box_tree.rs` before this split.
//!
//! Перенесено батчем SPLIT-BT8 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn lay_out_multicol_children` до конца файла, перед `mod tests`)
//! без правок тел.

use super::*;

/// CSS Multi-column Layout L1 — lays out `children` into N columns.
/// Returns content height (max column height, without padding/border).
///
/// `container_h` is the resolved content-box height of the multi-column container, used
/// by `column-fill: auto` to fill columns sequentially up to that height instead of
/// balancing content equally across all columns.
/// CSS Multi-column L1 §3.4 — true column fragmentation breaks block content
/// across columns. Lumen approximates this by geometrically slicing a box into
/// per-column pieces (see `lay_out_multicol_children`). That is only visually
/// faithful for a "simple" box: a leaf block whose paint is a flat fill
/// (background-color) — no children or text that a slice would duplicate, no
/// border whose cut edge would show. Anything else keeps the atomic
/// one-box-per-column placement.
fn box_is_column_sliceable(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::Block)
        && b.children.is_empty()
        && b.style.border_top_width == 0.0
        && b.style.border_bottom_width == 0.0
        && b.style.border_left_width == 0.0
        && b.style.border_right_width == 0.0
}

/// CSS Multicol §7.1 — balanced column height for atomic (unsliceable) boxes.
///
/// Returns the smallest column height `H` such that greedily packing `outer_hs`
/// (each box's margin-box height, in source order, opening a new column whenever
/// the running height would exceed `H`) fits within `n_cols` columns. This is the
/// target browsers minimise when `column-fill: balance` and items cannot be split
/// across columns — e.g. 9 cards of varying height fill 3 columns as 3/3/3 rather
/// than packing the first column to the container height.
fn balanced_column_height(outer_hs: &[f32], n_cols: usize) -> f32 {
    let total: f32 = outer_hs.iter().sum();
    if n_cols <= 1 || outer_hs.is_empty() {
        return total.max(1.0);
    }
    let max_item = outer_hs.iter().cloned().fold(0.0_f32, f32::max);
    // Any feasible height is at least the tallest single item and at least the
    // perfectly even split; the sum is always feasible (one column holds all).
    let mut lo = max_item.max(total / n_cols as f32);
    let mut hi = total.max(lo);
    let fits = |h: f32| -> bool {
        let mut cols = 1usize;
        let mut cur = 0.0f32;
        for &x in outer_hs {
            if cur > 0.0 && cur + x > h {
                cols += 1;
                if cols > n_cols {
                    return false;
                }
                cur = x;
            } else {
                cur += x;
            }
        }
        true
    };
    // Binary search for the minimal feasible height (~0.25 px precision).
    for _ in 0..40 {
        if hi - lo <= 0.25 {
            break;
        }
        let mid = (lo + hi) * 0.5;
        if fits(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi.ceil().max(1.0)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_multicol_children(
    children: &mut Vec<LayoutBox>,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    s: &ComputedStyle,
    em: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    container_h: Option<f32>,
) -> f32 {
    let cb = content_width;
    let col_gap = s.column_gap.resolve_or_zero(em, cb, viewport).max(0.0);

    // Compute column count from column-count / column-width.
    let n_cols: u32 = match (s.column_count, &s.column_width) {
        (Some(n), Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(cb), viewport) {
                let n_from_w = ((content_width + col_gap) / (w + col_gap)).floor() as u32;
                n.min(n_from_w).max(1)
            } else {
                n.max(1)
            }
        }
        (Some(n), None) => n.max(1),
        (None, Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(cb), viewport)
                && w > 0.0
            {
                ((content_width + col_gap) / (w + col_gap)).floor() as u32
            } else {
                1
            }
        }
        (None, None) => 1,
    }.max(1);

    let col_w = ((content_width - col_gap * (n_cols - 1) as f32) / n_cols as f32).max(0.0);

    // column-fill: balance distributes content equally; auto fills columns to container height.
    // When no container height is known, auto behaves like balance.
    let balance = s.column_fill_balance || container_h.is_none();

    // Move children out so the slice path can replace whole boxes with multiple
    // per-column fragment clones (the box count changes), then rebuild `children`.
    let mut work = std::mem::take(children);

    // Collect flow (non-abs, non-skip) child indices.
    let flow_idxs: Vec<usize> = work
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

    if flow_idxs.is_empty() {
        *children = work;
        return 0.0;
    }

    // Split flow children into segments separated by column-span:all elements.
    // Each entry is (regular_children, Option<span_all_child_idx>).
    let mut segments: Vec<(Vec<usize>, Option<usize>)> = Vec::new();
    let mut seg: Vec<usize> = Vec::new();
    for &i in &flow_idxs {
        if work[i].style.column_span_all {
            segments.push((std::mem::take(&mut seg), Some(i)));
        } else {
            seg.push(i);
        }
    }
    segments.push((seg, None));

    let mut cur_y = content_y;
    // Boxes placed into the rebuilt child list. Fragment clones (slice path) and
    // positioned originals (atomic / span path) are pushed here in turn; any box
    // not consumed (absolute / Skip placeholders) is appended unchanged at the end.
    let mut out: Vec<LayoutBox> = Vec::with_capacity(work.len());
    let mut consumed = vec![false; work.len()];

    for (seg_idxs, span_idx) in &segments {
        if !seg_idxs.is_empty() {
            // First pass at (0, 0) to measure intrinsic heights.
            for &i in seg_idxs {
                lay_out(&mut work[i], 0.0, 0.0, col_w, None, measurer, viewport, pcb, hp, false);
            }

            // Outer height of each segment child = margin_top + rect.height + margin_bottom.
            let outer_hs: Vec<f32> = seg_idxs.iter().map(|&i| {
                let c = &work[i];
                let mt = c.style.margin_top.resolve_or_zero(c.style.font_size, col_w, viewport);
                let mb = c.style.margin_bottom.resolve_or_zero(c.style.font_size, col_w, viewport);
                mt + c.rect.height + mb
            }).collect();

            let total_h: f32 = outer_hs.iter().sum();

            // CSS Multicol §3.4: when every box can be safely sliced, fragment the
            // segment's content across all columns by height (this is what browsers
            // do — a tall empty block spills from one column into the next). The
            // balanced column height is total/n_cols; column-fill:auto fills each
            // column to the container height instead.
            let all_sliceable =
                n_cols > 1 && seg_idxs.iter().all(|&i| box_is_column_sliceable(&work[i]));

            if all_sliceable {
                let col_h = if balance {
                    (total_h / n_cols as f32).ceil().max(1.0)
                } else {
                    container_h.unwrap_or_else(|| (total_h / n_cols as f32).ceil()).max(1.0)
                };

                // Virtual single-column stack: each box's border-box occupies
                // [virtual_top, virtual_top + height), with margins as gaps.
                let mut stack: Vec<(usize, f32, f32)> = Vec::with_capacity(seg_idxs.len());
                let mut v = 0.0f32;
                for (&i, &oh) in seg_idxs.iter().zip(outer_hs.iter()) {
                    let mt = work[i].style.margin_top
                        .resolve_or_zero(work[i].style.font_size, col_w, viewport);
                    stack.push((i, v + mt, work[i].rect.height));
                    v += oh;
                }

                // Emit one clipped fragment per (column, box) overlap.
                let mut seg_extent = 0.0f32;
                for c in 0..n_cols as usize {
                    let col_lo = c as f32 * col_h;
                    let col_hi = col_lo + col_h;
                    let col_x = content_x + c as f32 * (col_w + col_gap);
                    for &(i, bt, bh) in &stack {
                        let bb = bt + bh;
                        let ov_lo = bt.max(col_lo);
                        let ov_hi = bb.min(col_hi);
                        if ov_hi > ov_lo {
                            let mut frag = work[i].clone();
                            frag.rect.x = col_x;
                            frag.rect.y = cur_y + (ov_lo - col_lo);
                            frag.rect.width = col_w;
                            frag.rect.height = ov_hi - ov_lo;
                            seg_extent = seg_extent.max(ov_hi - col_lo);
                            out.push(frag);
                        }
                    }
                }
                for &i in seg_idxs {
                    consumed[i] = true;
                }
                cur_y += seg_extent.max(0.0);
            } else {
                // Atomic fallback: place each whole box into a column (greedy by height).
                // In balance mode the target is the optimal balanced column height
                // (smallest H that packs all boxes into n_cols columns) — matches how
                // browsers distribute unsliceable items (e.g. 9 cards → 3×3, not 5/4/0).
                // column-fill:auto fills each column to the container height instead.
                let target_h = if balance {
                    balanced_column_height(&outer_hs, n_cols as usize)
                } else {
                    container_h.unwrap_or_else(|| (total_h / n_cols as f32).ceil()).max(1.0)
                };

                let mut col_assignment = vec![0usize; seg_idxs.len()];
                let mut col_fill = vec![0.0f32; n_cols as usize];
                let mut cur_col = 0usize;
                for (j, &oh) in outer_hs.iter().enumerate() {
                    let height_overflow = col_fill[cur_col] + oh > target_h && oh > 0.0;
                    // Never advance past an empty column: a column must hold at least one item
                    // before overflowing to the next, otherwise an item taller than target_h
                    // would skip column 0 and leave it blank (CSS Multicol §3.4 — every column
                    // box is filled in order, starting from the first).
                    let col_nonempty = col_fill[cur_col] > 0.0;
                    if cur_col + 1 < n_cols as usize && col_nonempty && height_overflow {
                        cur_col += 1;
                    }
                    col_assignment[j] = cur_col;
                    col_fill[cur_col] += oh;
                }

                // Final positioning.
                let mut col_y = vec![cur_y; n_cols as usize];
                for (j, &i) in seg_idxs.iter().enumerate() {
                    let col = col_assignment[j];
                    let col_x = content_x + col as f32 * (col_w + col_gap);
                    lay_out(&mut work[i], col_x, col_y[col], col_w, None, measurer, viewport, pcb, hp, false);
                    let mb = work[i].style.margin_bottom
                        .resolve_or_zero(work[i].style.font_size, col_w, viewport);
                    col_y[col] = work[i].rect.y + work[i].rect.height + mb;
                    out.push(work[i].clone());
                    consumed[i] = true;
                }

                cur_y = col_y.into_iter().fold(cur_y, f32::max);
            }
        }

        // column-span: all — element spans the full column container width.
        if let Some(span_i) = *span_idx {
            lay_out(&mut work[span_i], content_x, cur_y, content_width, None, measurer, viewport, pcb, hp, false);
            let mb = work[span_i].style.margin_bottom
                .resolve_or_zero(work[span_i].style.font_size, content_width, viewport);
            cur_y = work[span_i].rect.y + work[span_i].rect.height + mb;
            out.push(work[span_i].clone());
            consumed[span_i] = true;
        }
    }

    // Preserve any non-flow boxes (absolute/fixed, Skip placeholders) unchanged.
    for (i, b) in work.into_iter().enumerate() {
        if !consumed[i] {
            out.push(b);
        }
    }
    *children = out;

    cur_y - content_y
}

/// CSS 2.1 §10.3.7 — does an absolutely positioned box resolve its `auto`
/// width by shrink-to-fit (BUG-745), or does it keep the legacy
/// "stretch to the containing block" behaviour?
///
/// Shrink-to-fit is the spec rule for *non-replaced* boxes, so the replaced
/// kinds (`<img>`, `<video>`, `<canvas>`, `<iframe>`, form controls) are
/// excluded: §10.3.8 sizes them from their intrinsic dimensions instead, and
/// their content is invisible to [`max_content_outer_width`] (an image's
/// intrinsic width lives in `BoxKind::Image`, not in child boxes), so measuring
/// them here would collapse them to their padding+border.
///
/// Two more kinds opt out because the intrinsic-width machinery does not model
/// them:
/// * `BoxKind::Table` already shrink-to-fits itself in `lay_out_inner` from
///   `table_intrinsic_content_width` (column widths + border-spacing), which the
///   block "widest child" rule of [`max_content_outer_width`] cannot reproduce;
/// * `display: grid`/`inline-grid` — a grid's max-content width is the sum of
///   its column max-contents plus gaps (the analogue of `flex_row_intrinsic_sum`
///   for the row axis), and no such rule exists yet, so the block rule would
///   under-measure a multi-column grid into one column's width. Stretching is
///   the safer failure mode until that rule lands.
fn abs_box_shrinks_to_fit(b: &LayoutBox) -> bool {
    !matches!(
        b.kind,
        BoxKind::Skip
            | BoxKind::Image { .. }
            | BoxKind::Video { .. }
            | BoxKind::Canvas { .. }
            | BoxKind::Iframe { .. }
            | BoxKind::FormControl { .. }
            | BoxKind::Table
    ) && !matches!(b.style.display, Display::Grid | Display::InlineGrid)
}

/// Positions absolutely/fixed-positioned deferred children of `parent`.
/// Called after parent's height is finalized so `my_pcb` is complete.
pub(crate) fn lay_out_abs_children(
    parent: &mut LayoutBox,
    deferred: &[(usize, f32, f32)],
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    my_pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    // CSS Anchor Positioning L1: collect all elements with `anchor-name` in the tree.
    // This registry is used to resolve `position-anchor` and `anchor()` function calls below.
    // CSS: anchor-name, position-anchor, anchor()
    let anchors = crate::anchor::collect_anchors(parent);

    for &(idx, static_x, static_y) in deferred {
        let cs = parent.children[idx].style.clone();
        let c_em = cs.font_size;

        let cb = if matches!(cs.position, Position::Fixed) {
            Rect::new(0.0, 0.0, viewport.width, viewport.height)
        } else {
            my_pcb
        };

        // CSS Anchor Positioning L1 §3.1 — intercept `anchor()` in top/right/bottom/left
        // before falling back to the plain length/auto value.
        // CSS: anchor(), position-anchor
        let default_anchor = cs.position_anchor.as_deref();
        let left = crate::anchor::resolve_inset(
            &anchors, &cs.left, cs.anchor_left.as_ref(), default_anchor, true, false, cb.x, cb.x + cb.width,
            c_em, cb.width, viewport,
        );
        let right = crate::anchor::resolve_inset(
            &anchors, &cs.right, cs.anchor_right.as_ref(), default_anchor, true, true, cb.x, cb.x + cb.width,
            c_em, cb.width, viewport,
        );
        let top = crate::anchor::resolve_inset(
            &anchors, &cs.top, cs.anchor_top.as_ref(), default_anchor, false, false, cb.y, cb.y + cb.height,
            c_em, cb.height, viewport,
        );
        let bottom = crate::anchor::resolve_inset(
            &anchors, &cs.bottom, cs.anchor_bottom.as_ref(), default_anchor, false, true, cb.y, cb.y + cb.height,
            c_em, cb.height, viewport,
        );

        let c_ml = cs.margin_left.resolve_or_zero(c_em, cb.width, viewport);
        let c_mr = cs.margin_right.resolve_or_zero(c_em, cb.width, viewport);
        let c_mt = cs.margin_top.resolve_or_zero(c_em, cb.height, viewport);
        let c_mb = cs.margin_bottom.resolve_or_zero(c_em, cb.height, viewport);

        // Доступная ширина для layout абсолютного child.
        let avail_w = if left.is_some() && right.is_some() && cs.width.is_none() {
            // Обе инсеты заданы, ширина `auto` → ширина выводится из зазора
            // между ними (CSS Position L3 §6), shrink-to-fit не применяется.
            (cb.width - left.unwrap_or(0.0) - right.unwrap_or(0.0)).max(0.0)
        } else if cs.width.is_none() && abs_box_shrinks_to_fit(&parent.children[idx]) {
            // CSS 2.1 §10.3.7 (BUG-745): у абсолютного не-replaced бокса с
            // `width: auto` и хотя бы одной `auto`-инсетой используемая ширина —
            // shrink-to-fit = min(max(min-content, available), max-content), а не
            // ширина содержащего блока. Разница видна не только в самой ширине:
            // ветка `right` ниже отсчитывает x от правого края содержащего блока
            // назад на `child.rect.width`, поэтому растянутый бокс с
            // `right: 16px` уезжал за левый край (`x = -16, w = 1024` вместо
            // карточки в углу) — форма «тост/тултип/cookie-баннер, приклеенный
            // к углу», пункт 4 BUG-733 на `tbank.ru`.
            //
            // `available` — свободное место содержащего блока за вычетом
            // заданных инсет и margin'ов; max/min-content уже включают
            // padding+border самого бокса (border-box), поэтому margin'ы
            // возвращаются обратно: `lay_out` трактует свой `available_width`
            // как margin-box.
            let child = &parent.children[idx];
            let free =
                (cb.width - left.unwrap_or(0.0) - right.unwrap_or(0.0) - c_ml - c_mr).max(0.0);
            let max_c = max_content_outer_width(child, measurer, viewport);
            let min_c = min_content_outer_width(child, measurer, viewport);
            max_c.min(min_c.max(free)) + c_ml + c_mr
        } else {
            cb.width
        };

        lay_out(&mut parent.children[idx], 0.0, 0.0, avail_w, None, measurer, viewport, my_pcb, hp, false);

        // CSS Position L3 §6: an abs-pos box with both `top` and `bottom` non-auto
        // and `height: auto` resolves its used height to fill the inset gap. Mirror of
        // the `avail_w` width-from-insets path above. Applied post-layout because the
        // gap height is a containing-block used value, not a content-driven size.
        if top.is_some() && bottom.is_some() && cs.height.is_none() {
            let resolved_h =
                (cb.height - top.unwrap_or(0.0) - bottom.unwrap_or(0.0) - c_mt - c_mb).max(0.0);
            parent.children[idx].rect.height = resolved_h;
        }

        let child = &mut parent.children[idx];

        // CSS Anchor Positioning L1 §4 — apply `anchor-size()` overrides for width/height.
        // Done before resolving `inset-area` so the element's used size (used to
        // align it within its position-area band) reflects the anchor-size result.
        let mut w_fixed = cs.width.is_some();
        let mut h_fixed = cs.height.is_some();
        if let Some(w) = cs.anchor_size_w.as_ref().and_then(|f| {
            crate::anchor::resolve_anchor_size(&anchors, f, cs.position_anchor.as_deref())
        }) {
            child.rect.width = w;
            w_fixed = true;
        }
        if let Some(h) = cs.anchor_size_h.as_ref().and_then(|f| {
            crate::anchor::resolve_anchor_size(&anchors, f, cs.position_anchor.as_deref())
        }) {
            child.rect.height = h;
            h_fixed = true;
        }

        // CSS Anchor Positioning L1 §5 — resolve `position-area` / `inset-area`.
        // A definite-size axis keeps its size and is aligned toward the anchor;
        // an `auto` axis stretches to fill its position-area band.
        // CSS: position-anchor, inset-area, position-area
        let elem_w = if w_fixed {
            crate::anchor::AxisSize::Fixed(child.rect.width)
        } else {
            crate::anchor::AxisSize::Auto
        };
        let elem_h = if h_fixed {
            crate::anchor::AxisSize::Fixed(child.rect.height)
        } else {
            crate::anchor::AxisSize::Auto
        };
        let anchored_pos = cs.position_anchor.as_deref().and_then(|anchor_name| {
            crate::anchor::resolve_inset_area(
                &anchors,
                anchor_name,
                cs.inset_area_row,
                cs.inset_area_col,
                cb,
                elem_w,
                elem_h,
            )
        });

        let (new_x, new_y) = if let Some(ref pos) = anchored_pos {
            // Anchor-positioned: override width/height only for auto (stretched) axes.
            if let Some(w) = pos.width {
                child.rect.width = w;
            }
            if let Some(h) = pos.height {
                child.rect.height = h;
            }
            (cb.x + pos.left, cb.y + pos.top)
        } else {
            // Normal abs-pos: resolve from left/right/top/bottom insets.
            let nx = match (left, right) {
                (Some(l), _)    => cb.x + l + c_ml,
                (None, Some(r)) => cb.x + cb.width - r - c_mr - child.rect.width,
                (None, None)    => static_x + c_ml,
            };
            let ny = match (top, bottom) {
                (Some(t), _)     => cb.y + t + c_mt,
                (None, Some(bv)) => cb.y + cb.height - bv - c_mb - child.rect.height,
                (None, None)     => static_y + c_mt,
            };
            (nx, ny)
        };

        let dx = new_x - child.rect.x;
        let dy = new_y - child.rect.y;
        shift_tree(child, dx, dy);
    }
}
