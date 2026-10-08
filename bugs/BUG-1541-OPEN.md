# BUG-1541 — CSSOM `filter`/`backdrop-filter`: `backdrop-filter` нет в `getComputedStyle`; недопустимое принимается; `blur(0)` не канонизируется; `flood-*`/`lighting-color`/`color-interpolation-filters` не разбираются

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** css-parser/layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`; `style/parse/transform.rs` — разбор `filter`/`backdrop-filter`)

## Симптом

`getComputedStyle(el).backdropFilter` — `""` (`'backdrop-filter' in cs` — `false`). `filter = "blur(5px 6px)"`, `"auto"`, `blur(-1px)` принимаются (должны отвергаться); сериализация `blur(0)` не приводится к `blur(0px)`; `filter: url(#x)` и `drop-shadow(…)` в computed — `none`; `backdrop-filter: !important` читается с суффиксом `!important`. `flood-color`, `flood-opacity`, `lighting-color`, `color-interpolation-filters`: `CSS.supports` `false` (9+9+2+5 сабтестов «doesn't seem to be supported in the computed style»). WAAPI: `el.animate({filter:["blur(0px)","blur(20px)"]})` + `pause()` + `currentTime=2000` — `getComputedStyle().filter` — `blur(20px)` (ожидается `blur(10px)`, интерполяция `filter` не работает — [BUG-1234](BUG-1234-OPEN.md)). 24 id, 560 из 877 сабтестов (+ 7 id / 672 из 954 — `animation/*-interpolation*`).

## Проба

Проба (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `getComputedStyle(t).backdropFilter` для `blur(5px)` | `""` | `blur(5px)` |
| `style.filter="blur(5px 6px)"` → computed | `none` (в `style` — записано) | отвергнуто |
| `style.filter="auto"` → `style.filter` | `auto` | `""` |
| `style.filter="blur(0)"` → `style.filter` | `blur(0)` | `blur(0px)` |
| `style.filter="url(#x)"` → computed | `none` | `url("#x")` |
| `CSS.supports("flood-color","red")` | `false` | `true` |


## Как найдено

WPT-RUN-14 срез 24: `css/filter-effects/parsing/{filter,backdrop-filter}-{computed,parsing-valid,parsing-invalid}`, `parsing/{flood-color,flood-opacity,lighting-color,color-interpolation-filters}-*`, `inheritance.html`, `backdrop-filter-important`, `filter-sign-function`, `animation/*`, `drop-shadow-currentcolor-inheritance`, `idlharness.any.html` (306 из 485: `SVGFilterElement` и интерфейсы `SVGFE*` перечислимы/отсутствуют — отдельная причина, SVG-FILTER).

## Что делать

Добавить `backdrop-filter` в карту; отвергать недопустимые аргументы/`auto`/отрицательные `blur`; канонизировать `blur(0)`; разбор и карта для `flood-color`/`flood-opacity`/`lighting-color`/`color-interpolation-filters`; `url()` в computed — после SVG-FILTER.

## Как проверить

`css/filter-effects/parsing/backdrop-filter-computed.html`, `parsing/filter-parsing-valid.html`.
