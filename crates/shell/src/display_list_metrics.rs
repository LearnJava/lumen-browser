//! Building a display list in paint order, measuring one, and the split-view
//! placeholder list.
//!
//! `paint_ordered` is the crate's single entry point for turning a laid-out
//! tree into a display list with the CSS 2.1 Appendix E painting order;
//! `content_height_of` / `content_width_of` derive the scrollable extent from
//! the emitted display list (the shell has no other source for it — see the
//! `CLAUDE.md` note that a spacer painting nothing leaves `max_scroll()` at 0),
//! `next_dl_epoch` versions it, and `build_split_placeholder` synthesises a
//! minimal list for a hibernated tab whose real one was evicted. Moved out of
//! `main.rs` by the SPLIT track (batches SH-5 and SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

/// Полная высота контента в CSS px — `max(rect.y + rect.height)` по всем
/// rect-несущим командам display list-а. Используется для clamping-а scroll_y.
pub(crate) fn content_height_of(dl: &lumen_paint::DisplayList) -> f32 {
    use lumen_paint::DisplayCommand;
    let mut max_y = 0.0_f32;
    for cmd in dl {
        let r = match cmd {
            DisplayCommand::FillRect { rect, .. }
            | DisplayCommand::FillRoundedRect { rect, .. }
            | DisplayCommand::DrawBorder { rect, .. }
            | DisplayCommand::DrawText { rect, .. }
            | DisplayCommand::DrawImage { rect, .. }
            | DisplayCommand::LazyImageSlot { rect, .. }
            | DisplayCommand::DrawBackgroundImage { rect, .. }
            | DisplayCommand::DrawOutline { rect, .. }
            | DisplayCommand::DrawLinearGradient { rect, .. }
            | DisplayCommand::DrawRadialGradient { rect, .. }
            | DisplayCommand::DrawConicGradient { rect, .. }
            | DisplayCommand::PushClipRect { rect, .. }
            | DisplayCommand::PushClipRoundedRect { rect, .. }
            | DisplayCommand::PushMaskImage { rect, .. }
            | DisplayCommand::PushMaskLinearGradient { rect, .. }
            | DisplayCommand::PushMaskRadialGradient { rect, .. }
            | DisplayCommand::PushMaskConicGradient { rect, .. }
            | DisplayCommand::PushMaskLayer { rect, .. } => rect,
            DisplayCommand::DrawCrossFade { dest, .. } => dest,
            DisplayCommand::PopClip
            | DisplayCommand::PushClipPath { .. }
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PopOpacity
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PushTransform { .. }
            | DisplayCommand::PopTransform
            | DisplayCommand::PopMask
            | DisplayCommand::PopMaskLayer
            | DisplayCommand::DrawLayerSnapshot { .. }
            | DisplayCommand::PushFilter { .. }
            | DisplayCommand::PopFilter
            | DisplayCommand::PushBackdropFilter { .. }
            | DisplayCommand::PopBackdropFilter
            | DisplayCommand::BeginStickyLayer { .. }
            | DisplayCommand::EndStickyLayer
            | DisplayCommand::BeginFixedLayer
            | DisplayCommand::EndFixedLayer
            | DisplayCommand::PushScrollLayer { .. }
            | DisplayCommand::PopScrollLayer
            | DisplayCommand::DrawSvgPath { .. }
            | DisplayCommand::DrawSvgFill { .. }
            | DisplayCommand::DrawSvgStroke { .. }
            | DisplayCommand::DrawScrollbar { .. }
            | DisplayCommand::PageBreak
            | DisplayCommand::BoxModelOverlay { .. } => continue,
        };
        let bottom = r.y + r.height;
        if bottom > max_y {
            max_y = bottom;
        }
    }
    max_y
}

/// Полная ширина контента в CSS px — `max(rect.x + rect.width)` по всем
/// rect-несущим командам display list-а. Используется для clamping-а scroll_x.
pub(crate) fn content_width_of(dl: &lumen_paint::DisplayList) -> f32 {
    use lumen_paint::DisplayCommand;
    let mut max_x = 0.0_f32;
    for cmd in dl {
        let r = match cmd {
            DisplayCommand::FillRect { rect, .. }
            | DisplayCommand::FillRoundedRect { rect, .. }
            | DisplayCommand::DrawBorder { rect, .. }
            | DisplayCommand::DrawText { rect, .. }
            | DisplayCommand::DrawImage { rect, .. }
            | DisplayCommand::LazyImageSlot { rect, .. }
            | DisplayCommand::DrawBackgroundImage { rect, .. }
            | DisplayCommand::DrawOutline { rect, .. }
            | DisplayCommand::DrawLinearGradient { rect, .. }
            | DisplayCommand::DrawRadialGradient { rect, .. }
            | DisplayCommand::DrawConicGradient { rect, .. }
            | DisplayCommand::PushClipRect { rect, .. }
            | DisplayCommand::PushClipRoundedRect { rect, .. }
            | DisplayCommand::PushMaskImage { rect, .. }
            | DisplayCommand::PushMaskLinearGradient { rect, .. }
            | DisplayCommand::PushMaskRadialGradient { rect, .. }
            | DisplayCommand::PushMaskConicGradient { rect, .. }
            | DisplayCommand::PushMaskLayer { rect, .. } => rect,
            DisplayCommand::DrawCrossFade { dest, .. } => dest,
            DisplayCommand::PopClip
            | DisplayCommand::PushClipPath { .. }
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PopOpacity
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PushTransform { .. }
            | DisplayCommand::PopTransform
            | DisplayCommand::PopMask
            | DisplayCommand::PopMaskLayer
            | DisplayCommand::DrawLayerSnapshot { .. }
            | DisplayCommand::PushFilter { .. }
            | DisplayCommand::PopFilter
            | DisplayCommand::PushBackdropFilter { .. }
            | DisplayCommand::PopBackdropFilter
            | DisplayCommand::BeginStickyLayer { .. }
            | DisplayCommand::EndStickyLayer
            | DisplayCommand::BeginFixedLayer
            | DisplayCommand::EndFixedLayer
            | DisplayCommand::PushScrollLayer { .. }
            | DisplayCommand::PopScrollLayer
            | DisplayCommand::DrawSvgPath { .. }
            | DisplayCommand::DrawSvgFill { .. }
            | DisplayCommand::DrawSvgStroke { .. }
            | DisplayCommand::DrawScrollbar { .. }
            | DisplayCommand::PageBreak
            | DisplayCommand::BoxModelOverlay { .. } => continue,
        };
        let right = r.x + r.width;
        if right > max_x {
            max_x = right;
        }
    }
    max_x
}

/// Build a minimal placeholder display list for a hibernated tab in split view.
///
/// Shows a dark grey background with the URL text — used when the hibernated
/// tab's full display list has been evicted from memory.
pub(crate) fn build_split_placeholder(url: &str) -> lumen_paint::DisplayList {
    use lumen_layout::{Color, FontStyle, FontWeight};
    use lumen_paint::DisplayCommand;

    let bg = Color { r: 30, g: 30, b: 35, a: 255 };
    let fg = Color { r: 180, g: 180, b: 190, a: 255 };
    vec![
        // Background fill — large enough to cover any viewport half.
        DisplayCommand::FillRect {
            rect: lumen_core::geom::Rect { x: 0.0, y: 0.0, width: 4096.0, height: 4096.0 },
            color: bg,
        },
        // URL label near vertical centre of a typical viewport half.
        DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: lumen_core::geom::Rect { x: 16.0, y: 300.0, width: 480.0, height: 20.0 },
            text: url.to_owned(),
            font_size: 13.0,
            color: fg,
            font_family: vec![],
            font_weight: FontWeight(400),
            font_style: FontStyle::Normal,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: None,
        },
    ]
}

/// Строит display list с правильным painting order (CSS 2.1 Appendix E, z-index stacking).
pub(crate) fn paint_ordered(layout: &lumen_layout::LayoutBox) -> DisplayList {
    let tree = StackingTree::build(layout);
    let order = PaintOrder::from_tree(&tree);
    build_display_list_ordered(layout, &tree, &order).0
}

/// Следующая версия display list-а; `0` пропускается — он зарезервирован за
/// «версия неизвестна» (BUG-405 срез 39).
pub(crate) fn next_dl_epoch(cur: u64) -> u64 {
    match cur.wrapping_add(1) {
        0 => 1,
        n => n,
    }
}
