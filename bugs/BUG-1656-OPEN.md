# BUG-1656 — `grid-template-columns/rows`: невалидный список `none auto` принимается, `inherit` сбрасывает в initial

**Статус:** OPEN
**Заведён:** 2026-10-20 (P3, найден при закрытии BUG-1383)
**Область:** layout (`crates/engine/layout/src/style/values/flexgrid.rs::parse_track_list`, `crates/engine/layout/src/style/apply/css_wide.rs` — ветка `"grid-template-columns" | …`)

## Симптом

Проба `--dump-layout` (`display:grid; width:800px`, `getComputedStyle(el).gridTemplateColumns`):

| CSS | получено | ожидается |
|---|---|---|
| `grid-template-columns: none auto` | `800px` | `none` (объявление невалидно — `none` нельзя сочетать с треками) |
| `grid-template-columns: none 100px` | `100px` | `none` |
| `grid-template-columns: 100px none` | `100px` | `none` |
| родитель `grid-template-columns: 50px 1fr [last]`, у ребёнка `style.gridTemplateColumns = 'inherit'` | `none` | `50px 0px [last]` (значение родителя) |

Второй случай: ветка `inherit` в `css_wide.rs` («None of the grid properties are inherited») копирует `init` вместо родителя и сбрасывает сразу все grid-свойства, а не только названное.

## Как проверить

`css/css-grid/parsing/grid-columns-rows-get-set-multiple.html` — подтесты `Test setting wrong/invalid values through CSS` и `… to 'inherit' through JS`.
