# BUG-1356 — `text-align: right|center` не сдвигает atomic inline (`<img>`, `inline-block`) в строке: выравнивается только текст

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 13, `css/CSS2` (tables, positioning, floats, floats-clear, abspos, stacking-context, zindex, zorder))
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs` — ветка `BoxKind::InlineBlockRow`; `align_lines` применяется к `InlineRun`)

## Симптом

`--dump-layout`, контейнер 500 px, `text-align` на родителе:

| разметка | получено | ожидается |
|---|---|---|
| `text-align:right` > `<span style="display:inline-block;width:40px">` | `x=0` | `x=460` |
| `text-align:right` > `<img width=40>` | `x=0` | `x=460` |
| `text-align:right` > `<img width=5><img width=5>` | `x=0` и `x=5` | `x=490` и `x=495` |
| `text-align:center` > `ab` + `inline-block 40px` | текст `x=230`, блок `x=270` (центрируется текст сам по себе, ширина блока не считается) | вся строка 80 px по центру: текст `x=210`, блок `x=250` |
| `text-align:right` > одна строка текста `ab` | `x=460` (верно) | — |

Текст в строке выравнивается, atomic inline в той же строке — нет; строка из одних atomic inline не выравнивается вовсе.

## Как найдено

WPT-RUN-14 срез 13: эталон `<div style="text-align:right"><img …><img …></div>` — основной приём серий `css/CSS2/positioning/right-*`, `left-*`, `*-applies-to-*`, `floats-clear/float-applies-to-*`. Пример: `positioning/right-004.xht` — эталон — `<div style="text-align:right"><img width=5 height=96><img width=5 height=96></div>`, полосы должны быть у правого края; Lumen кладёт `Image rect=(8,25.72,5,96)` и `(13,…)` у левого (снимок 500×300: у теста полосы справа, `x≈482…492`; у эталона — слева, `x≈8…18`). Тест (настоящая `border-right` при `direction:rtl`) рисуется верно, неверно рисуется эталон. Правило отнесения: эталон с `<img>` и `text-align: right|center` — 63 id (верхняя граница: у части из них есть и другая причина).

## Что делать

В ветке `InlineBlockRow` применять `text_align`/`direction` к смещению каждой строки, считая ширину atomic inline в ширину строки (сейчас `align_lines` видит только `InlineRun`).

## Как проверить

`css/CSS2/positioning/right-004.xht`, `floats-clear/float-applies-to-008a.xht`.
