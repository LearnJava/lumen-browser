# BUG-1314 — высота flex-/grid-контейнера игнорирует `min-height`/`max-height` (и логические `*-block-size`)

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 8, `css/css-grid`, вторая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/flex.rs`, `grid.rs` — `--dump-layout` печатает `min-h=`/`max-h=`, то есть значение разобрано и дошло до `ComputedStyle`, но размер контейнера по нему не зажимается)

## Симптом

`<div class=c><div style="height:10px"></div></div><i style="display:block;height:1px;background:red"></i>`, `.c{…}`, `--dump-layout` (высота `.c` / `y` следующего блока):

| `.c` | `display:block` | `display:grid` | `display:flex` |
|---|---|---|---|
| `min-height:50px` | 50 / 50 | **10 / 10** | **10 / 10** |
| `height:200px; max-height:50px` | 50 / 50 | **200 / 200** | **200 / 200** |
| `height:20px; min-height:50px` | 50 | **20** | **20** |
| `min-block-size:100px` | — | **10** | — |
| `width:200px; max-width:100px` | 100 | 100 | 100 |
| `width:50px; min-width:100px` | 100 | 100 | 100 |

Ширинная пара работает во всех трёх, высотная — только у блока. `inline-grid` ведёт себя как `grid`. `box-sizing:border-box` с `min-height` — тоже без эффекта.

## Как найдено

WPT-RUN-14 срез 8: `css/css-grid/grid-model/grid-min-max-height-001.html` (8 сабтестов: `expected height 100 but got 17`), `grid-box-sizing-001.html` (7), `layout-algorithm/grid-stretch-respects-min-size-001.html` (reftest: у вложенного grid с `min-height:100px` в обёртке 50×50 страница короче референса на 50 px — 91,72 против 141,72). `layout-algorithm/grid-track-ignores-max-size-002.html` и `grid-intrinsic-track-sizes-min-size-001.html` попали в кластер по тексту теста (в нём `min-*`/`max-*`), пробой не проверены. Кластер «`min/max-height` контейнера flex/grid игнорируется» в `docs/wpt-vendor-notes/css.md` §css-grid, часть 2. Для flex причина та же и описана в [BUG-1253](BUG-1253-OPEN.md) только частично (там — автоматический минимум *элемента*, а не ограничение контейнера).

## Что делать

В ветках раскладки flex- и grid-контейнера после определения высоты (`height` или по содержимому) зажимать её `min-height`/`max-height` (CSS Sizing L3 §5; для grid `max-block-size` не участвует в разрешении размеров строк — так сказано в `grid-track-ignores-max-size-002`, — поэтому итоговую высоту зажимать после track sizing). Тот же код — для `inline-grid`/`inline-flex` и логических свойств.

## Как проверить

`css/css-grid/grid-model/grid-min-max-height-001.html`, `grid-box-sizing-001.html`, `layout-algorithm/grid-stretch-respects-min-size-001.html`.
