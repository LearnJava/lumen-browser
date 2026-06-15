//! Канонические скалярные формулы CSS Compositing & Blending L1 §9–10.
//!
//! Единственный Rust-источник blend-математики для всех бэкендов. До PA-1
//! формулы существовали в трёх несвязанных местах: WGSL-шейдер
//! `BLEND_SHADER_SRC` (`renderer.rs`), tiny-skia-маппинг (`cpu_raster.rs`)
//! и femtovg-заглушка (2/17 режимов → SourceOver) — см.
//! docs/paint-pipeline-review-2026-06.md, Key finding 2.
//!
//! Формулы зеркалируют WGSL-шейдер (включая lum-коэффициенты Rec.601
//! 0.299/0.587/0.114), чтобы femtovg offscreen-путь (PA-3) дал тот же
//! результат, что wgpu. Все цвета — straight (non-premultiplied) f32-каналы
//! в [0,1].

use crate::display_list::BlendMode;

/// Separable blend function `B(Cs, Cb)` per channel (CSS Compositing L1 §9).
///
/// `cs` — source channel, `cb` — backdrop channel, both straight [0,1].
/// `Normal` и неотделимые режимы (Hue/Saturation/Color/Luminosity) возвращают
/// `cs` — для них используй [`blend_rgb`]. `PlusLighter` возвращает
/// `min(1, cs + cb)` (как в WGSL-шейдере); каноническая additive-композиция
/// L2 §6 — в [`mix_blend_rgba`].
#[must_use]
pub fn blend_channel(mode: BlendMode, cs: f32, cb: f32) -> f32 {
    match mode {
        BlendMode::Multiply => cs * cb,
        BlendMode::Screen => cs + cb - cs * cb,
        BlendMode::Overlay => {
            if cb <= 0.5 {
                2.0 * cs * cb
            } else {
                1.0 - 2.0 * (1.0 - cs) * (1.0 - cb)
            }
        }
        BlendMode::Darken => cs.min(cb),
        BlendMode::Lighten => cs.max(cb),
        BlendMode::ColorDodge => {
            if cb == 0.0 {
                0.0
            } else if cs == 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if cb == 1.0 {
                1.0
            } else if cs == 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        // HardLight — Overlay with Cs/Cb swapped.
        BlendMode::HardLight => {
            if cs <= 0.5 {
                2.0 * cs * cb
            } else {
                1.0 - 2.0 * (1.0 - cs) * (1.0 - cb)
            }
        }
        BlendMode::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 {
                    ((16.0 * cb - 12.0) * cb + 4.0) * cb
                } else {
                    cb.sqrt()
                };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::Exclusion => cs + cb - 2.0 * cs * cb,
        BlendMode::PlusLighter => (cs + cb).min(1.0),
        // Normal + non-separable — identity at channel level.
        BlendMode::Normal
        | BlendMode::Hue
        | BlendMode::Saturation
        | BlendMode::Color
        | BlendMode::Luminosity => cs,
    }
}

/// Blend function `B(Cs, Cb)` for a full RGB triple (CSS Compositing L1 §9–10).
///
/// Handles both separable modes (per-channel [`blend_channel`]) and
/// non-separable modes (Hue / Saturation / Color / Luminosity via
/// `SetLum`/`SetSat`, §10). Inputs/outputs — straight RGB in [0,1].
#[must_use]
pub fn blend_rgb(mode: BlendMode, cs: [f32; 3], cb: [f32; 3]) -> [f32; 3] {
    match mode {
        // Hue: hue of src, sat+lum of backdrop.
        BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        // Saturation: sat of src, hue+lum of backdrop.
        BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        // Color: hue+sat of src, lum of backdrop.
        BlendMode::Color => set_lum(cs, lum(cb)),
        // Luminosity: lum of src, hue+sat of backdrop.
        BlendMode::Luminosity => set_lum(cb, lum(cs)),
        _ => [
            blend_channel(mode, cs[0], cb[0]),
            blend_channel(mode, cs[1], cb[1]),
            blend_channel(mode, cs[2], cb[2]),
        ],
    }
}

/// CSS Compositing L1 §5 — blend `src` over `dst` with `mode`, then composite
/// `source-over`. Straight RGBA in [0,1] на входе и выходе.
///
/// Формула: `Cm = (1−αb)·Cs + αb·B(Cb,Cs)`, затем simple alpha compositing
/// `co = αs·Cm + αb·Cb·(1−αs)`, `αo = αs + αb·(1−αs)`; результат
/// un-premultiplied (деление на `αo`). `PlusLighter` — additive composite
/// L2 §6: `co = αs·Cs + αb·Cb`, `αo = min(1, αs+αb)`. Полностью прозрачный
/// результат — `[0,0,0,0]`.
#[must_use]
pub fn mix_blend_rgba(mode: BlendMode, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    let (cs, alpha_s) = ([src[0], src[1], src[2]], src[3]);
    let (cb, alpha_b) = ([dst[0], dst[1], dst[2]], dst[3]);

    if mode == BlendMode::PlusLighter {
        let ao = (alpha_s + alpha_b).min(1.0);
        if ao <= 0.0 {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let co = |i: usize| ((alpha_s * cs[i] + alpha_b * cb[i]).min(1.0)) / ao;
        return [co(0), co(1), co(2), ao];
    }

    let blended = blend_rgb(mode, cs, cb);
    let ao = alpha_s + alpha_b * (1.0 - alpha_s);
    if ao <= 0.0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let out = |i: usize| {
        // Cm: backdrop-alpha mixes the raw source colour with the blend result.
        let cm = (1.0 - alpha_b) * cs[i] + alpha_b * blended[i];
        (alpha_s * cm + alpha_b * cb[i] * (1.0 - alpha_s)) / ao
    };
    [out(0), out(1), out(2), ao]
}

/// Luminance of a straight RGB triple (Rec.601 weights, как в WGSL-шейдере).
#[must_use]
pub fn lum(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

/// `ClipColor` (CSS Compositing L1 §10): после SetLum компоненты могут выйти
/// за [0,1] — сжимает их к границам, сохраняя luminance.
#[must_use]
pub fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let mut result = c;
    if n < 0.0 {
        for ch in &mut result {
            *ch = l + (*ch - l) * l / (l - n);
        }
    }
    let l2 = lum(result);
    let x2 = result[0].max(result[1]).max(result[2]);
    if x2 > 1.0 {
        for ch in &mut result {
            *ch = l2 + (*ch - l2) * (1.0 - l2) / (x2 - l2);
        }
    }
    result
}

/// `SetLum` (CSS Compositing L1 §10): сдвигает все каналы так, чтобы
/// luminance стала `l`, с последующим [`clip_color`].
#[must_use]
pub fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

/// Saturation of a straight RGB triple: `max − min` (CSS Compositing L1 §10).
#[must_use]
pub fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// `SetSat` (CSS Compositing L1 §10): задаёт saturation `s`, сохраняя порядок
/// каналов (min→0, mid пропорционально, max→s).
#[must_use]
pub fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    // Indices of min/mid/max channels (stable for ties).
    let (imin, imid, imax) = if c[0] <= c[1] && c[0] <= c[2] {
        if c[1] <= c[2] { (0, 1, 2) } else { (0, 2, 1) }
    } else if c[1] <= c[0] && c[1] <= c[2] {
        if c[0] <= c[2] { (1, 0, 2) } else { (1, 2, 0) }
    } else if c[0] <= c[1] {
        (2, 0, 1)
    } else {
        (2, 1, 0)
    };
    let (cmin, cmid, cmax) = (c[imin], c[imid], c[imax]);
    let mut out = [0.0f32; 3];
    if cmax > cmin {
        out[imid] = (cmid - cmin) * s / (cmax - cmin);
        out[imax] = s;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    fn approx3(a: [f32; 3], b: [f32; 3]) -> bool {
        approx(a[0], b[0]) && approx(a[1], b[1]) && approx(a[2], b[2])
    }

    // ── separable channel formulas (§9) ──────────────────────────────────────

    #[test]
    fn multiply_darkens() {
        assert!(approx(blend_channel(BlendMode::Multiply, 0.5, 0.5), 0.25));
        assert!(approx(blend_channel(BlendMode::Multiply, 1.0, 0.7), 0.7));
        assert!(approx(blend_channel(BlendMode::Multiply, 0.0, 0.7), 0.0));
    }

    #[test]
    fn screen_lightens() {
        assert!(approx(blend_channel(BlendMode::Screen, 0.5, 0.5), 0.75));
        assert!(approx(blend_channel(BlendMode::Screen, 0.0, 0.7), 0.7));
        assert!(approx(blend_channel(BlendMode::Screen, 1.0, 0.3), 1.0));
    }

    #[test]
    fn overlay_branches_on_backdrop() {
        // cb <= 0.5 → multiply×2; cb > 0.5 → screen-like.
        assert!(approx(blend_channel(BlendMode::Overlay, 0.5, 0.25), 0.25));
        assert!(approx(blend_channel(BlendMode::Overlay, 0.5, 0.75), 0.75));
    }

    #[test]
    fn hard_light_is_overlay_with_swapped_args() {
        for &(cs, cb) in &[(0.2, 0.7), (0.8, 0.3), (0.5, 0.5)] {
            assert!(approx(
                blend_channel(BlendMode::HardLight, cs, cb),
                blend_channel(BlendMode::Overlay, cb, cs),
            ));
        }
    }

    #[test]
    fn darken_lighten_pick_extremes() {
        assert!(approx(blend_channel(BlendMode::Darken, 0.3, 0.6), 0.3));
        assert!(approx(blend_channel(BlendMode::Lighten, 0.3, 0.6), 0.6));
    }

    #[test]
    fn dodge_and_burn_edge_cases() {
        assert!(approx(blend_channel(BlendMode::ColorDodge, 0.5, 0.0), 0.0));
        assert!(approx(blend_channel(BlendMode::ColorDodge, 1.0, 0.5), 1.0));
        assert!(approx(blend_channel(BlendMode::ColorDodge, 0.5, 0.25), 0.5));
        assert!(approx(blend_channel(BlendMode::ColorBurn, 0.5, 1.0), 1.0));
        assert!(approx(blend_channel(BlendMode::ColorBurn, 0.0, 0.5), 0.0));
        assert!(approx(blend_channel(BlendMode::ColorBurn, 0.5, 0.75), 0.5));
    }

    #[test]
    fn soft_light_branches() {
        // cs <= 0.5: darkening branch; cs=0.5 — identity.
        assert!(approx(blend_channel(BlendMode::SoftLight, 0.5, 0.4), 0.4));
        // cs > 0.5, cb > 0.25: sqrt branch.
        let v = blend_channel(BlendMode::SoftLight, 1.0, 0.64);
        assert!(approx(v, 0.64 + (0.8 - 0.64)), "got {v}");
        // cs > 0.5, cb <= 0.25: polynomial branch (continuous, bounded).
        let p = blend_channel(BlendMode::SoftLight, 1.0, 0.2);
        assert!((0.0..=1.0).contains(&p));
    }

    #[test]
    fn difference_and_exclusion() {
        assert!(approx(blend_channel(BlendMode::Difference, 0.3, 0.8), 0.5));
        assert!(approx(blend_channel(BlendMode::Exclusion, 0.5, 0.5), 0.5));
        assert!(approx(blend_channel(BlendMode::Exclusion, 1.0, 1.0), 0.0));
    }

    #[test]
    fn normal_returns_source() {
        assert!(approx(blend_channel(BlendMode::Normal, 0.42, 0.9), 0.42));
    }

    // ── non-separable modes (§10) ────────────────────────────────────────────

    #[test]
    fn luminosity_takes_lum_of_source() {
        let cs = [1.0, 1.0, 1.0]; // lum = 1.0
        let cb = [0.5, 0.2, 0.8];
        let out = blend_rgb(BlendMode::Luminosity, cs, cb);
        assert!(approx(lum(out), 1.0), "lum(out) = {}", lum(out));
    }

    #[test]
    fn color_keeps_backdrop_lum() {
        let cs = [1.0, 0.0, 0.0];
        let cb = [0.5, 0.5, 0.5]; // lum = 0.5
        let out = blend_rgb(BlendMode::Color, cs, cb);
        assert!(approx(lum(out), 0.5), "lum(out) = {}", lum(out));
    }

    #[test]
    fn hue_keeps_backdrop_sat_and_lum() {
        let cs = [0.0, 1.0, 0.0];
        let cb = [0.8, 0.4, 0.4];
        let out = blend_rgb(BlendMode::Hue, cs, cb);
        assert!(approx(lum(out), lum(cb)), "lum: {} vs {}", lum(out), lum(cb));
        assert!(approx(sat(out), sat(cb)), "sat: {} vs {}", sat(out), sat(cb));
    }

    #[test]
    fn saturation_takes_sat_of_source() {
        // cs c sat=0.5 на этом backdrop не требует clip_color, поэтому
        // saturation переносится точно (при клиппинге она законно сжимается).
        let cs = [0.5, 0.0, 0.0]; // sat = 0.5
        let cb = [0.6, 0.5, 0.4];
        let out = blend_rgb(BlendMode::Saturation, cs, cb);
        assert!(approx(sat(out), 0.5), "sat(out) = {}", sat(out));
        assert!(approx(lum(out), lum(cb)));
    }

    #[test]
    fn set_sat_zero_is_grey() {
        let out = set_sat([0.9, 0.5, 0.1], 0.0);
        assert!(approx3(out, [0.0, 0.0, 0.0]));
    }

    #[test]
    fn set_sat_preserves_channel_order() {
        let out = set_sat([0.9, 0.5, 0.1], 0.4);
        assert!(out[0] >= out[1] && out[1] >= out[2]);
        assert!(approx(out[0] - out[2], 0.4));
    }

    #[test]
    fn clip_color_passes_in_gamut_through() {
        let c = [0.3, 0.6, 0.9];
        assert!(approx3(clip_color(c), c));
    }

    #[test]
    fn set_lum_clips_out_of_gamut() {
        // Saturated red pushed to high luminance must stay in [0,1].
        let out = set_lum([1.0, 0.0, 0.0], 0.9);
        for ch in out {
            assert!((0.0..=1.0 + 1e-6).contains(&ch), "channel out of gamut: {ch}");
        }
        assert!(approx(lum(out), 0.9));
    }

    // ── full §5 compositing ──────────────────────────────────────────────────

    #[test]
    fn mix_blend_normal_opaque_replaces_backdrop() {
        let out = mix_blend_rgba(BlendMode::Normal, [0.2, 0.4, 0.6, 1.0], [1.0, 1.0, 1.0, 1.0]);
        assert!(approx3([out[0], out[1], out[2]], [0.2, 0.4, 0.6]));
        assert!(approx(out[3], 1.0));
    }

    #[test]
    fn mix_blend_transparent_source_keeps_backdrop() {
        let dst = [0.7, 0.5, 0.3, 1.0];
        let out = mix_blend_rgba(BlendMode::Multiply, [0.1, 0.1, 0.1, 0.0], dst);
        assert!(approx3([out[0], out[1], out[2]], [0.7, 0.5, 0.3]));
        assert!(approx(out[3], 1.0));
    }

    #[test]
    fn mix_blend_multiply_opaque_uses_blend_result() {
        let out = mix_blend_rgba(BlendMode::Multiply, [0.5, 0.5, 0.5, 1.0], [0.5, 0.5, 0.5, 1.0]);
        assert!(approx3([out[0], out[1], out[2]], [0.25, 0.25, 0.25]));
    }

    #[test]
    fn mix_blend_transparent_backdrop_uses_raw_source() {
        // αb = 0 → Cm = Cs: blend formula must not darken against nothing.
        let out = mix_blend_rgba(BlendMode::Multiply, [0.5, 0.5, 0.5, 1.0], [0.0, 0.0, 0.0, 0.0]);
        assert!(approx3([out[0], out[1], out[2]], [0.5, 0.5, 0.5]));
        assert!(approx(out[3], 1.0));
    }

    #[test]
    fn mix_blend_both_transparent_is_transparent() {
        let out = mix_blend_rgba(BlendMode::Screen, [0.0; 4], [0.0; 4]);
        assert_eq!(out, [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn plus_lighter_is_additive() {
        let out =
            mix_blend_rgba(BlendMode::PlusLighter, [0.3, 0.3, 0.3, 1.0], [0.4, 0.4, 0.4, 1.0]);
        assert!(approx3([out[0], out[1], out[2]], [0.7, 0.7, 0.7]));
        assert!(approx(out[3], 1.0));
    }
}
