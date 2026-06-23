# BUG-115

**Статус:** FIXED 2026-06-23
**Компонент:** css-parser / paint
**Файл:** `crates/engine/layout/src/style.rs` (`BackgroundSize`), `crates/engine/paint/src/display_list.rs` (`bg_tile_geometry`, `gradient_paint_rects`)

## Описание

percent `background-size` (e.g. `40% 60%`, `20px 100%`) not supported — `resolve_box_length` returns `None` for `%`, so `BackgroundSize` fell back to `Auto` and the layer filled the whole positioning area instead of a percent-sized tile. TEST-45 `.no-repeat-demo`/`.repeated` residual.

## Исправление

`BackgroundSize::Length` теперь хранит две `BgSizeAxis` (`Auto` | `Px(f32)` | `Percent(fraction)`) вместо `(f32, Option<f32>)`. Процент резолвится отложенно против positioning area в paint-time (`BgSizeAxis::resolve(area)`), как у border-radius %:

- Парсинг: новый `parse_bg_size_axis` (распознаёт `%`), `parse_background_size_value` для лонгхендов `background-size`/`mask-size`, шортхенд `background … / <size>` и `parse_background_size_single` переведены на оси.
- Paint: `bg_tile_geometry` (изображения, общий путь femtovg + cpu_raster), `gradient_paint_rects` (градиенты), wgpu `renderer.rs` (image + mask) резолвят процент против oarea/painting-area; `auto`-ось выводится из второй через intrinsic ratio.

Верификация: `--screenshot` TEST-45 diff vs Edge 5.43% → 1.94% (остаток — CPU-бэкенд градиенты/текст, не дефект). Regression-тесты: `background_size_percent_pair`, `background_size_mixed_px_percent`, `background_shorthand_percent_size` (layout), `bg_tile_geometry_percent_resolves_against_oarea`, `bg_tile_geometry_mixed_px_percent` (paint).
