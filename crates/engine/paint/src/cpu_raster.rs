//! CPU-based rasterization using tiny-skia for deterministic pixel output on CI.
//!
//! Available only with feature="cpu-render"; no GPU dependencies, fully deterministic
//! across Windows/macOS/Linux.

use lumen_image::Image;
use lumen_layout::{Color, GradientStop, Length};
use crate::{DisplayCommand, CornerRadii};
use lumen_core::geom::Rect;

/// Rasterize display commands to an image using tiny-skia (CPU only, deterministic).
pub(crate) fn rasterize_cpu(
    width: u32,
    height: u32,
    commands: &[DisplayCommand],
    _scroll_x: f32,
    _scroll_y: f32,
) -> Result<Image, Box<dyn std::error::Error>> {
    use tiny_skia::Pixmap;

    let mut pixmap = Pixmap::new(width, height)
        .ok_or("Failed to create pixmap")?;

    // Fill background with white.
    pixmap.fill(tiny_skia::Color::from_rgba8(255, 255, 255, 255));

    for cmd in commands {
        match cmd {
            DisplayCommand::FillRect { rect, color } => {
                rasterize_fill_rect(&mut pixmap, rect, color)?;
            }
            DisplayCommand::FillRoundedRect { rect, color, radii } => {
                rasterize_fill_rounded_rect(&mut pixmap, rect, color, radii)?;
            }
            DisplayCommand::DrawBorder { rect, widths, colors, styles: _, radii } => {
                rasterize_draw_border(&mut pixmap, rect, widths, colors, radii)?;
            }
            DisplayCommand::DrawOutline { rect, width, style: _, color, offset } => {
                rasterize_draw_outline(&mut pixmap, rect, *width, color, *offset)?;
            }
            DisplayCommand::DrawLinearGradient { rect, angle_deg, stops, repeating } => {
                rasterize_linear_gradient(&mut pixmap, rect, *angle_deg, stops, *repeating)?;
            }
            DisplayCommand::DrawRadialGradient { rect, center_x_pct, center_y_pct, stops, repeating } => {
                rasterize_radial_gradient(
                    &mut pixmap, rect, *center_x_pct, *center_y_pct, stops, *repeating,
                )?;
            }
            DisplayCommand::DrawSvgPath { vertices, color } => {
                rasterize_svg_path(&mut pixmap, vertices, color)?;
            }
            // Remaining commands not implemented for CPU rasterization yet.
            _ => {
                // Skipped for now; will be implemented in later phases.
            }
        }
    }

    let data = pixmap.data().to_vec();
    Ok(Image {
        width,
        height,
        format: lumen_image::PixelFormat::Rgba8,
        data,
        icc_profile: None,
    })
}

fn rasterize_fill_rect(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    color: &Color,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::Paint;

    let paint = Paint {
        shader: tiny_skia::Shader::SolidColor(color_to_skia(*color)),
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };

    let skia_rect = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)
        .ok_or("Invalid rect dimensions")?;

    pixmap.fill_rect(skia_rect, &paint, tiny_skia::Transform::identity(), None);
    Ok(())
}

fn rasterize_fill_rounded_rect(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    color: &Color,
    radii: &CornerRadii,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::Paint;

    let paint = Paint {
        shader: tiny_skia::Shader::SolidColor(color_to_skia(*color)),
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };

    let mut pb = tiny_skia::PathBuilder::new();

    // Build rounded rect path: start from top-left, go clockwise.
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;

    let tl_x = radii.tl;
    let tl_y = radii.tl_y;
    let tr_x = radii.tr;
    let tr_y = radii.tr_y;
    let br_x = radii.br;
    let br_y = radii.br_y;
    let bl_x = radii.bl;
    let bl_y = radii.bl_y;

    // Top-left corner.
    pb.move_to(x0 + tl_x, y0);
    // Top edge.
    pb.line_to(x1 - tr_x, y0);
    // Top-right corner (use Bézier curve for rounded corner).
    pb.cubic_to(
        x1 - tr_x * 0.55,
        y0,
        x1,
        y0 + tr_y * 0.55,
        x1,
        y0 + tr_y,
    );
    // Right edge.
    pb.line_to(x1, y1 - br_y);
    // Bottom-right corner.
    pb.cubic_to(
        x1,
        y1 - br_y * 0.55,
        x1 - br_x * 0.55,
        y1,
        x1 - br_x,
        y1,
    );
    // Bottom edge.
    pb.line_to(x0 + bl_x, y1);
    // Bottom-left corner.
    pb.cubic_to(
        x0 + bl_x * 0.55,
        y1,
        x0,
        y1 - bl_y * 0.55,
        x0,
        y1 - bl_y,
    );
    // Left edge.
    pb.line_to(x0, y0 + tl_y);
    // Top-left corner (close).
    pb.cubic_to(
        x0,
        y0 + tl_y * 0.55,
        x0 + tl_x * 0.55,
        y0,
        x0 + tl_x,
        y0,
    );

    pb.close();

    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    Ok(())
}

fn rasterize_draw_border(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    widths: &[f32; 4],
    colors: &[Color; 4],
    _radii: &CornerRadii,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::Paint;

    let [top_w, right_w, bottom_w, left_w] = widths;
    let [top_c, right_c, bottom_c, left_c] = colors;

    // Simple solid border drawing (dashed/dotted skipped for now).

    // Top border.
    if *top_w > 0.0 {
        let paint = Paint {
            shader: tiny_skia::Shader::SolidColor(color_to_skia(*top_c)),
            anti_alias: true,
            force_hq_pipeline: false,
            blend_mode: tiny_skia::BlendMode::SourceOver,
        };
        let r = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, *top_w)
            .ok_or("Invalid rect")?;
        pixmap.fill_rect(r, &paint, tiny_skia::Transform::identity(), None);
    }

    // Right border.
    if *right_w > 0.0 {
        let paint = Paint {
            shader: tiny_skia::Shader::SolidColor(color_to_skia(*right_c)),
            anti_alias: true,
            force_hq_pipeline: false,
            blend_mode: tiny_skia::BlendMode::SourceOver,
        };
        let r = tiny_skia::Rect::from_xywh(
            rect.x + rect.width - right_w,
            rect.y,
            *right_w,
            rect.height,
        )
        .ok_or("Invalid rect")?;
        pixmap.fill_rect(r, &paint, tiny_skia::Transform::identity(), None);
    }

    // Bottom border.
    if *bottom_w > 0.0 {
        let paint = Paint {
            shader: tiny_skia::Shader::SolidColor(color_to_skia(*bottom_c)),
            anti_alias: true,
            force_hq_pipeline: false,
            blend_mode: tiny_skia::BlendMode::SourceOver,
        };
        let r = tiny_skia::Rect::from_xywh(
            rect.x,
            rect.y + rect.height - bottom_w,
            rect.width,
            *bottom_w,
        )
        .ok_or("Invalid rect")?;
        pixmap.fill_rect(r, &paint, tiny_skia::Transform::identity(), None);
    }

    // Left border.
    if *left_w > 0.0 {
        let paint = Paint {
            shader: tiny_skia::Shader::SolidColor(color_to_skia(*left_c)),
            anti_alias: true,
            force_hq_pipeline: false,
            blend_mode: tiny_skia::BlendMode::SourceOver,
        };
        let r =
            tiny_skia::Rect::from_xywh(rect.x, rect.y, *left_w, rect.height).ok_or("Invalid rect")?;
        pixmap.fill_rect(r, &paint, tiny_skia::Transform::identity(), None);
    }

    Ok(())
}

fn rasterize_draw_outline(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    width: f32,
    color: &Color,
    offset: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::Paint;

    if width <= 0.0 {
        return Ok(());
    }

    let paint = Paint {
        shader: tiny_skia::Shader::SolidColor(color_to_skia(*color)),
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };

    // Expand rect by offset.
    let x = rect.x - offset;
    let y = rect.y - offset;
    let w = rect.width + 2.0 * offset;
    let h = rect.height + 2.0 * offset;

    // Draw outline as a stroked rectangle.
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x, y);
    pb.line_to(x + w, y);
    pb.line_to(x + w, y + h);
    pb.line_to(x, y + h);
    pb.close();

    let stroke = tiny_skia::Stroke {
        width,
        ..Default::default()
    };

    if let Some(path) = pb.finish() {
        pixmap.stroke_path(
            &path,
            &paint,
            &stroke,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    Ok(())
}

/// CSS Images L3 §3.3 — resolve `GradientStop` positions to normalized [0,1].
///
/// Mirrors the GPU renderer's `resolve_gradient_stops`: unspecified first/last
/// stops default to 0/100%, runs of unspecified positions between explicit ones
/// are evenly distributed, and `Length::Px` stops are divided by `line_len`
/// (the pixel length of the gradient line). Returns `(position, color)` pairs.
fn resolve_stop_positions(stops: &[GradientStop], line_len: f32) -> Vec<(f32, Color)> {
    if stops.is_empty() {
        return vec![];
    }
    let n = stops.len();
    let mut positions: Vec<Option<f32>> = stops
        .iter()
        .map(|s| {
            s.position.as_ref().map(|l| match l {
                Length::Percent(p) => p / 100.0,
                Length::Px(v) if line_len > 0.0 => v / line_len,
                _ => 0.0,
            })
        })
        .collect();
    if positions[0].is_none() {
        positions[0] = Some(0.0);
    }
    if positions[n - 1].is_none() {
        positions[n - 1] = Some(1.0);
    }
    let mut i = 0;
    while i < n {
        if positions[i].is_some() {
            i += 1;
            continue;
        }
        let lo_i = i - 1;
        let lo_pos = positions[lo_i].unwrap_or(0.0);
        let mut hi_i = i + 1;
        while hi_i < n && positions[hi_i].is_none() {
            hi_i += 1;
        }
        let hi_pos = positions[hi_i.min(n - 1)].unwrap_or(1.0);
        let gap = (hi_i - lo_i) as f32;
        for (offset, pos) in positions[i..hi_i].iter_mut().enumerate() {
            let t = (i + offset - lo_i) as f32 / gap;
            *pos = Some(lo_pos + (hi_pos - lo_pos) * t);
        }
        i = hi_i;
    }
    stops
        .iter()
        .enumerate()
        .map(|(i, s)| (positions[i].unwrap_or(0.0), s.color))
        .collect()
}

/// Build tiny-skia `GradientStop`s from resolved `(position, color)` pairs.
///
/// For repeating gradients the resolved positions span `[first, last]` with
/// `last < 1`; rescaling to fill `[0,1]` turns that span into one tile so
/// `SpreadMode::Repeat` tiles it across the whole line. Returns the rescaled
/// stops plus the `(first, last)` fractions of the original line that the tile
/// occupies (caller shortens the gradient line to that sub-segment).
fn skia_gradient_stops(
    resolved: &[(f32, Color)],
    repeating: bool,
) -> Option<(Vec<tiny_skia::GradientStop>, f32, f32)> {
    if resolved.len() < 2 {
        return None;
    }
    let first = resolved.first().map(|s| s.0).unwrap_or(0.0);
    let last = resolved.last().map(|s| s.0).unwrap_or(1.0);
    let span = (last - first).max(1e-6);
    let (rescale, lo, hi) = if repeating {
        (true, first, last)
    } else {
        (false, 0.0, 1.0)
    };
    let stops = resolved
        .iter()
        .map(|&(pos, color)| {
            let p = if rescale { ((pos - first) / span).clamp(0.0, 1.0) } else { pos.clamp(0.0, 1.0) };
            tiny_skia::GradientStop::new(p, color_to_skia(color))
        })
        .collect();
    Some((stops, lo, hi))
}

/// CSS Images L3 §3.4 — linear gradient line endpoints in box-relative UV [0,1].
///
/// Mirrors the GPU renderer's `linear_gradient_uv_endpoints`. CSS angle
/// convention: 0° = "to top", 90° = "to right", 180° = "to bottom". Returns
/// `(start_uv, end_uv)` and the gradient-line pixel length (for px stops).
fn linear_uv_endpoints(w: f32, h: f32, angle_deg: f32) -> ([f32; 2], [f32; 2], f32) {
    if w <= 0.0 || h <= 0.0 {
        return ([0.0, 0.5], [1.0, 0.5], w.max(1.0));
    }
    let theta = angle_deg.to_radians();
    let dx = theta.sin();
    let dy = -theta.cos();
    let half_len = (w * dx.abs() + h * dy.abs()) / 2.0;
    if half_len < 1e-6 {
        return ([0.5, 0.5], [0.5, 0.5], 1.0);
    }
    let cx = w / 2.0;
    let cy = h / 2.0;
    let sx = (cx - dx * half_len) / w;
    let sy = (cy - dy * half_len) / h;
    let ex = (cx + dx * half_len) / w;
    let ey = (cy + dy * half_len) / h;
    ([sx, sy], [ex, ey], 2.0 * half_len)
}

/// CSS Images L3 §3.4 — `linear-gradient(...)` via tiny-skia `LinearGradient`.
fn rasterize_linear_gradient(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    angle_deg: f32,
    stops: &[GradientStop],
    repeating: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::{LinearGradient, Paint, Point, SpreadMode, Transform};

    let (start_uv, end_uv, line_len) = linear_uv_endpoints(rect.width, rect.height, angle_deg);
    let resolved = resolve_stop_positions(stops, line_len);
    let Some((skia_stops, lo, hi)) = skia_gradient_stops(&resolved, repeating) else {
        return Ok(());
    };

    // UV → pixel space; for repeating, clip the line to the [lo,hi] sub-segment.
    let px = |u: [f32; 2]| Point::from_xy(rect.x + u[0] * rect.width, rect.y + u[1] * rect.height);
    let full_start = start_uv;
    let dir = [end_uv[0] - start_uv[0], end_uv[1] - start_uv[1]];
    let seg = |t: f32| [full_start[0] + dir[0] * t, full_start[1] + dir[1] * t];
    let start = px(seg(lo));
    let end = px(seg(hi));

    let mode = if repeating { SpreadMode::Repeat } else { SpreadMode::Pad };
    let shader = LinearGradient::new(start, end, skia_stops, mode, Transform::identity())
        .ok_or("degenerate linear gradient")?;

    let paint = Paint {
        shader,
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };
    let skia_rect = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)
        .ok_or("Invalid rect dimensions")?;
    pixmap.fill_rect(skia_rect, &paint, Transform::identity(), None);
    Ok(())
}

/// CSS Images L3 §3.3 — `radial-gradient(...)` via tiny-skia `RadialGradient`.
///
/// Reproduces the GPU renderer's "farthest-corner" anisotropic ellipse: the
/// semi-axes are `rx = max(cx, 1-cx)`, `ry = max(cy, 1-cy)` in box-relative
/// units. tiny-skia radials are isotropic, so the ellipse is produced by
/// rendering a unit-ish circle and stretching it with a post-scale transform.
fn rasterize_radial_gradient(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    center_x_pct: f32,
    center_y_pct: f32,
    stops: &[GradientStop],
    repeating: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::{Paint, Point, RadialGradient, SpreadMode, Transform};

    let rx_px = center_x_pct.max(1.0 - center_x_pct).max(1e-3) * rect.width;
    let ry_px = center_y_pct.max(1.0 - center_y_pct).max(1e-3) * rect.height;
    let line_len = rx_px.max(ry_px).max(1.0);
    let resolved = resolve_stop_positions(stops, line_len);
    let Some((skia_stops, lo, hi)) = skia_gradient_stops(&resolved, repeating) else {
        return Ok(());
    };

    // Render the gradient in a normalized space where the ellipse is a unit
    // circle of radius `radius`, then scale x by rx and y by ry around the
    // centre to recover the ellipse. For repeating, shrink the radius to the
    // [lo,hi] sub-segment so SpreadMode::Repeat tiles outward.
    let cx = rect.x + center_x_pct * rect.width;
    let cy = rect.y + center_y_pct * rect.height;
    let radius = (hi - lo).max(1e-3);
    let center_norm = Point::from_xy(0.0, 0.0);
    let mode = if repeating { SpreadMode::Repeat } else { SpreadMode::Pad };
    let shader = RadialGradient::new(
        center_norm,
        center_norm,
        radius,
        skia_stops,
        mode,
        // Map normalized circle space to pixel ellipse: translate to centre,
        // scale by (rx, ry). `lo` offset for repeating handled via radius span.
        Transform::from_row(rx_px, 0.0, 0.0, ry_px, cx, cy),
    )
    .ok_or("degenerate radial gradient")?;

    let paint = Paint {
        shader,
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };
    let skia_rect = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)
        .ok_or("Invalid rect dimensions")?;
    pixmap.fill_rect(skia_rect, &paint, Transform::identity(), None);
    Ok(())
}

/// SVG 1.1 §11 — pre-tessellated SVG shape (flat triangle list) filled with a
/// solid colour.
///
/// `vertices.len()` is a multiple of 3; every consecutive triple is one triangle
/// in page-pixel coordinates, and `fill-opacity` / `stroke-opacity` is already
/// baked into `color` (strokes arrive tessellated into filled triangles too).
/// All triangles are merged into one path and filled in a single `SourceOver`
/// pass (Winding rule) so the union of the tessellation composites exactly once
/// — this avoids antialiasing seams along the shared internal edges that
/// per-triangle filling would produce, and matches the GPU renderer drawing the
/// whole shape in one `Fill` op.
fn rasterize_svg_path(
    pixmap: &mut tiny_skia::Pixmap,
    vertices: &[[f32; 2]],
    color: &Color,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::Paint;

    let mut pb = tiny_skia::PathBuilder::new();
    for tri in vertices.chunks_exact(3) {
        pb.move_to(tri[0][0], tri[0][1]);
        pb.line_to(tri[1][0], tri[1][1]);
        pb.line_to(tri[2][0], tri[2][1]);
        pb.close();
    }

    let Some(path) = pb.finish() else {
        return Ok(());
    };

    let paint = Paint {
        shader: tiny_skia::Shader::SolidColor(color_to_skia(*color)),
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };
    pixmap.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::identity(),
        None,
    );
    Ok(())
}

#[inline]
fn color_to_skia(color: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sample the RGBA8 pixel at `(x, y)` from a rasterized [`Image`].
    fn px(img: &Image, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let i = ((y * img.width + x) * 4) as usize;
        (img.data[i], img.data[i + 1], img.data[i + 2], img.data[i + 3])
    }

    /// `DrawSvgPath` fills the tessellated triangle interior with the solid
    /// colour and leaves the background white outside it.
    #[test]
    fn svg_path_fills_triangle_interior() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        // One triangle with a large axis-aligned interior so the centroid sample
        // is unambiguously inside (avoids antialiased edge pixels).
        let cmds = vec![DisplayCommand::DrawSvgPath {
            vertices: vec![[10.0, 10.0], [50.0, 10.0], [30.0, 50.0]],
            color: red,
        }];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");

        // Centroid ≈ (30, 23): solidly inside the triangle.
        assert_eq!(px(&img, 30, 23), (255, 0, 0, 255), "interior should be red");
        // Far corner outside the triangle stays white.
        assert_eq!(px(&img, 1, 1), (255, 255, 255, 255), "exterior stays white");
    }

    /// A degenerate path (fewer than 3 vertices) is a no-op, not a panic.
    #[test]
    fn svg_path_empty_is_noop() {
        let cmds = vec![DisplayCommand::DrawSvgPath {
            vertices: vec![],
            color: Color { r: 0, g: 0, b: 0, a: 255 },
        }];
        let img = rasterize_cpu(8, 8, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 4, 4), (255, 255, 255, 255), "background untouched");
    }
}
