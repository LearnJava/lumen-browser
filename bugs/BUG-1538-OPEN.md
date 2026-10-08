# BUG-1538 — SVG-фильтры не реализованы: `filter: url(#id)` и элемент `<filter>` с примитивами `fe*` ничего не делают

**Статус:** OPEN (ДОРАБОТКА → SVG-FILTER)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout/paint/js (нет типа SVG-фильтра: `FilterFn` — 9 функций CSS, `crates/engine/layout/src/style/values/transform.rs:149`; `<filter>`/`fe*` есть только в таблице имён `html-parser/src/foreign_content.rs`)

## Симптом

Для `<filter>` с любым из 16 проверенных примитивов (`feFlood`, `feOffset`, `feGaussianBlur`, `feColorMatrix`, `feComposite`, `feDropShadow`, `feDisplacementMap`, `feImage`, `feTile`, `feMorphology`, `feTurbulence`, `feDiffuseLighting`, `feConvolveMatrix`, `feComponentTransfer`, `feBlend`, `feMerge`) закрашенный `<rect filter="url(#f)">` рисуется без изменений; то же для `filter:url(#f)` на HTML-элементе (`getComputedStyle().filter` — `none`). `filter: url(#x) blur(1px)` применяет только `blur`. `flood-color`, `flood-opacity`, `lighting-color`, `color-interpolation-filters` — `CSS.supports` `false`. 94 id (из них 14 `reftest-wait`; по именам: `tainting-*` 17, `feImage*`/`svg-feimage-*` 17, `svg-mutation-*` 12, `effect-reference-*` 10), остальные — `filter-region-*`, `feconvolve-region-*`, `fe*` по одному. Крупнейший кластер модуля: 94 из 228 не зелёных id `filter-effects` (41 %), 10 % не зелёных среза.

## Проба

Проба (`--screenshot`, `<svg><filter id=f color-interpolation-filters=sRGB>PRIM</filter><rect width=100 height=100 fill=blue filter="url(#f)"/></svg>`, цвет пикселя `(50,50)`):

| PRIM | у нас | ожидается |
|---|---|---|
| `<feFlood flood-color="rgb(0,255,0)"/>` | `(0,0,255)` | `(0,255,0)` |
| `<feColorMatrix type="hueRotate" values="90"/>` | `(0,0,255)` | другой цвет |
| `<feOffset dx=20 dy=0/>`, `<feGaussianBlur stdDeviation=5/>`, `<feImage href=…/>`, `<feTurbulence …/>` и ещё 12 | `(0,0,255)` без изменений | изменён |
| `<div style="filter:url(#g)">` (HTML, `#g` = `feFlood` зелёный) | `(0,0,255)` | `(0,255,0)` |


## Как найдено

WPT-RUN-14 срез 24: `css/filter-effects` — `tainting-*`, `effect-reference-*`, `feImage-*`, `svg-feimage-*`, `svg-mutation-*`, `fe*-*`, `filter-region-*`, `filter-subregion-01`, `filter-external-*`, `svg-filter-*-user-space*`, `filter-turbulence-invalid-001`, `empty-element-with-filter*`, `feconvolve-region-*`, `lighting-region`, `morphology-mirrored`, `filter-scale-001`, `svg-visibility-hidden-element-with-filter-*`. В `crates/` `feGaussianBlur`/`feColorMatrix`/`feDisplacementMap` встречаются только в таблице тегов `html-parser/src/foreign_content.rs` (и интерфейс `SVGFilterElement` в JS-шиме); растеризации фильтр-графа нет. Ни `BUGS.md`, ни `ROADMAP.md` его не называли (`BUG-1294` — `<pattern>`, не фильтр). Нереализованная функциональность: тип фильтра-графа, 16+ примитивов, `filterUnits`/`primitiveUnits`, `in`/`result`, подрегионы, `color-interpolation-filters`, `url()` во внешних ресурсах — семейство, не точечная правка (`docs/probe-method.md` §8).

## Что делать

Задача `SVG-FILTER` (`ROADMAP.md`): тип `FilterRef::Url` в `ComputedStyle::filter`/`backdrop_filter` рядом с `FilterFn`; разбор `<filter>`/`fe*` и графа примитивов в `layout`; исполнение графа в `cpu_raster.rs` и `renderer.rs` (две независимые реализации, `crates/engine/paint/CLAUDE.md`); `getComputedStyle` для `url()`; свойства `flood-color`/`flood-opacity`/`lighting-color`/`color-interpolation-filters` (CSS-SPECS).

## Как проверить

`css/filter-effects/svg-feflood-001.html`, `tainting-feflood-001.html`, `effect-reference-feimage-001.html`.
