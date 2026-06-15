# BUG-094

**Статус:** FIXED 2026-06-11
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

text-shadow with blur PushFilter wrapper ~7% deviation — TEST-52: 6.82%. Fix: PA-2 offscreen filter layer, GPU Gaussian blur via femtovg filter_image
