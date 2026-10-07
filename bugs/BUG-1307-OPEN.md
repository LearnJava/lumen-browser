# BUG-1307 — `getComputedStyle()` не отдаёт ни одного grid-лонгхенда: `grid-template-*`, `grid-auto-*`, `grid-column/row/area`, `row-gap`/`column-gap`

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 7, `css/css-grid`)
**Область:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map` — там `grid` встречается только в `display`; тот же класс, что [BUG-1254](BUG-1254-FIXED.md) для flex)

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

## Дополнение: WPT-RUN-14 срез 8 (2026-10-06, `css/css-grid`, часть 2)

Вторая половина `css-grid` добавляет 46 id / 1 005 сабтестов с тем же симптомом (`gridTemplateColumns value "" not in array ["Npx Npx"]`, `grid-template-columns doesn't seem to be supported in the computed style`, `… should be canonical`): `parsing/grid-template-columns-computed.html` (140), `parsing/grid-shorthand-serialization.html` (89), `subgrid/grid-template-computed-nogrid.html` (50), `parsing/grid-area-computed.html` (35), `layout-algorithm/grid-flex-track-intrinsic-sizes-001/002/003` (30 + 5 + 102), `layout-algorithm/grid-automatic-minimum-for-auto-columns-001.html`. Для `subgrid` ожидается значение вида `subgrid [] [] [] [] [x]` (разрешённые имена линий), для `grid-template-areas` — сериализация строк. В `grid-lanes/` та же причина у 50 id / 765 сабтестов (в том числе `grid-lanes-grid-template-columns-computed-withcontent.html`, `flow-tolerance-interpolation.html` — 240 сабтестов `'from' value should be supported`); они учтены в `GRID-LANES`, но не закроются до него. Итого по `css-grid` — 75 id и около 2 000 сабтестов в обеих половинах без `grid-lanes`.
