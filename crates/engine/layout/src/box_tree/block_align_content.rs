//! CSS Box Alignment L3 §«Block Containers» — `align-content` of a block container.
//!
//! A block container whose `align-content` is not `normal` is an independent formatting context
//! (its margins stay inside) and its in-flow content moves along the block axis as one piece:
//! `start`/`end`/`center` take the free space between the content and the content box.

use super::*;

/// Does `align-content` of this block container differ from `normal`? Then the box is a BFC
/// root (see [`super::bfc::establishes_bfc`]) and [`shift_content`] applies. Not handled here:
/// flex and grid containers (they read the property themselves), a table cell (it aligns through
/// [`super::table_valign`]) and a vertical writing mode (`vertical_trampoline`).
pub(super) fn applies(s: &ComputedStyle) -> bool {
    !matches!(s.align_content, AlignValue::Auto | AlignValue::Normal)
        && !matches!(
            s.display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid | Display::TableCell
        )
        && s.writing_mode == crate::style::WritingMode::HorizontalTb
}

/// Distance the content moves down, for `free` = content-box height minus content height.
/// `baseline` falls back to `start`, `last baseline` to `end`; the distributions have no items to
/// space out: `stretch` and `space-between` fall back to `start`, `space-around` and
/// `space-evenly` to `safe center`. Overflowing content stays at the start unless
/// the value says `unsafe` (the default overflow alignment of a block container, CSS Box
/// Alignment L3 §4.4).
fn offset(s: &ComputedStyle, free: f32) -> f32 {
    let distributed = matches!(s.align_content, AlignValue::SpaceAround | AlignValue::SpaceEvenly);
    let safe = distributed || !s.content_align_extra.align_unsafe;
    if free < 0.0 && safe {
        return 0.0;
    }
    match s.align_content {
        AlignValue::Center | AlignValue::SpaceAround | AlignValue::SpaceEvenly => free / 2.0,
        AlignValue::End | AlignValue::LastBaseline => free,
        _ => 0.0,
    }
}

/// Moves the in-flow children of the finished block `b` (and the static positions of its
/// deferred abspos children) by the `align-content` offset. `content_height` is the height of
/// the flow inside the content box, `abs_deferred` the `(child, static_x, static_y)` list.
pub(super) fn shift_content(
    b: &mut LayoutBox,
    s: &ComputedStyle,
    (padding_top, padding_bottom): (f32, f32),
    content_height: f32,
    abs_deferred: &mut [(usize, f32, f32)],
) {
    if !applies(s) {
        return;
    }
    let frame_h = padding_top + padding_bottom + s.border_top_width + s.border_bottom_width;
    let dy = offset(s, b.rect.height - frame_h - content_height);
    if dy.abs() < 0.01 {
        return;
    }
    for child in &mut b.children {
        if !matches!(child.style.position, Position::Absolute | Position::Fixed) {
            shift_tree(child, 0.0, dy);
        }
    }
    for (_, _, y) in abs_deferred {
        *y += dy;
    }
}
