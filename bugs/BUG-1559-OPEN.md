# BUG-1559 — Корневой скроллер: `document.scrollingElement.scrollTop`/`clientHeight` не связаны с окном — `window.scrollTo(0,500)` оставляет `scrollTop` 0, `clientHeight` — высота документа

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — `scrollingElement`, `scrollTop`, `clientHeight` корня)

## Симптом

`window.scrollTo(0,500)` меняет `scrollY` (500), но `document.scrollingElement.scrollTop`/`document.documentElement.scrollTop` остаются `0`; запись `scrollingElement.scrollTop = 40` не двигает окно (`scrollY` остаётся); `documentElement.scrollTop = 60` читается назад `60` при `scrollY` 10; `document.documentElement.clientHeight` — высота документа (4050), а не окна (720, `innerHeight`); `scrollIntoView()` корня (`scrollY` 2000) оставляет `scrollingElement.scrollTop` равным 0. 17 id (`client-props-root`, `scrollingElement`, `scrollintoview`, …).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| документ высотой 4050: `documentElement.clientHeight` / `scrollingElement.clientHeight` / `innerHeight` | 4050 / 4050 / 720 | 720 / 720 / 720 |
| `window.scrollTo(0,500)` → `scrollY` / `scrollingElement.scrollTop` | 500 / 0 | 500 / 500 |
| `scrollingElement.scrollTop = 40` → `scrollY` | без изменений | 40 |
| `el.scrollIntoView()` корня → `scrollY` / `scrollingElement.scrollTop` | 2000 / 0 | 2000 / 2000 |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom-view/{client-props-root,scrollingElement,scrolling-quirks-vs-nonquirks,scrollintoview,scrollIntoView-*,scroll-zoom,scrollTo-zoom}.html`.

## Что делать

Привязать `scrollTop/scrollLeft/scrollWidth/scrollHeight/clientWidth/clientHeight` корневого элемента (и `body` в quirks) к вьюпорту и текущему смещению окна.

## Как проверить

`css/cssom-view/client-props-root.html`, `scrollingElement.html`.
