# BUG-1370 — `vertical-align: <length>|<percentage>` не растягивает строку: поднятый/опущенный inline-бокс вылезает за неё

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 14, `css/CSS2` (text, linebox, fonts, generated-content, lists, bidi-text))
**Область:** layout (`crates/engine/layout/src/box_tree/inline_wrap.rs` — вычисление высоты строки по `vertical-align`)

## Симптом

`font: 20px/1 Ahem; margin:0`, `<div style="position:relative"><span style="vertical-align:96px">X</span>X</div>`; `--dump-display-list`: поднятый `X` — `DrawText y=-96.00`, второй — `y=0.00`, высота `div` — 20 px (`--dump-layout`: `Block rect=(0,0,200,20)`). По CSS 2.1 §10.8.1 строка растёт до верха поднятого бокса: `div` — 116 px, поднятый `X` на `y=0`, обычный — на `y=96`. С `vertical-align: 50%` (50 % от `line-height` = 10 px) — то же: бокс на 10 px выше строки, строка не выросла.

## Как найдено

WPT-RUN-14 срез 14: `linebox/vertical-align-007.xht` … `-080.xht` — 33 id в группах `vertical-align-NNN` (length/percentage/top/bottom); тест кладёт поднятый `X` в `div` и ставит рядом абсолютно позиционированные `div` с `top: 96px` для проверки положения. Проба выше — отдельная страница.

## Что делать

В расчёте строки учитывать смещённые по вертикали inline-боксы: верх строки = max(верх каждого inline-бокса с учётом `vertical-align`), низ — min; `top`/`bottom` выравниваются по уже выросшей строке (§10.8.1).

## Как проверить

`css/CSS2/linebox/vertical-align-007.xht`, `-008.xht`, `-019.xht`.
