//! CSS Logical Properties and Values L1 — отображение логического имени
//! свойства на физическое по текущему writing-mode.
//!
//! Перенесено батчем SPLIT-ST7 из `crates/engine/layout/src/style.rs`
//! (анкер `fn resolve_logical_property`) без правок тел: функция и её
//! тест-модуль скопированы построчно.
//!
//! Пост-каскадная `resolve_logical_properties` перенесена сюда батчем
//! SPLIT-ST13 из `crates/engine/layout/src/style.rs` (анкер, следовавший
//! непосредственно за регионом ST-13's `style/adjust.rs`) без правок тела.

use crate::style::{ComputedStyle, Direction, Length, LengthOrAuto, Overflow, OverscrollBehavior, WritingMode};

/// Resolve CSS Logical Properties based on writing-mode.
///
/// Logical properties (CSS Logical Properties and Values L1) map to physical
/// properties depending on the writing mode:
/// - `inline-size` → `width` (horizontal) or `height` (vertical)
/// - `block-size` → `height` (horizontal) or `width` (vertical)
/// - `margin-inline` → `margin-left` / `margin-right` (ltr) or opposite (rtl)
/// - `inset-inline-start/end` → `left` / `right` or opposite depending on direction
///
/// Phase 0: simplified horizontal (LTR) only. Full vertical/RTL support in Phase 2.
/// Returns the physical property name to apply the value to.
/// Returns None if not a recognized logical property.
pub fn resolve_logical_property(logical_name: &str, writing_mode: &str) -> Option<&'static str> {
    // Phase 0: Only horizontal LTR. Other writing modes return None.
    if writing_mode != "horizontal-tb" && writing_mode != "horizontal" {
        return None;
    }

    match logical_name {
        // Block-direction properties (vertical in horizontal LTR).
        "block-size" => Some("height"),
        "min-block-size" => Some("min-height"),
        "max-block-size" => Some("max-height"),
        "margin-block" => Some("margin-top"), // would need split for full handling
        "margin-block-start" => Some("margin-top"),
        "margin-block-end" => Some("margin-bottom"),
        "padding-block" => Some("padding-top"), // would need split
        "padding-block-start" => Some("padding-top"),
        "padding-block-end" => Some("padding-bottom"),
        "border-block" => Some("border-top"), // simplified
        "border-block-start" => Some("border-top"),
        "border-block-end" => Some("border-bottom"),
        "inset-block" => Some("top"), // simplified
        "inset-block-start" => Some("top"),
        "inset-block-end" => Some("bottom"),

        // Inline-direction properties (horizontal in horizontal LTR).
        "inline-size" => Some("width"),
        "min-inline-size" => Some("min-width"),
        "max-inline-size" => Some("max-width"),
        "margin-inline" => Some("margin-left"), // would need split
        "margin-inline-start" => Some("margin-left"),
        "margin-inline-end" => Some("margin-right"),
        "padding-inline" => Some("padding-left"), // would need split
        "padding-inline-start" => Some("padding-left"),
        "padding-inline-end" => Some("padding-right"),
        "border-inline" => Some("border-left"), // simplified
        "border-inline-start" => Some("border-left"),
        "border-inline-end" => Some("border-right"),
        "inset-inline" => Some("left"), // simplified
        "inset-inline-start" => Some("left"),
        "inset-inline-end" => Some("right"),

        _ => None,
    }
}

/// CSS Logical Properties L1 — resolve logical properties to physical.
/// Depends on writing-mode to determine which physical properties correspond to inline/block axis.
/// Phase 0: horizontal-tb only (inline-start=left, inline-end=right, block-start=top, block-end=bottom).
pub(in crate::style) fn resolve_logical_properties(style: &mut ComputedStyle) {
    // In horizontal-tb writing mode (default, Phase 0):
    // inline-start = left, inline-end = right, block-start = top, block-end = bottom.
    // For other writing modes, mapping differs; Phase 1+ will implement full support.

    // CSS Logical Properties L1 §2 — inline-size / block-size (+ min-/max-) map
    // onto width/height by `writing-mode`: horizontal-tb → inline=width,
    // block=height; every vertical mode swaps the axes. `direction` does not
    // matter for sizes. Same "physical still unset" presence heuristic as the
    // rest of this module.
    let vertical_wm = matches!(
        style.writing_mode,
        WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr
    );
    let (inline_sz, block_sz) = (style.inline_size.clone(), style.block_size.clone());
    let (min_i, max_i, min_b, max_b) = match style.logical_min_max_sizes.as_deref() {
        Some(x) => (x.min_inline.clone(), x.max_inline.clone(), x.min_block.clone(), x.max_block.clone()),
        None => (None, None, None, None),
    };
    let (w, h, min_w, max_w, min_h, max_h) = if vertical_wm {
        (block_sz, inline_sz, min_b, max_b, min_i, max_i)
    } else {
        (inline_sz, block_sz, min_i, max_i, min_b, max_b)
    };
    if w.is_some() && style.width.is_none() {
        style.width = w;
    }
    if h.is_some() && style.height.is_none() {
        style.height = h;
    }
    if min_w.is_some() && style.min_width.is_none() {
        style.min_width = min_w;
    }
    if max_w.is_some() && style.max_width.is_none() {
        style.max_width = max_w;
    }
    if min_h.is_some() && style.min_height.is_none() {
        style.min_height = min_h;
    }
    if max_h.is_some() && style.max_height.is_none() {
        style.max_height = max_h;
    }

    // CSS Logical Properties L1 §4–§7 — inset / margin / padding / border-width,
    // `*-inline-*` and `*-block-*` → the physical side the element's own
    // `writing-mode` + `direction` map each flow-relative side onto.
    let [inline_start, inline_end, block_start, block_end] = flow_relative_sides(style);

    // inset-inline-* / inset-block-* → top/right/bottom/left.
    let insets = [
        style.inset_inline_start.clone(),
        style.inset_inline_end.clone(),
        style.inset_block_start.clone(),
        style.inset_block_end.clone(),
    ];
    for (v, side) in insets.into_iter().zip([inline_start, inline_end, block_start, block_end]) {
        let slot = match side {
            Side::Top => &mut style.top,
            Side::Right => &mut style.right,
            Side::Bottom => &mut style.bottom,
            Side::Left => &mut style.left,
        };
        if v != LengthOrAuto::Auto && *slot == LengthOrAuto::Auto {
            *slot = v;
        }
    }

    // margin-inline-* / margin-block-*.
    let margins = [
        style.margin_inline_start.clone(),
        style.margin_inline_end.clone(),
        style.margin_block_start.clone(),
        style.margin_block_end.clone(),
    ];
    for (v, side) in margins.into_iter().zip([inline_start, inline_end, block_start, block_end]) {
        let slot = match side {
            Side::Top => &mut style.margin_top,
            Side::Right => &mut style.margin_right,
            Side::Bottom => &mut style.margin_bottom,
            Side::Left => &mut style.margin_left,
        };
        if v != LengthOrAuto::ZERO && *slot == LengthOrAuto::ZERO {
            *slot = v;
        }
    }

    // padding-inline-* / padding-block-*.
    let paddings = [
        style.padding_inline_start.clone(),
        style.padding_inline_end.clone(),
        style.padding_block_start.clone(),
        style.padding_block_end.clone(),
    ];
    for (v, side) in paddings.into_iter().zip([inline_start, inline_end, block_start, block_end]) {
        let slot = match side {
            Side::Top => &mut style.padding_top,
            Side::Right => &mut style.padding_right,
            Side::Bottom => &mut style.padding_bottom,
            Side::Left => &mut style.padding_left,
        };
        if v != Length::Px(0.0) && *slot == Length::Px(0.0) {
            *slot = v;
        }
    }

    // border-inline-*-width / border-block-*-width.
    let borders = [
        style.border_inline_start_width,
        style.border_inline_end_width,
        style.border_block_start_width,
        style.border_block_end_width,
    ];
    for (v, side) in borders.into_iter().zip([inline_start, inline_end, block_start, block_end]) {
        let slot = match side {
            Side::Top => &mut style.border_top_width,
            Side::Right => &mut style.border_right_width,
            Side::Bottom => &mut style.border_bottom_width,
            Side::Left => &mut style.border_left_width,
        };
        if v > 0.0 && *slot == 0.0 {
            *slot = v;
        }
    }
}

/// A physical box side.
#[derive(Clone, Copy)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// CSS Writing Modes L3 §6.1 — the physical sides of `[inline-start,
/// inline-end, block-start, block-end]` for the element's own `writing-mode` and
/// `direction`. `sideways-lr` runs its inline axis bottom-to-top (Writing Modes
/// L4 §3.2), so there `ltr` starts at the bottom.
fn flow_relative_sides(style: &ComputedStyle) -> [Side; 4] {
    let rtl = style.direction == Direction::Rtl;
    match style.writing_mode {
        WritingMode::HorizontalTb => {
            if rtl {
                [Side::Right, Side::Left, Side::Top, Side::Bottom]
            } else {
                [Side::Left, Side::Right, Side::Top, Side::Bottom]
            }
        }
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            if rtl {
                [Side::Bottom, Side::Top, Side::Right, Side::Left]
            } else {
                [Side::Top, Side::Bottom, Side::Right, Side::Left]
            }
        }
        WritingMode::VerticalLr => {
            if rtl {
                [Side::Bottom, Side::Top, Side::Left, Side::Right]
            } else {
                [Side::Top, Side::Bottom, Side::Left, Side::Right]
            }
        }
        WritingMode::SidewaysLr => {
            if rtl {
                [Side::Top, Side::Bottom, Side::Left, Side::Right]
            } else {
                [Side::Bottom, Side::Top, Side::Left, Side::Right]
            }
        }
    }
}


/// CSS Overflow L3 §logical (BUG-505) — resolve `overflow-block`/
/// `overflow-inline` to the physical `overflow_x`/`overflow_y` pair.
///
/// Unlike `resolve_logical_property`'s two-axis-per-property split
/// (`margin-inline-start` and friends), `overflow-block`/`overflow-inline`
/// each map wholly onto one physical axis — but *which* axis flips with
/// `writing-mode`: in `horizontal-tb` the block axis is vertical
/// (`overflow-block` → `overflow-y`), in every vertical mode
/// (`vertical-rl`/`vertical-lr`/`sideways-rl`/`sideways-lr`) the block axis
/// is physically horizontal, so the mapping swaps (`overflow-block` →
/// `overflow-x`) — confirmed against `css/css-overflow/logical-overflow-
/// 001.html`. Same "physical field still at its default" presence
/// heuristic as `resolve_logical_properties` above (a later, lower-priority
/// physical declaration cannot be told apart from "never set" — accepted
/// simplification shared by every logical property in this module). Must
/// run before `coerce_overflow_axes` (`cascade.rs`) so the visible-vs-auto
/// adjustment sees the fully resolved physical pair, not a stale one.
pub(in crate::style) fn resolve_overflow_logical_properties(style: &mut ComputedStyle) {
    let vertical_wm = matches!(
        style.writing_mode,
        WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr
    );
    let (block_target, inline_target) = if vertical_wm {
        (&mut style.overflow_x, &mut style.overflow_y)
    } else {
        (&mut style.overflow_y, &mut style.overflow_x)
    };
    if style.overflow_block != Overflow::Visible && *block_target == Overflow::Visible {
        *block_target = style.overflow_block;
    }
    if style.overflow_inline != Overflow::Visible && *inline_target == Overflow::Visible {
        *inline_target = style.overflow_inline;
    }
}

/// CSS Overscroll Behavior L1 §2 (BUG-516) — resolve `overscroll-behavior-
/// block`/`overscroll-behavior-inline` to the physical `overscroll_behavior_x`/
/// `_y` pair. Same axis-swap shape as `resolve_overflow_logical_properties`
/// above: in `horizontal-tb` the block axis is vertical (`-block` → `_y`),
/// in every vertical writing mode the block axis is physically horizontal
/// (`-block` → `_x`). Same "physical field still at its default" presence
/// heuristic as the rest of this module.
pub(in crate::style) fn resolve_overscroll_behavior_logical_properties(style: &mut ComputedStyle) {
    let vertical_wm = matches!(
        style.writing_mode,
        WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr
    );
    let (block_target, inline_target) = if vertical_wm {
        (&mut style.overscroll_behavior_x, &mut style.overscroll_behavior_y)
    } else {
        (&mut style.overscroll_behavior_y, &mut style.overscroll_behavior_x)
    };
    if style.overscroll_behavior_block != OverscrollBehavior::Auto
        && *block_target == OverscrollBehavior::Auto
    {
        *block_target = style.overscroll_behavior_block;
    }
    if style.overscroll_behavior_inline != OverscrollBehavior::Auto
        && *inline_target == OverscrollBehavior::Auto
    {
        *inline_target = style.overscroll_behavior_inline;
    }
}

#[cfg(test)]
mod logical_properties_tests {
    use super::resolve_logical_property;

    #[test]
    fn resolve_inline_size() {
        assert_eq!(
            resolve_logical_property("inline-size", "horizontal-tb"),
            Some("width")
        );
    }

    #[test]
    fn resolve_block_size() {
        assert_eq!(
            resolve_logical_property("block-size", "horizontal-tb"),
            Some("height")
        );
    }

    #[test]
    fn resolve_margin_inline_start() {
        assert_eq!(
            resolve_logical_property("margin-inline-start", "horizontal"),
            Some("margin-left")
        );
    }

    #[test]
    fn resolve_padding_block_end() {
        assert_eq!(
            resolve_logical_property("padding-block-end", "horizontal-tb"),
            Some("padding-bottom")
        );
    }

    #[test]
    fn resolve_inset_inline() {
        assert_eq!(
            resolve_logical_property("inset-inline", "horizontal"),
            Some("left")
        );
    }

    #[test]
    fn resolve_unknown_property() {
        assert_eq!(resolve_logical_property("color", "horizontal"), None);
        assert_eq!(resolve_logical_property("display", "horizontal-tb"), None);
    }

    #[test]
    fn resolve_vertical_mode_unsupported() {
        // Phase 0: vertical modes return None
        assert_eq!(
            resolve_logical_property("inline-size", "vertical-rl"),
            None
        );
    }

    #[test]
    fn resolve_border_logical() {
        assert_eq!(
            resolve_logical_property("border-inline-start", "horizontal-tb"),
            Some("border-left")
        );
        assert_eq!(
            resolve_logical_property("border-block-start", "horizontal"),
            Some("border-top")
        );
    }

    #[test]
    fn resolve_min_max_size() {
        assert_eq!(
            resolve_logical_property("min-inline-size", "horizontal-tb"),
            Some("min-width")
        );
        assert_eq!(
            resolve_logical_property("max-block-size", "horizontal"),
            Some("max-height")
        );
    }
}
