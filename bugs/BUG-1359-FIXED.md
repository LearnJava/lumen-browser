# BUG-1359 — Блок, создающий BFC (`overflow:hidden`, `display:block` с `margin:auto`), рядом с float: сдвиг на ширину float, не пересекающегося с ним по вертикали

**Статус:** FIXED 2026-10-16 (P6)
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 13, `css/CSS2` (tables, positioning, floats, floats-clear, abspos, stacking-context, zindex, zorder))
**Область:** layout (`crates/engine/layout/src/box_tree/bfc.rs` / `float` — подбор позиции BFC-бокса среди float)

## Симптом

`--dump-layout`, 500 px, `float:left;width:50;height:75` (A), затем `float:left;clear:left;width:100;height:75` (B, лежит ниже A, `y=75…150`), затем `display:block;overflow:hidden;width:200;height:50` (C):

| случай | получено для C | ожидается (§9.5, граница BFC-бокса не пересекает поля float) |
|---|---|---|
| только A | `x=50, y=0` (верно) | `x=50, y=0` |
| A + B | **`x=100, y=0`** — сдвинут на ширину B, хотя B по вертикали (`75…150`) не пересекает C (`0…50`) | `x=50, y=0` |

Тест `floats/floats-wrap-top-below-bfc-001l.xht` (800×600, `body{width:400px}`, float A `50×75`, B `100×75` с `clear:left`, два `span` `200×50`): Lumen кладёт `span` в `(108,8)` и `(108,58)`, эталон (те же float'ы, `span` — `inline-block; vertical-align:top`) даёт синий блок на 50 px ниже — `y=58…157` против `8…107`. Это тот же класс «позиция BFC-бокса среди float», механизм по снимку не отделён от `A+B`-пробы.

## Как найдено

WPT-RUN-14 срез 13: серия `floats-wrap-bfc-*`, `floats-wrap-top-below-bfc-*`, `-inline-*`, `floats-wrap-bfc-with-margin-*`, `float-nowrap-*`, `floats-zero-height-wrap-*`, `floats-line-wrap-shifted-*` — 36 id (по именам, проба только на `-001l` и минимальной разметке выше). Часть `float-nowrap-*` — ещё и `ch` (BUG-1340).

## Что делать

При подборе позиции BFC-бокса учитывать только float'ы, пересекающие его по вертикали; если места рядом нет — сдвигать ниже до освобождения, а не вправо.

## Как проверить

`css/CSS2/floats/floats-wrap-top-below-bfc-001l.xht`, `floats-wrap-bfc-004.xht`.

## Исправление

Корень — не подбор позиции BFC-бокса, а два дефекта потока float'ов:

1. `block_flow_trampoline.rs` — `clear` на float'е сдвигал `frame.init.child_y` (позицию потока) до низа очищаемых float'ов и не возвращал назад: следующий in-flow сиблинг (C) считался стоящим на y=75 и сдвигался за B. Теперь после `place_float` позиция потока восстанавливается (CSS 2.1 §9.5.2 — `clear` сдвигает только сам float).
2. `FloatContext` хранил для float'а только `bottom_y`, то есть float «действовал» и выше своей верхней кромки. Добавлены `left_tops`/`right_tops` (верх margin-box) и запросы `left_edge_in_flow`/`right_edge_in_flow`, которые игнорируют float'ы, начинающиеся ниже верха блока. Их использует только позиционирование in-flow детей; размещение float'ов и строки по-прежнему идут через `*_edge_at` (правило 5 §9.5.1 — float не выше более раннего float'а).

Тест: `generated_float::bfc_block_ignores_cleared_float_below_it`.
