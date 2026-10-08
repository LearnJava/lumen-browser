# BUG-1336 — `display: table-column` / `table-column-group` (и `<col>`, `<colgroup>`): фон и рамка не рисуются — бокс 0×0

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** layout/paint (`crates/engine/layout/src/box_tree/table.rs` — колонкам не назначается прямоугольник по колонкам таблицы)

## Симптом

`--dump-layout`: `Block rect=(0,0,0,0) bg=#000000ff display=table-column-group`. В display list нет `FillRect` для этого бокса (у `table-row-group` с тем же `background: black` есть `FillRect (0,0,100,40)`).

| разметка (таблица 100 px, ячейка 40 px высотой) | пиксель в ячейке |
|---|---|
| `table-row-group` с `background: black` | чёрный |
| `table-row` с `background: black` | чёрный |
| `<col style="background:black">` | **белый** |
| `<colgroup style="background:black"><col></colgroup>` | **белый** |
| `display:table-column-group; background:black` | **белый** |

Рамки колонок (`border-bottom-width: 1in` у `table-column-group` в `border-collapse: collapse`) тоже не рисуются.

## Как найдено

WPT-RUN-14 срез 11: 40 id `css/CSS2/{backgrounds,borders}/*-applies-to-005/006.xht` — для каждого свойства (`background`, `-color`, `-image`, `-position`, `-repeat`, `-attachment`, `border`, `border-*-width`, `border-*-color`) два теста: `table-column-group` и `table-column`. Эталон — сплошной квадрат 96 px, Lumen рисует белое.

## Что делать

CSS 2.1 §17.5.1 (слои таблицы): фон колонки и группы колонок рисуется на прямоугольнике «колонка × высота всех строк», поверх фона таблицы, под фоном групп строк. Нужен прямоугольник колонки в `table.rs` (границы колонок уже вычислены для раскладки) и команда `FillRect`/`DrawBackgroundImage` на нём.

## Как проверить

`css/CSS2/backgrounds/background-applies-to-005.xht`, `css/CSS2/backgrounds/background-applies-to-006.xht`.
