# BUG-1298 — `border-width: thin|medium|thick` в шортхендах и `border-*-width` не разбирается; `border-style` без ширины даёт 0 вместо `medium` (3px)

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** layout (`crates/engine/layout/src/style/parse/box_sides.rs::apply_border_shorthand`/`apply_border_side_shorthand`, `style/apply/paint.rs` — `"border-width"`, `"border-top-width"`… — зовут `resolve_box_length`, а не `parse_line_width`)


## Симптом

`--screenshot`, `<div style="width:20px;height:20px;…">`, ширина левой рамки:

| стиль | получено | ожидается |
|---|---|---|
| `border:3px solid red` | 3 | 3 |
| `border:thin solid red` | 0 | 1 |
| `border:medium solid red` | 0 | 3 |
| `border:thick solid red` | 0 | 5 |
| `border-width:thick;border-style:solid` | 0 | 5 |
| `border-left:thick solid red` | 0 | 5 |
| `border-top-width:thick` + `border-style:solid` | 0 | 5 |
| `border:solid red` (ширина по умолчанию) | 0 | 3 (`medium`) |
| `border-style:solid;border-color:red` | 0 | 3 |

`outline`/`column-rule` те же ключевые слова разбирают (`parse_line_width`, `box_sides.rs:228`), `border` — нет: токен `thin` не проходит `resolve_box_length`, и ширина остаётся начальной 0. Заодно: начальная ширина границы — `medium` (3px), «обнуляется» она только при `border-style: none|hidden` (CSS Backgrounds 3 §4.1); `ComputedStyle::border_*_width` стартует с 0.0 и не восстанавливается, когда стиль появляется позже.

## Как найдено

WPT-RUN-14 срез 6: `border-{top,right,bottom,left}-width-{thin,medium,thick}.html` — 12 reftest, плюс `border-width-cssom.html`, `border-width-pixel-snapping-*` (не проверены). Влияет на любой реальный сайт: `border: solid` и `border: thin solid` — частые формы.

## Что делать

В шортхендах `border`, `border-<side>`, `border-width`, longhand `border-*-width` разбирать `<line-width>` через `parse_line_width`; начальное значение ширины — 3px, обнулять при `none|hidden` (вычисленное). Не трогать `resolve_box_length` (его зовут margin/padding).

## Как проверить

`css/css-backgrounds/border-{top,right,bottom,left}-width-{thin,medium,thick}.html`.
