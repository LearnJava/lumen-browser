# BUG-1310 — процентные `margin`/`padding` grid-item считаются от ширины контейнера, а не grid-области

**Статус:** FIXED 2026-10-20 (P3)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 7, `css/css-grid`)
**Область:** layout (`crates/engine/layout/src/box_tree/grid.rs` — база для `%` у item'а)

## Симптом

`--dump-layout`: grid `width:300px; grid-template-columns:200px`, item `width:50px; margin-left:30%`:

| разметка | получено | ожидается |
|---|---|---|
| `margin-left:30%` | левый край x = 98 (= 8 + 30 % · **300**) | 68 (= 8 + 30 % · 200, ширина области) |
| `margin-left:calc(10px + 30%)` | 108 | 78 |
| при `width:200px` (область = контейнер) | 68 | 68 (верно — поэтому прячется) |

CSS Grid L1 §6.2: проценты margin и padding item'а резолвятся от inline-размера его grid-области.

## Как найдено

WPT-RUN-14 срез 7: `alignment/grid-calc-margins-explicit-row.html`, `grid-items/percentage-margin-dynamic.html` (reftest, thick), `grid-items/grid-items-percentage-margins-*`/`-paddings-*` (5 файлов, 33 сабтеста; часть — вертикальные режимы, GRID-VWM-2, не разделены). До 7 id; подтверждена пробой только `calc-margins`.

## Что делать

Базу процентов margin/padding item'а брать как inline-размер области (для `auto`-трека — после его вычисления; до этого — 0 при intrinsic-вкладе).

## Как проверить

`css/css-grid/alignment/grid-calc-margins-explicit-row.html`.

## Исправление (2026-10-20, P3)

- Поля item'а в `post_final_item` (позиция по осям выравнивания), вклад в высоту строки (`item_block_size`), baseline-группы строк и столбцов и `inline_offer` (fit-content) резолвят `%` от inline-размера области (`GridInit::item_pct_base` — ширина охваченных столбцов) вместо ширины контейнера. Собственный padding item'а и так считался от предложенной ширины области.
- Вертикальные режимы (`grid_vertical::place_item`) не менялись — GRID-VWM. WPT-прогон `css/css-grid` не делался.
- Тесты: `crates/engine/layout/src/box_tree/tests/grid_item_percent_inset.rs`.
