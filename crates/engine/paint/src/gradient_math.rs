//! Общая скалярная математика градиентов для всех рендер-бэкендов.
//!
//! Единственный источник истины для resolve/sample-логики градиентов
//! (CSS Images L3 §3.3–3.4, L4 §3.7). До PA-1 эти функции были продублированы
//! трижды: `renderer.rs` (wgpu), `cpu_raster.rs` (tiny-skia) и
//! `backends/femtovg_backend.rs` — фиксы в одной копии молча не попадали
//! в остальные (см. docs/paint-pipeline-review-2026-06.md, Key finding 2).
//!
//! Все функции — чистые, без зависимостей от GPU/растеризатора, и используют
//! только IEEE-точные операции (без platform libm), поэтому пригодны для
//! кросс-OS bit-identical CPU snapshot гейта.

use lumen_core::geom::Size;
use lumen_layout::{Color, GradientStop, Length};

/// CSS Images L3 §3.3 — resolve `GradientStop` positions to normalized [0,1].
///
/// Канонический алгоритм для всех бэкендов: неуказанные first/last позиции
/// → 0/100%, серии неуказанных позиций между явными распределяются равномерно,
/// `Length::Px` делится на `line_len` (пиксельная длина градиентной линии).
/// Позиции НЕ зажимаются в [0,1] — repeating-градиентам нужны значения вне
/// диапазона; бэкенд, которому нужен clamp (femtovg-библиотека), делает его сам.
/// Возвращает пары `(position, color)`.
#[must_use]
pub fn resolve_stop_positions(stops: &[GradientStop], line_len: f32) -> Vec<(f32, Color)> {
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
                // `calc()` stop positions (e.g. `calc(50% + 10px)`) resolve their
                // inner `%` against the gradient-line length, then the whole pixel
                // result is normalized by `line_len` (CSS Images L3 §3.3, BUG-230).
                // Without this arm a Calc position fell through to `_ => 0.0`,
                // collapsing the stop to offset 0 and degenerating the gradient.
                // em/viewport units inside such a calc are out of scope here, so a
                // nominal em basis and a zero viewport are passed.
                Length::Calc(_) if line_len > 0.0 => l
                    .resolve(16.0, Some(line_len), Size { width: 0.0, height: 0.0 })
                    .map_or(0.0, |px| px / line_len),
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
    // Distribute runs of None between two explicit positions.
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

/// CSS Images L4 §3.1 — gradient colour interpolation is defined in
/// **premultiplied** sRGBA. The raster backends (tiny-skia, femtovg) interpolate
/// between adjacent stops in *straight* (non-premultiplied) space, which is only
/// equivalent when the two endpoints share the same alpha. When alpha varies
/// across a segment — most visibly a fade to the `transparent` keyword
/// (`rgba(0,0,0,0)`) — straight interpolation drags the colour toward black,
/// producing a dark/muddy fringe instead of the correct premultiplied fade
/// (orange → transparent should stay orange-hued, only its alpha dropping).
///
/// This subdivides every segment whose endpoints differ in alpha into `STEPS`
/// intermediate stops sampled by premultiplied interpolation (lerp the
/// premultiplied channels, then un-premultiply), so a straight-interpolating
/// backend reproduces the premultiplied curve between the dense stops. Segments
/// with equal endpoint alpha (the common all-opaque gradient) are emitted
/// verbatim — byte-identical to before, so solid gradients never regress.
///
/// Used by both raster stop builders (`femtovg_stops`, `skia_gradient_stops`)
/// so the window and the CPU snapshot stay in lock-step (BUG-190).
#[must_use]
pub fn premultiplied_subdivide_stops(resolved: &[(f32, Color)]) -> Vec<(f32, Color)> {
    if resolved.len() < 2 {
        return resolved.to_vec();
    }
    /// Intermediate stops inserted per transparency-bearing segment. 16 keeps the
    /// premultiplied curve visually smooth (≤1/255 step over a full fade) while
    /// staying well under the femtovg 256-texel gradient texture resolution.
    const STEPS: usize = 16;
    let mut out: Vec<(f32, Color)> = Vec::with_capacity(resolved.len() * 2);
    out.push(resolved[0]);
    for w in resolved.windows(2) {
        let (p0, c0) = w[0];
        let (p1, c1) = w[1];
        // Premultiplied and straight interpolation diverge only when the segment's
        // endpoint alphas differ; otherwise the un-premultiply cancels exactly.
        if c0.a != c1.a && (p1 - p0).abs() > 1e-6 {
            for k in 1..STEPS {
                let f = k as f32 / STEPS as f32;
                out.push((p0 + (p1 - p0) * f, lerp_color_premul(c0, c1, f)));
            }
        }
        out.push(w[1]);
    }
    out
}

/// Premultiplied linear interpolation between two straight RGBA8 colours
/// (CSS Images L4 §3.1): interpolate the premultiplied channels, then
/// un-premultiply. A fully-transparent result collapses to `rgba(0,0,0,0)`.
#[must_use]
pub fn lerp_color_premul(a: Color, b: Color, f: f32) -> Color {
    let aa = a.a as f32 / 255.0;
    let ba = b.a as f32 / 255.0;
    let lin = |x: f32, y: f32| x + (y - x) * f;
    let pr = lin(a.r as f32 * aa, b.r as f32 * ba);
    let pg = lin(a.g as f32 * aa, b.g as f32 * ba);
    let pb = lin(a.b as f32 * aa, b.b as f32 * ba);
    let pa = lin(aa, ba);
    if pa <= 1e-6 {
        return Color { r: 0, g: 0, b: 0, a: 0 };
    }
    let un = |p: f32| (p / pa).round().clamp(0.0, 255.0) as u8;
    Color { r: un(pr), g: un(pg), b: un(pb), a: (pa * 255.0).round().clamp(0.0, 255.0) as u8 }
}

/// Sample a resolved gradient stop list at position `t` (straight-colour linear
/// interpolation), mirroring the GPU `sample_grad`: `repeating` wraps `t` to
/// `[0,1)`, otherwise it clamps; positions outside the first/last stop take the
/// boundary colour.
#[must_use]
pub fn sample_gradient_color(resolved: &[(f32, Color)], t: f32, repeating: bool) -> Color {
    let n = resolved.len();
    if n == 0 {
        return Color { r: 0, g: 0, b: 0, a: 0 };
    }
    if n == 1 {
        return resolved[0].1;
    }
    let tc = if repeating { t - t.floor() } else { t.clamp(0.0, 1.0) };
    if tc <= resolved[0].0 {
        return resolved[0].1;
    }
    let last = n - 1;
    if tc >= resolved[last].0 {
        return resolved[last].1;
    }
    for i in 0..last {
        let (ap, ac) = resolved[i];
        let (bp, bc) = resolved[i + 1];
        if tc >= ap && tc <= bp {
            let s = bp - ap;
            let f = if s > 1e-4 { (tc - ap) / s } else { 0.0 };
            return lerp_color(ac, bc, f);
        }
    }
    resolved[last].1
}

/// Linear interpolation between two straight (non-premultiplied) RGBA8 colours.
#[must_use]
pub fn lerp_color(a: Color, b: Color, f: f32) -> Color {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f).round().clamp(0.0, 255.0) as u8;
    Color { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: l(a.a, b.a) }
}

/// CSS Images L4 §3.7 — отображает долю оборота `t` ∈ [0,1) в позицию сэмпла
/// внутри диапазона stop-ов градиента.
///
/// Для `repeating-conic-gradient` паттерн повторяется каждые (last − first)
/// доли оборота: `t` сворачивается в `[first, first+span)` через `rem_euclid`.
/// Для не-repeating (или вырожденного нулевого span) возвращает `t` без
/// изменений.
#[must_use]
pub fn conic_sample_t(t: f32, repeating: bool, first_pos: f32, last_pos: f32) -> f32 {
    let span = last_pos - first_pos;
    if repeating && span > 1e-6 {
        first_pos + (t - first_pos).rem_euclid(span)
    } else {
        t
    }
}

/// Deterministic `atan2(y, x)` returning radians in `(-π, π]`.
///
/// Pure approximation (Rajan's formula) using only IEEE-exact ops
/// (`+`,`-`,`*`,`/`,`min`,`max`,`abs`) — no platform libm — so the result is
/// bit-identical across Windows/macOS/Linux. Accuracy ≈ 0.004 rad, ample for an
/// angular gradient whose reference PNG is self-generated by this same path.
#[must_use]
pub fn atan2_det(y: f32, x: f32) -> f32 {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    let ax = x.abs();
    let ay = y.abs();
    let max = ax.max(ay);
    if max == 0.0 {
        return 0.0;
    }
    let a = ax.min(ay) / max; // tan of the smaller angle, [0, 1]
    let mut r = a * FRAC_PI_4 + 0.273 * a * (1.0 - a); // ≈ atan(a)
    if ay > ax {
        r = FRAC_PI_2 - r; // reflect across the 45° line
    }
    if x < 0.0 {
        r = PI - r;
    }
    if y < 0.0 {
        r = -r;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(color: Color, pos: Option<Length>) -> GradientStop {
        GradientStop { color, position: pos }
    }

    const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };
    const BLUE: Color = Color { r: 0, g: 0, b: 255, a: 255 };
    const GREEN: Color = Color { r: 0, g: 255, b: 0, a: 255 };

    // ── resolve_stop_positions ───────────────────────────────────────────────

    #[test]
    fn resolve_evenly_spaces_unpositioned_stops() {
        let stops = [stop(RED, None), stop(GREEN, None), stop(BLUE, None)];
        let r = resolve_stop_positions(&stops, 100.0);
        assert_eq!(r.len(), 3);
        assert!((r[0].0 - 0.0).abs() < 1e-6);
        assert!((r[1].0 - 0.5).abs() < 1e-6);
        assert!((r[2].0 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn resolve_keeps_explicit_positions() {
        let stops = [
            stop(RED, Some(Length::Percent(10.0))),
            stop(BLUE, Some(Length::Percent(90.0))),
        ];
        let r = resolve_stop_positions(&stops, 100.0);
        assert!((r[0].0 - 0.1).abs() < 1e-6);
        assert!((r[1].0 - 0.9).abs() < 1e-6);
    }

    #[test]
    fn resolve_px_stops_divide_by_line_len() {
        let stops = [
            stop(RED, Some(Length::Px(0.0))),
            stop(BLUE, Some(Length::Px(50.0))),
        ];
        let r = resolve_stop_positions(&stops, 200.0);
        assert!((r[0].0 - 0.0).abs() < 1e-6);
        assert!((r[1].0 - 0.25).abs() < 1e-6);
    }

    #[test]
    fn resolve_does_not_clamp_out_of_range() {
        // repeating-градиентам нужны позиции > 1.0 (px-стопы за концом линии).
        let stops = [
            stop(RED, Some(Length::Px(0.0))),
            stop(BLUE, Some(Length::Px(300.0))),
        ];
        let r = resolve_stop_positions(&stops, 200.0);
        assert!((r[1].0 - 1.5).abs() < 1e-6);
    }

    #[test]
    fn resolve_empty_input_is_empty() {
        assert!(resolve_stop_positions(&[], 100.0).is_empty());
    }

    #[test]
    fn resolve_calc_stop_resolves_against_line_len() {
        // BUG-230: `calc(50% + 10px)` must resolve its `%` against the gradient
        // line length, then normalize by it: (100 + 10) / 200 = 0.55. Before the
        // fix the Calc position fell to `_ => 0.0`, collapsing the gradient.
        use lumen_layout::CalcNode;
        let calc = Length::Calc(Box::new(CalcNode::Add(
            Box::new(CalcNode::Length(Length::Percent(50.0))),
            Box::new(CalcNode::Length(Length::Px(10.0))),
        )));
        let stops = [stop(RED, Some(Length::Px(0.0))), stop(BLUE, Some(calc))];
        let r = resolve_stop_positions(&stops, 200.0);
        assert!((r[1].0 - 0.55).abs() < 1e-6, "calc stop = {}, want 0.55", r[1].0);
    }

    // ── premultiplied_subdivide_stops (BUG-190) ──────────────────────────────

    const TRANSPARENT: Color = Color { r: 0, g: 0, b: 0, a: 0 };

    #[test]
    fn subdivide_opaque_segment_unchanged() {
        // Equal alpha (both opaque) → premul == straight → emit verbatim.
        let resolved = vec![(0.0, RED), (1.0, BLUE)];
        assert_eq!(premultiplied_subdivide_stops(&resolved), resolved);
    }

    #[test]
    fn subdivide_inserts_stops_for_alpha_varying_segment() {
        let orange = Color { r: 255, g: 200, b: 100, a: 204 }; // rgba(...,0.8)
        let resolved = vec![(0.0, orange), (0.7, TRANSPARENT)];
        let out = premultiplied_subdivide_stops(&resolved);
        // endpoints preserved + 15 interior stops (STEPS-1).
        assert_eq!(out.len(), 2 + 15);
        assert_eq!(out.first().unwrap().1, orange);
        assert_eq!(out.last().unwrap().1, TRANSPARENT);
    }

    #[test]
    fn subdivide_fade_to_transparent_keeps_hue() {
        // orange → `transparent` (rgba 0,0,0,0): premultiplied interpolation must
        // hold the orange hue while alpha falls, NOT drift toward black.
        let orange = Color { r: 255, g: 200, b: 100, a: 204 };
        let out = premultiplied_subdivide_stops(&[(0.0, orange), (1.0, TRANSPARENT)]);
        for &(_, c) in &out {
            if c.a == 0 {
                continue; // fully transparent endpoint carries no colour
            }
            // Hue (R:G:B ratio) stays orange to within rounding.
            assert!(c.r >= c.g && c.g >= c.b, "channel order lost: {c:?}");
            assert!((c.r as i32 - 255).abs() <= 1, "R drifted: {c:?}");
            assert!((c.g as i32 - 200).abs() <= 1, "G drifted: {c:?}");
            assert!((c.b as i32 - 100).abs() <= 1, "B drifted: {c:?}");
        }
    }

    #[test]
    fn lerp_premul_constant_hue_for_color_to_transparent() {
        let orange = Color { r: 255, g: 200, b: 100, a: 204 };
        let mid = lerp_color_premul(orange, TRANSPARENT, 0.5);
        assert_eq!((mid.r, mid.g, mid.b), (255, 200, 100));
        assert_eq!(mid.a, 102); // alpha halved (0.8 → 0.4)
    }

    #[test]
    fn lerp_premul_fully_transparent_collapses() {
        let a = Color { r: 10, g: 20, b: 30, a: 0 };
        let b = Color { r: 200, g: 100, b: 50, a: 0 };
        assert_eq!(lerp_color_premul(a, b, 0.5), TRANSPARENT);
    }

    #[test]
    fn lerp_premul_equal_alpha_matches_straight() {
        // Constant alpha → premultiplied result equals straight interpolation.
        let a = Color { r: 0, g: 0, b: 0, a: 128 };
        let b = Color { r: 255, g: 255, b: 255, a: 128 };
        let premul = lerp_color_premul(a, b, 0.5);
        let straight = lerp_color(a, b, 0.5);
        assert!((premul.r as i32 - straight.r as i32).abs() <= 1);
        assert_eq!(premul.a, straight.a);
    }

    // ── sample_gradient_color ────────────────────────────────────────────────

    #[test]
    fn sample_at_boundaries_returns_stop_colors() {
        let resolved = vec![(0.0, RED), (1.0, BLUE)];
        assert_eq!(sample_gradient_color(&resolved, 0.0, false), RED);
        assert_eq!(sample_gradient_color(&resolved, 1.0, false), BLUE);
    }

    #[test]
    fn sample_midpoint_interpolates() {
        let resolved = vec![(0.0, RED), (1.0, BLUE)];
        let mid = sample_gradient_color(&resolved, 0.5, false);
        assert_eq!(mid.r, 128);
        assert_eq!(mid.b, 128);
        assert_eq!(mid.a, 255);
    }

    #[test]
    fn sample_clamps_outside_range_when_not_repeating() {
        let resolved = vec![(0.2, RED), (0.8, BLUE)];
        assert_eq!(sample_gradient_color(&resolved, -1.0, false), RED);
        assert_eq!(sample_gradient_color(&resolved, 2.0, false), BLUE);
    }

    #[test]
    fn sample_wraps_when_repeating() {
        let resolved = vec![(0.0, RED), (1.0, BLUE)];
        // t=1.5 → wraps to 0.5
        let c = sample_gradient_color(&resolved, 1.5, true);
        assert_eq!(c.r, 128);
    }

    #[test]
    fn sample_single_stop_returns_it() {
        let resolved = vec![(0.5, GREEN)];
        assert_eq!(sample_gradient_color(&resolved, 0.9, false), GREEN);
    }

    #[test]
    fn sample_empty_returns_transparent() {
        let c = sample_gradient_color(&[], 0.5, false);
        assert_eq!(c.a, 0);
    }

    // ── conic_sample_t ───────────────────────────────────────────────────────

    #[test]
    fn conic_sample_t_non_repeating_is_identity() {
        assert!((conic_sample_t(0.0, false, 0.0, 0.25) - 0.0).abs() < 1e-6);
        assert!((conic_sample_t(0.7, false, 0.0, 0.25) - 0.7).abs() < 1e-6);
        assert!((conic_sample_t(1.0, false, 0.0, 0.25) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn conic_sample_t_repeating_tiles_pattern() {
        let (first, last) = (0.0, 0.25);
        assert!((conic_sample_t(0.05, true, first, last) - 0.05).abs() < 1e-6);
        assert!((conic_sample_t(0.30, true, first, last) - 0.05).abs() < 1e-6);
        assert!((conic_sample_t(0.55, true, first, last) - 0.05).abs() < 1e-6);
        assert!((conic_sample_t(0.875, true, first, last) - 0.125).abs() < 1e-6);
    }

    #[test]
    fn conic_sample_t_repeating_zero_span_is_identity() {
        assert!((conic_sample_t(0.6, true, 0.5, 0.5) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn conic_sample_t_repeating_nonzero_first() {
        let v = conic_sample_t(0.75, true, 0.1, 0.4);
        assert!((v - 0.15).abs() < 1e-5, "0.75 folds into [0.1, 0.4): got {v}");
    }

    // ── atan2_det ────────────────────────────────────────────────────────────

    #[test]
    fn atan2_det_matches_libm_within_tolerance() {
        for &(y, x) in &[
            (0.0f32, 1.0f32),
            (1.0, 0.0),
            (1.0, 1.0),
            (-1.0, 1.0),
            (1.0, -1.0),
            (-1.0, -1.0),
            (0.5, -2.0),
            (-3.0, 0.25),
        ] {
            let approx = atan2_det(y, x);
            let exact = y.atan2(x);
            assert!(
                (approx - exact).abs() < 0.005,
                "atan2_det({y}, {x}) = {approx}, libm = {exact}"
            );
        }
    }

    #[test]
    fn atan2_det_origin_is_zero() {
        assert_eq!(atan2_det(0.0, 0.0), 0.0);
    }
}
