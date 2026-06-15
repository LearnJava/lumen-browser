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
