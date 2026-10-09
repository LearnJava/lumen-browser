# BUG-1575 — CSSOM `scroll-snap-*`, `scroll-padding*`, `scroll-margin*`: нет в `getComputedStyle`, шорткоды не канонизируются, невалидное принимается

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** layout/js (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`; разбор `scroll-padding`/`scroll-margin`/`scroll-snap-align`/`scroll-snap-type` в `style/parse/`)

## Симптом

Раскладка знает свойства (привязка работает), а CSSOM — нет: `getComputedStyle(el).scrollSnapType` — `""`, логические варианты `scroll-padding-block`/`scroll-margin-inline` не разбираются в `element.style`.

## Проба

`run_smoke.py` + testharness, `getComputedStyle(el)` / `el.style`:

| вызов | у нас | ожидается |
|---|---|---|
| `getComputedStyle(s).scrollSnapType` при `scroll-snap-type: y mandatory` | `""` | `y mandatory` |
| `getComputedStyle(s1).scrollSnapAlign` при `scroll-snap-align: start` | `""` | `start` |
| `getComputedStyle(s1).scrollSnapStop` | `""` | `normal` |
| `getComputedStyle(s).scrollInitialTarget` | `""` | `none` |
| `style.scrollSnapAlign = "start start"` → чтение | `start start` | `start` |
| `style.scrollSnapType = "inline proximity"` → чтение | `inline proximity` | `inline` |
| `style.scrollMarginBlock = "1px 2px"` → чтение `scrollMarginTop` | `""` | `1px` |
| `style.scrollPadding = "1px 2px"` → `scrollPaddingBottom` | `1px` | `1px` |
| `style.scrollPadding = "1px 2px"` → `scrollPaddingBlock` | `""` | `1px` |

## Как найдено

WPT-RUN-14 срез 26: `css/css-scroll-snap/parsing/*` (`scroll-padding-computed`, `scroll-padding-block-inline-*`, `scroll-snap-align-valid`, `scroll-snap-type-valid`, `scroll-margin-block-inline-*`), `inheritance.html`.

## Что делать

Добавить свойства в `computed_style_to_map`, разобрать `scroll-padding-block/-inline`, `scroll-margin-block/-inline` как шорткоды логических сторон с физической раскладкой по `writing-mode`/`direction`, канонизировать `scroll-snap-align` и `scroll-snap-type` (убрать значения по умолчанию), отклонять лишние значения.

## Как проверить

`css/css-scroll-snap/parsing/scroll-snap-align-valid.html`, `scroll-padding-computed.html`.
