# BUG-066

**Статус:** FIXED 2026-06-07
**Компонент:** paint
**Файл:** `crates/engine/paint/src/renderer.rs:6409`

## Описание

render_tile() в Renderer не имеет #[cfg(feature = "cpu-render")] но вызывает crate::cpu_raster::rasterize_cpu — clippy lumen-shell --all-targets падает без фичи cpu-render
