# BUG-1726 — `filter: drop-shadow()`: тень не видна, если источник вне холста или обрезан клипом предка

**Статус:** OPEN
**Заведён:** 2026-10-27 (P1, остаток [BUG-1540](BUG-1540-FIXED.md))
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs::svg_filter_chain_window`, `cpu_raster/drop_shadow.rs`, wgpu `renderer/svg_filter_layer.rs`)

## Симптом

После BUG-1540 `drop-shadow()` рисуется окном слоя: ink источника ∩ холст + смещение + 3σ. Не работает то, что требует входа за пределами этого окна:

1. **Источник вне холста.** `filters-drop-shadow-003` (`top:-1000px; left:-1000px; filter: drop-shadow(1000px 1000px 0 green)`): сам блок целиком за левым верхним краем, а его тень должна лечь в видимую область. Слой — это холст, ink за ним не растеризуется, тень пуста (90000 px diff). Тот же класс: `drop-shadow-with-3d-transform` (`overflow:hidden`-контейнер, блок наполовину снаружи).
2. **Клип предка режет вход фильтра.** `drop-shadow-clipped-001`: красный потомок с `left:105px` внутри `overflow:hidden; width:100px` — клип родителя действует и на пиксели внутри группы фильтра, ink пуст, тени нет. По спецификации клип предка режет *результат* фильтра, а вход видит целиком.
3. **Цвет через `color-mix()`/`var()`** (`invalidation-css-dropshadow-002`: `drop-shadow(… color-mix(in srgb, currentcolor 100%, black))`) — `parse_drop_shadow` зовёт `parse_color_legacy`; цвет с `currentcolor` внутри `color-mix()` не раскрывается от `color` бокса (не проверено, отбрасывается функция или цвет).
4. Не измерены пиксельным диффом (`reftest-wait` + `/common/`): `drop-shadow-currentcolor-dynamic-001…003`, `svg-mutation-drop-shadow-{color,offset}`, `tainting-css-dropshadow-*` (заражение цепочки `currentcolor`, см. `SVG-FILTER-7`). Нужен `tests/wpt/run_corpus.py --prefixes css/filter-effects`.
5. `drop-shadow()` внутри `backdrop-filter` и `filter()` как `<image>` — не делаются (CPU-путь backdrop игнорирует функцию).

## Что делать

(1) растеризовать слой фильтра шире холста на смещение (или строить тень из bbox источника) — решение вместе с ограничением памяти; (2) выносить клип предка из группы фильтра на композит (как `push_shape_filter` для SVG); (3) разбор `<color>` через общий CSS Color L5 парсер вместо `parse_color_legacy`.
