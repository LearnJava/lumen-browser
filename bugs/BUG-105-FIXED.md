# BUG-105

**Статус:** FIXED 2026-06-22
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

CSS Masonry layout — TEST-63: 48% (thr 0.5%). Страница использует `display: masonry; column-count: 3`.

## Реальная причина

`display: masonry` — невалидное значение в стабильных браузерах: Edge его игнорирует,
элемент остаётся `display: block`, а `column-count: 3` делает его CSS multicol-контейнером.
Lumen уже сбрасывал `display: masonry` → `block`, так что страница тоже шла через multicol.
Дефект был в балансировке multicol: при `column-fill: balance` (это initial-значение по
спеке, а Lumen ошибочно умолчанием держал `auto`) с заданной высотой контейнера колонки
заполнялись последовательно до высоты контейнера (5/4/0) вместо равномерного разбиения.

## Фикс

1. `column_fill_balance` по умолчанию `true` (spec default = `balance`).
2. `balanced_column_height` — бинарный поиск минимальной высоты колонки H, при которой
   жадная упаковка неразрезаемых боксов укладывается в `n_cols` колонок. 9 карт разной
   высоты → 3×3, как в Edge (раньше greedy `total/N` + count-cap давал 2/2/5).

Геометрия теперь пиксель-в-пиксель с Edge (заливки/позиции). Остаток 2.02% (CPU-diff) =
`border-radius: 4px` edge-AA (класс BUG-176) + текст заголовка/меток (rule 3) → KNOWN_DEBTORS.
