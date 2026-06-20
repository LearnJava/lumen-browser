# BUG-198

**Статус:** FIXED 2026-06-20
**Компонент:** layout/paint
**Тест:** TEST-70 (7.82% → 1.63% → KNOWN_DEBTORS BUG-176)

## Описание

`object-fit`/`object-position` для inline SVG: fill/contain/cover/none/scale-down +
viewBox scaling. Остаток после BUG-110.

## Корень

Две причины, обе подтверждены пиксельным сравнением с Edge-эталоном:

1. **Главная (layout).** BUG-110 завёл viewBox→viewport-маппинг через CSS
   `object-fit`/`object-position` (`compute_object_fit_transform`), считая это
   «Edge ground truth». На самом деле **inline `<svg>` НЕ является CSS replaced
   element**, поэтому `object-fit`/`object-position` к нему не применяются —
   Chrome/Edge фитят viewBox исключительно по атрибуту `preserveAspectRatio`
   (SVG 1.1 §7.8). Пиксельный анализ TEST-70: все 5 боксов («fill»/«contain»/
   «cover»/«none»/«scale-down») Edge рисует одинаково как `meet` (contain), а
   названные `object-fit`-классы игнорируются. Lumen же растягивал (`fill`),
   кропал (`cover`) и не масштабировал (`none`) viewBox → svg-фон и эллипсы
   уезжали по размеру/позиции.

2. **Вторичная (paint).** SVG `<ellipse>`/`<circle>` эмитятся как `FillRoundedRect`
   с эллиптическими углами `rx=w/2, ry=h/2`. И femtovg `draw_fill_rounded_rect`, и
   `CornerRadii::clamped_to_box` клампили каждый радиус в `min(w/2, h/2)` →
   x-радиус широкого эллипса (например 120) схлопывался до y-радиуса (45) →
   круглые углы → форма «стадион» вместо эллипса (заметнее всего на cover/fill).

## Фикс

1. `lay_out_svg_root` (`box_tree.rs`) вызывает новую
   `compute_preserve_aspect_ratio_transform` (meet/slice + align по §7.8) вместо
   `compute_object_fit_transform` (удалена; object-fit остаётся в силе для
   `<img>`-встроенного SVG через DrawImage-путь). Default — `xMidYMid meet`.
2. `CornerRadii::clamped_to_box` переписан на CSS Backgrounds L3 §5.5
   (единый scale-factor по всем углам, по-осевой), femtovg `draw_fill_rounded_rect`
   переиспользует его. Эллиптические углы (rx≠ry) сохраняются.

## Тесты

- `compute_preserve_aspect_ratio_transform`: `preserve_aspect_ratio_meet_letterboxes_uniformly`,
  `preserve_aspect_ratio_slice_covers`, `preserve_aspect_ratio_xminymin_top_left`,
  `preserve_aspect_ratio_meet_scales_up_small_viewbox`,
  `svg_root_inline_svg_ignores_object_fit_uses_preserve_aspect_ratio` (box_tree.rs).
- `clamped_to_box_preserves_wide_ellipse` (display_list.rs).

## Остаток

1.63% = kappa-безье-аппроксимация эллиптических дуг SVG `<ellipse>`/`<circle>` vs
точная дуга Edge + sub-pixel AA по периметру эллипса и внутреннему `<rect>`. Тот же
класс, что BUG-176 → TEST-70 в `KNOWN_DEBTORS` (BUG-176, baseline 1.63%).
Геометрия и заливки совпадают с Edge пиксель-в-пиксель.
