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

Страницы тестов целиком (со скриптом `getComputedStyle(subgrid)['grid-template-columns']`) падают ещё и на тексте ошибки: ожидается `subgrid [x] [b] [] [] [b]`, отдаётся `""` — это [BUG-1307](BUG-1307-FIXED.md) (CSSOM grid-лонгхендов), не раскладка.

## Что делать

1. Ортогональный subgrid: имена и `NameFill` по оси, подставленной из родителя (`GridInit::subgrid_ctx` уже меняет источники дорожек, проверить имена и зеркало `rtl`/`vertical-rl`).
2. Вклад subgrid в `minmax(0, auto)`-дорожки родителя (BUG-1318 оставил `fr`/`minmax()` на старом пути).
3. `010`/`012`: разобрать расхождение ширин.

## Как проверить

`css/css-grid/subgrid/parent-repeat-auto-fit-001.html`, `repeat-auto-fill-003.html`, `line-names-005.html`, `line-names-007.html`.

## Что сделано (P6, ветка `p6-bug-1394`, 2026-10-20)

Инструмент: `--viewport 800x600 --ahem` пиксель-дифф (`tests/wpt/reftest_pixdiff.py`, `<script>` вырезается), замер `dev-release`.

1. `style/values/flexgrid.rs::split_track_list_tokens` — `[names]` отдельный токен без пробелов (`repeat(5,[a]30px)`, `[a][b]`, `30px[d]`); тест `track_list_names_need_no_whitespace`.
2. `grid.rs::add_area_line_names` — неявные линии `<area>-start/-end` из `grid-template-areas` входят в имена линий оси, subgrid наследует их вместе с линиями родителя.
3. `subgrid.rs::line_names_between` — пара `<area>-start/-end`, лишь частично попавшая в диапазон subgrid, обрезается по его краям (`grid-column: a` берёт часть области родителя, которую покрывает subgrid); тест `area_names_are_clipped_to_the_span`.
4. `grid.rs` (Step 3) — `content_sized_col_widths` работает и с `repeat(auto-fill|auto-fit)`, и со схлопнутыми `auto-fit`-дорожками: `minmax(0, auto)` рядом с `auto-fit` получает остаток.

Замер (px, было → стало): `line-names-005` 12 829 → **identical**, `-008` 4 629 → 596, `parent-repeat-auto-fit-001` 16 720 → **identical**, `-002` 12 880 (без изменений), `line-names-010` 10 594 → **identical**, `-007` identical, `-012` 2 352, `repeat-auto-fill-006` 1 876, `-003` 14 090. `graphic_tests/run.py --ipc`: расхождений в grid-сценах нет (изменившиеся 61/112 — view-transitions/SVG, к сетке не относятся).

## Остаток (OPEN)

- `repeat-auto-fill-003/006`: вертикальный (`writing-mode: vertical-*`) grid-контейнер **как item горизонтальной сетки** даёт высоту 0, если его столбцы неявные (`grid-auto-flow: column` + `grid-auto-columns`): `intrinsic.rs::grid_col_intrinsic_sum` для `column_flow && vertical` возвращает `None`, `orthogonal_fit_content_height` падает в «самый длинный ребёнок» = 0. Воспроизводится и без subgrid (`<div style="display:inline-grid"><div style="display:grid;writing-mode:vertical-lr;grid-auto-flow:column;grid-auto-columns:8px;grid-template-rows:15px 15px">…`). Нужна оценка числа неявных столбцов по размещению (или пробная раскладка).
- `parent-repeat-auto-fit-002`: auto-fit **строки** родителя, subgrid по строкам — расхождение во всех ячейках, не только в `minmax(0, auto)`.
- `line-names-008` (596 px, у ячеек с `position: absolute` в subgrid с `padding-left` — откуда отсчитывается `grid-column: N` у abspos x) и `line-names-012` (2 352 px, ~1 px у границ вложенных subgrid `rtl`→`ltr`).
