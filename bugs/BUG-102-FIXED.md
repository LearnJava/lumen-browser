# BUG-102

**Статус:** FIXED 2026-06-17
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`, `crates/engine/paint/src/svg_path.rs`, `crates/engine/layout/src/style.rs`

## Описание

SVG stroke-linecap/linejoin/dasharray advanced attributes not rendered — TEST-60: 11.51% (thr 0.5%); Phase 1.

## Причина

Две независимые проблемы:

1. **Главная — `stroke-width`/`stroke-dasharray`/`stroke-dashoffset` молча
   игнорировались на standards-mode страницах** (`<!DOCTYPE html>`). Их значения
   (`stroke-width="20"`) — unitless SVG user units, но `apply_declaration`
   резолвил их через `resolve_box_length` → `parse_length_q`, который отвергает
   unitless-числа в non-quirks режиме (только `0` или quirks). Поэтому штрихи
   рисовались дефолтной inherited-шириной 1px, а dash не применялся вовсе.
   Юнит-тест `svg_presentation_attributes_applied` проходил лишь потому, что его
   HTML без doctype → quirks-режим. `stroke-linecap`/`linejoin` (enum-парсинг)
   работали.

2. **Tessellate-артефакты joins.** Старый `stroke_contour_ex` для bevel/round
   использовал нормаль исходящего сегмента для общего quad'а, скашивая входящий
   сегмент → шипы в углах.

## Исправление

1. `resolve_svg_length` (`style.rs`) — резолвит SVG-геометрические длины с
   fallback на unitless→px независимо от quirks-режима; применён к
   `stroke-width`/`stroke-dasharray`/`stroke-dashoffset`.
2. Переписан `stroke_contour_ex` (`svg_path.rs`): по quad'у на сегмент с
   общими per-vertex точками — folded inner-miter на вогнутой стороне и общая
   miter-точка на выпуклой (в пределах miterlimit); bevel/round/over-limit
   заполняют внешний клин через `emit_join`. Гладко на flattened-кривых, чисто
   в острых углах.

Регресс-тесты: `svg_stroke_geometry_unitless_in_standards_mode` (style.rs),
обновлён `stroke_ex_bevel_join_has_extra_triangle` (svg_path.rs).

TEST-60: 11.51% → 1.41%, TEST-54: 5.58% → 2.30%. Остаток (AA-швы triangle-soup,
stroke-edge AA, self-intersecting fill, dash-on-curve) вынесен в **BUG-173**,
оба теста — в `KNOWN_DEBTORS`.
