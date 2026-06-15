//! Общая dash/dot-геометрия для border/outline во всех рендер-бэкендах.
//!
//! Единственный источник истины для разбиения стороны рамки на сегменты
//! (CSS Backgrounds L3 §4.2, `border-style: dashed | dotted`). До PA-1
//! алгоритм был продублирован в `backends/femtovg_backend.rs` и инлайн в
//! `renderer.rs::emit_border_side`, а `cpu_raster.rs` вовсе игнорировал
//! `BorderStyle` (BUG-080) — см. docs/paint-pipeline-review-2026-06.md,
//! Key finding 2. PA-5 подключает эти функции в cpu_raster.
//!
//! Все функции возвращают пары `(offset, length)` вдоль стороны длиной
//! `total`; ориентацию (горизонталь/вертикаль) и форму точки (квадрат/круг)
//! выбирает вызывающий бэкенд.

/// Returns `(offset, length)` pairs along a border side of length `total` for a
/// `dashed` border of thickness `width`.
///
/// Chrome/Edge (Skia): full side width, `n = round(total / period)`, leading=0.
/// Dash size is fixed at `max(6, 2·width)`, gap at `max(4, width)`; only the
/// step is adjusted so the last dash ends exactly at `total`. Offsets use
/// `floor()` to match Edge pixel-snapping. Reproduces Edge's observed counts:
/// 2px→18, 4px→15, 8px→8, 16px→4 dashes on a 180px side. Empty when
/// `total <= 0`.
#[must_use]
pub fn dashed_border_offsets(total: f32, width: f32) -> Vec<(f32, f32)> {
    if total <= 0.0 {
        return Vec::new();
    }
    let target_dash = (width * 2.0).max(6.0);
    let target_gap = width.max(4.0);
    let n = ((total / (target_dash + target_gap)).round() as usize).max(1);
    let step = if n > 1 { (total - target_dash) / (n - 1) as f32 } else { 0.0 };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let offset = (i as f32 * step).floor();
        let seg_end = (offset + target_dash).min(total);
        if seg_end > offset {
            out.push((offset, seg_end - offset));
        }
    }
    out
}

/// Returns `(offset, length)` pairs along a border side of length `total` for a
/// `dotted` border of thickness `width`.
///
/// Chrome/Edge (Skia): `n = floor(total / (2·dot)) + 1` dots, symmetric
/// placement (`floor(i·step)` for the first half, mirrored for the second) —
/// short gaps at both ends, equal middle gaps. `dot = max(1, width)`.
/// Reproduces Edge's observed counts: 2px→46, 4px→23, 8px→12, 16px→6 dots on a
/// 180px side. The caller decides square (≤2px) vs round rendering. Empty when
/// `total <= 0`.
#[must_use]
pub fn dotted_border_offsets(total: f32, width: f32) -> Vec<(f32, f32)> {
    if total <= 0.0 {
        return Vec::new();
    }
    let dot_len = width.max(1.0);
    let n = ((total / (dot_len * 2.0)).floor() as usize + 1).max(1);
    let span = total - dot_len;
    let step = if n > 1 { span / (n - 1) as f32 } else { 0.0 };
    let mid = (n - 1) / 2;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let offset = if i <= mid {
            (i as f32 * step).floor()
        } else {
            let j = (n - 1 - i) as f32;
            span.floor() - (j * step).floor()
        };
        let seg_end = (offset + dot_len).min(total);
        if seg_end > offset {
            out.push((offset, seg_end - offset));
        }
    }
    out
}

/// Разбивает полосу длиной `total_length` на серию dash-сегментов
/// `(offset, length)` по pattern-у `(dash_len, gap_len)`. Совпадает с
/// Chrome/Edge (Skia): `n = floor(total / period)`, `leading = gap / 2`.
/// Используется для `outline-style: dashed | dotted` (wgpu `emit_outline_side`).
///
/// Возвращает empty при degenerate-входе: `total_length <= 0`,
/// `dash_len <= 0`. При `gap_len <= 0` возвращает один full-length сегмент
/// (= Solid fallback). Если полоса короче одного даша, возвращает один
/// сегмент с offset=0.
#[must_use]
pub fn dash_segments(total_length: f32, dash_len: f32, gap_len: f32) -> Vec<(f32, f32)> {
    if total_length <= 0.0 || dash_len <= 0.0 {
        return Vec::new();
    }
    if gap_len <= 0.0 {
        return vec![(0.0, total_length)];
    }
    let period = dash_len + gap_len;
    let n_floor = (total_length / period).floor() as i32;
    let n_dashes = n_floor.max(1) as usize;
    // leading=gap/2 matches Chrome/Edge (Skia) phase offset.
    // For too-short fallback (n_floor<1) start at corner (offset=0).
    let leading = if n_floor >= 1 { gap_len * 0.5 } else { 0.0 };
    let mut out = Vec::with_capacity(n_dashes);
    let mut x = leading;
    for _ in 0..n_dashes {
        let seg_start = x.max(0.0);
        let seg_end = (x + dash_len).min(total_length);
        if seg_end > seg_start {
            out.push((seg_start, seg_end - seg_start));
        }
        x += period;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashed_matches_edge_counts_on_180px_side() {
        assert_eq!(dashed_border_offsets(180.0, 2.0).len(), 18);
        assert_eq!(dashed_border_offsets(180.0, 4.0).len(), 15);
        assert_eq!(dashed_border_offsets(180.0, 8.0).len(), 8);
        assert_eq!(dashed_border_offsets(180.0, 16.0).len(), 4);
    }

    #[test]
    fn dashed_last_dash_ends_at_total() {
        let segs = dashed_border_offsets(180.0, 4.0);
        let (off, len) = *segs.last().unwrap();
        assert!((off + len - 180.0).abs() < 1.0, "last dash must end at total: {off}+{len}");
    }

    #[test]
    fn dotted_matches_edge_counts_on_180px_side() {
        assert_eq!(dotted_border_offsets(180.0, 2.0).len(), 46);
        assert_eq!(dotted_border_offsets(180.0, 4.0).len(), 23);
        assert_eq!(dotted_border_offsets(180.0, 8.0).len(), 12);
        assert_eq!(dotted_border_offsets(180.0, 16.0).len(), 6);
    }

    #[test]
    fn dotted_is_symmetric() {
        let segs = dotted_border_offsets(180.0, 8.0);
        let first = segs.first().unwrap().0;
        let (last_off, last_len) = *segs.last().unwrap();
        let trailing = 180.0 - (last_off + last_len);
        assert!((first - trailing).abs() <= 1.0, "symmetric ends: lead {first}, trail {trailing}");
    }

    #[test]
    fn degenerate_inputs_are_empty() {
        assert!(dashed_border_offsets(0.0, 4.0).is_empty());
        assert!(dotted_border_offsets(-5.0, 4.0).is_empty());
        assert!(dash_segments(0.0, 6.0, 3.0).is_empty());
        assert!(dash_segments(100.0, 0.0, 3.0).is_empty());
    }

    #[test]
    fn dash_segments_zero_gap_is_solid() {
        assert_eq!(dash_segments(50.0, 6.0, 0.0), vec![(0.0, 50.0)]);
    }

    #[test]
    fn dash_segments_leading_is_half_gap() {
        let segs = dash_segments(100.0, 6.0, 4.0);
        assert!(!segs.is_empty());
        assert!((segs[0].0 - 2.0).abs() < 1e-6, "leading = gap/2: {}", segs[0].0);
    }

    #[test]
    fn dash_segments_too_short_gives_single_dash_at_origin() {
        let segs = dash_segments(4.0, 6.0, 4.0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].0, 0.0);
        assert!((segs[0].1 - 4.0).abs() < 1e-6);
    }

    #[test]
    fn dash_segments_exact_fit() {
        // dash=4, gap=2 → period=6; total=10 → floor(10/6)=1 dash;
        // leading=gap/2=1; сегмент: (1, 4).
        let segs = dash_segments(10.0, 4.0, 2.0);
        assert_eq!(segs.len(), 1);
        assert!((segs[0].0 - 1.0).abs() < 1e-6);
        assert!((segs[0].1 - 4.0).abs() < 1e-6);
    }

    #[test]
    fn dash_segments_centered_leftover() {
        // dash=2, gap=2 → period=4; total=10 → floor(10/4)=2 dashes;
        // leading=gap/2=1; сегменты (1,2),(5,2).
        let segs = dash_segments(10.0, 2.0, 2.0);
        assert_eq!(segs, vec![(1.0, 2.0), (5.0, 2.0)]);
    }

    #[test]
    fn dash_segments_count_for_typical_outline() {
        // Outline width=2, dashed: dash=4, gap=2; полоса 100 px.
        // n=floor(100/6)=16 dashes; leading=1.
        let segs = dash_segments(100.0, 4.0, 2.0);
        assert_eq!(segs.len(), 16);
    }
}
