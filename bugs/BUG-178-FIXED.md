# BUG-178

**Статус:** FIXED 2026-06-17
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs` (`preferred_inline_block_width` ~3750, `max_content_outer_width` ~3822)

## Описание

Shrink-to-fit (auto-width) контейнер с несколькими floated-детьми получал ширину,
равную максимуму ширин детей, а не их сумме. По CSS 2.1 §9.5.1 несколько флоатов
одного направления выстраиваются бок о бок на одной линии, поэтому max-content
ширина блока-контейнера = сумма margin-box ширин его флоатов (а не max).

TEST-51 (scrollbar rendering): `<div style="float:left">`-обёртка без явной ширины
содержит два `float:left` бокса по 200px (`.scroll-both` + `.scroll-nooverflow`,
у первого `margin-right:24`). Обёртка сжималась до 200px (max одного ребёнка) →
второй флоат не помещался рядом и сбрасывался на новую строку (правило 8 §9.5.1).
Визуально: третий бокс верхнего ряда уезжал под второй. Замер обёртки до фикса:
`rect=(225, 52.2, 200, 280)`; после: `rect=(225, 52.2, 424, 140)`, дети при
`y=72.2` стоят рядом (x=225 и x=449). TEST-51 9.91% → 1.09%.

## Причина

В `preferred_inline_block_width` и `max_content_outer_width` ветка блочного
(вертикального) потока сводила ширины детей через `fold(0, f32::max)`, не
различая in-flow и floated детей и игнорируя горизонтальные margin'ы флоатов.

## Фикс

В обеих функциях для блок-контейнера считаем две величины:
- `inflow_max` — максимум ширин in-flow детей (вертикальный стек, как раньше);
- `float_sum` — сумма margin-box ширин floated-детей (`width + margin-left + margin-right`),
  т.к. они выстраиваются бок о бок.

Итог = `inflow_max.max(float_sum)`. Узкая область действия: затрагивает только
auto-width боксы с несколькими флоатами; контейнеры с явной CSS-шириной выходят
раньше (early return на explicit width).

## Тест

`shrink_to_fit_float_wrapper_sums_inner_floats_side_by_side` (box_tree.rs):
auto-width float-обёртка с двумя `float:left` детьми (120px + margin-right 20 + 120px)
в широком контейнере — проверяет, что второй флоат остаётся на той же линии
(`b.y == a.y`) и справа от первого (`b.x == a.x + 140`).

## Остаток

TEST-51 1.09% = BUG-124 (дробные layout Y-координаты vs пиксельное округление Edge),
отдельный OPEN-баг. TEST-51 → KNOWN_DEBTORS (`BUG-124`, 1.09).
