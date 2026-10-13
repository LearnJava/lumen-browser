# BUG-1585 — Ширина границы и контура: `getComputedStyle` отдаёт дробные значения, раскладка не привязывает их к целым пикселям; `outline-offset` не округляется

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`; разбор `<line-width>` в `style/parse/`)

## Симптом

CSS Values 4 / CSS Backgrounds 3 требуют привязывать использованное значение `border-*-width` и `outline-width` к целому числу пикселей устройства (значения от 0 до 1 → 1px, остальные — округление вниз), а `outline-offset` — к целому. Движок оставляет их дробными.

## Проба

`run_smoke.py` + testharness:

| вызов | у нас | ожидается |
|---|---|---|
| `border:1.5px solid` → `borderTopWidth` | `1.5px` | `1px` |
| `border:0.5px solid` | `0.5px` | `1px` |
| `border:2.7px solid` | `2.7px` | `2px` |
| `outline:3.4px solid` → `outlineWidth` | `3.4px` | `3px` |
| `outline-offset:1.5px` → `outlineOffset` | `""` | `1px` |
| `getBoundingClientRect().width` при `width:20px; border:1.5px solid; margin:2px` | 23 | 22 |

## Как найдено

WPT-RUN-14 срез 26: `css/css-borders/border-width-rounding.tentative.html`, `outline-offset-rounding.tentative.html`, `subpixel-border-width.tentative.html`, `subpixel-borders-with-child.html`.

## Что делать

Привязывать при вычислении стиля (учитывая `devicePixelRatio`), хранить привязанное значение для раскладки, отдавать его из `getComputedStyle`; добавить `outline-offset` в `computed_style_to_map` (BUG-1278).

## Как проверить

`css/css-borders/border-width-rounding.tentative.html`.
