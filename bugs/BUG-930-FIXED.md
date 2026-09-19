# BUG-930 — Canvas 2D: wide-gamut цвет читается обратно потерявшим пространство

**Статус:** FIXED 2026-09-19 (P3)
**Компонент:** js (`crates/js/src/canvas2d.rs` — нативы `_lumen_canvas2d_set_fill_style`/
`_lumen_canvas2d_set_stroke_style`/`_lumen_canvas2d_set_shadow_color`),
canvas (`crates/engine/canvas/src/color.rs` — `CanvasColor`),
layout (`crates/engine/layout/src/style/parse/color.rs` — `parse_color_function`,
`crates/engine/layout/src/style/values/color.rs` — `ColorFloat::to_css_string`)
**Заведён:** 2026-08-30 (P3) при закрытии [BUG-451](BUG-451-FIXED.md) — это его
явно названный остаток, а не новая находка

## Исправлено 2026-09-19 (P3)

`CanvasColor` получил поле `wide: Option<ColorFloat>` — рисование (`r/g/b/a`)
остаётся sRGB-байтовым независимо от исходного пространства, но когда значение
разобрано из функциональной формы (`color(<space> …)` — CSS Color L4 §10.1,
или `color-mix()`, смешанный `in srgb`, — CSS Color L5 §10.2 использует
*предопределённое* пространство `srgb`, не legacy-числовую модель, поэтому CSS
Color L4 §4.2 требует сохранить функциональную форму при сериализации),
`wide` хранит исходный `ColorFloat`, и `to_css_string` сериализует именно его.

Новая публичная `lumen_layout::parse_color_function(s) -> Option<ColorFloat>`
разбирает обе формы (литеральный `color()` и `color-mix(in srgb, …)`) без
округления в `u8` между шагами — иначе `color-mix(in srgb, red, blue)` читался
бы как `color(srgb 0.5019608 0 0.5019608)` вместо точного `0.5` (128/255 не
равно 0.5 при обратном делении). Для этого `parse_color_mix` в том же файле
разложен на float-ядро (`parse_color_mix_f32`, возвращает `[f32; 4]` до
округления) и тонкую u8-обёртку для существующих потребителей.
`ColorFloat::to_css_string` — новый метод в lumen-layout, переиспользуемый:
печатает `color(<space> r g b[ / a])` через `Display` для `f32` (кратчайшая
round-trip-ящая десятичная запись Rust), альфа опускается для непрозрачного.

Регресс: `crates/engine/canvas/src/color.rs` (`wide_gamut_color_function_round_trips_through_serialization`,
`color_mix_in_srgb_round_trips_through_functional_form`,
`wide_gamut_color_still_gamut_maps_to_srgb_bytes_for_rendering`),
`crates/js/src/dom/tests/v8_bug930_canvas_wide_gamut.rs` (через реальный `ctx.fillStyle`/`ctx.strokeStyle`).
Существующий `v8_bug930_canvas_currentcolor.rs` обновлён: `color-mix(in srgb,
black, currentcolor)` теперь ожидаемо сериализуется как `color(srgb 0.5 0.5
0.5)`, а не `#808080` — это то же самое исправление, применённое к уже
существующему регрессу.

Вне скоупа (не являются частью дефекта, которого касалось «Направление
починки» ниже — типизированные цветовые объекты, отдельная фича Canvas 2D,
не парсинг строк): `2d.fillStyle.CSSHSL` (конструктор `CSSHSL`, CSS Typed OM)
и серия `2d.fillStyle.colorObject.*`. `2d.gradient.colormix` не читает
`fillStyle` обратно (только пиксельные ассерты `addColorStop`) — этой правки
не касается, рисование не менялось. Manual `…/manual/wide-gamut-canvas/*` не
прогонялся (нет автоматизации для manual-тестов).

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
