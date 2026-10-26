//! CSS Box Alignment L3 §5 — `justify-self` / `align-self` of a box inside its containing block
//! along one physical axis, shared by the in-flow block flows (`block_flow_trampoline`,
//! `vertical_trampoline`) and absolutely positioned boxes (`multicol_abspos`).

use super::*;

/// The overflow-aware alignment of `child` in `parent`'s inline axis: `Start`/`End`/`Center`
/// (a `baseline` falls back to its fallback alignment) plus `safe`, or `None` for the values
/// that leave the box where flow put it (`normal`, `stretch`, `auto` resolving to either).
///
/// `justify-self: auto` takes the parent's `justify-items` (§6.3). `horizontal` — the axis is
/// physically x; `rev` — the parent's inline-start edge is the high one (right / bottom).
/// `start`/`end` follow the parent, `self-start`/`self-end` the child's own writing mode,
/// `left`/`right` the physical side.
pub(super) fn inline_alignment(
    parent: &ComputedStyle,
    child: &ComputedStyle,
    horizontal: bool,
    rev: bool,
) -> Option<(AlignValue, bool)> {
    let (value, own, side, safe) = if matches!(child.justify_self, AlignValue::Auto) {
        let x = &parent.content_align_extra;
        (parent.justify_items, x.justify_items_own, x.justify_items_side, x.justify_items_safe)
    } else {
        let x = &child.content_align_extra;
        (child.justify_self, x.justify_self_own, x.justify_self_side, x.justify_self_safe)
    };
    let value = super::grid_vertical::resolve_own(value, own, child, horizontal, !rev);
    let align = match super::grid_vertical::resolve_side(value, side, parent.writing_mode, rev) {
        AlignValue::Baseline => AlignValue::Start,
        AlignValue::LastBaseline => AlignValue::End,
        a => a,
    };
    matches!(align, AlignValue::Start | AlignValue::End | AlignValue::Center).then_some((align, safe))
}

/// Border-box start (physical coordinate) of a box of border-box `size` with margins
/// `m_lo`/`m_hi` aligned by `align` inside the area `[lo, lo + cell]`. `safe` falls back to
/// `start` when the margin box overflows the area; without it an overflowing box still aligns
/// by its own side (`unsafe`, CSS Box Alignment L3 §4.4).
pub(super) fn aligned_border_start(
    align: AlignValue,
    safe: bool,
    rev: bool,
    lo: f32,
    cell: f32,
    size: f32,
    m_lo: f32,
    m_hi: f32,
) -> f32 {
    let (m_s, m_e) = if rev { (m_hi, m_lo) } else { (m_lo, m_hi) };
    let align = if safe && size + m_s + m_e > cell && matches!(align, AlignValue::End | AlignValue::Center) {
        AlignValue::Start
    } else {
        align
    };
    let off = super::grid_vertical::axis_offset(align, cell, size, m_s, m_e, false, false);
    if rev { lo + cell - off - size } else { lo + off }
}

/// How an auto-sized in-flow block child's size along its parent's inline axis follows
/// `justify-self` (CSS Box Alignment L3 §5.2): `Fit` — `fit-content` (an alignment is set, the
/// free space is to be distributed); `Stretch` — an explicit `stretch`, which also fills the
/// axis of a box whose own inline axis is the other one (orthogonal flow, where `normal` keeps
/// the content-based size).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum InlineSizing {
    Fit,
    Stretch,
}

/// `None` unless `child` is a plain block-level box with `auto` size and non-`auto` margins on
/// the axis (`horizontal` — x) — those keep the flow's own sizing.
pub(super) fn inline_sizing(parent: &ComputedStyle, child: &LayoutBox, horizontal: bool, rev: bool) -> Option<InlineSizing> {
    let cs = &child.style;
    let (lo_auto, hi_auto, sized) = if horizontal {
        (cs.margin_left.is_auto(), cs.margin_right.is_auto(), cs.width.is_some())
    } else {
        (cs.margin_top.is_auto(), cs.margin_bottom.is_auto(), cs.height.is_some())
    };
    if lo_auto
        || hi_auto
        || sized
        || !matches!(child.kind, BoxKind::Block | BoxKind::FlowRoot)
        || !matches!(cs.display, Display::Block | Display::FlowRoot)
        || matches!(cs.float_side, FloatSide::Left | FloatSide::Right)
        || matches!(cs.position, Position::Absolute | Position::Fixed)
    {
        return None;
    }
    if inline_alignment(parent, cs, horizontal, rev).is_some() {
        return Some(InlineSizing::Fit);
    }
    let value = if matches!(cs.justify_self, AlignValue::Auto) { parent.justify_items } else { cs.justify_self };
    matches!(value, AlignValue::Stretch).then_some(InlineSizing::Stretch)
}

/// A [`UsedSizeOverride`] giving `child` the border-box size `border_box` along the axis
/// (`horizontal` — width), expressed in its own `box-sizing`; `frame` is its padding + border
/// on that axis.
pub(super) fn size_override(child: &ComputedStyle, horizontal: bool, border_box: f32, frame: f32) -> UsedSizeOverride {
    let v = match child.box_sizing {
        BoxSizing::ContentBox => (border_box - frame).max(0.0),
        BoxSizing::BorderBox => border_box.max(0.0),
    };
    if horizontal {
        UsedSizeOverride { width: Some(v), ..Default::default() }
    } else {
        UsedSizeOverride { height: Some(v), ..Default::default() }
    }
}
