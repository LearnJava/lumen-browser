# BUG-1309 — grid: внутренняя ширина (`width:max-content`, `inline-grid`) не учитывает неявные столбцы (`grid-auto-flow: column`); `inline-grid` с `aspect-ratio`-ребёнком растягивается на всю строку

**Статус:** FIXED 2026-10-20 (P3)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 7, `css/css-grid`)
**Область:** layout (`crates/engine/layout/src/box_tree/grid.rs` — вклад неявных треков в `min-content`/`max-content`/shrink-to-fit)

## Симптом

`--dump-layout`, два ребёнка `width:90px;height:10px`:

| разметка | получено | ожидается |
|---|---|---|
| `display:grid; grid-template-columns:1fr 1fr; width:max-content` | 180 | 180 (верно) |
| `display:grid; grid-auto-flow:column; width:max-content` | **90**, второй ребёнок на x=53 (поверх первого) | 180 |
| `display:inline-grid; grid-auto-flow:column` | 90 | 180 |
| `…; grid-auto-columns:1fr; grid-auto-flow:column; width:max-content` | 90 | 180 |
| то же, но `width:300px` | столбцы по 150, дети на x=8 и 158 | верно |
| `display:inline-grid; grid-template-rows:100px` + ребёнок `height:100%; aspect-ratio:1/1` | 784 × 100 | 100 × 100 (ширина items передаётся через пропорцию) |
| `display:inline-grid; aspect-ratio:1/1; min-height:60px; grid-template-columns:repeat(auto-fill,50px)` | 784 × 784 | 60 × 60 |

Раскладка внутри явно заданной ширины верна — врёт только внутренний размер контейнера.

## Как найдено

WPT-RUN-14 срез 7: `child-border-box-and-max-content-001/002.html`, `grid-items/aspect-ratio-001…005.html`, `grid-definition/grid-auto-repeat-aspect-ratio-001/002.html`, `grid-with-aspect-ratio-uses-content-box-height-for-track-sizing.html` (все reftest, thick; 8 id) — вклад неявных треков в intrinsic-размер устанавливали пробами; остальные — по тексту теста.

## Что делать

В `min/max-content` контейнера учитывать столбцы, созданные авто-размещением (`grid-auto-columns`, `grid-auto-flow: column`); ширину shrink-to-fit `inline-grid` брать из этого вклада и из transferred-размера item'ов с `aspect-ratio`.

## Как проверить

`css/css-grid/child-border-box-and-max-content-001.html`, `grid-items/aspect-ratio-001.html`, `grid-definition/grid-auto-repeat-aspect-ratio-001.html`.

## Дополнение: WPT-RUN-14 срез 8 (2026-10-06, `css/css-grid`, часть 2)

Тот же механизм (intrinsic-размер ребёнка с `aspect-ratio` и `%`-высотой) у пяти reftest: `layout-algorithm/grid-fit-content-width-percent-height-aspect-ratio-001.html`, `grid-float-intrinsic-width-percent-height-aspect-ratio-001.html`, `grid-inline-grid-intrinsic-width-percent-height-aspect-ratio-001.html`, `grid-max-content-width-percent-height-aspect-ratio-001.html`, `grid-min-content-width-percent-height-aspect-ratio-001.html` — все thick. Причина не проверена пробой: отнесены по имени файла и по тому, что в них `aspect-ratio` в grid-контейнере с intrinsic-шириной.

## Исправление (2026-10-20, P3)

Часть симптомов из таблицы уже не воспроизводилась (`grid-auto-flow: column` без `fr` складывал столбцы — GRID-BASELINE-2); остальное закрыто в `box_tree/intrinsic.rs`, `stretch_basis.rs`, `grid_auto_cols.rs`:

- **`fr` в неявных/явных столбцах** (`grid-auto-columns: 1fr` + `grid-auto-flow: column`, `width: max-content`): `grid_col_sum_by_tracks` считал такие треки как `None` и падал в «самый широкий ребёнок» (90 вместо 180). Теперь flex-трек вкладывается как `auto`, а `expand_flexible_columns` применяет §11.7.1.
- **`fit-content(L)` не был реализован в раскладке** (столбцы 0 ширины): новый `TrackKind::Fit(L)` — `minmax(auto, max-content)` с пределом `L`, не растягивается.
- **`%`-высота ребёнка + `aspect-ratio` в `inline-grid`/`max-content`/`min-content`/`fit-content`/float**: `grid_item_area_heights` знает высоту области item'а при фиксированных `grid-template-rows` и передаёт её базой процентов (`with_container_height`); блок с `aspect-ratio`, `width: auto` и определённой `height` отдаёт ширину по пропорции (`stretch_basis::canvas_transferred_width`).
- **`repeat(auto-fill, <длина>)` при неопределённой ширине** считался 0 столбцов: теперь повторяется минимальное число раз, достигающее определённой `min-width` (в т.ч. перенесённой из `min-height` через `aspect-ratio`, §7.2.3.2), иначе один раз. Ожидаемый размер `grid-auto-repeat-aspect-ratio-001/002` — 100×100 (а не 60×60, как значилось в таблице).
- `min-height` через `aspect-ratio` переносится в минимальную ширину (`min_height_transferred_width`); `scoped_for_children` создаёт базу процентов и от px-высоты без внешней базы.

Тесты: `box_tree/tests/grid_intrinsic_ratio.rs`.

