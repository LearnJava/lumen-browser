# BUG-1680 — корневой `<svg>` с `padding`/`border` не увеличивает свой border-box

**Статус:** OPEN
**Заведён:** 2026-10-22 (P6, побочная находка при BUG-1405)
**Область:** layout (`crates/engine/layout/src/box_tree/svg.rs` — раскладка `BoxKind::SvgRoot`)

## Симптом

```html
<svg style="position:absolute;left:50px;top:50px;background:blue;width:30px;height:30px;
            padding:10px;border:solid red 25px" width="200" height="200">
  <rect width="200" height="200" fill="green"/>
</svg>
```

`lumen --dump-layout` печатает `SvgRoot rect=(50.00, 50.00, 30.00, 30.00) … p=(10,10,10,10) bw=(25,25,25,25)`:
border-box равен content-box (30×30), хотя ожидается 110×110 (30 + 2·10 + 2·25) — для замещаемого элемента `width`/`height`
задают content-box. Рамка и фон рисуются на 30×30, содержимое SVG не сдвигается на padding + border.

## Влияние

WPT `css/css-masking/clip-path`: тесты `<geometry-box>` на `<svg>` (`clip-path-borderBox-1c…1e`, `contentBox-1c`, `-1e`,
`paddingBox-1c`, `-1e`, `fillBox-1a`/`1b`, `strokeBox-1a…1c`, `viewBox-1a…1d`, ≈17 id) расходятся с эталоном на 3–12 тыс. px,
хотя `clip-path` с `<geometry-box>` разобран и посчитан верно (BUG-1405): опорный прямоугольник строится от неверного box-а.

## Как проверить

`clip-path-borderBox-1c.html`: ожидается круг радиуса от центра border-box 80×80, получается синий квадрат 30×30.
