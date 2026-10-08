# BUG-1313 — grid: `space-evenly`/`space-around`/`center` с переполнением центрирует небезопасно; `minmax(auto, <max < min-content>)` не поднимает трек до min-content

**Статус:** FIXED 2026-10-08 (P6)
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

## Исправление

1. `grid_content_distribution` (`grid.rs`) принимает флаг `safe`: при переполнении `space-around`/`space-evenly`
   откатываются к `safe center` (старт-выравнивание, смещение 0), `center`/`end` без ключевого слова остаются
   небезопасными (как у Edge), `safe center`/`safe end` прижимаются к началу. Флаг берётся из
   `content_align_extra.justify_safe`/`align_safe` — разбор `safe`/`unsafe` уже был.
2. `minmax(auto, <length>)` — новый `TrackKind::Bounded` в `grid_auto_cols.rs`: база трека — минимальный вклад
   элементов, предел роста — длина, поднятая до базы (Grid L1 §11.5). Одноколоночная сетка тоже идёт этим путём.
   Вклад элемента не ниже его определённого `min-width`. Для `inline-grid`/shrink-to-fit тот же трек считает
   `grid_col_sum_by_tracks` (`intrinsic.rs`).

Тесты: `box_tree/tests/grid_overflow_align.rs`. WPT: `grid-content-distribution-026/027` — PASS; остальные id
доходят до своих проверок, но целиком не проходят из-за других дефектов: `grid-items-minimum-width-001` 30/44
(остаток — [BUG-1414](BUG-1414-OPEN.md)), `-002` (`inline-grid` перебивается `grid.css` — [BUG-1413](BUG-1413-OPEN.md)),
`grid-content-alignment-overflow-001/002` (высоты с `min-/max-height` — [BUG-1314](BUG-1314-FIXED.md), scroll-размеры —
[BUG-1415](BUG-1415-OPEN.md)), `grid-minimum-size-grid-items-021` (used-значения `grid-template-*` у `getComputedStyle`
и размеры изображений — к этой правке отношения не имеет).
