//! P1/SPLIT-DL1: код после `mod tests` из `display_list.rs` — CSS Custom
//! Highlight API L1 хелпер. Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-1).

use super::*;

/// CSS Custom Highlight API L1 — helper to emit DrawText with highlight name.
/// Phase 0: stores highlight name in DrawText for future rendering.
/// Phase 1: will fetch ranges from CSS.highlights and emit overlay rects.
#[allow(clippy::too_many_arguments)]
pub fn emit_text_with_highlights(
    rect: Rect,
    text: &str,
    font_size: f32,
    color: Color,
    font_family: Vec<String>,
    font_weight: FontWeight,
    font_style: FontStyle,
    font_stretch: FontStretch,
    font_variation_axes: Vec<([u8; 4], f32)>,
    font_features: Vec<([u8; 4], u32)>,
    tab_size: f32,
    highlight_name: Option<String>,
    text_orientation: Option<TextOrientation>,
    out: &mut DisplayList,
) {
    out.push(DisplayCommand::DrawText {
        font_stretch,
        rect,
        text: text.to_string(),
        font_size,
        color,
        font_family,
        font_weight,
        font_style,
        font_variation_axes,
        font_features,
        font_palette: None,
        tab_size,
        highlight_name,
        text_orientation,
    });
}
