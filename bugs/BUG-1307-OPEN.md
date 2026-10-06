# BUG-1307 — `getComputedStyle()` не отдаёт ни одного grid-лонгхенда: `grid-template-*`, `grid-auto-*`, `grid-column/row/area`, `row-gap`/`column-gap`

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 7, `css/css-grid`)
**Область:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map` — там `grid` встречается только в `display`; тот же класс, что [BUG-1254](BUG-1254-OPEN.md) для flex)

## Симптом

`display:grid; grid-template-columns:100px 1fr; grid-template-rows:30px auto; grid-template-areas:'a b' 'c d'; grid-auto-columns:20px; grid-auto-rows:10px; grid-auto-flow:column; row-gap:3px; column-gap:4px; justify-items:center`, `--dump-layout` + скрипт:

| свойство | получено | ожидается |
|---|---|---|
| `gridTemplateColumns` / `Rows` / `Areas` | `""` | `100px 400px`… (разрешённые px по трекам) |
| `gridAutoColumns` / `gridAutoRows` / `gridAutoFlow` | `""` | `20px` / `10px` / `column` |
| `rowGap` / `columnGap` | `""` | `3px` / `4px` |
| у элемента: `gridColumnStart/End`, `gridRowStart/End`, `gridColumn`, `gridRow`, `gridArea`, `order` | `""` | `2` / `auto` / `1`… |
| `getPropertyValue('grid-template-columns')` | `""` | то же |

`justify-items` отдаётся (`center`), `justify-self`/`align-self` — `auto`.

## Как найдено

WPT-RUN-14 срез 7: 29 id / 974 сабтеста `css/css-grid` (`grid-layout-properties.html` 138, `inheritance.html` 20, `grid-definition/grid-*-support-*` (named-lines, flexible-lengths, repeat, template-areas, columns-rows) и `*-resolved-values-*` по 40–80 сабтестов, `grid-minimum-contribution-with-percentages`, `animation/grid-template-columns-neutral-keyframe-*`). Сообщения: `gridTemplateColumns value "" not in array ["150px"]`, `grid-auto-columns doesn't seem to be supported in the computed style`. Для `grid-template-*` ожидается *разрешённое* значение (список px по трекам после раскладки) — как у `resolved_geometry` (CSSOM-9), не серилизация указанного.

## Что делать

Строки для перечисленных свойств в `computed_style_to_map` (указанные значения для `grid-auto-*`, `gap`, line-based placement; для `grid-template-columns/rows` — разрешённые размеры треков из раскладки, `none` для не-grid). `animation/grid-template-columns-*` ждёт ещё [BUG-1305](BUG-1305-OPEN.md) (неявный кадр).

## Как проверить

`css/css-grid/grid-layout-properties.html`, `inheritance.html`, `grid-definition/grid-support-grid-template-columns-rows-001.html`, `grid-definition/grid-template-columns-rows-resolved-values-001.html`.
