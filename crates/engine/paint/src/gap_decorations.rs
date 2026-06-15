//! CSS Gap Decorations L1 — visual rules rendered inside flex/grid/multicol gaps.
//!
//! Phase 0: geometry and emit logic.
//! Phase 1 (P4): wire `gap-rule-width`, `gap-rule-style`, `gap-rule-color` from
//! `ComputedStyle` into `GapDecorationContext` and call `emit_gap_rules()`.

use lumen_core::geom::Rect;
use lumen_layout::{BorderStyle, Color, LayoutBox};

use crate::display_list::{CornerRadii, DisplayCommand};

/// Parameters for gap rule rendering.
///
/// P4 constructs this from `ComputedStyle` fields `gap-rule-width/style/color`
/// and passes it to [`emit_gap_rules`].
///
/// // CSS: gap-rule-width, gap-rule-style, gap-rule-color
pub struct GapDecorationContext {
    /// Thickness of the rule line in CSS px.
    pub rule_width: f32,
    /// Visual style of the rule (matches `<line-style>` grammar).
    pub rule_style: BorderStyle,
    /// Resolved rule color (currentColor already resolved).
    pub rule_color: Color,
}

/// One inter-cell gap in a flex, grid, or multicol layout.
///
/// Each `GapSegment` covers the full gap rectangle; `emit_gap_rules` centers
/// the rule within it.
pub struct GapSegment {
    /// The gap rectangle in layout coordinates (px from viewport top-left).
    /// For column gaps this spans the full container height; for row gaps
    /// it spans the full container width.
    pub rect: Rect,
    /// `true` → row gap (horizontal rule drawn between two rows).
    /// `false` → column gap (vertical rule drawn between two columns).
    pub horizontal: bool,
}

/// Emits [`DisplayCommand::DrawBorder`] entries for gap decorations between
/// flex/grid/multicol cells.
///
/// `_boxes` — positioned child boxes (reserved for Phase 1 gap-position
/// inference; currently ignored, gaps are passed explicitly).
/// `gaps` — gap segments to decorate.
/// `ctx` — decoration context (width, style, color).
///
/// Returns an empty `Vec` when:
/// - `ctx.rule_style` is `BorderStyle::None`, or
/// - `ctx.rule_width` ≤ 0.
///
/// Rules are centered inside each gap rectangle. Thin gaps (smaller than the
/// rule width) are clamped so the rule stays within the gap bounds.
///
/// Phase 0: Solid/Dashed/Dotted are fully supported. Double and other styles
/// render as Solid (same behaviour as `emit_column_rules`).
pub fn emit_gap_rules(
    _boxes: &[LayoutBox],
    gaps: &[GapSegment],
    ctx: &GapDecorationContext,
) -> Vec<DisplayCommand> {
    if !ctx.rule_style.is_visible() || ctx.rule_width <= 0.0 {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(gaps.len());

    for gap in gaps {
        if gap.rect.width <= 0.0 || gap.rect.height <= 0.0 {
            continue;
        }

        let cmd = if gap.horizontal {
            // Row gap: draw a horizontal rule centered vertically in the gap.
            let rule_h = ctx.rule_width.min(gap.rect.height);
            let rule_y = gap.rect.y + (gap.rect.height - rule_h) * 0.5;
            // Emit as bottom-side only: rect.y = rule_y, rect.height = rule_h.
            // Renderer draws bottom side at rect.y + rect.height - widths[2].
            DisplayCommand::DrawBorder {
                rect: Rect::new(gap.rect.x, rule_y, gap.rect.width, rule_h),
                widths: [0.0, 0.0, rule_h, 0.0],
                colors: [
                    Color::TRANSPARENT,
                    Color::TRANSPARENT,
                    ctx.rule_color,
                    Color::TRANSPARENT,
                ],
                styles: [
                    BorderStyle::None,
                    BorderStyle::None,
                    ctx.rule_style,
                    BorderStyle::None,
                ],
                radii: CornerRadii::default(),
            }
        } else {
            // Column gap: draw a vertical rule centered horizontally in the gap.
            let rule_w = ctx.rule_width.min(gap.rect.width);
            let rule_x = gap.rect.x + (gap.rect.width - rule_w) * 0.5;
            // Emit as right-side only: rect.x = rule_x, rect.width = rule_w.
            // Renderer draws right side at rect.x + rect.width - widths[1].
            DisplayCommand::DrawBorder {
                rect: Rect::new(rule_x, gap.rect.y, rule_w, gap.rect.height),
                widths: [0.0, rule_w, 0.0, 0.0],
                colors: [
                    Color::TRANSPARENT,
                    ctx.rule_color,
                    Color::TRANSPARENT,
                    Color::TRANSPARENT,
                ],
                styles: [
                    BorderStyle::None,
                    ctx.rule_style,
                    BorderStyle::None,
                    BorderStyle::None,
                ],
                radii: CornerRadii::default(),
            }
        };

        out.push(cmd);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Rect;

    fn red() -> Color {
        Color { r: 255, g: 0, b: 0, a: 255 }
    }

    fn ctx(style: BorderStyle, width: f32) -> GapDecorationContext {
        GapDecorationContext { rule_width: width, rule_style: style, rule_color: red() }
    }

    fn col_gap(x: f32, y: f32, w: f32, h: f32) -> GapSegment {
        GapSegment { rect: Rect::new(x, y, w, h), horizontal: false }
    }

    fn row_gap(x: f32, y: f32, w: f32, h: f32) -> GapSegment {
        GapSegment { rect: Rect::new(x, y, w, h), horizontal: true }
    }

    #[test]
    fn gap_rule_none_style_emits_nothing() {
        let cmds = emit_gap_rules(&[], &[col_gap(10.0, 0.0, 20.0, 100.0)], &ctx(BorderStyle::None, 2.0));
        assert!(cmds.is_empty());
    }

    #[test]
    fn gap_rule_zero_width_emits_nothing() {
        let cmds = emit_gap_rules(&[], &[col_gap(10.0, 0.0, 20.0, 100.0)], &ctx(BorderStyle::Solid, 0.0));
        assert!(cmds.is_empty());
    }

    #[test]
    fn column_gap_emits_vertical_draw_border() {
        // gap rect: x=40, y=0, w=20, h=100; rule_width=2 → rule_x=49, rule_w=2
        let cmds = emit_gap_rules(&[], &[col_gap(40.0, 0.0, 20.0, 100.0)], &ctx(BorderStyle::Solid, 2.0));
        assert_eq!(cmds.len(), 1);
        if let DisplayCommand::DrawBorder { rect, widths, styles, .. } = &cmds[0] {
            // Centered in gap: x=40 + (20-2)/2 = 49
            assert!((rect.x - 49.0).abs() < 0.01, "rule_x={}", rect.x);
            assert!((rect.width - 2.0).abs() < 0.01);
            assert!((rect.height - 100.0).abs() < 0.01);
            // Right side only
            assert_eq!(widths[1], 2.0);
            assert_eq!(widths[0], 0.0);
            assert_eq!(styles[1], BorderStyle::Solid);
        } else {
            panic!("expected DrawBorder");
        }
    }

    #[test]
    fn row_gap_emits_horizontal_draw_border() {
        // gap rect: x=0, y=50, w=200, h=16; rule_width=2 → rule_y=57, rule_h=2
        let cmds = emit_gap_rules(&[], &[row_gap(0.0, 50.0, 200.0, 16.0)], &ctx(BorderStyle::Dashed, 2.0));
        assert_eq!(cmds.len(), 1);
        if let DisplayCommand::DrawBorder { rect, widths, styles, .. } = &cmds[0] {
            assert!((rect.y - 57.0).abs() < 0.01, "rule_y={}", rect.y);
            assert!((rect.height - 2.0).abs() < 0.01);
            assert!((rect.width - 200.0).abs() < 0.01);
            // Bottom side only
            assert_eq!(widths[2], 2.0);
            assert_eq!(widths[0], 0.0);
            assert_eq!(styles[2], BorderStyle::Dashed);
        } else {
            panic!("expected DrawBorder");
        }
    }

    #[test]
    fn multiple_gaps_emit_multiple_commands() {
        let gaps = vec![col_gap(20.0, 0.0, 10.0, 100.0), col_gap(60.0, 0.0, 10.0, 100.0)];
        let cmds = emit_gap_rules(&[], &gaps, &ctx(BorderStyle::Solid, 1.0));
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn rule_wider_than_gap_is_clamped_to_gap_width() {
        // rule_width=30 > gap.width=20 → rule_w clamped to 20, rule_x = gap.x (no centering offset)
        let cmds = emit_gap_rules(&[], &[col_gap(10.0, 0.0, 20.0, 100.0)], &ctx(BorderStyle::Solid, 30.0));
        assert_eq!(cmds.len(), 1);
        if let DisplayCommand::DrawBorder { rect, widths, .. } = &cmds[0] {
            assert!((rect.width - 20.0).abs() < 0.01);
            assert!((widths[1] - 20.0).abs() < 0.01);
        } else {
            panic!("expected DrawBorder");
        }
    }
}
