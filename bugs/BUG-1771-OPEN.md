# BUG-1771 — `getComputedStyle` инсетов: abspos в `position: relative` inline-контейнере и в grid-области считается от неверного блока

**Статус:** OPEN
**Заведён:** 2026-10-28 (P3, остаток [BUG-1556](BUG-1556-FIXED.md))
**Область:** layout (`crates/engine/layout/src/box_tree/` — containing block abspos; `crates/engine/layout/src/resolved_geometry.rs::child_ctx`)

## Симптом

1. `css/cssom/getComputedStyle-insets-relpos-inline.html` (4 сабтеста): `position: absolute` блок внутри `<span style="position: relative">` — containing block это inline-бокс span (его ограничивающий прямоугольник), а Lumen считает от ближайшего позиционированного **блока** (`.ifc`). Ожидается `left: 0px; right: 140px`, у нас `left` отсчитывается от левого края `.ifc`, `right` — `76.5px`.
2. `css/cssom/getComputedStyle-insets-grid.html` (1 сабтест): `position: absolute` потомок абсолютно позиционированного grid-контейнера с `grid-column: 2` — containing block это область сетки (CSS Grid §9.2), `left` ждёт `0px`; `resolved_geometry` берёт padding box контейнера.

## Проба

`<div class=ifc style="position:relative;width:max-content"> Lorem <span style="position:relative">ipsum dolor <div style="position:absolute;inset-inline-start:0;width:5em;height:1em;top:1em"></div></span> sit amet</div>` — `getBoundingClientRect()` цели: `x` равен `x` самого `.ifc` (8), а не `x` span (73).

## Что делать

Раскладка: containing block абсолютного потомка inline-бокса с `position: relative` — прямоугольник, охватывающий его фрагменты (CSS 2.1 §10.1 п.4). `resolved_geometry`: для abspos в grid-контейнере брать область сетки.

## Как проверить

`run_smoke.py /css/cssom/getComputedStyle-insets-relpos-inline.html /css/cssom/getComputedStyle-insets-grid.html`; после правки удалить оба `.ini`.
