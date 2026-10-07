# BUG-1319 — subgrid не наследует имена линий родителя

**Статус:** FIXED 2026-10-07 (P6)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 8, `css/css-grid`, вторая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/grid.rs`, subgrid: CSS Grid L2 §9.3 «Subgrid line names» — имена линий родителя добавляются к собственным линиям subgrid)

## Симптом

`--dump-layout`, родитель `.o{display:grid;width:300px;grid-auto-rows:20px}`:

| разметка | получено | ожидается |
|---|---|---|
| `.o{grid-template-columns:[x] 100px 100px [y] 100px [z]}`, `<i style="grid-column:y">` прямо в `.o` | x = 200, ширина 100 | x = 200, ширина 100 |
| то же, `<i style="grid-column:y">` в subgrid на 3 дорожки (`grid-column:1/4; grid-template-columns:subgrid`) | **x = 0, ширина 0** | x = 200, ширина 100 |
| `grid: auto / [a] 50px 50px [a] 50px 50px [a]` + subgrid `span 3` + ребёнок `grid-column: span a / a -1` | высота страницы 138 | 90 (`line-names-001`) |

Прямое размещение по имени (первая строка) работает — ломается именно проход имён через subgrid.

## Как найдено

WPT-RUN-14 срез 8, `css/css-grid/subgrid/`: `line-names-001…012` (10 id), `repeat-auto-fill-001…008` (8), `parent-repeat-auto-fit-001/002` (2) — все 20 reftest `thick`. Для `line-names-*` причина подтверждена пробой (таблица), для `repeat-auto-fill-*` и `parent-repeat-auto-fit-*` — нет, они сгруппированы по имени файла.

## Что делать

При вычислении линий subgrid добавлять к его списку имён имена соответствующих линий родителя (в порядке следования), раскрывать `repeat(auto-fill, <line-names>)` по числу линий; разрешение `grid-column: <name>` у детей subgrid вести по этому объединённому списку.

## Как проверить

`css/css-grid/subgrid/line-names-001.html`, `repeat-auto-fill-001.html`, `parent-repeat-auto-fit-001.html`.

## Решение (2026-10-07, P6)

Корень шире описания: имена линий у subgrid не хранились вообще (`names: &[]`), а значение `subgrid [a] [b]` с именами не разбиралось как subgrid (дорожки получались пустыми, контейнер переставал быть subgrid).

- Разбор (`style/values/flexgrid.rs`): `subgrid <line-name-list>` — `[..]`, `repeat(N, [..]+)`, одно `repeat(auto-fill, [..]+)` (`NameFill` в `ComputedStyle::grid_template_{col,row}_subgrid_fill`, имена — в `grid_template_*_line_names`); в `GridRepeat` добавлены `names: RepeatLineNames` (имена до/внутри/после `repeat(auto-*)`), чтобы имена родителя шли за раскрытием повторов (раньше `repeat(auto-fill, [a] 10px)` считался один раз и сдвигал индексы).
- Раскладка (`box_tree/grid.rs`): `axis_line_names` строит имена линий оси (с раскрытым `repeat`; у subgrid — собственные + имена охваченных линий родителя, `subgrid_line_names`; `auto-fill` в subgrid повторяется, пока влезает рядом с написанными именами, список режется по числу линий). Имена лежат в `GridInit::{col,row}_names` и уходят в `SubgridContext::names` (срез `line_names_between`), так что вложенные subgrid передают их дальше. Вклад subgrid в авто-дорожки (`grid_auto_cols`) размещает детей по тем же именам.
- Позиции детей subgrid прижимаются к явной сетке (L2 §9: неявных дорожек нет) — `GridAxis::clamp`; без этого `y 5` при отсутствии линий `y` уходило за последнюю дорожку (x = 0, ширина 0).

Тесты: `box_tree/tests/grid_subgrid_line_names.rs` (10: пример из «Симптома», подсетка на части родителя, собственные имена, `auto-fill` в subgrid, имена `repeat(auto-fill)` родителя, вложенный subgrid, разбор). Пиксель-дифф подкаталога (`--viewport 800x600 --ahem`, скрипт самопроверки вырезан): `identical` 30 → 36 из 98; у 20 целевых — `line-names-001/002/004/006/009/011`, `repeat-auto-fill-001/002/004/005/007/008`. Остаток — [BUG-1394](BUG-1394-OPEN.md); страницы целиком ещё падают на `getComputedStyle(subgrid)['grid-template-columns']` ([BUG-1307](BUG-1307-OPEN.md)).
