# BUG-1319 — subgrid не наследует имена линий родителя

**Статус:** OPEN
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
