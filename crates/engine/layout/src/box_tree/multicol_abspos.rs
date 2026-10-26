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
use crate::resolved_geometry::contains_fixed_descendants;

/// CSS Multi-column Layout L1 — lays out `children` into N columns.
/// Returns content height (max column height, without padding/border).
///
/// `container_h` is the resolved content-box height of the multi-column container, used
/// by `column-fill: auto` to fill columns sequentially up to that height instead of
/// balancing content equally across all columns.
/// CSS Multi-column L1 §3.4 — a segment is cut across the columns (`multicol_flow`) when the
/// container has more than one column (or wraps into rows) and flows horizontally; what inside
/// an item may be cut is decided there box by box. A vertical container keeps the atomic
/// placement.
fn segment_is_sliceable(n_cols: u32, col_rows: bool, definite_height: bool, container: &ComputedStyle) -> bool {
    // A single column with a definite height still overflows into extra columns (§3.5).
    (n_cols > 1 || col_rows || definite_height) && matches!(container.writing_mode, crate::style::WritingMode::HorizontalTb)
}

/// An in-flow item of a grid cut by `emit_grid_fragments`: a border-less block that may hold
/// children (blocks, text runs). A leaf is cut by its box alone; an item with children is cut
/// only where no child straddles the window edge (otherwise `emit_grid_fragments` keeps the
/// whole grid atomic), so a break never splits a line box or a nested block.
fn box_is_cuttable_item(b: &LayoutBox, container: &ComputedStyle) -> bool {
    let vertical = |m: crate::style::WritingMode| !matches!(m, crate::style::WritingMode::HorizontalTb);
    matches!(b.kind, BoxKind::Block)
        && b.style.break_inside != crate::style::BreakValue::Avoid
        && vertical(b.style.writing_mode) == vertical(container.writing_mode)
        && [b.style.border_top_width, b.style.border_bottom_width, b.style.border_left_width, b.style.border_right_width]
            .iter()
            .all(|w| *w == 0.0)
}

/// CSS Fragmentation L3 §5.1 / CSS Gap Decorations L1 §6.2 — a grid (or wrapped row flex) container that is cut across
/// the columns of a multicol container as a *grid* (its rows are split between the columns and
/// the row gaps at a break are dropped), rather than kept as one atomic box. Only the simple
/// case: a plain horizontal `display: grid` box without padding (a border is cut with the box), whose
/// in-flow children are border-less blocks (`box_is_cuttable_item`), so cutting a child by a column
/// window repeats nothing and hides no border. `subgrid` axes, `break-inside: avoid` and a forced break of the
/// grid itself keep the atomic path; a forced break of an item is cut at by `emit_grid_fragments`
/// (`column-fill: auto` only, a balanced container falls back to the atomic path there).
fn is_fragmentable_grid(b: &LayoutBox, container: &ComputedStyle) -> bool {
    let s = &b.style;
    let is_subgrid = |t: &[crate::style::GridTrackSize]| t.first() == Some(&crate::style::GridTrackSize::Subgrid);
    matches!(b.kind, BoxKind::Block)
        && (matches!(s.display, Display::Grid)
            // A wrapped row flex container is cut by its flex lines the same way (the lines are
            // the row tracks, `flex_trampoline::finish_frame`); one without ≥ 2 lines has no
            // tracks and `emit_grid_fragments` falls back to the atomic path.
            // A wrapped column flex container is cut by its items' block extents; the flex lines
            // (columns) are the tracks the gap painter reads (`flex_trampoline::finish_frame`).
            || (matches!(s.display, Display::Flex)
                && matches!(s.flex_direction, crate::style::FlexDirection::Row | crate::style::FlexDirection::Column)
                && matches!(s.flex_wrap, crate::style::FlexWrap::Wrap)))
        && matches!(s.writing_mode, crate::style::WritingMode::HorizontalTb)
        && matches!(container.writing_mode, crate::style::WritingMode::HorizontalTb)
        && !matches!(s.position, Position::Absolute | Position::Fixed)
        && s.break_inside != crate::style::BreakValue::Avoid
        && !super::multicol_fragmentation::has_forced_break(s)
        && !is_subgrid(&s.grid_template_columns)
        && !is_subgrid(&s.grid_template_rows)
        && [&s.padding_top, &s.padding_bottom, &s.padding_left, &s.padding_right]
            .iter()
            .all(|p| matches!(p, Length::Px(v) if *v == 0.0))
        && b.children.iter().all(|c| {
            matches!(c.kind, BoxKind::Skip)
                || matches!(c.style.position, Position::Absolute | Position::Fixed)
                || box_is_cuttable_item(c, &b.style)
        })
}

/// CSS Multicol L1 §3 — the used `column-count` of a container whose content box is
/// `content_width` wide in the inline axis, from `column-count` and `column-width`.
pub(super) fn used_column_count(s: &ComputedStyle, em: f32, content_width: f32, col_gap: f32, viewport: Size) -> u32 {
    (match (s.column_count, &s.column_width) {
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
    })
    .max(1)
}

/// LAYOUT-2 срез 7: pure precompute for the multicol dispatch arm — column
/// count/width, `column-fill` mode, and the split of flow children into
/// segments (by `column-span: all` boundaries) with each segment's
/// slice-vs-atomic decision. None of this ever calls `lay_out` on a child —
/// unlike `flex::build_flex_init`'s Step 1 probe, whether a segment is
/// "sliceable" is a pure function of each item's `style`/`kind`
/// ([`segment_is_sliceable`]), not of anything a layout pass would
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
    legend: Option<NodeId>,
) -> Option<Box<super::multicol_trampoline::MulticolInit>> {
    use super::multicol_trampoline::{MulticolInit, SegmentInit};

    let col_gap = s.multicol_column_gap(em, content_width, viewport);

    let n_cols = used_column_count(s, em, content_width, col_gap, viewport);

    let col_w = ((content_width - col_gap * (n_cols - 1) as f32) / n_cols as f32).max(0.0);

    // CSS Multicol L1 §7: `column-fill: balance` distributes content equally
    // (capped by the column height limit); `auto` fills each column up to the
    // limit before opening the next. The limit is the definite content-box
    // `height`, else a definite `max-height` (§7.1 — an auto-height multicol is
    // as tall as its content up to `max-height`). With no limit at all,
    // `column-fill: auto` keeps everything in the first column.
    // CSS Multicol L2 §4.2: a definite `column-height` is the column height whatever the
    // container's own height is (`column-wrap: nowrap` then spills into inline overflow columns).
    // A `column-height` is the height of every column; any other limit is the container's own
    // block size, which the segments between `column-span: all` elements share (Multicol L1 §7.1).
    let limit_shared = !s.column_height_px(em, viewport).is_some_and(|h| h > 0.0);
    let container_h = s.column_height_px(em, viewport).filter(|h| *h > 0.0).or(container_h).or_else(|| {
        let max_len = s.max_height.as_ref()?;
        let max_h = resolve_block_size(max_len, em, available_height, viewport)?;
        Some(match s.box_sizing {
            BoxSizing::ContentBox => max_h,
            BoxSizing::BorderBox => (max_h
                - padding_top
                - padding_bottom
                - s.border_top_width
                - s.border_bottom_width)
                .max(0.0),
        })
    });
    let balance = s.column_fill_balance;
    // CSS Multicol L2 §4.2 / §4.4: a definite `column-height` fixes the block size of every
    // column; overflow columns then open new rows (`row-gap` apart) instead of extending
    // the inline axis.
    // `column-height: auto` with an explicit `column-wrap: wrap` takes the container's definite
    // block size as the column height (Multicol L2 §4.2).
    let col_h = s.column_height_px(em, viewport).filter(|_| s.column_rows_wrap(em, viewport)).or_else(|| {
        (s.column_height.is_none() && s.column_wrap_explicit && !s.column_wrap_nowrap)
            .then_some(container_h)
            .flatten()
            .filter(|h| *h > 0.0)
    });
    let col_rows = col_h.map(|col_h| super::multicol_trampoline::ColRows {
        col_h,
        row_gap: s.row_gap.resolve_or_zero(em, container_h.unwrap_or(0.0), viewport).max(0.0),
    });

    // CSS Multicol L2 §4.4: an explicit `column-wrap: wrap` rows up forced breaks without a
    // `column-height` too.
    let wrap_auto = (s.column_wrap_explicit && !s.column_wrap_nowrap && col_rows.is_none())
        .then(|| s.row_gap.resolve_or_zero(em, container_h.unwrap_or(0.0), viewport).max(0.0));

    // CSS Multicol §6.1: a `column-span: all` descendant reached through plain
    // block wrappers spans the container too — split those wrappers around it so
    // the spanner becomes a direct child (see `multicol_span`).
    super::multicol_span::hoist_nested_spanners(children);
    // CSS Fragmentation L3 §4.1: a forced column break inside a wrapper cuts the wrapper.
    super::multicol_break::hoist_nested_breaks(children);

    // Collect flow (non-abs, non-skip) child indices, without moving `children`
    // yet — the empty case below must leave it completely untouched.
    let flow_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        // Rendered legend multicol-fieldset'а стоит на границе (`place_rendered_legend`), в колонки
        // он не попадает и доезжает до `children` как нефлоу-бокс.
        .filter(|(_, c)| legend != Some(c.node))
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
    // pure function of the container's style, never of a laid-out `.rect`.
    let mut segments: Vec<SegmentInit> = Vec::new();
    let mut seg: Vec<usize> = Vec::new();
    for &i in &flow_idxs {
        if super::multicol_span::is_column_spanner(&work[i]) {
            let grid_frag = n_cols > 1 && container_h.is_some() && col_rows.is_none() && seg.len() == 1 && is_fragmentable_grid(&work[seg[0]], s);
            let sliceable = segment_is_sliceable(n_cols, col_rows.is_some(), container_h.is_some(), s);
            segments.push(SegmentInit {
                item_idxs: std::mem::take(&mut seg),
                span_idx: Some(i),
                sliceable,
                grid_frag,
            });
        } else {
            seg.push(i);
        }
    }
    let grid_frag = n_cols > 1 && container_h.is_some() && col_rows.is_none() && seg.len() == 1 && is_fragmentable_grid(&work[seg[0]], s);
    let sliceable = segment_is_sliceable(n_cols, col_rows.is_some(), container_h.is_some(), s);
    segments.push(SegmentInit { item_idxs: seg, span_idx: None, sliceable, grid_frag });

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
        limit_shared,
        col_rows,
        wrap_auto,
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
        row_used: None,
        oof_out: Vec::new(),
        frag_cols: None,
        frag_segments: 0,
        keep_flow: false,
        nested_out: None,
        nested: Vec::new(),
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
///
/// `BoxKind::FormControl` is excluded *except* `Button`/`Select`, whose used
/// width already comes from their rendered content (BUG-926,
/// [`form_control_fit_content_width`]) rather than a fixed replaced-element
/// intrinsic size — an absolutely positioned icon `<button>` with only
/// `right` set has no `left`/`width`, so without this it stretched to the
/// full containing block instead of shrinking to its SVG icon (BUG-1047).
/// The remaining kinds (checkbox, radio, text entry, range, …) keep the old
/// replaced-element behaviour: they have no rendered label to measure.
fn abs_box_shrinks_to_fit(b: &LayoutBox) -> bool {
    if let BoxKind::FormControl { kind } = &b.kind {
        return matches!(kind, FormControlKind::Button | FormControlKind::Select { .. });
    }
    !matches!(
        b.kind,
        BoxKind::Skip
            | BoxKind::Image { .. }
            | BoxKind::Video { .. }
            | BoxKind::Canvas { .. }
            | BoxKind::Iframe { .. }
            | BoxKind::Table
    ) && !matches!(b.style.display, Display::Grid | Display::InlineGrid)
}

/// `(width, height)` of an abs-pos box is `auto` for the insets-driven sizing of CSS Position L3
/// §6. A form control's UA-default `width`/`height` (`apply_ua_form_controls`) counts as `auto`
/// (§5.3 «semi-replaced»: they stretch like non-replaced boxes); the fixed-size checkbox/radio do not.
fn abs_auto_sizes(kind: &BoxKind, s: &ComputedStyle) -> (bool, bool) {
    let semi_replaced = match kind {
        BoxKind::FormControl { kind: FormControlKind::Input { input_type, .. } } => {
            !matches!(input_type, lumen_dom::InputType::Checkbox | lumen_dom::InputType::Radio)
        }
        BoxKind::FormControl { .. } => true,
        _ => false,
    };
    (
        s.width.is_none() || (semi_replaced && s.width_is_ua_default),
        s.height.is_none() || (semi_replaced && s.height_is_ua_default),
    )
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
    let with_cb: Vec<(usize, f32, f32, Option<Rect>)> =
        deferred.iter().map(|&(i, x, y)| (i, x, y, None)).collect();
    lay_out_abs_children_with(parent, &with_cb, measurer, viewport, my_pcb, hp);
}

/// [`lay_out_abs_children`] with a per-child containing-block override: `Some(cb)` replaces the
/// containing block of that child (a grid area, CSS Grid L1 §9.2) for both `absolute` and `fixed`.
pub(super) fn lay_out_abs_children_with(
    parent: &mut LayoutBox,
    deferred: &[(usize, f32, f32, Option<Rect>)],
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    my_pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    // CSS Anchor Positioning L1: collect all elements with `anchor-name` in the tree.
    // This registry is used to resolve `position-anchor` and `anchor()` function calls below.
    // CSS: anchor-name, position-anchor, anchor()
    let anchors = crate::anchor::collect_anchors(parent);
    // BUG-1483: each child sees only the anchors before it in tree order; `child_seq[i]` is the
    // pre-order index `collect_anchors` gave `parent.children[i]` (the parent itself is 0).
    let child_seq: Vec<u32> = if anchors.is_empty() {
        Vec::new()
    } else {
        let mut next = 1u32;
        parent
            .children
            .iter()
            .map(|c| {
                let at = next;
                let mut stack = vec![c];
                while let Some(b) = stack.pop() {
                    next += 1;
                    stack.extend(b.children.iter());
                }
                at
            })
            .collect()
    };
    // css-transforms-1 §2: `parent` itself captures its `position: fixed` children when it
    // carries a transform/filter/…; otherwise they resolve against the viewport.
    let fixed_cb = fixed_cb_rect(parent);

    let static_rtl = static_x_is_right_edge(parent);
    for &(idx, static_x, static_y, cb_override) in deferred {
        let cb = if let Some(cb) = cb_override {
            cb
        } else if matches!(parent.children[idx].style.position, Position::Fixed) {
            fixed_cb.unwrap_or_else(|| Rect::new(0.0, 0.0, viewport.width, viewport.height))
        } else {
            my_pcb
        };
        let rtl = static_rtl && cb_override.is_none();
        let seen = child_seq.get(idx).map(|&seq| anchors.before(seq));
        let anchors = seen.as_deref().unwrap_or(&anchors);
        // The alignment axes are the containing block's (CSS Box Alignment L3 §5.3): known when
        // it is `parent` itself; any other ancestor falls back to the box's own flow.
        let cb_flow = (cb_override.is_none() && establishes_abs_cb(&parent.style)).then(|| parent.style.clone());
        place_abs_child(
            &mut parent.children[idx], static_x, rtl, static_y, cb, cb_flow.as_deref(), anchors, measurer, viewport,
            my_pcb, hp,
        );
    }
}

/// CSS 2.1 §10.3.7: the static position of an abs-pos box in a block-flow parent whose
/// `direction` is `rtl` is its *right* edge (the box's margin edge sits at the parent's content
/// right edge), not the left one. For such a parent the block-flow loop hands
/// `container_right` over as `static_x` and [`place_abs_child`] subtracts the box width.
pub(super) fn static_x_is_right_edge(parent: &LayoutBox) -> bool {
    let s = &parent.style;
    s.direction == crate::style::Direction::Rtl
        && matches!(s.writing_mode, crate::style::WritingMode::HorizontalTb)
        && matches!(parent.kind, BoxKind::Block | BoxKind::FlowRoot)
        && matches!(s.display, Display::Block | Display::FlowRoot)
        && s.column_count.is_none()
        && s.column_width.is_none()
}

/// Lays out one absolutely/fixed-positioned `child` against its containing
/// block `cb` and moves it to its final position (CSS Position L3 §6).
/// `static_x`/`static_y` is the static position used for an axis whose insets
/// are both `auto`; `my_pcb` is the positioned containing block handed down to
/// the child's own layout.
#[allow(clippy::too_many_arguments)]
fn place_abs_child(
    child: &mut LayoutBox,
    static_x: f32,
    static_x_is_right: bool,
    static_y: f32,
    cb: Rect,
    cb_flow: Option<&ComputedStyle>,
    anchors: &crate::anchor::AnchorRegistry,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    my_pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    let cs = child.style.clone();
    let c_em = cs.font_size;
    let flow = cb_flow.unwrap_or(&cs);

    // CSS Anchor Positioning L1 §3.1 — intercept `anchor()` in top/right/bottom/left
    // before falling back to the plain length/auto value.
    // CSS: anchor(), position-anchor
    let default_anchor = cs.position_anchor.as_deref();
    let left = crate::anchor::resolve_inset(
        anchors, &cs.left, cs.anchor_left.as_ref(), default_anchor, true, false, cb.x, cb.x + cb.width,
        c_em, cb.width, viewport,
    );
    let right = crate::anchor::resolve_inset(
        anchors, &cs.right, cs.anchor_right.as_ref(), default_anchor, true, true, cb.x, cb.x + cb.width,
        c_em, cb.width, viewport,
    );
    let top = crate::anchor::resolve_inset(
        anchors, &cs.top, cs.anchor_top.as_ref(), default_anchor, false, false, cb.y, cb.y + cb.height,
        c_em, cb.height, viewport,
    );
    let bottom = crate::anchor::resolve_inset(
        anchors, &cs.bottom, cs.anchor_bottom.as_ref(), default_anchor, false, true, cb.y, cb.y + cb.height,
        c_em, cb.height, viewport,
    );

    // CSS Box Alignment L3 §5.3: a self-alignment other than `normal` between two insets makes an
    // `auto` size `fit-content` instead of stretched, and places the box inside them.
    let (axis_x, axis_y) = (abs_axis_alignment(&cs, flow, true), abs_axis_alignment(&cs, flow, false));
    let align_x = axis_x.aligned.filter(|_| left.is_some() && right.is_some());
    let align_y = axis_y.aligned.filter(|_| top.is_some() && bottom.is_some());

    let c_ml = cs.margin_left.resolve_or_zero(c_em, cb.width, viewport);
    let c_mr = cs.margin_right.resolve_or_zero(c_em, cb.width, viewport);
    let c_mt = cs.margin_top.resolve_or_zero(c_em, cb.height, viewport);
    let c_mb = cs.margin_bottom.resolve_or_zero(c_em, cb.height, viewport);

    // Доступная ширина для layout абсолютного child.
    let (w_auto, h_auto) = abs_auto_sizes(&child.kind, &cs);
    let avail_w = if left.is_some() && right.is_some() && w_auto && align_x.is_none() {
        // Обе инсеты заданы, ширина `auto` → ширина выводится из зазора
        // между ними (CSS Position L3 §6), shrink-to-fit не применяется.
        (cb.width - left.unwrap_or(0.0) - right.unwrap_or(0.0)).max(0.0)
    } else if cs.width.is_none() && abs_box_shrinks_to_fit(child) {
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
        let free =
            (cb.width - left.unwrap_or(0.0) - right.unwrap_or(0.0) - c_ml - c_mr).max(0.0);
        // A form control's content (button label/icon) does not wrap, so its
        // max-content and min-content coincide — same shortcut `intrinsic.rs`
        // uses at the call sites of `form_control_fit_content_width`.
        let (max_c, min_c) = match form_control_fit_content_width(child, measurer, viewport) {
            Some(fc) => (fc, fc),
            None => (
                max_content_outer_width(child, measurer, viewport),
                min_content_outer_width(child, measurer, viewport),
            ),
        };
        // BUG-1484: the intrinsic widths resolve a percentage `padding` against 0; here the
        // containing block is definite, so the frame takes its real share of `cb.width`.
        let pct_pad = |l: &Length| l.resolve_or_zero(c_em, cb.width, viewport) - l.resolve_or_zero(c_em, 0.0, viewport);
        let pad_extra = pct_pad(&cs.padding_left) + pct_pad(&cs.padding_right);
        (max_c + pad_extra).min((min_c + pad_extra).max(free)) + c_ml + c_mr
    } else {
        cb.width
    };

    // CSS Position L3 §6: an abs-pos box with both `top` and `bottom` non-auto
    // and `height: auto` resolves its used height to fill the inset gap. Mirror of
    // the `avail_w` width-from-insets path above. The gap is a containing-block
    // used value, so the box is laid out with it as a definite height — a
    // percentage height inside it must see that size, not the content-driven one.
    // BUG-1385 / Sizing L4 §4.1: with a ratio and `height: auto`, the block size follows the
    // inline size instead of stretching between the block insets — when `width` is authored, or
    // when the two inline insets stretch it (then the box keeps that stretched width). With an
    // authored `height` and `width: auto`, the ratio gives the width (`block_ratio_width`).
    let ratio = cs.aspect_ratio.is_some_and(|(aw, ah)| aw > 0.0 && ah > 0.0);
    let ratio_stretched_w = ratio && left.is_some() && right.is_some() && cs.width.is_none() && cs.height.is_none();
    let ratio_sized_h = ratio && cs.height.is_none() && (cs.width.is_some() || ratio_stretched_w);
    let stretched_h = (top.is_some() && bottom.is_some() && h_auto && !ratio_sized_h && align_y.is_none())
        .then(|| (cb.height - top.unwrap_or(0.0) - bottom.unwrap_or(0.0) - c_mt - c_mb).max(0.0));
    // A vertical box is as tall as its inline size: it fills the room only when it has none
    // (`stretch`ed), otherwise `fit-content` once aligned (the room it is laid out in is the
    // whole containing block).
    let vertical_child = cs.writing_mode != crate::style::WritingMode::HorizontalTb
        && matches!(child.kind, BoxKind::Block | BoxKind::FlowRoot);
    let stretched_h = stretched_h.or_else(|| {
        (vertical_child && align_y.is_some() && h_auto && top.is_some() && bottom.is_some()).then(|| {
            let room = (cb.height - top.unwrap_or(0.0) - bottom.unwrap_or(0.0) - c_mt - c_mb).max(0.0);
            max_content_outer_height(child, measurer, viewport).min(room)
        })
    });
    // CSS Position L3 §5.3: a form control with a UA-default `width` is «semi-replaced» — two insets
    // stretch it like a non-replaced box instead of keeping the 174px/200px/… UA size.
    let semi_stretched_w = left.is_some() && right.is_some() && w_auto && matches!(child.kind, BoxKind::FormControl { .. });
    // An explicit `stretch` fills the room along the block axis of a vertical box too.
    let block_stretched_w = vertical_child && axis_x.stretch && left.is_some() && right.is_some() && w_auto;
    let stretched_w = (ratio_stretched_w || semi_stretched_w || block_stretched_w).then(|| {
        let room = if block_stretched_w {
            (cb.width - left.unwrap_or(0.0) - right.unwrap_or(0.0)).max(0.0)
        } else {
            avail_w
        };
        (room - c_ml - c_mr).max(0.0)
    });
    // CSS 2.1 §10.5: a percentage `height` of an out-of-flow box resolves against
    // the padding box of its containing block, whose height is always definite.
    // CSS 2.1 §10.3.7 / Box 4: `padding` and `margin` percentages resolve against the containing
    // block's width, not against the inset gap or shrink-to-fit width `avail_w` stands for (BUG-1484).
    let pct_base = (avail_w != cb.width).then_some(cb.width);
    match (stretched_h, stretched_w) {
        (None, None) if pct_base.is_none() => {
            lay_out(child, 0.0, 0.0, avail_w, Some(cb.height), measurer, viewport, my_pcb, hp, false);
        }
        (None, None) => lay_out_with_used_size(
            child, 0.0, 0.0, avail_w, Some(cb.height), measurer, viewport, my_pcb, hp, false,
            UsedSizeOverride { percentage_base: pct_base, ..Default::default() },
        ),
        (h, w) => lay_out_with_used_size(
            child, 0.0, 0.0, avail_w, Some(cb.height), measurer, viewport, my_pcb, hp, false,
            UsedSizeOverride {
                width: w, height: h, box_sizing: Some(BoxSizing::BorderBox), percentage_base: pct_base,
                ..Default::default()
            },
        ),
    }
    if let Some(h) = stretched_h {
        child.rect.height = h;
    }

    // CSS Anchor Positioning L1 §4 — apply `anchor-size()` overrides for width/height.
    // Done before resolving `inset-area` so the element's used size (used to
    // align it within its position-area band) reflects the anchor-size result.
    let mut w_fixed = cs.width.is_some() && !semi_stretched_w;
    let mut h_fixed = cs.height.is_some() && stretched_h.is_none();
    if let Some(w) = cs.anchor_size_w.as_ref().and_then(|f| {
        crate::anchor::resolve_anchor_size_or_fallback(
            anchors, f, cs.position_anchor.as_deref(), c_em, cb.width, viewport,
        )
    }) {
        child.rect.width = w;
        w_fixed = true;
    }
    if let Some(h) = cs.anchor_size_h.as_ref().and_then(|f| {
        crate::anchor::resolve_anchor_size_or_fallback(
            anchors, f, cs.position_anchor.as_deref(), c_em, cb.height, viewport,
        )
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
            anchors,
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
        // Normal abs-pos: resolve from left/right/top/bottom insets. An axis with both insets and a
        // definite size is over-determined: auto margins absorb the free space, otherwise
        // `justify-self` / `align-self` place the box inside the inset-modified containing block.
        let aligned = |fixed: bool, align: Option<super::self_align::Alignment>, near: Option<f32>, far: Option<f32>, auto_m: bool, origin: f32, extent: f32, size: f32, m: (f32, f32)| {
            let (Some(n), Some(f)) = (near, far) else { return None };
            let mut a = align?;
            (fixed && !auto_m).then(|| {
                // CSS Position L3 §5.2: without a `safe`/`unsafe` keyword the margin box stays inside
                // the containing block joined with the inset-modified one (`hull` stays `None` for
                // an explicit `unsafe`, see `abs_axis_alignment`).
                a.hull = a.hull.map(|_| (origin.min(origin + n), (origin + extent).max(origin + extent - f)));
                super::self_align::aligned_border_start(
                    a,
                    super::self_align::AlignArea { lo: origin + n, cell: extent - n - f, size, m_lo: m.0, m_hi: m.1 },
                )
            })
        };
        let aligned_x = aligned(
            w_fixed || align_x.is_some(), align_x, left, right,
            cs.margin_left.is_auto() || cs.margin_right.is_auto(),
            cb.x, cb.width, child.rect.width, (c_ml, c_mr),
        );
        let aligned_y = aligned(
            h_fixed || align_y.is_some(), align_y, top, bottom,
            cs.margin_top.is_auto() || cs.margin_bottom.is_auto(),
            cb.y, cb.height, child.rect.height, (c_mt, c_mb),
        );
        let nx = match (left, right) {
            (Some(_), Some(_)) if aligned_x.is_some() => aligned_x.unwrap_or(cb.x),
            (Some(l), Some(r)) if w_fixed => {
                cb.x + l + c_ml + auto_margin_shift(
                    cb.width - l - r - c_ml - c_mr - child.rect.width,
                    cs.margin_left.is_auto(),
                    cs.margin_right.is_auto(),
                )
            }
            (Some(l), _)    => cb.x + l + c_ml,
            (None, Some(r)) => cb.x + cb.width - r - c_mr - child.rect.width,
            (None, None) if static_x_is_right => static_x - c_mr - child.rect.width,
            (None, None)    => static_x + c_ml,
        };
        let ny = match (top, bottom) {
            (Some(_), Some(_)) if aligned_y.is_some() => aligned_y.unwrap_or(cb.y),
            (Some(t), Some(bv)) if h_fixed => {
                cb.y + t + c_mt + auto_margin_shift(
                    cb.height - t - bv - c_mt - c_mb - child.rect.height,
                    cs.margin_top.is_auto(),
                    cs.margin_bottom.is_auto(),
                )
            }
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

/// CSS Box Alignment L3 §«Absolute-Position»: the alignment — `Start`/`End`/`Center`, whether it
/// is `safe`, and whether the axis runs against the physical direction — that `justify-self`
/// (the containing block's inline axis) or `align-self` (its block axis) gives `cs` along the
/// physical axis `is_x`. `None` for `auto`/`normal`/`stretch`: the box keeps its inset-driven
/// position and size. `flow` is the containing block's style, supplying the writing mode and
/// direction the axes are defined by.
fn abs_axis_alignment(cs: &ComputedStyle, flow: &ComputedStyle, is_x: bool) -> AxisAlign {
    use crate::style::{Direction, WritingMode};
    let rtl = flow.direction == Direction::Rtl;
    let (inline, rev) = if flow.writing_mode == WritingMode::HorizontalTb {
        (is_x, is_x && rtl)
    } else if is_x {
        (false, matches!(flow.writing_mode, WritingMode::VerticalRl | WritingMode::SidewaysRl))
    } else {
        (true, rtl != (flow.writing_mode == WritingMode::SidewaysLr))
    };
    let cx = &cs.content_align_extra;
    let (mut align, safe, explicit_unsafe) = if inline {
        let v = super::grid_vertical::resolve_own(cs.justify_self, cx.justify_self_own, cs, is_x, !rev);
        (
            super::grid_vertical::resolve_side(v, cx.justify_self_side, flow.writing_mode, rev),
            cx.justify_self_safe,
            cx.justify_self_unsafe,
        )
    } else {
        (super::grid_vertical::resolve_own(cs.align_self, cx.self_own, cs, is_x, !rev), cx.self_safe, cx.self_unsafe)
    };
    match align {
        AlignValue::LastBaseline => align = AlignValue::End,
        AlignValue::Baseline => align = AlignValue::Start,
        _ => {}
    }
    AxisAlign {
        // `hull` is a placeholder here: the caller fills the interval in once the insets are known.
        aligned: matches!(align, AlignValue::Start | AlignValue::End | AlignValue::Center).then_some(
            super::self_align::Alignment { align, safe, hull: (!explicit_unsafe).then_some((0.0, 0.0)), rev },
        ),
        stretch: align == AlignValue::Stretch,
    }
}

/// [`abs_axis_alignment`]'s result for one physical axis.
struct AxisAlign {
    /// `None` — the box keeps its inset-driven position and size.
    aligned: Option<super::self_align::Alignment>,
    /// An explicit `stretch`: also fills the axis of a box whose *block* axis it is, which
    /// `normal` leaves content-sized.
    stretch: bool,
}

/// Offset of an abs-pos box inside the gap between its two insets when the axis
/// is over-determined (CSS 2.1 §10.3.7 / §10.6.4): `free` is the leftover space;
/// it goes to `auto` margins — split evenly when both are `auto` (never
/// negative, the start margin is then 0), all of it to the start margin when only
/// that one is `auto`; otherwise the end margin absorbs it and nothing shifts.
fn auto_margin_shift(free: f32, start_auto: bool, end_auto: bool) -> f32 {
    match (start_auto, end_auto) {
        (true, true) => free.max(0.0) / 2.0,
        (true, false) => free,
        _ => 0.0,
    }
}

/// Padding box of `b` — the containing block it provides to positioned descendants.
pub(super) fn padding_box_rect(b: &LayoutBox) -> Rect {
    let s = &b.style;
    Rect::new(
        b.rect.x + s.border_left_width,
        b.rect.y + s.border_top_width,
        (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
        (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
    )
}

/// Whether a box with style `s` is the containing block of absolutely
/// positioned descendants: positioned itself, or one of the properties that
/// also capture `position: fixed` (css-transforms-1 §2, css-contain-2 §3.2).
pub(crate) fn establishes_abs_cb(s: &ComputedStyle) -> bool {
    !matches!(s.position, Position::Static) || contains_fixed_descendants(s)
}

/// `Some(padding box)` when `b` is the containing block of `position: fixed`
/// descendants instead of the viewport (css-transforms-1 §2).
pub(super) fn fixed_cb_rect(b: &LayoutBox) -> Option<Rect> {
    contains_fixed_descendants(&b.style).then(|| padding_box_rect(b))
}

/// An anchor-positioned box is placed by `container_anchor`'s post-pass, which
/// reads the final anchor rects — nothing to correct here.
pub(super) fn uses_anchor_positioning(s: &ComputedStyle) -> bool {
    s.position_anchor.is_some()
        || s.anchor_left.is_some()
        || s.anchor_right.is_some()
        || s.anchor_top.is_some()
        || s.anchor_bottom.is_some()
        || s.anchor_size_w.is_some()
        || s.anchor_size_h.is_some()
}

/// Second placement of out-of-flow descendants once `b`, their containing
/// block, has its final size (CSS Position L3 §2.2).
///
/// A box laid out under a non-positioned wrapper is placed when the *wrapper*
/// finishes, against the `pcb` handed down at that point — the width of the
/// real containing block is known by then, but its height still reads 0
/// (`children_pcb` is built before the children). `bottom`, `top: <percent>`
/// and `top` + `bottom` with `height: auto` therefore landed against a
/// zero-high block. `position: fixed` under a transformed/filtered ancestor
/// additionally used the viewport for both axes. Direct children are placed
/// by [`lay_out_abs_children`] with the final rect already, so only deeper
/// descendants are visited; the walk stops where another box takes over as the
/// containing block (an absolute box's CB is the nearest [`establishes_abs_cb`]
/// ancestor, a fixed box's the nearest `contains_fixed_descendants` one).
pub(crate) fn fix_out_of_flow_descendants(
    b: &mut LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    if !establishes_abs_cb(&b.style) {
        return;
    }
    let abs_cb = padding_box_rect(b);
    let fixed_cb = fixed_cb_rect(b);
    // (box, abs CB still `b`, fixed CB still `b`, direct child of `b`)
    let mut stack: Vec<(&mut LayoutBox, bool, bool, bool)> = Vec::new();
    for c in b.children.iter_mut() {
        stack.push((c, true, fixed_cb.is_some(), true));
    }
    while let Some((x, abs_open, fixed_open, direct)) = stack.pop() {
        if !direct && !uses_anchor_positioning(&x.style) {
            match x.style.position {
                // The size of a box whose height follows the containing block
                // (`height: 50%`, `top` + `bottom`) was derived from the zero-high
                // first-pass block, and so was everything laid out inside it.
                Position::Absolute if abs_open && height_follows_cb(&x.kind, &x.style, viewport) => {
                    refit_out_of_flow_child(x, abs_cb, measurer, viewport, hp);
                }
                Position::Absolute if abs_open => reposition_abs_child(x, abs_cb, viewport),
                Position::Fixed if fixed_open => {
                    if let Some(cb) = fixed_cb {
                        refit_out_of_flow_child(x, cb, measurer, viewport, hp);
                    }
                }
                _ => {}
            }
        }
        let abs_open = abs_open && !establishes_abs_cb(&x.style);
        let fixed_open = fixed_open && !contains_fixed_descendants(&x.style);
        if abs_open || fixed_open {
            for c in x.children.iter_mut() {
                stack.push((c, abs_open, fixed_open, false));
            }
        }
    }
}

/// Moves an already laid-out absolute `child` to where its insets put it in
/// the final containing block `cb`. An axis with both insets `auto` keeps the
/// static position the first pass gave it. The child's own layout is not
/// redone: `cb` differs from the first-pass one in height only.
fn reposition_abs_child(child: &mut LayoutBox, cb: Rect, viewport: Size) {
    let cs = &child.style;
    let em = cs.font_size;
    let left = cs.left.resolve(em, cb.width, viewport);
    let right = cs.right.resolve(em, cb.width, viewport);
    let top = cs.top.resolve(em, cb.height, viewport);
    let bottom = cs.bottom.resolve(em, cb.height, viewport);
    let (ml, mr) = (cs.margin_left.resolve_or_zero(em, cb.width, viewport), cs.margin_right.resolve_or_zero(em, cb.width, viewport));
    let (mt, mb) = (cs.margin_top.resolve_or_zero(em, cb.height, viewport), cs.margin_bottom.resolve_or_zero(em, cb.height, viewport));
    // Mirrors the `top` + `bottom` + `height: auto` rule of `place_abs_child`.
    if top.is_some() && bottom.is_some() && abs_auto_sizes(&child.kind, cs).1 {
        child.rect.height = (cb.height - top.unwrap_or(0.0) - bottom.unwrap_or(0.0) - mt - mb).max(0.0);
    }
    let x = match (left, right) {
        (Some(l), _) => cb.x + l + ml,
        (None, Some(r)) => cb.x + cb.width - r - mr - child.rect.width,
        (None, None) => child.rect.x,
    };
    let y = match (top, bottom) {
        (Some(t), _) => cb.y + t + mt,
        (None, Some(bv)) => cb.y + cb.height - bv - mb - child.rect.height,
        (None, None) => child.rect.y,
    };
    let (dx, dy) = (x - child.rect.x, y - child.rect.y);
    shift_tree(child, dx, dy);
}

/// Whether the used height of an out-of-flow box with style `s` is a function
/// of its containing block's height: a percentage `height`/`min-height`/
/// `max-height`, or `top` + `bottom` around `height: auto`. Probed by resolving
/// against two different bases, which also covers `calc()` mixing `%`.
fn height_follows_cb(kind: &BoxKind, s: &ComputedStyle, viewport: Size) -> bool {
    let em = s.font_size;
    let tracks = |l: &Option<Length>| {
        l.as_ref().is_some_and(|l| l.resolve(em, Some(0.0), viewport) != l.resolve(em, Some(100.0), viewport))
    };
    let stretched = abs_auto_sizes(kind, s).1 && !s.top.is_auto() && !s.bottom.is_auto();
    stretched || tracks(&s.height) || tracks(&s.min_height) || tracks(&s.max_height)
}

/// Re-lays out an out-of-flow `child` against its final containing block `cb`:
/// the padding box of the transformed/filtered ancestor for `position: fixed`
/// (the first pass sized it against the viewport, which changes `width: auto`,
/// percentages and everything below), or the positioned ancestor for an
/// absolute box that was placed through a static wrapper (the first pass saw a
/// zero-high block).
/// An axis with both insets `auto` keeps its first-pass (static) position.
fn refit_out_of_flow_child(
    child: &mut LayoutBox,
    cb: Rect,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let em = child.style.font_size;
    let ml = child.style.margin_left.resolve_or_zero(em, cb.width, viewport);
    let mt = child.style.margin_top.resolve_or_zero(em, cb.height, viewport);
    let (static_x, static_y) = (child.rect.x - ml, child.rect.y - mt);
    place_abs_child(child, static_x, false, static_y, cb, None, &crate::anchor::AnchorRegistry::default(), measurer, viewport, cb, hp);
}
