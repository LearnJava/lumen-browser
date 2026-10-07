# BUG-1358 — `border-collapse: collapse` + `table-layout: fixed`: ячейка с широкой боковой рамкой смещена влево на всю ширину рамки

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 13, `css/CSS2` (tables, positioning, floats, floats-clear, abspos, stacking-context, zindex, zorder))
**Область:** layout (`crates/engine/layout/src/box_tree/table.rs` — геометрия ячеек collapsed-модели при фиксированной раскладке)

## Симптом

`display:table` через `<table>`, `border-collapse: collapse; table-layout: fixed; width: 400px`, `td { padding: 0 24px }`, средняя ячейка `width: 80px; border-left/right: 36px solid orange` (`tables/fixed-table-layout-003e01.xht`). `--dump-layout` ячеек (x от края таблицы):

| режим | ячейка 1 | ячейка 2 (с рамками) | ячейка 3 |
|---|---|---|---|
| `border-collapse: separate` | `x=0, w=100` | `x=100, w=200` | `x=300, w=100` |
| `border-collapse: collapse` | `x=0, w=100` | **`x=64`**, `w=200` | `x=228`, `w=100` |

В collapsed-модели середина ячейки уехала влево на 36 px (ширина рамки), перекрыв первую ячейку; третья — на 72 px. Снимок 800×600: оранжевая полоса теста `x=72…271`, эталона — `x=108…307` (сдвиг ровно на 36), синяя — `108…235` против `144…271`.

## Как найдено

WPT-RUN-14 срез 13: 12 id `fixed-table-layout-003e*` + 8 `-003f*` + 10 `fixed-table-layout-*` + 8 `collapsing-border-model-*` (последние — по имени, без пробы). Зонтик: 38 id; проба — только `fixed-table-layout-003e01`. Смежно с [BUG-1346](BUG-1346-OPEN.md) (та же геометрия collapsed-границ, другой симптом).

## Что делать

Позиция ячейки в collapsed-модели — по линиям сетки, граница делится пополам по обе стороны линии; сейчас вычитается вся ширина.

## Как проверить

`css/CSS2/tables/fixed-table-layout-003e01.xht`, `collapsing-border-model-003.xht`.
