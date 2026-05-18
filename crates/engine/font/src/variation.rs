//! Variable fonts runtime: применение gvar deltas к glyph outline.
//!
//! Mapping реальной задачи на код:
//! - parser `gvar` уже разбирает per-glyph TupleVariation-ы — список
//!   (peak, intermediate, points, x_deltas, y_deltas).
//! - `tuple_scalar(coords, variation)` уже считает scalar по tent-функции
//!   peak / intermediate.
//! - **Этот модуль** реализует *runtime applicator*: берёт `&mut [Contour]`
//!   и набор variations + normalized axis coords, и сдвигает outline-точки
//!   так, чтобы получился глиф для текущей instance variation-space.
//!
//! Алгоритм (OpenType spec, "Glyph Variation Data" → "Applying variation
//! deltas to glyph contour points"):
//!
//! Для каждой `TupleVariation` v:
//!
//! - `scalar = tuple_scalar(coords, v)`. Если 0 — skip variation.
//! - Если `v.points == All` — каждая outline-точка контуров получает
//!   `dx += scalar · v.x_deltas[i]`, `dy += scalar · v.y_deltas[i]`
//!   (per-glyph аккумулятор). Phantom-точки (`>= total_outline_points`)
//!   пропускаются — Phase 0 outline-rendering их не использует.
//! - Если `v.points == Explicit(list)` — отметить эти точки как
//!   *touched*. Untouched точки (в каждом контуре, per-axis) получают
//!   delta через **IUP** (Interpolation of Untouched Points; см. ниже).
//!
//! IUP для untouched-точки `u` между двумя touched-соседями `prev` / `next`
//! (cyclically в пределах контура):
//!
//! - если `orig_u <= min(orig_p, orig_n)` → берём delta соседа с меньшим
//!   `orig` (точка лежит "за" обоими — shift вместе с ним);
//! - если `orig_u >= max(orig_p, orig_n)` → delta соседа с большим `orig`
//!   (симметрично);
//! - иначе linear interpolate: `t = (orig_u - orig_p)/(orig_n - orig_p)`,
//!   `delta = (1-t)·delta_p + t·delta_n`.
//!
//! Если в контуре только один touched (или `prev_touched == next_touched`):
//! все untouched точки контура смещаются на ту же дельту, что touched.
//! Если в контуре 0 touched — variation не вкладывает delta в этот контур.
//!
//! IUP применяется *независимо* по осям X и Y (`orig_x` для X-IUP,
//! `orig_y` для Y-IUP). Spec явно: "the same procedure is applied
//! separately to both X and Y coordinates".
//!
//! Все variations накапливаются в один `(dx, dy)` per-point буфер,
//! который в конце прибавляется к base-координатам. Это совпадает с
//! линейностью variation-space — две variations с пересекающимися
//! touched-наборами дают сумму своих эффектов.
//!
//! Phase 0 ограничения:
//! - Phantom points (LSB / RSB / TSB / BSB, 4 штуки за outline) не
//!   применяются — они нужны только для advance/sidebearing, что
//!   обрабатывается через `HVAR`/`VVAR` отдельно. Explicit-индексы,
//!   ссылающиеся на phantom (`>= outline_point_count`), молча
//!   пропускаются.
//! - Round-to-grid (`postScript hinting`) после варьирования не
//!   применяется — outline остаётся в design-units (i16). Sub-pixel
//!   accuracy дешевле просто сохранить через f32 на момент rasterize.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/gvar>
//! ("Inferred Deltas for Un-Referenced Point Numbers" + "Applying
//! Variations to Glyph Contours").

use crate::glyf::Contour;
use crate::gvar::{PointNumbers, TupleVariation, tuple_scalar};

/// Применяет набор `TupleVariation` к outline-контурам, имитируя
/// glyph-instance для заданных normalized axis coordinates `coords`.
///
/// `contours` мутируется in-place: каждой `OutlinePoint.{x, y}` приписана
/// final variation-position (база + scaled deltas с IUP-инференсом).
///
/// Длина `coords` должна совпадать с `axis_count` font-а (тот же
/// `axis_count`, который был в `gvar.parse`). Если variation
/// peak/intermediate имеет другую длину — variation тихо пропускается
/// (defensive — malformed font).
///
/// Возвращает количество применённых variations (для диагностики /
/// тестов). Variation считается применённой, если её `scalar != 0` и
/// она внесла хоть один не-нулевой delta в аккумулятор.
pub fn apply_variations_to_simple_outline(
    contours: &mut [Contour],
    variations: &[TupleVariation],
    coords: &[f32],
) -> usize {
    let total_points: usize = contours.iter().map(|c| c.points.len()).sum();
    if total_points == 0 || variations.is_empty() {
        return 0;
    }

    let mut acc_dx = vec![0.0_f32; total_points];
    let mut acc_dy = vec![0.0_f32; total_points];

    // Снимок base-координат для IUP (variations не должны "видеть" друг
    // друга — каждая работает над оригинальной outline).
    let orig_x: Vec<i16> = contours
        .iter()
        .flat_map(|c| c.points.iter().map(|p| p.x))
        .collect();
    let orig_y: Vec<i16> = contours
        .iter()
        .flat_map(|c| c.points.iter().map(|p| p.y))
        .collect();

    // Конец каждого контура (exclusive index в flat-array).
    let mut contour_ends: Vec<usize> = Vec::with_capacity(contours.len());
    let mut running = 0;
    for c in contours.iter() {
        running += c.points.len();
        contour_ends.push(running);
    }

    let mut applied = 0;
    for v in variations {
        // Защита от malformed: peak / intermediate должны соответствовать coords.
        if v.peak.len() != coords.len() {
            continue;
        }
        if let Some((start, end)) = &v.intermediate
            && (start.len() != coords.len() || end.len() != coords.len())
        {
            continue;
        }

        let scalar = tuple_scalar(coords, v);
        if scalar == 0.0 {
            continue;
        }

        let contributed = apply_single_variation(
            v,
            scalar,
            &orig_x,
            &orig_y,
            &contour_ends,
            &mut acc_dx,
            &mut acc_dy,
        );
        if contributed {
            applied += 1;
        }
    }

    if applied == 0 {
        return 0;
    }

    // Apply accumulator to base positions. Round-half-away-from-zero
    // (стандарт OpenType при квантовании в design-units).
    let mut flat_idx = 0;
    for c in contours.iter_mut() {
        for p in c.points.iter_mut() {
            let new_x = (p.x as f32) + acc_dx[flat_idx];
            let new_y = (p.y as f32) + acc_dy[flat_idx];
            p.x = round_half_away_from_zero(new_x);
            p.y = round_half_away_from_zero(new_y);
            flat_idx += 1;
        }
    }

    applied
}

/// Применяет одну `TupleVariation` к аккумулятору. Возвращает `true`,
/// если variation реально вложила что-то ненулевое (для All-точек или
/// non-empty Explicit с IUP-распространением).
fn apply_single_variation(
    v: &TupleVariation,
    scalar: f32,
    orig_x: &[i16],
    orig_y: &[i16],
    contour_ends: &[usize],
    acc_dx: &mut [f32],
    acc_dy: &mut [f32],
) -> bool {
    let total_points = orig_x.len();
    debug_assert_eq!(orig_x.len(), orig_y.len());
    debug_assert_eq!(orig_x.len(), acc_dx.len());

    match &v.points {
        PointNumbers::All => {
            // x_deltas / y_deltas длиной (total_outline_points + 4 phantom).
            // Берём первые total_points, остальные — phantom, пропускаем.
            let take = v.x_deltas.len().min(total_points);
            if v.y_deltas.len() < take {
                return false;
            }
            let mut contributed = false;
            for i in 0..take {
                let dx = scalar * v.x_deltas[i] as f32;
                let dy = scalar * v.y_deltas[i] as f32;
                if dx != 0.0 {
                    acc_dx[i] += dx;
                    contributed = true;
                }
                if dy != 0.0 {
                    acc_dy[i] += dy;
                    contributed = true;
                }
            }
            contributed
        }
        PointNumbers::Explicit(points) => {
            if points.is_empty() {
                return false;
            }
            // Build per-point scaled-delta vectors с маской touched.
            let mut touched = vec![false; total_points];
            let mut tdx = vec![0.0_f32; total_points];
            let mut tdy = vec![0.0_f32; total_points];

            let pairs = points.len().min(v.x_deltas.len()).min(v.y_deltas.len());
            for (k, &raw_idx) in points.iter().take(pairs).enumerate() {
                let idx = raw_idx as usize;
                if idx >= total_points {
                    // Phantom-index — пропускаем (Phase 0 не применяем deltas
                    // к LSB/RSB/TSB/BSB; HVAR/VVAR делает это отдельно).
                    continue;
                }
                tdx[idx] = scalar * v.x_deltas[k] as f32;
                tdy[idx] = scalar * v.y_deltas[k] as f32;
                touched[idx] = true;
            }

            // IUP per contour, per axis. Если в контуре нет ни одной touched
            // точки — variation не вкладывает в этот контур.
            let mut start = 0;
            for &end in contour_ends {
                if end > start {
                    iup_contour(&touched[start..end], &mut tdx[start..end], &orig_x[start..end]);
                    iup_contour(&touched[start..end], &mut tdy[start..end], &orig_y[start..end]);
                }
                start = end;
            }

            // Накапливаем.
            let mut contributed = false;
            for i in 0..total_points {
                if tdx[i] != 0.0 {
                    acc_dx[i] += tdx[i];
                    contributed = true;
                }
                if tdy[i] != 0.0 {
                    acc_dy[i] += tdy[i];
                    contributed = true;
                }
            }
            contributed
        }
    }
}

/// IUP per contour, per axis. `touched[i]` — была ли variation реально
/// указала точку `i`; `delta[i]` — её scaled delta (для touched) или 0
/// (для untouched на входе). Untouched точки получают inferred delta из
/// соседей; touched остаются без изменений.
///
/// `orig` — оригинальные координаты этой оси для этого контура.
///
/// Spec OpenType: untouched точка `u` между touched `p` и `n`:
/// - `orig_u <= min(orig_p, orig_n)` → delta_u = delta of point with smaller orig;
/// - `orig_u >= max(orig_p, orig_n)` → delta_u = delta of point with larger orig;
/// - иначе linear interpolate.
///
/// Если touched-точка единственная в контуре (или все touched —
/// `prev_touched == next_touched`): все untouched сдвигаются на ту же
/// дельту. Если в контуре 0 touched — variation просто не вкладывает.
fn iup_contour(touched: &[bool], delta: &mut [f32], orig: &[i16]) {
    let n = touched.len();
    debug_assert_eq!(delta.len(), n);
    debug_assert_eq!(orig.len(), n);
    if n == 0 {
        return;
    }
    let touched_count = touched.iter().filter(|&&t| t).count();
    if touched_count == 0 {
        return; // variation не вкладывает в этот контур
    }
    if touched_count == n {
        return; // всё уже touched
    }
    // Если только одна touched точка — все untouched сдвигаются на ту же.
    if touched_count == 1 {
        let only_idx = touched.iter().position(|&t| t).unwrap();
        let only_delta = delta[only_idx];
        for i in 0..n {
            if !touched[i] {
                delta[i] = only_delta;
            }
        }
        return;
    }

    // Множественные touched. Для каждой untouched точки находим ближайших
    // touched соседей prev (cyclically назад) и next (cyclically вперёд).
    for i in 0..n {
        if touched[i] {
            continue;
        }
        // prev touched (cyclically backwards)
        let prev = (1..=n)
            .find_map(|step| {
                let idx = (i + n - step) % n;
                if touched[idx] { Some(idx) } else { None }
            })
            .unwrap(); // touched_count >= 2, гарантированно найдём
        let next = (1..=n)
            .find_map(|step| {
                let idx = (i + step) % n;
                if touched[idx] { Some(idx) } else { None }
            })
            .unwrap();
        if prev == next {
            // Только один touched — обработали выше; sanity.
            delta[i] = delta[prev];
            continue;
        }
        let orig_u = orig[i] as f32;
        let orig_p = orig[prev] as f32;
        let orig_n = orig[next] as f32;
        let delta_p = delta[prev];
        let delta_n = delta[next];

        if (orig_p - orig_n).abs() < f32::EPSILON {
            // prev и next имеют одинаковую orig — нет градиента, берём delta_p
            // (или delta_n — они эквивалентны; если разные — spec считает их
            // colinear, берём prev как канонический).
            delta[i] = delta_p;
            continue;
        }

        let (lo_orig, lo_delta, hi_orig, hi_delta) = if orig_p < orig_n {
            (orig_p, delta_p, orig_n, delta_n)
        } else {
            (orig_n, delta_n, orig_p, delta_p)
        };

        if orig_u <= lo_orig {
            // точка "за" более маленьким соседом — берём его дельту
            delta[i] = lo_delta;
        } else if orig_u >= hi_orig {
            delta[i] = hi_delta;
        } else {
            let t = (orig_u - lo_orig) / (hi_orig - lo_orig);
            delta[i] = lo_delta + t * (hi_delta - lo_delta);
        }
    }
}

/// Round half away from zero — OpenType convention for design-unit
/// quantization. `f32::round` в Rust именно так и работает (round to
/// nearest, ties away from zero), но явно обернём, чтобы defensively
/// клипать к i16-диапазону.
fn round_half_away_from_zero(v: f32) -> i16 {
    let rounded = v.round();
    if rounded > i16::MAX as f32 {
        i16::MAX
    } else if rounded < i16::MIN as f32 {
        i16::MIN
    } else {
        rounded as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyf::OutlinePoint;

    fn pt(x: i16, y: i16) -> OutlinePoint {
        OutlinePoint { x, y, on_curve: true }
    }

    fn contour(points: &[OutlinePoint]) -> Contour {
        Contour {
            points: points.to_vec(),
        }
    }

    fn variation_all(
        axis_count: usize,
        peak: Vec<f32>,
        deltas: &[(i16, i16)],
    ) -> TupleVariation {
        assert_eq!(peak.len(), axis_count);
        TupleVariation {
            peak,
            intermediate: None,
            points: PointNumbers::All,
            x_deltas: deltas.iter().map(|&(x, _)| x).collect(),
            y_deltas: deltas.iter().map(|&(_, y)| y).collect(),
        }
    }

    fn variation_explicit(
        peak: Vec<f32>,
        indices: Vec<u16>,
        deltas: &[(i16, i16)],
    ) -> TupleVariation {
        assert_eq!(indices.len(), deltas.len());
        TupleVariation {
            peak,
            intermediate: None,
            points: PointNumbers::Explicit(indices),
            x_deltas: deltas.iter().map(|&(x, _)| x).collect(),
            y_deltas: deltas.iter().map(|&(_, y)| y).collect(),
        }
    }

    #[test]
    fn empty_inputs_noop() {
        let mut contours: Vec<Contour> = Vec::new();
        let applied = apply_variations_to_simple_outline(&mut contours, &[], &[]);
        assert_eq!(applied, 0);
    }

    #[test]
    fn empty_variations_noop() {
        let mut contours = vec![contour(&[pt(0, 0), pt(100, 0), pt(100, 100), pt(0, 100)])];
        let applied = apply_variations_to_simple_outline(&mut contours, &[], &[1.0]);
        assert_eq!(applied, 0);
        // outline не изменился
        assert_eq!(contours[0].points[0], pt(0, 0));
        assert_eq!(contours[0].points[2], pt(100, 100));
    }

    #[test]
    fn scalar_zero_skips_variation() {
        let mut contours = vec![contour(&[pt(0, 0), pt(100, 0), pt(50, 50)])];
        // peak = [1.0]; coords = [0.0] → scalar = 0
        let v = variation_all(1, vec![1.0], &[(10, 20), (10, 20), (10, 20)]);
        let applied = apply_variations_to_simple_outline(&mut contours, &[v], &[0.0]);
        assert_eq!(applied, 0);
        assert_eq!(contours[0].points[0], pt(0, 0));
    }

    #[test]
    fn all_points_full_scalar_applies_delta() {
        let mut contours = vec![contour(&[pt(0, 0), pt(100, 0), pt(50, 50)])];
        // peak = [1.0]; coords = [1.0] → scalar = 1.0
        let v = variation_all(1, vec![1.0], &[(10, 20), (-5, 30), (0, 0)]);
        let applied = apply_variations_to_simple_outline(&mut contours, &[v], &[1.0]);
        assert_eq!(applied, 1);
        assert_eq!(contours[0].points[0], pt(10, 20));
        assert_eq!(contours[0].points[1], pt(95, 30));
        assert_eq!(contours[0].points[2], pt(50, 50));
    }

    #[test]
    fn all_points_half_scalar_scales_delta() {
        let mut contours = vec![contour(&[pt(0, 0), pt(100, 0)])];
        // peak = [1.0]; coords = [0.5] → scalar = 0.5
        let v = variation_all(1, vec![1.0], &[(20, 0), (40, 0)]);
        let applied = apply_variations_to_simple_outline(&mut contours, &[v], &[0.5]);
        assert_eq!(applied, 1);
        // 0.5 * 20 = 10; 0.5 * 40 = 20
        assert_eq!(contours[0].points[0], pt(10, 0));
        assert_eq!(contours[0].points[1], pt(120, 0));
    }

    #[test]
    fn all_points_phantom_indices_ignored() {
        // outline = 2 точки; deltas в variation = 6 (2 outline + 4 phantom).
        let mut contours = vec![contour(&[pt(0, 0), pt(100, 0)])];
        let v = variation_all(
            1,
            vec![1.0],
            &[(5, 0), (10, 0), (99, 99), (99, 99), (99, 99), (99, 99)],
        );
        apply_variations_to_simple_outline(&mut contours, &[v], &[1.0]);
        assert_eq!(contours[0].points[0], pt(5, 0));
        assert_eq!(contours[0].points[1], pt(110, 0));
    }

    #[test]
    fn explicit_single_touched_propagates_to_whole_contour() {
        // 4 точки в контуре, touched = [0]; delta = (10, 5).
        // IUP с одним touched: остальные сдвигаются на ту же дельту.
        let mut contours = vec![contour(&[pt(0, 0), pt(100, 0), pt(100, 100), pt(0, 100)])];
        let v = variation_explicit(vec![1.0], vec![0], &[(10, 5)]);
        apply_variations_to_simple_outline(&mut contours, &[v], &[1.0]);
        assert_eq!(contours[0].points[0], pt(10, 5));
        assert_eq!(contours[0].points[1], pt(110, 5));
        assert_eq!(contours[0].points[2], pt(110, 105));
        assert_eq!(contours[0].points[3], pt(10, 105));
    }

    #[test]
    fn explicit_iup_linear_interpolation() {
        // 5 точек на горизонтальной линии: orig_x = 0, 25, 50, 75, 100.
        // touched = [0, 4] с delta_x = (0, 0) и (40, 0).
        // Точки 1, 2, 3 — untouched. По IUP:
        //   t1: orig 25 в [0, 100], t = 0.25 → delta = 10
        //   t2: orig 50 → t = 0.5 → delta = 20
        //   t3: orig 75 → t = 0.75 → delta = 30
        let mut contours = vec![contour(&[
            pt(0, 0),
            pt(25, 0),
            pt(50, 0),
            pt(75, 0),
            pt(100, 0),
        ])];
        let v = variation_explicit(vec![1.0], vec![0, 4], &[(0, 0), (40, 0)]);
        apply_variations_to_simple_outline(&mut contours, &[v], &[1.0]);
        assert_eq!(contours[0].points[0], pt(0, 0));
        assert_eq!(contours[0].points[1], pt(35, 0)); // 25 + 10
        assert_eq!(contours[0].points[2], pt(70, 0)); // 50 + 20
        assert_eq!(contours[0].points[3], pt(105, 0)); // 75 + 30
        assert_eq!(contours[0].points[4], pt(140, 0)); // 100 + 40
    }

    #[test]
    fn explicit_iup_clamp_below_min() {
        // Контур: orig_x = 10, 0, 30, 100. touched = [1, 2] с delta (5, 15).
        // Точка 0: orig 10. cyclically prev_touched = 2 (orig 30), next_touched = 1 (orig 0).
        //   lo = orig 0 (delta 5), hi = orig 30 (delta 15). orig_u 10 — между ними.
        //   t = (10-0)/(30-0) = 0.333 → delta = 5 + 0.333*10 = 8.33 → round → 8
        // Точка 3: orig 100. prev_touched = 2 (orig 30, delta 15), next = 1 (orig 0, delta 5).
        //   lo = orig 0 (delta 5), hi = orig 30 (delta 15). orig 100 >= hi → delta = hi_delta = 15.
        let mut contours = vec![contour(&[pt(10, 0), pt(0, 0), pt(30, 0), pt(100, 0)])];
        let v = variation_explicit(vec![1.0], vec![1, 2], &[(5, 0), (15, 0)]);
        apply_variations_to_simple_outline(&mut contours, &[v], &[1.0]);
        assert_eq!(contours[0].points[0].x, 18); // 10 + 8.33 → round → 18
        assert_eq!(contours[0].points[1].x, 5); // 0 + 5
        assert_eq!(contours[0].points[2].x, 45); // 30 + 15
        assert_eq!(contours[0].points[3].x, 115); // 100 + 15 (clamped to hi)
    }

    #[test]
    fn explicit_iup_clamp_above_max() {
        // 3 точки: orig_x = 0, 50, 100. touched = [0, 1] с delta (10, 20).
        // Точка 2: orig 100. cyclically prev_touched = 1 (orig 50, delta 20),
        //   next_touched = 0 (orig 0, delta 10).
        //   lo = orig 0 (delta 10), hi = orig 50 (delta 20). orig 100 >= hi → delta 20.
        let mut contours = vec![contour(&[pt(0, 0), pt(50, 0), pt(100, 0)])];
        let v = variation_explicit(vec![1.0], vec![0, 1], &[(10, 0), (20, 0)]);
        apply_variations_to_simple_outline(&mut contours, &[v], &[1.0]);
        assert_eq!(contours[0].points[2].x, 120); // 100 + 20 (clamped to delta of point with max orig)
    }

    #[test]
    fn explicit_iup_no_touched_in_contour_leaves_alone() {
        // Контур 1: 3 точки. Контур 2: 2 точки.
        // touched = [0, 1] (оба в контуре 1).
        // Контур 2 не имеет touched → не сдвигается.
        let mut contours = vec![
            contour(&[pt(0, 0), pt(50, 0), pt(100, 0)]),
            contour(&[pt(200, 200), pt(300, 300)]),
        ];
        let v = variation_explicit(vec![1.0], vec![0, 1], &[(10, 0), (20, 0)]);
        apply_variations_to_simple_outline(&mut contours, &[v], &[1.0]);
        // Контур 1: touched получают дельту, IUP для точки 2.
        assert_eq!(contours[0].points[0].x, 10);
        assert_eq!(contours[0].points[1].x, 70);
        // Контур 2: не тронут.
        assert_eq!(contours[1].points[0], pt(200, 200));
        assert_eq!(contours[1].points[1], pt(300, 300));
    }

    #[test]
    fn multiple_variations_accumulate() {
        // Две variations на 2D-axis с пересекающимися эффектами.
        let mut contours = vec![contour(&[pt(0, 0), pt(100, 100)])];
        let v1 = variation_all(2, vec![1.0, 0.0], &[(10, 0), (10, 0)]);
        let v2 = variation_all(2, vec![0.0, 1.0], &[(0, 5), (0, 5)]);
        let applied = apply_variations_to_simple_outline(&mut contours, &[v1, v2], &[1.0, 1.0]);
        assert_eq!(applied, 2);
        assert_eq!(contours[0].points[0], pt(10, 5));
        assert_eq!(contours[0].points[1], pt(110, 105));
    }

    #[test]
    fn iup_uses_original_not_accumulated() {
        // Две variations с разными touched-наборами. IUP во второй должна
        // смотреть на base-orig, а не на сдвинутые v1 координаты.
        // outline: 3 точки orig_x = 0, 50, 100. v1 touched [0,2] с delta (10, 30),
        // v2 touched [0,2] с delta (5, 5). Обе variations используют base orig.
        let mut contours = vec![contour(&[pt(0, 0), pt(50, 0), pt(100, 0)])];
        let v1 = variation_explicit(vec![1.0], vec![0, 2], &[(10, 0), (30, 0)]);
        let v2 = variation_explicit(vec![1.0], vec![0, 2], &[(5, 0), (5, 0)]);
        apply_variations_to_simple_outline(&mut contours, &[v1, v2], &[1.0]);
        // v1: orig 0 → +10; orig 50 → IUP t=0.5 → +20; orig 100 → +30
        // v2: orig 0 → +5; orig 50 → IUP t=0.5 → +5 (linear от 5 до 5); orig 100 → +5
        // total: 0→15, 50→25 (50+20+5=75? no — 50+20 then +5 → 75)
        // Hmm — base 50 + 20 (v1 IUP) + 5 (v2 IUP) = 75
        assert_eq!(contours[0].points[0].x, 15);
        assert_eq!(contours[0].points[1].x, 75);
        assert_eq!(contours[0].points[2].x, 135);
    }

    #[test]
    fn intermediate_region_partial_scalar() {
        // peak=[1.0], intermediate=([0.2], [0.8]); coords=[0.5].
        // По tuple_scalar tent: до peak линейный рост от start до peak,
        // от peak — линейный спад до end.
        // coords = 0.5 — между start 0.2 и peak 1.0; scalar = (0.5-0.2)/(1.0-0.2) = 0.375
        let mut contours = vec![contour(&[pt(0, 0), pt(100, 0)])];
        let v = TupleVariation {
            peak: vec![1.0],
            intermediate: Some((vec![0.2], vec![0.8])),
            points: PointNumbers::All,
            x_deltas: vec![100, 100],
            y_deltas: vec![0, 0],
        };
        apply_variations_to_simple_outline(&mut contours, &[v], &[0.5]);
        // scalar ≈ 0.375; delta_x = 37.5 → round-half-away → 38
        assert_eq!(contours[0].points[0].x, 38);
        assert_eq!(contours[0].points[1].x, 138);
    }

    #[test]
    fn axis_count_mismatch_silently_skips() {
        // peak длина 2, coords длина 1 — variation пропускается.
        let mut contours = vec![contour(&[pt(0, 0), pt(100, 0)])];
        let v = variation_all(2, vec![1.0, 1.0], &[(50, 0), (50, 0)]);
        let applied = apply_variations_to_simple_outline(&mut contours, &[v], &[1.0]);
        assert_eq!(applied, 0);
        assert_eq!(contours[0].points[0], pt(0, 0));
    }

    #[test]
    fn round_half_away_from_zero_basics() {
        assert_eq!(round_half_away_from_zero(0.0), 0);
        assert_eq!(round_half_away_from_zero(0.4), 0);
        assert_eq!(round_half_away_from_zero(0.5), 1);
        assert_eq!(round_half_away_from_zero(0.51), 1);
        assert_eq!(round_half_away_from_zero(-0.5), -1);
        assert_eq!(round_half_away_from_zero(-0.4), 0);
        assert_eq!(round_half_away_from_zero(1.5), 2);
        // Clamp.
        assert_eq!(round_half_away_from_zero(50000.0), i16::MAX);
        assert_eq!(round_half_away_from_zero(-50000.0), i16::MIN);
    }

    #[test]
    fn iup_contour_all_touched_unchanged() {
        let touched = vec![true, true, true];
        let mut delta = vec![1.0, 2.0, 3.0];
        let orig = vec![0_i16, 10, 20];
        iup_contour(&touched, &mut delta, &orig);
        assert_eq!(delta, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn iup_contour_no_touched_unchanged() {
        let touched = vec![false, false, false];
        let mut delta = vec![0.0, 0.0, 0.0];
        let orig = vec![0_i16, 10, 20];
        iup_contour(&touched, &mut delta, &orig);
        assert_eq!(delta, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn iup_contour_cyclic_wrap() {
        // 4 точки. touched [0, 2]. Untouched 1 и 3.
        // orig_x = 0, 10, 20, 30. delta = (5, _, 15, _).
        // Точка 1: prev=0 (orig 0, delta 5), next=2 (orig 20, delta 15).
        //   lo=orig 0 (d=5), hi=orig 20 (d=15). orig 10 в середине → t=0.5 → delta=10.
        // Точка 3: prev=2 (orig 20, delta 15), next=0 (orig 0, delta 5) cyclically.
        //   lo=orig 0 (d=5), hi=orig 20 (d=15). orig 30 >= hi → delta = 15.
        let touched = vec![true, false, true, false];
        let mut delta = vec![5.0, 0.0, 15.0, 0.0];
        let orig = vec![0_i16, 10, 20, 30];
        iup_contour(&touched, &mut delta, &orig);
        assert_eq!(delta[0], 5.0);
        assert!((delta[1] - 10.0).abs() < 1e-5);
        assert_eq!(delta[2], 15.0);
        assert_eq!(delta[3], 15.0);
    }
}
