# BUG-1313 — grid: `space-evenly`/`space-around`/`center` с переполнением центрирует небезопасно; `minmax(auto, <max < min-content>)` не поднимает трек до min-content

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 7, `css/css-grid`)
**Область:** layout (`crates/engine/layout/src/box_tree/grid.rs` — выравнивание контента контейнера и размер трека)

## Симптом

`--dump-layout`:

| разметка | получено | ожидается |
|---|---|---|
| grid 50 × 50, `align-content/justify-content: space-evenly`, ребёнок 100 × 100 | ребёнок на y = −17 (центр, вылезает вверх и влево) | на 0: fallback для `space-evenly` — `safe center`, при переполнении — начало (CSS Align L3 §5.1) |
| grid `width:100px; grid:10px 10px / minmax(auto,0px)`, ребёнок 1 `width:60px`, ребёнок 2 без размера | второй ребёнок 0 × 10 | 60 × 10: у `minmax(auto,0px)` минимум `auto` = min-content ребёнка 1 поднимает базу трека до 60 |

## Как найдено

WPT-RUN-14 срез 7: `alignment/grid-content-distribution-026/027.html`, `grid-content-alignment-overflow-001/002.html` (4 id); `grid-items/grid-items-minimum-width-001…004.html`, `grid-minimum-size-grid-items-021.html` (4 id / 173 сабтеста; вертикальные варианты — GRID-VWM-2). Всего 8 id.

## Что делать

(1) Для `space-*` и `center` без `safe`/`unsafe` при переполнении свободное место ≤ 0 → fallback `start`. (2) В track sizing максимум трека поднимать до базы, если `max < base` (Grid L1 §11.5, шаг «if growth limit < base size»).

## Как проверить

`css/css-grid/alignment/grid-content-distribution-026.html`, `grid-items/grid-items-minimum-width-001.html`.
