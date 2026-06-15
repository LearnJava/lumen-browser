# BUG-084

**Статус:** FIXED 2026-06-12
**Компонент:** paint
**Файл:** `crates/engine/paint/src/cpu_raster.rs, display_list.rs:185`

## Описание

border-radius residual 1.5% deviation after BUG-036 fix — TEST-36: 1.50%; classified as rasterizer-quality (pure AA difference on fractional-pixel curves, not implementation-gap). % radii now resolved correctly by CornerRadii::from_style_and_box; remains tiny_skia AA vs Edge AA on sub-pixel boundaries (Phase 4+ task).
