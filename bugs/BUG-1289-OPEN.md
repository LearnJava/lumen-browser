# BUG-1289 — `transform` на таблице (`<table>`, `display: table`) не применяется

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 5, `css/css-transforms`)
**Область:** layout/paint (бокс-обёртка таблицы не получает `PushTransform`)

## Симптом

`--screenshot` 800×600, `--dump-display-list`:

| разметка | получено | ожидается |
|---|---|---|
| `<table style="background:blue;transform:translate(300px,0)">` | на x = 0, в display list нет `PushTransform` | на x = 300 |
| `<div style="display:table;…;transform:translate(300px,100px)">` | на месте | сдвиг |
| `<table style="position:absolute;transform:translate(300px,0)">` | на месте | сдвиг |
| `display:inline-block` / `display:list-item` с тем же `transform` | работает | — |

## Как найдено

WPT-RUN-14 срез 5: 16 упавших reftest (`transform-table-*` 7, `transform-display-*`, `transform-abspos-*`,
`transform-generated-*`); пара `transform-table-004.html` — повёрнутая таблица с `caption`, у эталона таблица внутри `<div>`
с тем же `transform`, а у теста она остаётся на месте (разница строк по снимку: тест x 8…50, эталон x 165…207).

## Что делать

Найти, где для боксов `Table`/`display:table` пропускается `forward_box_transform` (стековый контекст создаётся, а
трансформ не эмитится), и применить трансформ к бокс-обёртке (CSS Transforms L1 §2: «transformable element» включает
`display: table`).

## Как проверить

`css/css-transforms/transform-table-00{1…9}.html`, `transform-display-002.html`, `transform-abspos-006.html`.
