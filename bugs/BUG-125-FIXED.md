# BUG-125

**Статус:** FIXED 2026-06-22
**Компонент:** layout/paint
**Файл:** `crates/engine/layout/src/property_trees.rs`

## Описание

CSS Motion Path L1 (offset-path/offset-distance/offset-rotate) — боксы на
horizontal/diagonal/cubic-bezier путях были смещены на пол-бокса вправо-вниз
относительно Edge (TEST-76: 3.18%).

## Причина

И `forward_box_transform` (paint), и `PropertyTrees::walk` (compositing/hit-test)
ставили на путевую точку **top-left** бокса, а не его `offset-anchor`. По CSS
Motion Path L1 §3.3 на путь садится anchor (initial `auto` = `transform-origin` =
центр `50% 50%`), и поворот идёт вокруг него. Отсутствовал член `T(-anchor)`,
поэтому центр оказывался на пол-бокса дальше путевой точки.

## Фикс

Добавлен `motion_anchor_px()` (offset-anchor → px, `auto` → transform-origin).
В `forward_box_transform` матрица motion-path строится вокруг anchor:
`T(rect + path) · R(θ) · m · T(-(rect + anchor))` (transform-origin = offset-anchor
по спеку). В `walk` motion = `T(path) · R(θ) · T(-anchor)`. Регресс-тесты:
`motion_path_centres_anchor_on_path_point` (lib.rs),
`offset_path_at_zero_distance_centres_anchor_on_path_start` (property_trees.rs).

Проверка: детерминированный CPU-снимок TEST-76 vs Edge — боксы пиксель-в-пиксель
(маскирование диагональной полосы row 2 → 0.01%). Остаток 0.54% = отдельный дефект
`calc()` в позициях color-stop градиента (диагональный трек-индикатор не рисуется) →
BUG-230, TEST-76 запаркован как KNOWN_DEBTOR.
