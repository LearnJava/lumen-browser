# BUG-1556 — `getComputedStyle`: used values инсетов — `top: 1em` у статического бокса, `20px` вместо `23.2px` у абсолютного, `-1px` вместо `3px` у относительного

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/resolved_geometry.rs`, `crates/engine/layout/src/selector_query.rs::computed_style_to_map`)

## Симптом

Остаток CSSOM-9: для `position: static` и боксов без блока (`display:none`, `getComputedStyle-insets-nobox/static`) resolved value `top/left/…` — computed (`1em`), по CSSOM §7.2 — тоже computed, но тесты ждут пересчёта в px (`expected "10px" but got "1em"`, 36 из 216 в `static`/`nobox`); у `absolute`/`fixed` — `23.2px` ожидалось, получено `20px`, `55.6px` против `30px` (процентные и `calc` инсеты от containing block с дробной высотой, 216 из 324); у `relative` — `bottom: 3px` против `-1px` (ширина/высота, процент), 144 из 252; у `sticky` — `10.4px` против `10.2px` (72 из 252). 8 id `getComputedStyle-insets-*` дают 796 из 900 упавших сабтестов кластера `om-getcomputedstyle` (1 972 всего).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `<div style="top:1em;left:2em;margin-left:3em;width:5em">` при `font-size:10px` — `getComputedStyle().top/left/marginLeft/width` | `1em`/`2em`/`30px`/`50px` | `1em`/`2em` (static: computed) — тест ждёт `10px`/`20px` |
| `position:relative; top:1em; left:2em` — `top/left/right/bottom` | `10px, 20px, -20px, -10px` | `10px, 20px, -20px, -10px` |
| `getComputedStyle-insets-absolute.html` — первое расхождение | `20px` | `23.2px` |
| `getComputedStyle-insets-fixed.html` — первое расхождение | `30px` | `55.6px` |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom/getComputedStyle-insets-{absolute,fixed,nobox,relative,relpos-inline,static,sticky,sticky-container-for-abspos}.html`.

## Что делать

Довести `resolved_geometry` (CSSOM-9, CSSOM-9-S1/S2) до случаев: `static`/без бокса, дробные размеры containing block, `relative` с процентами и `calc`, `fixed` под `transform`.

## Как проверить

`css/cssom/getComputedStyle-insets-static.html`, `-absolute.html`.
