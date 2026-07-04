//! Variable-font text rendering helper (BUG-109).
//!
//! `font-variation-settings` axis values (`wght`/`wdth`/`slnt`/`opsz`/…) reach
//! the display list as `font_variation_axes`, and the GPU renderer
//! (`renderer.rs::push_text_glyphs`) already applies them via `gvar` deltas.
//! The femtovg window backend, however, hands text to femtovg's own shaper,
//! which has no API for variation coordinates — so varied runs rendered in the
//! default window backend came out at the face's default instance.
//!
//! This module bridges that gap: it resolves each glyph's outline at the
//! requested axis coordinates through [`lumen_font::Font::glyph_resolved_with_coords`]
//! and emits backend-agnostic [`PathCmd`]s in screen pixels (Y-down). A backend
//! (femtovg) translates them into its own path type and fills with the text
//! colour — picking up the canvas transform/clip/anti-aliasing for free.
//!
//! The contour walk mirrors `lumen_font::rasterizer::walk_contour` (TrueType
//! quadratic outlines, on/off-curve point handling, implied midpoints) so the
//! filled shape matches the CPU/GPU rasterizers.

use lumen_font::{Font, OutlinePoint, Outline, VariationCoords};

/// One path-building command in screen pixels (origin top-left, Y down).
///
/// Backend-agnostic: a renderer maps each variant to its own path API
/// (`move_to`/`line_to`/`quad_to`/`close`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCmd {
    /// Begin a new sub-path at `(x, y)`.
    MoveTo(f32, f32),
    /// Straight segment to `(x, y)`.
    LineTo(f32, f32),
    /// Quadratic Bézier with control `(cx, cy)` ending at `(x, y)`.
    QuadTo(f32, f32, f32, f32),
    /// Close the current sub-path.
    Close,
}

#[inline]
fn midpoint(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5)
}

/// Walks one glyph contour, emitting [`PathCmd`]s. `map` converts a font-unit
/// point (Y up) to a screen pixel (Y down). Mirrors the rasterizer's edge walk:
/// a leading off-curve point synthesises a start anchor at the midpoint of the
/// last and first points, and two consecutive off-curve points imply an
/// on-curve midpoint between them.
fn emit_contour(
    pts: &[OutlinePoint],
    map: &impl Fn(&OutlinePoint) -> (f32, f32),
    out: &mut Vec<PathCmd>,
) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    let to_px = |i: usize| -> (f32, f32) { map(&pts[i]) };

    let first_on = (0..n).find(|&i| pts[i].on_curve);
    let (start_idx, init_anchor) = match first_on {
        Some(i) => (i, to_px(i)),
        // All points off-curve → synthetic anchor between last and first.
        None => (n - 1, midpoint(to_px(n - 1), to_px(0))),
    };

    out.push(PathCmd::MoveTo(init_anchor.0, init_anchor.1));
    let mut pending: Option<(f32, f32)> = None;

    for offset in 1..=n {
        let i = (start_idx + offset) % n;
        let p = to_px(i);
        if pts[i].on_curve {
            match pending.take() {
                None => out.push(PathCmd::LineTo(p.0, p.1)),
                Some(c) => out.push(PathCmd::QuadTo(c.0, c.1, p.0, p.1)),
            }
        } else if let Some(c) = pending {
            let m = midpoint(c, p);
            out.push(PathCmd::QuadTo(c.0, c.1, m.0, m.1));
            pending = Some(p);
        } else {
            pending = Some(p);
        }
    }

    // Close an all-off-curve contour back to the synthetic anchor. For
    // contours with an on-curve start the loop already wrapped through
    // `start_idx` at `offset == n`, closing the shape.
    if first_on.is_none()
        && let Some(c) = pending
    {
        out.push(PathCmd::QuadTo(c.0, c.1, init_anchor.0, init_anchor.1));
    }
    out.push(PathCmd::Close);
}

/// Builds filled-glyph path commands for a text run rendered with
/// `font-variation-settings` axes applied.
///
/// - `font_bytes`: the resolved face (must contain `glyf` + `gvar`).
/// - `axes`: user-space variation settings as `(tag, value)` (e.g. `(*b"wght", 700.0)`).
/// - `text`: the run's text.
/// - `font_size`: CSS pixels.
/// - `origin_x` / `box_top_y`: the run's pen origin and text-box top, in screen
///   pixels (the baseline is derived from the face's ascent/descent like the
///   CPU/GPU rasterizers).
/// - `tab_size`: tab advance in pixels (CSS Text L3 §10.1); `<= 0` disables tab
///   handling and shapes the whole string as one segment.
///
/// Returns `None` when the bytes do not parse **or the face is not variable** —
/// in that case variation has no effect and the caller should fall back to its
/// native text path. Returns `Some` (possibly empty for whitespace-only runs)
/// when the face is variable and was processed here.
#[allow(clippy::too_many_arguments)]
pub fn build_varied_text_paths(
    font_bytes: &[u8],
    axes: &[([u8; 4], f32)],
    features: &[([u8; 4], u32)],
    text: &str,
    font_size: f32,
    origin_x: f32,
    box_top_y: f32,
    tab_size: f32,
) -> Option<Vec<PathCmd>> {
    if font_size <= 0.0 {
        return None;
    }
    let font = Font::parse(font_bytes).ok()?;

    // Only handle variable faces here; static faces render identically through
    // the backend's native path, so defer to it.
    let fvar = font.fvar().ok().filter(lumen_font::Fvar::is_variable)?;
    let avar = font.avar().unwrap_or_default();
    let coords = VariationCoords::from_css_settings(&fvar, &avar, axes);

    let head = font.head().ok()?;
    let hhea = font.hhea().ok()?;
    let cmap = font.cmap().ok()?;
    let hmtx = font.hmtx().ok()?;
    let shaper = lumen_font::Shaper::with_features(&font, features);

    let units_per_em = f32::from(head.units_per_em);
    if units_per_em == 0.0 {
        return None;
    }
    let ascent = f32::from(hhea.ascent);
    let descent = f32::from(hhea.descent);
    let denom = ascent - descent;
    let ascent_ratio = if denom != 0.0 { ascent / denom } else { 0.8 };
    let baseline_y = box_top_y + font_size * ascent_ratio;
    let scale = font_size / units_per_em;

    let mut out: Vec<PathCmd> = Vec::new();
    let mut cursor_x = origin_x;
    let mut first_segment = true;
    let segments: Vec<&str> = if tab_size > 0.0 {
        text.split('\t').collect()
    } else {
        vec![text]
    };

    for segment in segments {
        if !first_segment {
            cursor_x += tab_size;
        }
        first_segment = false;
        if segment.is_empty() {
            continue;
        }
        let glyph_ids: Vec<u16> = segment
            .chars()
            .map(|ch| cmap.glyph_index(ch as u32).unwrap_or(0))
            .collect();
        let shaped = shaper.shape(&glyph_ids, &hmtx);
        for sg in &shaped {
            let pen_x = cursor_x + sg.x_offset as f32 * scale;
            let pen_baseline = baseline_y - sg.y_offset as f32 * scale;
            if let Ok(Some(glyph)) = font.glyph_resolved_with_coords(sg.glyph_id, coords.as_slice())
                && let Outline::Simple(contours) = &glyph.outline
            {
                let map = |p: &OutlinePoint| -> (f32, f32) {
                    (
                        pen_x + p.x as f32 * scale,
                        pen_baseline - p.y as f32 * scale,
                    )
                };
                for contour in contours {
                    emit_contour(&contour.points, &map, &mut out);
                }
            }
            cursor_x += sg.x_advance as f32 * scale;
        }
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bundled Inter is static (no `fvar`): the helper defers to the backend.
    #[test]
    fn static_font_returns_none() {
        const INTER: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");
        let cmds = build_varied_text_paths(INTER, &[(*b"wght", 700.0)], &[], "Ag", 16.0, 0.0, 0.0, 0.0);
        assert!(cmds.is_none(), "static face must defer to the native text path");
    }

    #[test]
    fn garbage_bytes_return_none() {
        assert!(build_varied_text_paths(&[0u8; 8], &[], &[], "x", 16.0, 0.0, 0.0, 0.0).is_none());
    }

    #[test]
    fn emit_contour_triangle_produces_closed_subpath() {
        // Three on-curve points → MoveTo + 2× LineTo (loop closes through start)
        // + Close.
        let pts = vec![
            OutlinePoint { x: 0, y: 0, on_curve: true },
            OutlinePoint { x: 100, y: 0, on_curve: true },
            OutlinePoint { x: 50, y: 100, on_curve: true },
        ];
        let map = |p: &OutlinePoint| (p.x as f32, p.y as f32);
        let mut out = Vec::new();
        emit_contour(&pts, &map, &mut out);
        assert_eq!(
            out,
            vec![
                PathCmd::MoveTo(0.0, 0.0),
                PathCmd::LineTo(100.0, 0.0),
                PathCmd::LineTo(50.0, 100.0),
                PathCmd::LineTo(0.0, 0.0),
                PathCmd::Close,
            ]
        );
    }

    #[test]
    fn emit_contour_off_curve_control_emits_quad() {
        // on, off, on → MoveTo + QuadTo(control, end) + LineTo(back to start) + Close.
        let pts = vec![
            OutlinePoint { x: 0, y: 0, on_curve: true },
            OutlinePoint { x: 50, y: 100, on_curve: false },
            OutlinePoint { x: 100, y: 0, on_curve: true },
        ];
        let map = |p: &OutlinePoint| (p.x as f32, p.y as f32);
        let mut out = Vec::new();
        emit_contour(&pts, &map, &mut out);
        assert_eq!(
            out,
            vec![
                PathCmd::MoveTo(0.0, 0.0),
                PathCmd::QuadTo(50.0, 100.0, 100.0, 0.0),
                PathCmd::LineTo(0.0, 0.0),
                PathCmd::Close,
            ]
        );
    }
}
