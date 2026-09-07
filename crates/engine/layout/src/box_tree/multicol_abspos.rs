//! Multi-column layout (`build_multicol_init`, driven by
//! `multicol_trampoline::run`) and absolutely/fixed positioned box placement
//! (`lay_out_abs_children`) — two small, unrelated layout modes that shared
//! the tail of `box_tree.rs` before this split.
//!
//! Перенесено батчем SPLIT-BT8 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn lay_out_multicol_children` до конца файла, перед `mod tests`)
//! без правок тел. LAYOUT-2 срез 7 (`p1-layout2-multicol-trampoline`)
//! перевела `lay_out_multicol_children` на явный heap-стек — pure precompute
//! осталась здесь как `build_multicol_init`, per-item dispatch переехал в
//! `multicol_trampoline.rs`.

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
pub(super) fn balanced_column_height(outer_hs: &[f32], n_cols: usize) -> f32 {
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

/// LAYOUT-2 срез 7: pure precompute for the multicol dispatch arm — column
/// count/width, `column-fill` mode, and the split of flow children into
/// segments (by `column-span: all` boundaries) with each segment's
/// slice-vs-atomic decision. None of this ever calls `lay_out` on a child —
/// unlike `flex::build_flex_init`'s Step 1 probe, whether a segment is
/// "sliceable" is a pure function of each item's `style`/`kind`
/// ([`box_is_column_sliceable`]), not of anything a layout pass would
/// produce — so, mirroring `grid::build_grid_init`/`table::build_table_init`,
/// the whole precompute runs natively here and only the per-item dispatch
/// (measure pass + atomic segments' real placement pass + `column-span: all`
/// elements) is captured into [`MulticolInit`] for `multicol_trampoline::run`
/// to drive on an explicit heap stack. Returns `None` when every child is
/// out-of-flow (absolute/fixed/`Skip`) — the removed function's early
/// `return 0.0` with `children` left untouched.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_multicol_init(
    children: &mut Vec<LayoutBox>,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    s: &Arc<ComputedStyle>,
    em: f32,
    viewport: Size,
    children_pcb: Rect,
    container_h: Option<f32>,
    own_pcb: Rect,
    cb: f32,
    is_positioned: bool,
    padding_top: f32,
    padding_bottom: f32,
    size_contained: bool,
    field_intrinsic: Option<(f32, f32)>,
    available_height: Option<f32>,
) -> Option<Box<super::multicol_trampoline::MulticolInit>> {
    use super::multicol_trampoline::{MulticolInit, SegmentInit};

    let col_gap = s.column_gap.resolve_or_zero(em, content_width, viewport).max(0.0);

    // Compute column count from column-count / column-width.
    let n_cols: u32 = match (s.column_count, &s.column_width) {
        (Some(n), Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(content_width), viewport) {
                let n_from_w = ((content_width + col_gap) / (w + col_gap)).floor() as u32;
                n.min(n_from_w).max(1)
            } else {
                n.max(1)
            }
        }
        (Some(n), None) => n.max(1),
        (None, Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(content_width), viewport)
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

    // Collect flow (non-abs, non-skip) child indices, without moving `children`
    // yet — the empty case below must leave it completely untouched.
    let flow_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

    if flow_idxs.is_empty() {
        return None;
    }

    // Move children out so the trampoline's slice path can replace whole
    // boxes with multiple per-column fragment clones (the box count changes).
    let work = std::mem::take(children);

    // Split flow children into segments separated by column-span:all elements,
    // deciding slice-vs-atomic per segment up front (CSS Multicol §3.4) — a
    // pure function of `box_is_column_sliceable`, never of a laid-out `.rect`.
    let mut segments: Vec<SegmentInit> = Vec::new();
    let mut seg: Vec<usize> = Vec::new();
    for &i in &flow_idxs {
        if work[i].style.column_span_all {
            let sliceable = n_cols > 1 && seg.iter().all(|&j| box_is_column_sliceable(&work[j]));
            segments.push(SegmentInit {
                item_idxs: std::mem::take(&mut seg),
                span_idx: Some(i),
                sliceable,
            });
        } else {
            seg.push(i);
        }
    }
    let sliceable = n_cols > 1 && seg.iter().all(|&j| box_is_column_sliceable(&work[j]));
    segments.push(SegmentInit { item_idxs: seg, span_idx: None, sliceable });

    let consumed = vec![false; work.len()];

    Some(Box::new(MulticolInit {
        content_x,
        content_y,
        content_width,
        col_gap,
        n_cols,
        col_w,
        balance,
        container_h,
        segments,
        children_pcb,
        s: Arc::clone(s),
        em,
        cb,
        is_positioned,
        own_pcb,
        padding_top,
        padding_bottom,
        size_contained,
        field_intrinsic,
        available_height,
        work,
        consumed,
        out: Vec::with_capacity(0),
        cur_y: content_y,
    }))
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
