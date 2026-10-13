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
