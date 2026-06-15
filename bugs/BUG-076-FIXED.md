# BUG-076

**Статус:** FIXED 2026-06-11
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

box-shadow blur spread ~1% deviation — TEST-15: 1.06% (thr 0.5%). Fix: PA-2 offscreen filter layer, GPU Gaussian blur via femtovg filter_image
