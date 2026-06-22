# BUG-143

**Статус:** FIXED 2026-06-22
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

TEST-75: 16.97% (thr 0.5%). Страница использует `display: grid; grid-template-rows: masonry`
с тремя секциями (`masonry-auto-flow: next/ordered/definite-first`).

## Реальная причина

`grid-template-rows: masonry` не поддерживается стабильными браузерами — Edge игнорирует
значение, и контейнер раскладывается как обычный grid. Эталон TEST-75 — это обычный
3-колоночный grid, а не waterfall. Lumen же запускал свой masonry-алгоритм и расходился.

## Фикс

1. `lay_out_grid`: `masonry` на любой оси → пустой track-list (`none`) → обычный grid
   (совпадает с поведением Edge). Waterfall-диспетч удалён.
2. Grid placement сортирует items по `order` (CSS Grid §6) — раньше это делал только
   masonry-путь; нужно для секции `ordered`.
3. Grid `align: stretch` больше не растягивает item с явной `height` (CSS Grid §11.2) —
   боксы сохраняют заданную высоту и прижимаются к верху ячейки, как в Edge.
4. Сопутствующий [BUG-232]: column flex двойного-считал border у `.label`-полос, сдвигая
   секции вниз. Исправлено форсированием `box-sizing: border-box` при re-layout.

Результат: TEST-75 0.25% (CPU-diff) — геометрия пиксель-в-пиксель с Edge во всех трёх
секциях (source order / order-property / definite-first).
