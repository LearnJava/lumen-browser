//! Blend-mode compositing (PA-3): CSS `mix-blend-mode` (Compositing &
//! Blending L1 §5) через CPU pixel-level `mix_blend_rgba` на offscreen-слое.
//!
//! Вырезано из `femtovg_backend.rs` (SPLIT-PR2 срез 4/4) без изменения
//! поведения — чисто перенос кода между модулями.

use super::*;

/// Маппинг CSS MixBlendMode → femtovg CompositeOperation.
///
/// femtovg поддерживает только базовые Porter-Duff операции через OpenGL.
/// CSS Compositing & Blending L1 режимы (Multiply, Screen, Overlay и др.)
/// аппроксимируются через SourceOver — визуально неточно, но не вызывает ошибок.
pub(super) fn blend_to_composite(mode: BlendMode) -> femtovg::CompositeOperation {
    match mode {
        BlendMode::Normal => femtovg::CompositeOperation::SourceOver,
        BlendMode::PlusLighter => femtovg::CompositeOperation::Lighter,
        // Остальные CSS blend modes не поддерживаются OpenGL ES 2.0 — fallback.
        _ => femtovg::CompositeOperation::SourceOver,
    }
}

/// Entry pushed onto `FemtovgBackend::blend_layer_stack` by `PushBlendMode`.
///
/// Между Push и Pop все отрисовки идут в `src_image_id`. На `PopBlendMode`
/// `composite_blend_layer` смешивает `src_image_id` поверх `backdrop_rgba` через
/// `mix_blend_rgba` (CSS Compositing L1 §5) и композитит результат поверх
/// `prev_render_target`.
pub(super) struct BlendLayerEntry {
    /// CSS blend mode to apply.
    pub(super) mode: BlendMode,
    /// Offscreen image capturing the source layer (draws between Push and Pop).
    pub(super) src_image_id: femtovg::ImageId,
    /// Snapshot of the previous render target taken at PushBlendMode time,
    /// cropped to `bbox` when set (BUG-272 срез 14) or the full framebuffer
    /// otherwise. Premultiplied RGBA u8, dimensions `backdrop_w × backdrop_h`.
    pub(super) backdrop_rgba: Vec<u8>,
    /// Width of the backdrop snapshot in pixels.
    pub(super) backdrop_w: usize,
    /// Height of the backdrop snapshot in pixels.
    pub(super) backdrop_h: usize,
    /// Render target active before PushBlendMode — restored on PopBlendMode.
    pub(super) prev_render_target: femtovg::RenderTarget,
    /// BUG-272 срез 14: device-pixel `(x0, y0, w, h)` when `src_image_id` was
    /// acquired bbox-sized via [`FemtovgBackend::acquire_bbox_layer`] instead
    /// of full-framebuffer via [`FemtovgBackend::acquire_layer`] — same
    /// convention as [`OpacityLayerEntry::bbox`]. Mix-blend is pure per-pixel
    /// work (no neighbour sampling), so unlike `PushFilter` this applies
    /// whenever `bounds` maps to an on-target bbox — no blur-style exception.
    pub(super) bbox: Option<(f32, f32, usize, usize)>,
}

impl FemtovgBackend {
    /// Composites a blend-mode layer (PA-3) onto the previous render target.
    ///
    /// Algorithm:
    /// 1. Flush so src_image_id FBO has latest content.
    /// 2. Screenshot src_image to get source pixels (premultiplied RGBA u8).
    /// 3. Restore prev_render_target.
    /// 4. For each pixel: unpremultiply both source and backdrop → `mix_blend_rgba`
    ///    (CSS Compositing L1 §5) → re-premultiply result.
    /// 5. Upload result image and draw with `CompositeOperation::Source` to
    ///    replace the backdrop area with the blended result.
    pub(super) fn composite_blend_layer(&mut self, entry: BlendLayerEntry) {
        let BlendLayerEntry { mode, src_image_id, backdrop_rgba, backdrop_w, backdrop_h, prev_render_target, bbox } = entry;

        // BUG-272 срез 14: undo the Push-time bbox save()+translate()+scissor(),
        // if any — mirrors composite_opacity_layer's own restore() (canvas
        // save/restore state is independent of which GL render target is
        // bound, so doing this before the flush/screenshot below is safe).
        if bbox.is_some() {
            self.canvas.restore();
        }

        // Step 1: flush pending commands so src_image_id is fully rendered.
        self.canvas.flush();

        // Step 2: screenshot the source offscreen image. Reads whatever RT is
        // currently bound (src_image_id) at its own texture size — bbox-sized
        // when `bbox` is set (matches `backdrop_rgba`'s already-cropped size),
        // full-framebuffer otherwise (BUG-272 срез 14).
        let src_rgba = self.canvas.screenshot()
            .map(|img| img.buf().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect::<Vec<u8>>())
            .unwrap_or_default();

        // Step 3: restore the previous render target.
        self.switch_render_target(prev_render_target);

        // Step 4+5: CPU pixel-level blend + composite with Source operation.
        if src_rgba.len() == backdrop_rgba.len() && !src_rgba.is_empty() {
            let n = src_rgba.len() / 4;
            let mut result = vec![0u8; src_rgba.len()];
            for i in 0..n {
                let si = i * 4;
                // Premultiplied → straight for source.
                let sa = src_rgba[si + 3] as f32 / 255.0;
                let s_str = if sa > 0.0 {
                    [src_rgba[si] as f32 / 255.0 / sa,
                     src_rgba[si + 1] as f32 / 255.0 / sa,
                     src_rgba[si + 2] as f32 / 255.0 / sa,
                     sa]
                } else {
                    [0.0; 4]
                };
                // Premultiplied → straight for backdrop.
                let da = backdrop_rgba[si + 3] as f32 / 255.0;
                let d_str = if da > 0.0 {
                    [backdrop_rgba[si] as f32 / 255.0 / da,
                     backdrop_rgba[si + 1] as f32 / 255.0 / da,
                     backdrop_rgba[si + 2] as f32 / 255.0 / da,
                     da]
                } else {
                    [0.0; 4]
                };
                let out = mix_blend_rgba(mode, s_str, d_str);
                // Straight → premultiplied for output.
                let ao = out[3];
                result[si]     = ((out[0] * ao) * 255.0).round().clamp(0.0, 255.0) as u8;
                result[si + 1] = ((out[1] * ao) * 255.0).round().clamp(0.0, 255.0) as u8;
                result[si + 2] = ((out[2] * ao) * 255.0).round().clamp(0.0, 255.0) as u8;
                result[si + 3] = (ao * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            let pixels: Vec<rgb::RGBA8> = result.chunks_exact(4)
                .map(|c| rgb::RGBA8 { r: c[0], g: c[1], b: c[2], a: c[3] })
                .collect();
            let img_ref = imgref::ImgRef::new(&pixels, backdrop_w, backdrop_h);
            // BUG-272 (item 5, bbox variant срез 14): reuse a pooled image via
            // `update_image` instead of `create_image`-ing a fresh GPU upload
            // on every Pop — `bbox_cpu_upload_pool` when the source layer was
            // bbox-sized, `cpu_upload_pool` otherwise (see their field docs).
            // Fall back to a fresh upload if the pooled slot fails to update
            // (e.g. driver-level image loss).
            let acquired = match bbox {
                Some(_) => self.acquire_bbox_cpu_upload_image(backdrop_w, backdrop_h),
                None => self.acquire_cpu_upload_image(backdrop_w, backdrop_h),
            };
            let result_id = self.upload_to_pool(acquired, img_ref);
            if let Some(result_id) = result_id {
                self.canvas.save();
                self.canvas.reset_transform();
                // Source operation: replace dest pixels with the blended
                // result image. GL only blends pixels the drawn primitive
                // rasterizes, so restricting that primitive to `bbox` (BUG-272
                // срез 14) leaves everything outside it untouched — the same
                // reasoning `composite_clip_layer`'s bbox path relies on.
                self.canvas.global_composite_operation(femtovg::CompositeOperation::Copy);
                match bbox {
                    Some((x0, y0, w, h)) => {
                        let x = x0 / self.scale as f32;
                        let y = y0 / self.scale as f32;
                        let cw = w as f32 / self.scale as f32;
                        let ch = h as f32 / self.scale as f32;
                        let paint = femtovg::Paint::image(result_id, x, y, cw, ch, 0.0, 1.0);
                        let mut path = femtovg::Path::new();
                        path.rect(x, y, cw, ch);
                        self.canvas.fill_path(&path, &paint);
                    }
                    None => {
                        // BUG-320: fill the whole active target (band FBO
                        // during a scroll-blit pass) so the band-sized layer
                        // composites 1:1.
                        let (css_w, css_h) = self.current_rt_css_size();
                        let paint = femtovg::Paint::image(result_id, 0.0, 0.0, css_w, css_h, 0.0, 1.0);
                        let mut path = femtovg::Path::new();
                        path.rect(0.0, 0.0, css_w, css_h);
                        self.canvas.fill_path(&path, &paint);
                    }
                }
                self.canvas.restore();
                match bbox {
                    Some(_) => self.release_bbox_cpu_upload_image(result_id, backdrop_w, backdrop_h),
                    None => self.release_cpu_upload_image(result_id, backdrop_w, backdrop_h),
                }
            }
        }
        // BUG-272: src_image_id came from acquire_bbox_layer (bbox) or
        // acquire_layer (full-frame) — recycle to the matching pool.
        match bbox {
            Some((_, _, w, h)) => self.release_bbox_layer(src_image_id, w, h),
            None => self.release_layer(src_image_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_to_composite_normal() {
        let op = blend_to_composite(BlendMode::Normal);
        assert!(matches!(op, femtovg::CompositeOperation::SourceOver));
    }

    #[test]
    fn blend_to_composite_lighter() {
        let op = blend_to_composite(BlendMode::PlusLighter);
        assert!(matches!(op, femtovg::CompositeOperation::Lighter));
    }

    // ── PA-3: blend mode compositing (CPU pixel math) ────────────────────────

    fn approx_u8(a: u8, b: u8) -> bool { (a as i32 - b as i32).abs() <= 2 }

    fn blend_composite_pixel(mode: BlendMode, src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
        let premul_to_str = |px: [u8; 4]| -> [f32; 4] {
            let a = px[3] as f32 / 255.0;
            if a > 0.0 {
                [px[0] as f32 / 255.0 / a, px[1] as f32 / 255.0 / a, px[2] as f32 / 255.0 / a, a]
            } else {
                [0.0; 4]
            }
        };
        let s = premul_to_str(src);
        let d = premul_to_str(dst);
        let out = mix_blend_rgba(mode, s, d);
        let ao = out[3];
        [(out[0]*ao*255.0).round().clamp(0.0,255.0) as u8,
         (out[1]*ao*255.0).round().clamp(0.0,255.0) as u8,
         (out[2]*ao*255.0).round().clamp(0.0,255.0) as u8,
         (ao*255.0).round().clamp(0.0,255.0) as u8]
    }

    #[test]
    fn blend_composite_multiply_opaque_on_opaque() {
        // src=0.5 grey opaque, dst=0.5 grey opaque → multiply → 0.25 grey.
        let result = blend_composite_pixel(BlendMode::Multiply,
            [128, 128, 128, 255], [128, 128, 128, 255]);
        // 0.25 * 255 ≈ 64 (premultiplied, alpha=1).
        assert!(approx_u8(result[0], 64), "R: expected ≈64, got {}", result[0]);
        assert!(approx_u8(result[3], 255), "A: expected 255, got {}", result[3]);
    }

    #[test]
    fn blend_composite_screen_lightens() {
        // src=0.5 grey, dst=0.5 grey → screen → 0.75 grey.
        // screen(a,b) = a + b - a*b = 0.5+0.5-0.25 = 0.75.
        let result = blend_composite_pixel(BlendMode::Screen,
            [128, 128, 128, 255], [128, 128, 128, 255]);
        // 0.75 * 255 ≈ 191
        assert!(approx_u8(result[0], 191), "R: expected ≈191, got {}", result[0]);
        assert!(approx_u8(result[3], 255));
    }

    #[test]
    fn blend_composite_transparent_src_keeps_backdrop() {
        // Fully transparent source → result equals backdrop.
        let result = blend_composite_pixel(BlendMode::Multiply,
            [0, 0, 0, 0], [200, 100, 50, 255]);
        assert!(approx_u8(result[0], 200), "R: expected ≈200, got {}", result[0]);
        assert!(approx_u8(result[1], 100), "G: expected ≈100, got {}", result[1]);
    }

    #[test]
    fn blend_composite_difference_gives_abs_difference() {
        // src=white opaque, dst=grey opaque → difference → grey.
        // difference(1.0, 0.5) = |0.5-1.0| = 0.5 → ~128.
        let result = blend_composite_pixel(BlendMode::Difference,
            [255, 255, 255, 255], [128, 128, 128, 255]);
        assert!(approx_u8(result[0], 127), "R: expected ≈127, got {}", result[0]);
    }

    #[test]
    fn blend_composite_overlay_on_dark_backdrop_is_multiply_like() {
        // Overlay with cb<0.5: result ≈ 2*cs*cb (multiply branch).
        // cs=0.5 (128), cb=0.25 (64) → 2*0.5*0.25 = 0.25 → ≈64.
        let result = blend_composite_pixel(BlendMode::Overlay,
            [128, 128, 128, 255], [64, 64, 64, 255]);
        assert!(approx_u8(result[0], 64), "R: expected ≈64, got {}", result[0]);
    }
}
