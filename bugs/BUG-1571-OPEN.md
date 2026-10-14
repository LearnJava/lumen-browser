# BUG-1571 — Нет `Element.scrollParent`, `scrollIntoViewIfNeeded`, `window.resizeTo/resizeBy/moveTo/moveBy`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid*.js`)

## Симптом

`typeof el.scrollParent`, `typeof el.scrollIntoViewIfNeeded`, `typeof window.resizeTo`, `moveTo`, `moveBy`, `resizeBy` — `undefined`. Тесты: `scrollParent*.html` (3 id, 17 сабтестов), `scrollintoview-zero-height-item` (`span.scrollIntoViewIfNeeded is not a function`), `resizeTo-negative` (`w.resizeTo is not a function`).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `typeof Element.prototype.scrollParent` | `undefined` | `function` |
| `typeof Element.prototype.scrollIntoViewIfNeeded` | `undefined` | `function` |
| `typeof window.resizeTo + typeof window.moveTo + typeof window.resizeBy + typeof window.moveBy` | `undefined` ×4 | `function` ×4 |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom-view/{scrollParent,scrollParent-shadow-tree,scrollParent-quirks-mode,scrollintoview-zero-height-item,resizeTo-negative}.html`.

## Что делать

Реализовать `scrollParent` (CSSOM View, предложение), `scrollIntoViewIfNeeded`, методы окна (для окна без `window.open` — no-op, как в Chromium для неоткрытых окон).

## Как проверить

`css/cssom-view/scrollParent.html`.
