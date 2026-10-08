# BUG-1421 — `object-fit` и `object-position` не действуют на `<canvas>`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout/paint (`<canvas>` как replaced-элемент, `object-fit`/`object-position`)

## Симптом

`<body style="margin:0"><canvas id=c width=16 height=8 style="width:48px;height:32px;object-fit:contain"></canvas>`, на
холсте `fillStyle='green'; fillRect(0,0,16,8)`; `--screenshot` 100×50:

| `object-fit` | зелёных | рамка | ожидается |
|---|---|---|---|
| `contain` | 1536 | (0,0)–(47,31) | 1152, (0,4)–(47,27) |
| `<img>` с тем же размером и `contain` (контроль) | 1152 | (0,4)–(47,27) | |

Холст всегда растягивается по content-box, как при `fill`.

## Как найдено

WPT-RUN-14 срез 19: `css-images/object-fit-{contain,cover,fill,none,scale-down}-png-00{1,2}c.html`,
`object-fit-containcontainintrinsicsize-png-001c.tentative.html`, `object-position-png-00{1,2}c.html` — 14 id.
Эти тесты ещё содержат `class="reftest-wait"` (скрипт рисует на холсте и снимает класс) — часть падения может быть
WPT-RUN-15; `object-fit` игнорируется независимо от этого (проба выше без `reftest-wait`).

## Что делать

Передать `object_fit`/`object_position` в команду рисования холста так же, как для `DrawImage`.

## Как проверить

Страница из таблицы; `css/css-images/object-fit-contain-png-001c.html`.
