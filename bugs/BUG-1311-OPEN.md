# BUG-1311 — `align-self`/`justify-self: stretch` не растягивает replaced-элемент (`<img>`, `<canvas>`, `<input type=range>`) в grid-области

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 7, `css/css-grid`)
**Область:** layout (`crates/engine/layout/src/box_tree/grid.rs` — `stretch` у item'а с внутренними размерами/пропорцией)

## Симптом

`--dump-layout`, `display:grid; grid-template:100px/200px; width:100px; height:100px`, `<canvas width=10 height=10>`:

| разметка item'а | получено | ожидается |
|---|---|---|
| `align-self:stretch` | 10 × 10 | высота 100, ширина по пропорции 1:1 = 100 (зелёный квадрат 100 × 100) |
| `justify-self:stretch` | 10 × 10 | ширина 200 (область), высота по пропорции |
| оба | 10 × 10 | 200 × 100 |

CSS Box Alignment §6.2/Grid §6.2: у replaced-элемента `stretch` растягивает auto-размер оси; пропорция сохраняется, если вторая ось тоже auto.

## Как найдено

WPT-RUN-14 срез 7: `alignment/replaced-alignment-with-aspect-ratio-001…009.html`, `grid-item-aspect-ratio-stretch-1…4.html`, `grid-item-no-aspect-ratio-stretch-1…6.html`, `grid-align-stretching-replaced-items.html`, `grid-self-alignment-stretch-input-range.html`, `grid-items/replaced-element-011…017.html`, `grid-img-item-percent-max-height-001.html`, `percentage-size-indefinite-replaced.html`, `stretch-grid-item-*.html` (checkbox/radio/button/text input) — 47 id (45 reftest, 39 thick, 6 thin-only).

## Что делать

В `stretch`-ветке item'а с внутренними размерами растягивать ось, у которой `auto`, и пересчитывать вторую по пропорции (после `aspect-ratio`-логики); для form controls — по тем же правилам.

## Как проверить

`css/css-grid/alignment/replaced-alignment-with-aspect-ratio-001.html`, `grid-item-aspect-ratio-stretch-1.html`.
