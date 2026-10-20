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

## Журнал работы (P6, WIP на ветке `p6-bug-1394`, 2026-10-20)

Инструмент: `.tmp/pd.py <тест-без-.html>...` (не в git; обёртка над `tests/wpt/reftest_pixdiff.py`: абсолютный `--binary`, `--ahem`, 800x600, `<script>` из тестов и эталонов вырезается в `.tmp/sgs/`), `.tmp/crop.py a.png b.png x y w h scale out.png` — увеличенный кроп пары скриншотов. Нужен `cargo build --profile dev-release -p lumen-shell` (~5 мин).

Сделано (код, `cargo check` проходит, бинарь после правок 2–3 НЕ пересобран и не проверен):
1. `style/values/flexgrid.rs::split_track_list_tokens` — `[names]` отдельный токен без пробелов (`repeat(5,[a]30px)`, `[a][b]`, `30px[d]`); раньше трек в `repeat(5,[a]30px)` пропадал, сетка `line-names-008` (4-я ячейка) разъезжалась. Тест `track_list_names_need_no_whitespace` (`box_tree/tests/grid_subgrid_line_names.rs`) зелёный.
2. `grid.rs::add_area_line_names` (вызов в `axis_line_names`, и в `grid_auto_cols::collect_col_contributions`) — неявные линии `<area>-start/-end` из `grid-template-areas` попадают в имена линий оси, значит subgrid наследует их вместе с линиями родителя (`line-names-005` ячейки 11/12, `line-names-008` 596 px вместо 4629 после п.1 и до п.2).
3. `grid.rs` (Step 3) — путь `content_sized_col_widths` теперь работает и с `repeat(auto-fill|auto-fit)` и со схлопнутыми `auto-fit`-дорожками (если `col_gap == 0` либо схлопнутых нет): `minmax(0, auto)` рядом с `auto-fit` получает остаток ширины (`parent-repeat-auto-fit-001/002`: у `fit-0a/0b/2b…5` последняя дорожка оставалась 0). Проверить пиксель-диффом и `graphic_tests` (могут сдвинуться сетки с `repeat(auto-*)` + `auto`).

Замер до правок 2–3 (`.tmp/pd.py`, 800x600 ahem): `parent-repeat-auto-fit-001/002` 16720/12880, `repeat-auto-fill-003` 14090 (в 003 у нас почти пусто: вложенные `.subgrid` с `writing-mode: vertical-lr` в inline-grid не рисуются — главный нерешённый кусок), `-006` 1876, `line-names-005` 12829 (дифф только в ячейках 11/12 — это п.2, rtl-ячейки 2/3 уже совпадают), `-008` 4629→596, `-010` 10594, `-012` 2352. `line-names-007` уже identical.

Дальше: (а) пересобрать `dev-release`, прогнать `.tmp/pd.py` по 9 тестам из «Как проверить» плюс весь `css/css-grid/subgrid`; (б) `repeat-auto-fill-003/006` — ортогональный вложенный subgrid; (в) `line-names-010/012` и остаток 008 (596 px); (г) если `direction` subgrid противоположно родителю — развернуть `sizes`/`names` в `GridInit::subgrid_ctx` (идея, не делалась); (д) полный `python graphic_tests/run.py --continue-on-fail` (правка 3 может двигать пиксели), цель — в CAPABILITIES.md/BUGS при закрытии.
