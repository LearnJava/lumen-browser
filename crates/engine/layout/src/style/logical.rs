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

use crate::style::{ComputedStyle, Length, LengthOrAuto};

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

    // CSS Logical Properties L1 §2 — inline-size / block-size → width / height.
    if style.inline_size.is_some() && style.width.is_none() {
        style.width = style.inline_size.clone();
    }
    if style.block_size.is_some() && style.height.is_none() {
        style.height = style.block_size.clone();
    }

    // CSS Logical Properties L1 §4 — inset-inline-* / inset-block-* → top/right/bottom/left.
    // Phase 0: horizontal-tb (inline-start=left, inline-end=right).
    if style.inset_inline_start != LengthOrAuto::Auto && style.left == LengthOrAuto::Auto {
        style.left = style.inset_inline_start.clone();
    }
    if style.inset_inline_end != LengthOrAuto::Auto && style.right == LengthOrAuto::Auto {
        style.right = style.inset_inline_end.clone();
    }
    if style.inset_block_start != LengthOrAuto::Auto && style.top == LengthOrAuto::Auto {
        style.top = style.inset_block_start.clone();
    }
    if style.inset_block_end != LengthOrAuto::Auto && style.bottom == LengthOrAuto::Auto {
        style.bottom = style.inset_block_end.clone();
    }

    // CSS Logical Properties L1 §5 — margin-inline-* / margin-block-* → margin-left/right/top/bottom.
    if style.margin_inline_start != LengthOrAuto::ZERO && style.margin_left == LengthOrAuto::ZERO {
        style.margin_left = style.margin_inline_start.clone();
    }
    if style.margin_inline_end != LengthOrAuto::ZERO && style.margin_right == LengthOrAuto::ZERO {
        style.margin_right = style.margin_inline_end.clone();
    }
    if style.margin_block_start != LengthOrAuto::ZERO && style.margin_top == LengthOrAuto::ZERO {
        style.margin_top = style.margin_block_start.clone();
    }
    if style.margin_block_end != LengthOrAuto::ZERO && style.margin_bottom == LengthOrAuto::ZERO {
        style.margin_bottom = style.margin_block_end.clone();
    }

    // CSS Logical Properties L1 §6 — padding-inline-* / padding-block-* → padding-left/right/top/bottom.
    if style.padding_inline_start != Length::Px(0.0) && style.padding_left == Length::Px(0.0) {
        style.padding_left = style.padding_inline_start.clone();
    }
    if style.padding_inline_end != Length::Px(0.0) && style.padding_right == Length::Px(0.0) {
        style.padding_right = style.padding_inline_end.clone();
    }
    if style.padding_block_start != Length::Px(0.0) && style.padding_top == Length::Px(0.0) {
        style.padding_top = style.padding_block_start.clone();
    }
    if style.padding_block_end != Length::Px(0.0) && style.padding_bottom == Length::Px(0.0) {
        style.padding_bottom = style.padding_block_end.clone();
    }

    // CSS Logical Properties L1 §7 — border-inline-*-width / border-block-*-width.
    if style.border_inline_start_width > 0.0 && style.border_left_width == 0.0 {
        style.border_left_width = style.border_inline_start_width;
    }
    if style.border_inline_end_width > 0.0 && style.border_right_width == 0.0 {
        style.border_right_width = style.border_inline_end_width;
    }
    if style.border_block_start_width > 0.0 && style.border_top_width == 0.0 {
        style.border_top_width = style.border_block_start_width;
    }
    if style.border_block_end_width > 0.0 && style.border_bottom_width == 0.0 {
        style.border_bottom_width = style.border_block_end_width;
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
