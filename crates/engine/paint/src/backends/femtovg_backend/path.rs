//! Vector path drawing: solid/rounded rects, border sides, rounded border ring.
//!
//! Вырезано из `femtovg_backend.rs` (SPLIT-PR2 срез 2/4) без изменения
//! поведения — чисто перенос кода между модулями.

use super::*;

/// Appends a closed rounded-rectangle contour to `path` using cubic-Bézier
/// quarter-ellipse corners (kappa ≈ 0.5523). `radii` carries per-corner (x, y)
/// radii and is assumed already clamped to the box. Shared by the border ring
/// (BUG-175) so both outer and inner contours use identical corner geometry.
fn append_rounded_rect_outline(
    path: &mut femtovg::Path,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radii: &CornerRadii,
) {
    const K: f32 = 0.5523;
    let (tl_x, tl_y) = (radii.tl, radii.tl_y);
    let (tr_x, tr_y) = (radii.tr, radii.tr_y);
    let (br_x, br_y) = (radii.br, radii.br_y);
    let (bl_x, bl_y) = (radii.bl, radii.bl_y);

    path.move_to(x + tl_x, y);
    path.line_to(x + w - tr_x, y);
    path.bezier_to(
        x + w - tr_x + K * tr_x, y,
        x + w,                   y + tr_y - K * tr_y,
        x + w,                   y + tr_y,
    );
    path.line_to(x + w, y + h - br_y);
    path.bezier_to(
        x + w,                   y + h - br_y + K * br_y,
        x + w - br_x + K * br_x, y + h,
        x + w - br_x,            y + h,
    );
    path.line_to(x + bl_x, y + h);
    path.bezier_to(
        x + bl_x - K * bl_x, y + h,
        x,                   y + h - bl_y + K * bl_y,
        x,                   y + h - bl_y,
    );
    path.line_to(x, y + tl_y);
    path.bezier_to(
        x,                   y + tl_y - K * tl_y,
        x + tl_x - K * tl_x, y,
        x + tl_x,            y,
    );
    path.close();
}

impl FemtovgBackend {
    /// Рисует залитый прямоугольник.
    pub(super) fn draw_fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let mut path = femtovg::Path::new();
        path.rect(x, y, w, h);
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
    }

    /// Fills a circle (used for dotted borders wider than 2px, where Edge renders
    /// round dots rather than squares).
    fn draw_fill_circle(&mut self, cx: f32, cy: f32, r: f32, color: Color) {
        let mut path = femtovg::Path::new();
        path.circle(cx, cy, r);
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
    }

    /// Renders one border side (top/right/bottom/left) honoring its `BorderStyle`.
    /// `horizontal` = true for top/bottom (pattern runs along X), false for
    /// left/right (along Y). `width` is the side thickness in CSS px. Geometry
    /// mirrors the wgpu `emit_border_side` so the femtovg (default) backend draws
    /// the same dash/dot/double pattern Edge produces (BUG-080). Solid/None fall
    /// back to a single filled quad — unchanged from the previous behavior.
    pub(super) fn draw_border_side(
        &mut self,
        side_rect: Rect,
        horizontal: bool,
        width: f32,
        color: Color,
        style: BorderStyle,
    ) {
        let total = if horizontal { side_rect.width } else { side_rect.height };
        match style {
            BorderStyle::Dashed => {
                for (offset, len) in dashed_border_offsets(total, width) {
                    if horizontal {
                        self.draw_fill_rect(side_rect.x + offset, side_rect.y, len, side_rect.height, color);
                    } else {
                        self.draw_fill_rect(side_rect.x, side_rect.y + offset, side_rect.width, len, color);
                    }
                }
            }
            BorderStyle::Dotted => {
                // dot_len ≤ 2px → squares (no AA circle); otherwise round dots.
                let use_rect = width.max(1.0) <= 2.0;
                for (offset, len) in dotted_border_offsets(total, width) {
                    if use_rect {
                        if horizontal {
                            self.draw_fill_rect(side_rect.x + offset, side_rect.y, len, side_rect.height, color);
                        } else {
                            self.draw_fill_rect(side_rect.x, side_rect.y + offset, side_rect.width, len, color);
                        }
                    } else if horizontal {
                        let cx = side_rect.x + offset + len / 2.0;
                        let cy = side_rect.y + side_rect.height / 2.0;
                        self.draw_fill_circle(cx, cy, side_rect.height / 2.0, color);
                    } else {
                        let cx = side_rect.x + side_rect.width / 2.0;
                        let cy = side_rect.y + offset + len / 2.0;
                        self.draw_fill_circle(cx, cy, side_rect.width / 2.0, color);
                    }
                }
            }
            BorderStyle::Double => {
                // CSS Backgrounds L3 §4.2: two solid lines ~1/3 width, gap ~1/3.
                // width < 3px → no room for a gap, fall back to solid.
                if width < 3.0 {
                    self.draw_fill_rect(side_rect.x, side_rect.y, side_rect.width, side_rect.height, color);
                    return;
                }
                let line = (width / 3.0).max(1.0);
                if horizontal {
                    self.draw_fill_rect(side_rect.x, side_rect.y, side_rect.width, line, color);
                    self.draw_fill_rect(side_rect.x, side_rect.y + width - line, side_rect.width, line, color);
                } else {
                    self.draw_fill_rect(side_rect.x, side_rect.y, line, side_rect.height, color);
                    self.draw_fill_rect(side_rect.x + width - line, side_rect.y, line, side_rect.height, color);
                }
            }
            BorderStyle::Solid | BorderStyle::None => {
                self.draw_fill_rect(side_rect.x, side_rect.y, side_rect.width, side_rect.height, color);
            }
        }
    }

    /// Рисует залитый прямоугольник с разными радиусами углов.
    /// Draw a rounded rectangle with per-corner elliptical radii.
    ///
    /// When `rx == ry` for all corners the path is identical to the circular case.
    /// For elliptical corners (rx ≠ ry) we use cubic Bézier approximation of a
    /// quarter-ellipse with the Geng–Zwart kappa constant ≈ 0.5523.
    pub(super) fn draw_fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: CornerRadii,
        color: Color,
    ) {
        // Kappa constant for cubic Bézier approximation of a quarter-circle/ellipse.
        const K: f32 = 0.5523;

        // Clamp radii via CSS Backgrounds §5.5 (single scale factor over all
        // corners), preserving elliptical corners (rx ≠ ry). The previous
        // per-radius `min(w/2, h/2)` cap collapsed a wide SVG `<ellipse>` into a
        // circle → stadium shape instead of an ellipse (BUG-198).
        let clamped = radii.clamped_to_box(w, h);
        let (tl_x, tl_y) = (clamped.tl, clamped.tl_y);
        let (tr_x, tr_y) = (clamped.tr, clamped.tr_y);
        let (br_x, br_y) = (clamped.br, clamped.br_y);
        let (bl_x, bl_y) = (clamped.bl, clamped.bl_y);

        // Fast path: all corners circular — delegate to femtovg built-in.
        if (tl_x - tl_y).abs() < 0.5 && (tr_x - tr_y).abs() < 0.5
            && (br_x - br_y).abs() < 0.5 && (bl_x - bl_y).abs() < 0.5
        {
            let mut path = femtovg::Path::new();
            path.rounded_rect_varying(x, y, w, h, tl_x, tr_x, br_x, bl_x);
            let paint = femtovg::Paint::color(lumen_to_fvg(color));
            self.canvas.fill_path(&path, &paint);
            return;
        }

        // Elliptical path: build manually with cubic Bézier corners.
        let mut path = femtovg::Path::new();
        // Start at top-left corner's right end.
        path.move_to(x + tl_x, y);
        // Top edge → top-right corner.
        path.line_to(x + w - tr_x, y);
        path.bezier_to(
            x + w - tr_x + K * tr_x, y,
            x + w,                   y + tr_y - K * tr_y,
            x + w,                   y + tr_y,
        );
        // Right edge → bottom-right corner.
        path.line_to(x + w, y + h - br_y);
        path.bezier_to(
            x + w,                    y + h - br_y + K * br_y,
            x + w - br_x + K * br_x, y + h,
            x + w - br_x,             y + h,
        );
        // Bottom edge → bottom-left corner.
        path.line_to(x + bl_x, y + h);
        path.bezier_to(
            x + bl_x - K * bl_x, y + h,
            x,                   y + h - bl_y + K * bl_y,
            x,                   y + h - bl_y,
        );
        // Left edge → top-left corner.
        path.line_to(x, y + tl_y);
        path.bezier_to(
            x,             y + tl_y - K * tl_y,
            x + tl_x - K * tl_x, y,
            x + tl_x,     y,
        );
        path.close();
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
    }

    /// Draws a uniform-coloured solid border whose corners follow `border-radius`
    /// (BUG-175). The border is the even-odd ring between the outer rounded-rect
    /// (border box, outer radii) and the inner rounded-rect (padding box, inner
    /// radii = outer − side width per CSS Backgrounds L3 §5.5). `widths` is the
    /// per-side width `[top, right, bottom, left]`; when the border is thicker
    /// than the box (no inner area) the whole rounded box is filled.
    pub(super) fn draw_rounded_border_ring(
        &mut self,
        rect: Rect,
        widths: [f32; 4],
        color: Color,
        radii: CornerRadii,
    ) {
        let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let [top, right, bottom, left] = widths;

        // Outer contour: same clamp the background fill uses, so the border's
        // outer edge coincides with the rounded background edge.
        let outer = radii.clamped_to_box(w, h);

        let mut path = femtovg::Path::new();
        append_rounded_rect_outline(&mut path, x, y, w, h, &outer);

        // Inner contour (padding box). Skip the hole when the border swallows the
        // whole box — then the ring degenerates to a solid rounded rect.
        let iw = w - left - right;
        let ih = h - top - bottom;
        if iw > 0.0 && ih > 0.0 {
            let inner = radii.inner_for_border(widths).clamped_to_box(iw, ih);
            append_rounded_rect_outline(&mut path, x + left, y + top, iw, ih, &inner);
        }

        let paint = femtovg::Paint::color(lumen_to_fvg(color))
            .with_fill_rule(femtovg::FillRule::EvenOdd);
        self.canvas.fill_path(&path, &paint);
    }
}
