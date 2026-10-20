# BUG-1659 — Самостоятельный SVG-документ рисуется со сдвигом (8,13) вместо (0,0)

**Статус:** OPEN
**Заведён:** 2026-10-20 (P3, выделено из [BUG-1420](BUG-1420-FIXED.md))
**Область:** layout (самостоятельный SVG-документ верхнего уровня: `parse_xml_flavoured` + UA-стили `body`)

## Симптом

`full.svg` — `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8" viewBox="0 0 16 8" preserveAspectRatio="none">` с четырьмя
`<rect>` по 8×4. `lumen --viewport 200x100 --dump-display-list full.svg`: `PushClipRect (8,13,16,8)`, `FillRect (8,13,8,4)` … — содержимое
сдвинуто на (8,13) от угла. У Chrome корень SVG-документа — сам `documentElement`, поля нет: (0,0). Тот же сдвиг видел BUG-1420 у
`<object>` до исправления (там белая подложка + такой же SVG).

## Причина (гипотеза, не проверена)

HTML5 tree builder в XML-режиме оборачивает `<svg>` в `html`/`body`: `body` получает UA-поле 8 px, а `<svg>` лежит в строке инлайна
(+5 px по вертикали до базовой линии — отсюда 13, а не 8).

## Как проверить

`lumen --viewport 200x100 --dump-display-list full.svg` — первая команда `PushClipRect` должна быть в (0,0).
