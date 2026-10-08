# BUG-1562 — `matchMedia`: `media` недопустимой строки — как написано (`bogus`), а не `not all`; `[object Object]` вместо `[object MediaQueryList]`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js (`crates/js/src/shim/` — `matchMedia`, `MediaQueryList`)

## Симптом

`matchMedia("bogus").media` — `bogus` (CSSOM View §4.2: недопустимый запрос сериализуется как `not all`), `matchMedia("(").media` верно — `not all`; `Object.prototype.toString.call(mql)` — `[object Object]`; `MediaQueryList` в созданном iframe недоступен (`contentDocument` — `null`, BUG-480). 8 id (`MediaQueryList-*`, `matchMedia`; 35 из 48 сабтестов).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `matchMedia("bogus").media` | `bogus` | `not all` |
| `Object.prototype.toString.call(matchMedia("(min-width:1px)"))` | `[object Object]` | `[object MediaQueryList]` |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom-view/{MediaQueryList-*,MediaQueryListEvent,matchMedia,matchMedia-display-none-iframe}.html`.

## Что делать

Нормализация `media` по Media Queries 4 (`not all` для невалидных), `Symbol.toStringTag`; iframe — после FRAME (BUG-480).

## Как проверить

`css/cssom-view/matchMedia.html`.
