//! CSS 2.1 §8.3.1 margin collapsing along the block axis of a vertical writing
//! mode (CSS Writing Modes L3 §7.1 — the block axis is physical `x`, so the
//! margins that collapse are the left/right ones: block-start is the left side
//! for `vertical-lr`, the right one for `vertical-rl`).
//!
//! Mirror of `bfc::{collapsed_top_margin, collapsed_bottom_margin}` for a box
//! whose children stack along `x`: a block's block-start margin collapses with
//! its first in-flow block child's, its block-end margin with its last one's,
//! and the margins of an empty block collapse through it. Like the horizontal
//! version it folds the largest margin of the chain (`max`), so a negative one
//! only matters when nothing else adjoins it.

use super::*;

/// Per-`run` memo of [`collapsed_margin`] for the block-start and block-end side
/// (see `bfc::MarginCollapseCache` for the keying).
#[derive(Default)]
pub(super) struct MarginCaches {
    pub(super) start: MarginCollapseCache,
    pub(super) end: MarginCollapseCache,
}

/// Block-start and block-end margin of `b` (physical right/left for `rl`).
pub(super) fn own_margins(b: &LayoutBox, rl: bool, cb: f32, viewport: Size) -> (f32, f32) {
    let em = b.style.font_size;
    let ml = b.style.margin_left.resolve_or_zero(em, cb, viewport);
    let mr = b.style.margin_right.resolve_or_zero(em, cb, viewport);
    if rl { (mr, ml) } else { (ml, mr) }
}

/// Padding + border on the block-start (`start`) or block-end side of `b`.
fn frame_on_side(b: &LayoutBox, start: bool, rl: bool, cb: f32, viewport: Size) -> f32 {
    let em = b.style.font_size;
    let s = &b.style;
    if start != rl {
        s.padding_left.resolve_or_zero(em, cb, viewport) + s.border_left_width
    } else {
        s.padding_right.resolve_or_zero(em, cb, viewport) + s.border_right_width
    }
}

/// An in-flow child: not a float, not out of flow, not a `::marker`/`Skip`.
fn is_in_flow(c: &LayoutBox) -> bool {
    !matches!(c.kind, BoxKind::Marker { .. } | BoxKind::Skip)
        && c.style.float_side == FloatSide::None
        && !matches!(c.style.position, Position::Absolute | Position::Fixed)
}

/// The in-flow child whose margin on the block-start (`first`) or block-end side
/// adjoins `b`'s: a plain block of the same writing mode (an orthogonal one is an
/// independent formatting context) without clearance. Anything else — an inline
/// run, a replaced box — ends the chain.
fn edge_child(b: &LayoutBox, first: bool) -> Option<&LayoutBox> {
    let c = if first {
        b.children.iter().find(|c| is_in_flow(c))
    } else {
        b.children.iter().rev().find(|c| is_in_flow(c))
    }?;
    (c.style.clear == ClearSide::None
        && matches!(c.kind, BoxKind::Block)
        && c.style.writing_mode == b.style.writing_mode)
        .then_some(c)
}

/// Does `b`'s block-start margin collapse with its first child's (so that the
/// child's margin is part of `b`'s own)? No block-start padding/border, no BFC.
pub(crate) fn escapes_start(b: &LayoutBox, rl: bool, cb: f32, viewport: Size) -> bool {
    matches!(b.kind, BoxKind::Block)
        && !establishes_bfc(b)
        && frame_on_side(b, true, rl, cb, viewport) == 0.0
        && edge_child(b, true).is_some()
}

/// Does `b`'s block-end margin collapse with its last child's? As
/// [`escapes_start`], and the block-size must be `auto` (a definite one keeps the
/// child's margin inside the box).
pub(crate) fn escapes_end(b: &LayoutBox, rl: bool, cb: f32, viewport: Size) -> bool {
    matches!(b.kind, BoxKind::Block)
        && !establishes_bfc(b)
        && b.style.width.is_none()
        && frame_on_side(b, false, rl, cb, viewport) == 0.0
        && edge_child(b, false).is_some()
}

/// The margin on the block-start (`start`) or block-end side of `b` once it has
/// collapsed with the margins of its first/last-child chain.
pub(super) fn collapsed_margin(
    b: &LayoutBox,
    start: bool,
    rl: bool,
    cb: f32,
    viewport: Size,
    cache: &mut MarginCollapseCache,
) -> f32 {
    if let Some(&(cached_cb, v)) = cache.get(&(b.node, b.origin.role))
        && cached_cb == cb
    {
        return v;
    }
    let side = |n: &LayoutBox| {
        let (s, e) = own_margins(n, rl, cb, viewport);
        if start { s } else { e }
    };
    // Same suffix-max walk as `bfc::collapsed_top_margin`: every node on the chain
    // gets its own cache entry, so a deep single-child chain stays linear.
    let mut chain: Vec<((NodeId, BoxRole), f32)> = Vec::new();
    let mut node = b;
    let mut tail = f32::NEG_INFINITY;
    loop {
        let key = (node.node, node.origin.role);
        if !std::ptr::eq(node, b)
            && let Some(&(cached_cb, v)) = cache.get(&key)
            && cached_cb == cb
        {
            tail = v;
            break;
        }
        chain.push((key, side(node)));
        let descends = if start { escapes_start(node, rl, cb, viewport) } else { escapes_end(node, rl, cb, viewport) };
        match descends.then(|| edge_child(node, start)).flatten() {
            Some(next) => node = next,
            None => break,
        }
    }
    let mut running = tail;
    for (key, own) in chain.into_iter().rev() {
        running = running.max(own);
        cache.insert(key, (cb, running));
    }
    running
}

/// CSS 2.1 §8.3.1 — two adjoining margins: both non-negative → the larger, both
/// negative → the more negative, mixed → their sum.
pub(super) fn adjoin(a: f32, b: f32) -> f32 {
    if a >= 0.0 && b >= 0.0 {
        a.max(b)
    } else if a < 0.0 && b < 0.0 {
        a.min(b)
    } else {
        a + b
    }
}

/// A block whose own start and end margins adjoin (empty, no block-size, no
/// padding/border, no clearance): the margins collapse *through* it.
pub(super) fn collapses_through(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::Block)
        && !establishes_bfc(b)
        && b.style.clear == ClearSide::None
        && b.style.width.is_none()
        && !has_in_flow_content(b)
        && b.rect.width.abs() < 0.01
}
