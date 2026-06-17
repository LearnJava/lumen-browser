# BUG-108

**Статус:** FIXED 2026-06-17
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

::selection pseudo-element: background-color/color override not applied — TEST-66: 6.18%

## Реальная причина

`66-selection-pseudo.html` НЕ триггерит выделение текста — `::selection` правила в нём
информационные (комментарий «not visible without user selection»), а видимый контент это
обычные свотчи с фиксированными background. Расхождение с Edge давал не `::selection`, а
**отсутствие parent↔last-child bottom margin collapse** (CSS 2.1 §8.3.1).

Каждая `.section` (auto height, без bottom padding/border) содержала `.swatch-row` с
`margin-bottom: 30px` последним ребёнком + собственный `margin-bottom: 30px`. Lumen
коллапсил margin'ы только на верхней грани (parent↔first-child) и между соседями, но
bottom-маргин последнего ребёнка НЕ убегал из родителя — оставался внутри `content_height`.
В итоге секция была 113.6px вместо 83.6px, плюс свой 30px margin → +30px пустоты на стык
секций. Свотчи накапливали дрейф: секция 2 +31px, секция 3 +62px (TEST-66 5.24%).

## Фикс

Симметрично существующему `collapsed_top_margin`/`b_collapses_top`:
- `last_collapsible_child` — последний in-flow Block-ребёнок (зеркало `first_collapsible_child`).
- `collapsed_bottom_margin` — рекурсивный collapsed bottom margin по цепочке последних детей
  (auto height, без bottom padding/border, не BFC).
- `b_collapses_bottom` — гейт (in_block_flow + Block + не BFC + нет bottom padding/border +
  auto height). Корень элемента не коллапсит автоматически (`in_block_flow == false`).
- В подсчёте `content_height`: при `b_collapses_bottom` bottom-маргин последнего ребёнка
  вычитается из высоты (убегает наружу), если ниже него нет float.
- `child_mb` в block-children loop теперь `collapsed_bottom_margin(child)` (collapse-through
  для соседей и родителя).

TEST-66 5.24%→1.08%. Остаток — текст (font-parity, rule 3: «Ignore text for now») +
border-radius AA на свотчах. Прогон 2026-06-17 09:53 без регрессий (TEST-00/TEST-21 —
gdigrab-шум на границе).

Регресс-тесты: `parent_last_child_bottom_margin_collapses`,
`bottom_margin_not_collapsed_through_padding`. Обновлён snapshot `paragraph_with_styles`
(body теперь 44px вместо 49 — p's bottom margin убегает, как и top уже убегал → body на y=5).
