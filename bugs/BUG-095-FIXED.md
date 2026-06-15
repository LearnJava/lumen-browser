# BUG-095

**Статус:** FIXED 2026-06-09
**Компонент:** layout/paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

background-origin/background-clip positioning ~32% deviation — TEST-53: 31.78%→11.55%; femtovg (default) backend stretched bg-image over whole box, ignoring background-size/position/repeat/origin/clip. Ported wgpu tiling math to femtovg `draw_background_image`. Residual 11.55% = BUG-113 row drift + image resample/text AA
