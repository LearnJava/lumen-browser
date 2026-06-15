# BUG-098

**Статус:** FIXED 2026-06-11
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

mix-blend-mode: PushBlendMode/PopBlendMode layers ~14% deviation — PA-3: offscreen CPU mix_blend_rgba для всех 15 CSS blend modes
