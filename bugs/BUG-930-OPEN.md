# BUG-930 — Canvas 2D: `currentColor` отбрасывается как невалидный, а wide-gamut цвет читается обратно потерявшим пространство

**Статус:** OPEN
**Компонент:** js (`crates/js/src/canvas2d.rs` — нативы `_lumen_canvas2d_set_fill_style`/
`_lumen_canvas2d_set_stroke_style`/`_lumen_canvas2d_set_shadow_color`),
canvas (`crates/engine/canvas/src/color.rs` — `CanvasColor`)
**Заведён:** 2026-08-30 (P3) при закрытии [BUG-451](BUG-451-FIXED.md) — это его
явно названный остаток, а не новая находка

## Почему это один баг, а не два

Обе половины — одно и то же: `CanvasColor` это четыре `u8` и ничего больше, а
`from_css_str` видит только строку. Ни элемента (чей `color` нужен для
`currentColor`), ни цветового пространства (нужного, чтобы прочитать значение
обратно) в этом типе нет.

## Симптом 1 — `currentColor` игнорируется

```js
canvas.setAttribute('style', 'color: magenta');
ctx.fillStyle = 'currentColor';   // отброшено как невалидное
ctx.fillStyle;                    // прежнее значение, например '#000000'
```

HTML LS §4.12.5.1.3 требует разрешить keyword в вычисленный `color` элемента
`<canvas>`, а для контекста, не связанного с элементом, — в непрозрачный чёрный.
До BUG-451 значение молча удерживало предыдущий цвет вместе со всеми остальными
неподдержанными формами; теперь оно так же молча игнорируется — режим отказа стал
спек-корректным для *невалидного* значения, но `currentColor` валиден.

Тот же отказ уносит всё, что содержит keyword внутри:
`color-mix(in srgb, black, currentcolor)`, `rgb(from currentColor r g b)`.

## Симптом 2 — wide-gamut значение теряет пространство при чтении

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

1. **`currentColor`** чинится не в парсере, а на границе: элемент нативу уже
   известен (`nid`), так что keyword надо разрешать в
   `_lumen_canvas2d_set_*_style` — подставлять вычисленный `color` элемента перед
   вызовом `CanvasColor::from_css_str`. Отдельно решить, что делать при
   `color`, которого ещё нет (страница может писать `fillStyle` до первой
   раскладки).
2. **Wide-gamut** требует float-варианта `CanvasColor` с полем `ColorSpace` (по
   образцу `ColorFloat` в lumen-layout) и хранения исходного пространства ради
   сериализации; рисование при этом может оставаться sRGB-байтовым.

Половины независимы: первую можно закрыть отдельным срезом, не трогая тип.

## Цена в WPT

измерено прогоном категории 2026-08-30 — пять файлов, у каждого 0/1 сабтеста:
`2d.fillStyle.colormix`, `2d.fillStyle.colormix.currentcolor`,
`2d.strokeStyle.colormix`, `2d.gradient.colormix`, `2d.fillStyle.CSSHSL`.
Первые четыре падают ТОЛЬКО на сериализации — цвет разбирается и рисуется
верно, но читается обратно как `#800080` вместо `color(srgb 0.5 0 0.5)`;
`CSSHSL` требует ещё и типизированных цветовых объектов (серия
`2d.fillStyle.colorObject.*`). Плюс wide-gamut `…/manual/wide-gamut-canvas/*`.
