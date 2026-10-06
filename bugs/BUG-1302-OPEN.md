# BUG-1302 — Внешняя `box-shadow` рисуется под прозрачным фоном: тень не вырезается по границе бокса (`rgba`-фон просвечивает цвет тени)

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** paint (`crates/engine/paint/src/display_list/box_shadow.rs::emit_box_shadows` — внешняя тень рисуется целиком, без клипа «снаружи бордер-бокса»)


## Симптом

CSS Backgrounds 3 §7.1.1: «the shadow is only drawn **outside** the border edge; clipped inside the border box». Проба 800×600: `<div style="width:40px;height:40px;background:rgba(0,0,255,.5);box-shadow:30px 0 rgba(255,165,0,.5)">`: правая часть тени, лежащая под самим блоком (30 px из 40), видна **сквозь** полупрозрачный синий фон (на снимке — фиолетовая полоса внутри блока); эталон — чистый полупрозрачный синий. То же для блока с `border: 10px double` (внутри рамки, где фон прозрачный) и для `box-shadow: 0 0 0 4px red` у блока без фона — тень заливает внутренность (`FillRect (462,20,100,100)` под нулевым фоном — красный пиксель в центре бокса).

Непрозрачные фоны проблему маскируют, поэтому в живых страницах она редка; в WPT — 16 reftest с `rgba`-фонами.

## Как найдено

WPT-RUN-14 срез 6: `box-shadow-039…042.html`, `box-shadow-body.html`, `box-shadow-outset-without-border-radius-001.html`, `box-shadow-multiple-001.html`, `box-shadow-radius-00{0,1,2}.html`, `box-shadow-table-border-collapse-001.html`, `box-shadow/slice-block-fragmentation-*` — 16 `thick`, 3 `thin-only`. Отдельно, **другая причина** (3 `thick` из `box-shadow-overlapping-00{1…4}`, не разделено на BUG): `box-shadow` на `<span>` внутри строки в display list не попадает вовсе (проба `<span style="box-shadow:0 0 0 10px red">` — снимок без тени); нужен отдельный BUG, когда этот кластер дойдёт до очереди.

## Что делать

Клипить внешнюю тень по `rect − border_box` (с радиусами) — через существующий clip-стек или маску «выколотого» прямоугольника; для инлайна — тень на каждый фрагмент строки.

## Как проверить

`css/css-backgrounds/box-shadow-{039,040,body,multiple-001,radius-001,overlapping-001}.html`.
