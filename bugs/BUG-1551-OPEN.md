# BUG-1551 — CSSOM: значения в `style`/`cssText` не канонизируются — `rgba(5,7,9,.5)` → `rgba(5, 7, 9, 0.502)`, `.5%`/`-0px`/`url(a)` остаются как написаны

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js::_lumen_canonicalize_longhand`), css-parser (`serialize-values`)

## Симптом

`cssom/serialize-values.html`: 140 из 697 сабтестов, из них 105 — `background-position` (`5% .5%` → ожидается `5% 0.5%`, `-.5%` → `-0.5%`, `5% -0px` → `5% 0px`, `.1em` → `0.1em`), 3 — `content`, 3 — `line-height`, по 2 — `min/max-width|height`, `font-size`, `baseline-shift`, `outline-color`; `rgba(5, 7, 10, 0.5)` читается как `rgba(5, 7, 10, 0.502)` (альфа округляется через 8 бит, `0.5` должно оставаться `0.5`); `url(http://localhost/)` без кавычек — ожидается `url("http://localhost/")`; `font-family`: `"serif"` у `font-family` с кавычками теряет кавычки (`font-family-serialization-001`: 12 из 24).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `style.cssText="background-color:rgba(5,7,9,0.5)"` → `style.cssText` | `background-color: rgba(5, 7, 9, 0.502);` | `background-color: rgba(5, 7, 9, 0.5);` |
| `style.cssText="background:url(a.png) no-repeat"` | `background: url(a.png) no-repeat;` | `background: url("a.png") no-repeat;` |
| `style.cssText="width:calc(1px + 2px)"` | `width: calc(3px);` | `width: calc(3px);` |
| `style.backgroundPosition="5% .5%"` | `5% .5%` | `5% 0.5%` |
| `style.cssText="a-b:c;color:red"` | `a-b: c; color: red;` | `color: red;` (неизвестное свойство отбрасывается) |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom/serialize-values.html` (140 из 697), `font-family-serialization-001.html`, `cssstyledeclaration-csstext.html` (3 из 11), `css-style-reparse.html`.

## Что делать

Привести значения в `_lumen_canonicalize_longhand` к CSSOM §6.7.2 (serialize a CSS component value): числа без ведущей точки, URL в кавычках, альфа без округления через 8 бит (`0.5`), `-0` → `0`; отбрасывать неизвестные свойства в `cssText`.

## Как проверить

`css/cssom/serialize-values.html` — 105 сабтестов `background-position` и по одному `rgba`/`url` зелёные.
