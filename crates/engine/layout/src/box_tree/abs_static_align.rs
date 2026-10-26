//! CSS Box Alignment L3 §5.3 / CSS Position L3 §5.2 — `justify-self` / `align-self` of an
//! absolutely positioned child of a block container on an axis whose two insets are `auto`.
//!
//! The static position of such a box is a rectangle in the container's flow: along the inline
//! axis it is the container's content box, along the block axis it has no extent — it is the
//! point where the box would have started in flow. `start` keeps the box where
//! `lay_out_abs_children` put it; `center`/`end` move it by half / all of the free space of that
//! rectangle, which along the block axis is negative (the box ends up above the point).
//!
//! Not handled: a container with inline content (the static-position rectangle of a box inside an
//! inline element is a point in the line) and vertical writing modes.
//!
//! Flex and grid containers handle their own abspos children (`flex_trampoline`, `grid_abspos`).

use super::*;
use super::self_align::{aligned_border_start, AlignArea};
use crate::style::{Direction, WritingMode};

/// Whether the abspos children of `b` have their static position inside a horizontal block flow
/// that this module aligns: a plain block container (`BoxKind::Block`/`FlowRoot` — not a table,
/// multicol or a vertical writing mode, whose static positions are computed elsewhere).
fn applies(b: &LayoutBox) -> bool {
    let s = &b.style;
    s.writing_mode == WritingMode::HorizontalTb
        && matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot)
        && matches!(s.display, Display::Block | Display::FlowRoot | Display::InlineBlock | Display::ListItem)
        && s.column_count.is_none()
        && s.column_width.is_none()
}

/// Moves each deferred abspos child of the finished block `b` from its start-aligned static
/// position to the one `justify-self` (inline axis) / `align-self` (block axis) gives it.
/// `content_x`/`content_width` is the content box of `b` along x, `abs_deferred` the
/// `(child, static_x, static_y)` list the block flow collected.
pub(super) fn align_static_positions(
    b: &mut LayoutBox,
    content_x: f32,
    content_width: f32,
    abs_deferred: &[(usize, f32, f32)],
    viewport: Size,
) {
    if abs_deferred.is_empty() || !applies(b) {
        return;
    }
    // An abspos box inside an inline element is a sibling of the container's inline runs here
    // (the build flattens it out), and its static-position rectangle is the point in the line
    // instead of the content box. The tree does not say which parent the box had, so a container
    // with inline content keeps the start-aligned static position.
    if b.children.iter().any(|c| matches!(c.kind, BoxKind::InlineRun { .. } | BoxKind::InlineBlockRow)) {
        return;
    }
    let parent = Arc::clone(&b.style);
    let rtl = parent.direction == Direction::Rtl;
    let right_edge = super::multicol_abspos::static_x_is_right_edge(b);
    for &(idx, static_x, static_y) in abs_deferred {
        let child = &b.children[idx];
        let cs = &child.style;
        if super::multicol_abspos::uses_anchor_positioning(cs) {
            continue;
        }
        let em = cs.font_size;
        let m_l = cs.margin_left.resolve_or_zero(em, content_width, viewport);
        let m_r = cs.margin_right.resolve_or_zero(em, content_width, viewport);
        let m_t = cs.margin_top.resolve_or_zero(em, content_width, viewport);
        let m_b = cs.margin_bottom.resolve_or_zero(em, content_width, viewport);
        let mut dx = 0.0;
        if matches!(cs.left, LengthOrAuto::Auto)
            && matches!(cs.right, LengthOrAuto::Auto)
            && let Some(a) = super::self_align::inline_alignment(&parent, cs, true, rtl)
        {
            let at_static = if right_edge { static_x - m_r - child.rect.width } else { static_x + m_l };
            let area = AlignArea { lo: content_x, cell: content_width, size: child.rect.width, m_lo: m_l, m_hi: m_r };
            dx = aligned_border_start(a, area) - at_static;
        }
        let mut dy = 0.0;
        if matches!(cs.top, LengthOrAuto::Auto)
            && matches!(cs.bottom, LengthOrAuto::Auto)
            && let Some(mut a) = super::multicol_abspos::abs_axis_alignment(cs, &parent, false).aligned
        {
            // The rectangle has no block extent: nothing to keep the margin box inside of.
            a.hull = None;
            let area = AlignArea { lo: static_y, cell: 0.0, size: child.rect.height, m_lo: m_t, m_hi: m_b };
            dy = aligned_border_start(a, area) - (static_y + m_t);
        }
        if dx.abs() > 0.01 || dy.abs() > 0.01 {
            shift_tree(&mut b.children[idx], dx, dy);
        }
    }
}
