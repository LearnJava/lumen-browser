# BUG-1308 — проценты в `grid-template-rows` и `row-gap` при неопределённой высоте контейнера считаются от ширины

**Статус:** FIXED 2026-10-20 (P3)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 7, `css/css-grid`)
**Область:** layout (`crates/engine/layout/src/box_tree/grid.rs` — разрешение `%` в треках и gap)

## Симптом

`--dump-layout`, ширина контейнера 200, высота `auto`:

| разметка | получено | ожидается |
|---|---|---|
| `display:grid; grid-template-rows:60%` + ребёнок `height:15px` | высота контейнера 120 (= 60 % · **200**, ширина) | 15 (процент при неопределённой высоте — `auto`, CSS Grid L1 §7.2.1) |
| то же в окне 1008 × 604 | 604,8 (= 60 % · 1008) | 15 |
| `gap:10%`, `grid-template-rows:90px 90px`, `width:200px`, высота `auto` | промежуток по y = 20 (от ширины) | 0 (процент `row-gap` от неопределённой высоты — 0, §8.3) |
| то же при `height:400px` | промежуток по y = 40 | 40 (верно) |

## Как найдено

WPT-RUN-14 срез 7: `grid-definition/grid-percentage-rows-indefinite-height-001/002.html` (120 + 4 сабтеста), `alignment/grid-gutters-009…016.html` (8 reftest, 7 thick), `grid-definition/flex-item-grid-container-percentage-rows-001.html`. 10 id / 124 сабтеста.

## Что делать

Проценты по оси строк резолвить от высоты контейнера, когда она определена; иначе — как `auto` для трека и 0 для `gap`; ось столбцов — от ширины (как сейчас).

## Как проверить

`css/css-grid/grid-definition/grid-percentage-rows-indefinite-height-001.html`, `alignment/grid-gutters-009.html`.

## Исправление (2026-10-20, P3)

- `row-gap` в `build_grid_init` (и собственный gap subgrid в `layout_dispatch.rs`) резолвится от `definite_content_height` — при неопределённой высоте `%` даёт 0 (Grid L1 §8.3); `column-gap` по-прежнему от ширины.
- Строки: `%` (и `calc()` с `%`) в `grid-template-rows`, `minmax()` и `grid-auto-rows` при неопределённой высоте читаются как `auto` (`GridTrackSize::percent_as_auto`, §7.2.1); при определённой — резолвятся от высоты контейнера, не от ширины. `GridInit::auto_row` несёт нормализованный `grid-auto-rows`.
- Тесты: `crates/engine/layout/src/box_tree/tests/grid_percent_rows.rs`. WPT-прогон `css/css-grid` не делался.
