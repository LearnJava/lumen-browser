# BUG-213

**Статус:** FIXED 2026-06-23 (DEBTOR)
**Компонент:** css-parser/layout
**Тест:** TEST-97 (diff 2.78% → KNOWN_DEBTORS BUG-128)

## Описание

CSS Lists `counter-set` — порядок reset→increment→set; set перекрывает increment.

## Расследование

Порядок применения уже спек-корректен (CSS Lists L3 §4):
`crates/engine/layout/src/counters.rs` `walk()` зовёт `apply_reset` →
`apply_increment` → `apply_set` именно в этом порядке, а парсинг
`counter-set` → `style.counter_set` присутствует в `style.rs:11909`.

Визуальная сверка с Edge (`97-counter-set-{edge,lumen-cropped,diff}.png`):
все пять значений счётчика рендерятся одинаково в обоих движках —
**5** (set 5 на reset-0), **6** (inc), **0** (inc затем set — set перекрывает
inc), **1** (inc от 0), **42** (set на never-reset-этим-элементом счётчике).
Цветные боксы строк и их границы совпадают пиксель-в-пиксель; diff-картинка
содержит только глифы текста.

## Вывод

Дефекта движка нет. Остаток TEST-97 2.78% — чистый font-parity:
`::before { content: counter(c) }` метки + текст строк рисуются Inter sans
против Edge sans, разная ширина токена «inc+set» сдвигает глифы по X
(ghosting). Тот же класс, что BUG-128.

## Регрессия

`counter_set_test97_sibling_rows` (counters.rs) фиксирует всю
последовательность r1–r5 (5/6/0/1/42).

TEST-97 внесён в `KNOWN_DEBTORS` (run.py) с baseline 2.78% → BUG-128.
