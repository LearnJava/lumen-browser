# BUG-144

**Статус:** OPEN
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

CSS filter visual rendering (TEST-30): rows 1-3 deviate 18.81% from Edge (down from 23.61% after PA-4); PA-2 grayscale/sepia/brightness/invert/contrast/saturate/hue-rotate/blur do not match Edge pixel-for-pixel; backdrop-filter (row 4) now implemented via PA-4 but rows 1-3 remain wrong
