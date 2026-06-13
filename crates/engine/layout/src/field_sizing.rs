/// CSS Basic User Interface L4 §4.4 — `field-sizing: content` intrinsic sizing algorithm.
///
/// When a form control carries `field-sizing: content`, its dimensions come from the
/// text it contains rather than from fixed UA defaults.  This module contains the
/// pure measurement algorithm that P4 wires to the CSS property.
///
/// # CSS: field-sizing (P4 wiring instructions)
///
/// P4 must:
/// 1. Add `pub field_sizing: FieldSizing` to `ComputedStyle` (default `FieldSizing::Fixed`).
/// 2. Add `FieldSizing` enum: `Fixed` (UA defaults apply) / `Content` (this module).
/// 3. Parse `field-sizing: content | fixed` in `apply_declaration`.
/// 4. In `apply_ua_form_controls`: skip assigning `style.width` / `style.height`
///    when `style.field_sizing == FieldSizing::Content` for `"input"` / `"textarea"`.
/// 5. In `lay_out_box`, at the `is_replaced && s.width.is_none()` branch, call
///    `field_sizing_content_intrinsic` and use the result as the border-box size.
///    The `value_text` for `<input>` is `doc.get(node).get_attr("value").unwrap_or("")`;
///    for `<textarea>` it is the concatenated text content of child text nodes.
use crate::box_tree::measure_text_w;
use crate::TextMeasurer;

/// Minimum intrinsic width for a content-sized form control (avoids zero-width fields).
const FIELD_MIN_WIDTH_PX: f32 = 10.0;
/// UA inner horizontal padding applied on each side for text-entry controls.
const FIELD_HORIZ_PAD_PX: f32 = 3.0;
/// UA inner vertical padding applied on each side for text-entry controls.
const FIELD_VERT_PAD_PX: f32 = 1.0;

/// Computes content-based intrinsic dimensions for an HTML form control under
/// `field-sizing: content` (CSS Basic UI L4 §4.4).
///
/// Returns `(padding_box_width, padding_box_height)`.  The caller adds border
/// widths to obtain the full border-box dimensions.
///
/// Sizing rules:
/// - `"input"`: `width = max(FIELD_MIN_WIDTH_PX, text_width + 2 * FIELD_HORIZ_PAD_PX)`;
///   `height = line_height_px + 2 * FIELD_VERT_PAD_PX`.
/// - `"textarea"`: `width = max(FIELD_MIN_WIDTH_PX, max_line_width + 2 * FIELD_HORIZ_PAD_PX)`;
///   `height = line_count * line_height_px + 2 * FIELD_VERT_PAD_PX`.
/// - All other tags: `(0.0, 0.0)` — not a text-entry control.
///
/// `value_text` for `<input>` is the `value` attribute; for `<textarea>` it is the
/// text content (lines delimited by `'\n'`).
///
/// `line_height_px` should be the resolved CSS `line-height` for the element
/// (typically `style.font_size * 1.2` when `line-height: normal`).
pub fn field_sizing_content_intrinsic(
    tag: &str,
    value_text: &str,
    font_size_px: f32,
    line_height_px: f32,
    m: &dyn TextMeasurer,
) -> (f32, f32) {
    match tag {
        "input" => {
            let text_w = measure_text_w(value_text, font_size_px, 0.0, 0.0, m);
            let width = (text_w + 2.0 * FIELD_HORIZ_PAD_PX).max(FIELD_MIN_WIDTH_PX);
            let height = line_height_px + 2.0 * FIELD_VERT_PAD_PX;
            (width, height)
        }
        "textarea" => {
            let lines: Vec<&str> = value_text.split('\n').collect();
            let max_line_w = lines
                .iter()
                .map(|line| measure_text_w(line, font_size_px, 0.0, 0.0, m))
                .fold(0.0_f32, f32::max);
            let line_count = lines.len().max(1) as f32;
            let width = (max_line_w + 2.0 * FIELD_HORIZ_PAD_PX).max(FIELD_MIN_WIDTH_PX);
            let height = line_count * line_height_px + 2.0 * FIELD_VERT_PAD_PX;
            (width, height)
        }
        _ => (0.0, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stub: each character is exactly `font_size_px * 0.5` wide (half-em monospace).
    struct HalfEmMeasurer;
    impl TextMeasurer for HalfEmMeasurer {
        fn char_width(&self, _ch: char, font_size_px: f32) -> f32 {
            font_size_px * 0.5
        }
    }

    const FONT: f32 = 16.0; // px
    const LH: f32 = 19.2;   // line-height: normal ≈ 1.2 × font-size
    const CH: f32 = 8.0;    // half-em at 16px

    #[test]
    fn test_input_empty_value_returns_minimum_width() {
        let (w, h) = field_sizing_content_intrinsic("input", "", FONT, LH, &HalfEmMeasurer);
        // text_w = 0 → clamped to FIELD_MIN_WIDTH_PX
        assert_eq!(w, FIELD_MIN_WIDTH_PX);
        assert!((h - (LH + 2.0 * FIELD_VERT_PAD_PX)).abs() < 0.01);
    }

    #[test]
    fn test_input_short_value_adds_padding() {
        // "ab" = 2 chars × 8px = 16px content, + 6px padding = 22px
        let (w, h) = field_sizing_content_intrinsic("input", "ab", FONT, LH, &HalfEmMeasurer);
        let expected_w = 2.0 * CH + 2.0 * FIELD_HORIZ_PAD_PX;
        assert!((w - expected_w).abs() < 0.01, "width {w} != {expected_w}");
        assert!((h - (LH + 2.0 * FIELD_VERT_PAD_PX)).abs() < 0.01);
    }

    #[test]
    fn test_textarea_single_line() {
        // "hello" = 5 chars × 8px = 40px, 1 line
        let (w, h) = field_sizing_content_intrinsic("textarea", "hello", FONT, LH, &HalfEmMeasurer);
        let expected_w = 5.0 * CH + 2.0 * FIELD_HORIZ_PAD_PX;
        let expected_h = 1.0 * LH + 2.0 * FIELD_VERT_PAD_PX;
        assert!((w - expected_w).abs() < 0.01, "width {w} != {expected_w}");
        assert!((h - expected_h).abs() < 0.01, "height {h} != {expected_h}");
    }

    #[test]
    fn test_textarea_multi_line_uses_widest_line() {
        // "hi\nhello" → lines ["hi"(16px), "hello"(40px)]; max = 40px; 2 lines
        let (w, h) = field_sizing_content_intrinsic("textarea", "hi\nhello", FONT, LH, &HalfEmMeasurer);
        let expected_w = 5.0 * CH + 2.0 * FIELD_HORIZ_PAD_PX; // "hello" is widest
        let expected_h = 2.0 * LH + 2.0 * FIELD_VERT_PAD_PX;
        assert!((w - expected_w).abs() < 0.01, "width {w} != {expected_w}");
        assert!((h - expected_h).abs() < 0.01, "height {h} != {expected_h}");
    }

    #[test]
    fn test_unknown_tag_returns_zero() {
        let (w, h) = field_sizing_content_intrinsic("div", "some text", FONT, LH, &HalfEmMeasurer);
        assert_eq!(w, 0.0);
        assert_eq!(h, 0.0);
    }
}
