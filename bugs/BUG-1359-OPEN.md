# BUG-1359 — Блок, создающий BFC (`overflow:hidden`, `display:block` с `margin:auto`), рядом с float: сдвиг на ширину float, не пересекающегося с ним по вертикали

**Статус:** OPEN
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
