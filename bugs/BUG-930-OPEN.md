# BUG-930 — Canvas 2D: wide-gamut цвет читается обратно потерявшим пространство

**Статус:** OPEN
**Компонент:** js (`crates/js/src/canvas2d.rs` — нативы `_lumen_canvas2d_set_fill_style`/
`_lumen_canvas2d_set_stroke_style`/`_lumen_canvas2d_set_shadow_color`),
canvas (`crates/engine/canvas/src/color.rs` — `CanvasColor`)
**Заведён:** 2026-08-30 (P3) при закрытии [BUG-451](BUG-451-FIXED.md) — это его
явно названный остаток, а не новая находка

## Сужен 2026-09-19 (P3)

Исходно баг описывал две независимые половины одной нехватки контекста у
`CanvasColor` (четыре `u8` и ничего больше). Половина 1 — `currentColor`
отбрасывался как невалидный — закрыта в этом срезе: разрешение keyword'а
сделано на границе JS/натив (`_lumen_c2d_resolve_current_color`,
`crates/js/src/shim/web_api_shim_mid.js`), а не в самом парсере — элемент уже
известен нативу по `nid`, так что подстановка вычисленного `color` (или
непрозрачного чёрного, если стиль ещё не посчитан) в текст CSS-значения перед
вызовом `CanvasColor::from_css_str` покрывает и голый `currentColor`, и вложенные
формы (`color-mix(in srgb, black, currentcolor)`), так как замена — текстовая, до
разбора. Регресс — `crates/js/src/dom/tests/v8_bug930_canvas_currentcolor.rs`.

Остаётся только половина 2 (wide-gamut сериализация) — она по-прежнему требует
изменения самого типа `CanvasColor`, а не границы вызова.

## Симптом — wide-gamut значение теряет пространство при чтении

```js
ctx.fillStyle = 'color(display-p3 0 1 0)';
ctx.fillStyle;   // '#00ff00' — гамут-маппинг в sRGB, пространство потеряно
                 // спека: 'color(display-p3 0 1 0)'
```

`parse_color` отдаёт `Color` (sRGB, 8 бит), поэтому и рисование, и сериализация
идут уже по сведённому значению. Каскад для того же входа сохраняет
`CssColor::Wide(ColorFloat)` с `ColorSpace` — то есть нужный тип в движке есть,
он просто не доходит до Canvas 2D.

## Направление починки

**Wide-gamut** требует float-варианта `CanvasColor` с полем `ColorSpace` (по
образцу `ColorFloat` в lumen-layout) и хранения исходного пространства ради
сериализации; рисование при этом может оставаться sRGB-байтовым.

## Цена в WPT

измерено прогоном категории 2026-08-30 — пять файлов, у каждого 0/1 сабтеста:
`2d.fillStyle.colormix`, `2d.fillStyle.colormix.currentcolor`,
`2d.strokeStyle.colormix`, `2d.gradient.colormix`, `2d.fillStyle.CSSHSL`.
Первые четыре падают ТОЛЬКО на сериализации — цвет разбирается и рисуется
верно, но читается обратно как `#800080` вместо `color(srgb 0.5 0 0.5)`;
`CSSHSL` требует ещё и типизированных цветовых объектов (серия
`2d.fillStyle.colorObject.*`). Плюс wide-gamut `…/manual/wide-gamut-canvas/*`.
