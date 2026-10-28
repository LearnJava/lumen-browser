# BUG-1557 — `document.elementFromPoint`/`elementsFromPoint` не отдают inline-элементы и возвращают `null`/`[]` в области вьюпорта вне `<body>`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js/layout (hit-test: `document.elementFromPoint`, `elementsFromPoint`; `crates/js/src/shim/web_api_shim_mid*.js`, `crates/engine/layout/src/`)

## Симптом

Точка над `<a>`, `<b>`, `<span>` внутри блока: `elementFromPoint` — `DIV`, `elementsFromPoint` — `DIV>BODY>HTML` (в цепочке нет inline-элемента); точка во вьюпорте ниже `<body>` — `elementFromPoint` — `null` (по CSSOM View §6 — корневой элемент), `elementsFromPoint(300,300)` — `[]` (ожидалось `HTML`). `elementFromPoint()` без аргументов не бросает `TypeError`; `iframe.contentDocument.elementsFromPoint` недоступен (BUG-480). 27 id, 57 из 96 сабтестов.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| над `<a>`/`<b>`/`<span>` в `div` — `elementFromPoint(центр)`, `elementsFromPoint(центр)` | `DIV`, `DIV>BODY>HTML` | `A`, `A>DIV>BODY>HTML` (и `B`, `SPAN`) |
| точка (60,70) во вьюпорте 1024×720 ниже `<body>` высотой 20 px — `elementFromPoint` | `null` | `HTML` |
| `elementFromPoint()` без аргументов | не бросает | `TypeError` |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom-view/{elementFromPoint*,elementsFromPoint*,elementFromPosition,negativeMargins}.html` (27 id).

## Что делать

Hit-test по inline-боксам (`InlineRun`-фрагментам и атомарным inline), пустая область вьюпорта → корневой элемент; проверка числа аргументов.

## Как проверить

`css/cssom-view/elementsFromPoint-inline-htb-ltr.html`, `elementsFromPoint-invalid-cases.html`.

## Срез 1 (2026-10-28, P3) — влит

Сделано: `HitTestResult::inline_source` (строгое попадание в `InlineFrag`, в т. ч. вертикальные writing-mode), цепочка inline-элементов в `_lumen_hit_nids` (`web_api_shim_mid.js`), корень для пустой точки вьюпорта, `[]`/`null` вне вьюпорта, учёт прокрутки страницы, `TypeError` на недостающие/нефинитные аргументы. Подробности — `subsystems/js.md`.

WPT (31 файл `elementFromPoint*`/`elementsFromPoint*`/`elementFromPosition`/`negativeMargins`): 57 из 103 сабтестов (было около 39 из 96); `elementsFromPoint-inline-{htb-ltr,htb-rtl,vlr-ltr,vrl-ltr,nested}`, `elementsFromPoint-invalid-cases`, `elementFromPoint-parameters` — PASS.

Остаток: `iframe.contentDocument.elementFromPoint` (BUG-480), SVG-цели (`text`/`tspan`/`rect`/`foreignObject`), псевдоэлементы и ячейки таблицы в `elementsFromPoint` (лишние TR/TBODY), `rtl` в вертикальных режимах (layout ставит ран в начало), порядок `negativeMargins`, `elementFromPoint-visibility-hidden-resizer`, shadow-деревья в `document.elementsFromPoint`.
