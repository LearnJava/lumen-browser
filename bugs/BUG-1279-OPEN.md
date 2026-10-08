# BUG-1279 — CPU-растр рисует `outline` штрихом по краю бокса: половина ширины уходит внутрь, снаружи только половина

**Статус:** OPEN
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 3, `css/css-ui`)
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs::rasterize_draw_outline`)

## Симптом

`<div style="margin:40px;width:20px;height:20px;outline:10px solid blue">`, `body{margin:0}`, `--screenshot`:
синий занимает x = 35…64 (30 px), ожидается 30…69 (40 px = 20 + 2·10). То есть outline сдвинут на `width/2`
внутрь: рисуется полоса 5 px снаружи и 5 px поверх бокса.

`outline-001.html`: зелёный бокс 60×60 с `outline: 20px green` в красном контейнере 100×100 — в Lumen виден красный
ободок шириной 10 px (эталон — сплошной зелёный квадрат 100×100).

Display list верен: `DrawOutline (28.00, 45.72, 60.00, 60.00) w=20.00` — ошибается растеризатор.

## Причина

`rasterize_draw_outline` строит путь по прямоугольнику `rect ± offset` и обводит его `stroke_path` шириной `width`.
Штрих tiny-skia центрирован по пути, поэтому outline начинается не от края `rect + offset`, а на `width/2` внутри.
wgpu (`renderer.rs`, ветка `DrawOutline`) и femtovg (`femtovg_backend.rs`) рисуют снаружи — расходится только CPU-путь,
то есть `--screenshot`, IPC `Screenshot` (reftest-исполнитель WPT) и CPU-снимки графтестов.

## Что делать

Обводить путь, смещённый наружу на `width/2` (`rect ± (offset + width/2)`), либо залить кольцо между
`rect ± offset` и `rect ± (offset + width)` четырьмя полосами, как femtovg. Отрицательный `outline-offset`
(`negative-outline-offset.html`) — тот же расчёт. Пиксели меняются: регенерировать CPU-эталоны графтестов
(`graphic_tests/snapshots/cpu/`) в том же коммите.

## Как проверить

WPT `css/css-ui/outline-0NN.html`, `outline-color-*`, `outline-style-*`, `outline-offset*.html`,
`outline-with-padding-001.html`, `negative-outline-offset.html`, `translucent-outline.html`,
`subpixel-outline-width.tentative.html` — 33 reftest (все `thick` в `reftest_pixdiff.py`; без `outline-offset-inset-*`,
это нереализованное значение `inset`).
