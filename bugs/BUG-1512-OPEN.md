# BUG-1512 — Scroll-state container queries (`container-type: scroll-state`, `@container scroll-state(stuck|snapped|scrollable|scrolled)`) не реализованы

**Статус:** OPEN (ДОРАБОТКА → CQ-SCROLL-STATE)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** css-parser/layout (`container-type: scroll-state`, `@container scroll-state(…)`)

## Симптом

`CSS.supports('container-type','scroll-state')` — `true`, но запрос `@container scroll-state(scrollable: bottom){…}` не срабатывает на контейнере с `container-type:scroll-state;overflow:auto` и прокручиваемым содержимым; разбор условия не знает `scroll-state()`, `cssRules` пуст (BUG-1455), все `*-parsing`/`*-serialization` падают. Состояния `stuck`, `snapped`, `scrollable`, `scrolled` (`scroll-state-*`, 34 теста) не вычисляются; для `stuck` нужен `position:sticky` (BUG-1468).

## Проба

Проба (`--mcp`): `.c{container-type:scroll-state;overflow:auto;width:100px;height:50px}` + содержимое 300px + `@container scroll-state(scrollable:bottom){#t{color:green}}` → `black`, ожидается `green`.

## Как найдено

WPT-RUN-14 срез 22: `container-queries/scroll-state/*` — 54 id, 203 из 211 сабтестов (`at-container-{scrollable,scrolled,snapped,stuck}-{parsing,serialization}` — 8 id; `scroll-state-*` — 46 id).

## Что делать

Разбор `scroll-state()` (BUG-1455 для CSSOM), состояния прокрутки контейнера в запросах и их инвалидация по прокрутке.

## Как проверить

`css/css-conditional/container-queries/scroll-state/scroll-state-scrollable-axis.html`.
