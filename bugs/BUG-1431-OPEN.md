# BUG-1431 — Процентный `margin-top` первого потомка (схлопывание через родителя): база — ширина содержащего блока родителя, а не его собственная ширина

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout (`crates/engine/layout/src/box_tree/bfc.rs::collapsed_top_margin`, `:149–157`)

## Симптом

`--dump-layout`, `body{margin:0}`, `p{height:5px}`:

| разметка | y `p` | ожидается |
|---|---|---|
| `<div style="width:300px"><p style="margin:1% 0 0 0">` | **8** | 3 |
| `<div style="width:300px"><p style="margin:calc(10px + 1%) 0 0 0">` | **18** | 13 |
| `<div style="width:500px"><p style="margin:1% 0 0 0">` | 8 | 5 |
| `<div style="width:300px"><div style="height:5px"></div><p style="margin:1% 0 0 0">` | 8 (5+3) | 8 — верно |
| `<div style="width:300px;padding-top:1px"><p style="margin:10% 0 0 0">` | 31 (1+30) | верно |
| `<div style="width:300px;display:flex"><p style="margin:10% 0 0 0;width:5px">` | 30 | 30 — верно |
| `<div style="width:300px"><p style="margin:0 0 10% 0"></p><div style="height:5px">` | 35 (5+30) | верно |

8 = 1 % от 800 (ширина окна/содержащего блока родителя), а не от `width:300px` родителя.

## Как найдено

WPT-RUN-14 срез 19: `css-values/calc-margin-block-1.html` — 5 строк с `margin: calc(10px + 1%)` у `<p>` в `<div style="border: medium solid
green; width: 500px">`: в нашем снимке отступ 18 вместо 15 (3 px + 15). Основная причина падения этого теста — `border: medium` не даёт
границы ([BUG-1298](BUG-1298-OPEN.md): ширина 0), вторая — этот баг (базу процента берут от окна).

## Что делать

В `collapsed_top_margin` перед переходом к `first_collapsible_child` ставить `cur_cb` равным content-box ширине узла
(с учётом `width`/`max-width`, а не `cur_cb − padding − border`).

## Как проверить

Страница из таблицы (первая строка); `css/css-values/calc-margin-block-1.html` после BUG-1298.
