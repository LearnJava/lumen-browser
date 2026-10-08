# BUG-1394 — subgrid: остаток WPT `css/css-grid/subgrid` после имён линий (BUG-1319)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P6, при закрытии [BUG-1319](BUG-1319-FIXED.md))
**Область:** layout (`crates/engine/layout/src/box_tree/grid.rs`, `grid_vertical.rs`, `grid_auto_cols.rs` — subgrid в ортогональном `writing-mode`/`direction: rtl`, `minmax()` в родителе, padding/border самого subgrid)

## Симптом

После BUG-1319 (имена линий родителя, `subgrid [a] [b]`, `repeat(auto-fill, [..])`, прижим позиций к явной сетке) `--viewport 800x600 --ahem` пиксель-дифф (`tests/wpt/reftest_pixdiff.py`, скрипт самопроверки `getComputedStyle` вырезан) даёт `identical` у `line-names-001/002/004/006/009/011`, `repeat-auto-fill-001/002/004/005/007/008`. Остаются `thick`:

| тест | дифф, px | что видно |
|---|---|---|
| `parent-repeat-auto-fit-001/002` | 16 720 / 12 880 | у ячеек, где в родителе есть `minmax(0, auto)`, ширина последней дорожки не та (вклад элементов subgrid в `minmax()`-дорожку не считается — `grid_auto_cols` берёт только `auto`/длины); остальные ячейки совпадают с эталоном |
| `repeat-auto-fill-003`, `-006` | 87 590 / 171 320 | вложенный subgrid с `writing-mode: vertical-lr` и `grid-template-rows: subgrid` — имена родителя по ортогональной оси; причина не разобрана |
| `line-names-005` | 12 829 | `direction: rtl` у родителя/subgrid — имена линий при зеркальной оси не переворачиваются (`SubgridContext::names` берётся в порядке родителя) |
| `line-names-007`, `-008`, `-010`, `-012` | 1 080 / 4 158 / 10 594 / 13 046 | размещение по именам верное (у `007` проверено глазами), расходятся ширины на несколько px — вклад `margin/border/padding` самого subgrid на краю; `010`/`012` не разбирались |

Страницы тестов целиком (со скриптом `getComputedStyle(subgrid)['grid-template-columns']`) падают ещё и на тексте ошибки: ожидается `subgrid [x] [b] [] [] [b]`, отдаётся `""` — это [BUG-1307](BUG-1307-OPEN.md) (CSSOM grid-лонгхендов), не раскладка.

## Что делать

1. Ортогональный subgrid: имена и `NameFill` по оси, подставленной из родителя (`GridInit::subgrid_ctx` уже меняет источники дорожек, проверить имена и зеркало `rtl`/`vertical-rl`).
2. Вклад subgrid в `minmax(0, auto)`-дорожки родителя (BUG-1318 оставил `fr`/`minmax()` на старом пути).
3. `010`/`012`: разобрать расхождение ширин.

## Как проверить

`css/css-grid/subgrid/parent-repeat-auto-fit-001.html`, `repeat-auto-fill-003.html`, `line-names-005.html`, `line-names-007.html`.
