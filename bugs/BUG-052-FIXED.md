# BUG-052

**Статус:** FIXED 2026-05-31
**Компонент:** paint/cpu_raster
**Файл:** `crates/engine/paint/src/cpu_raster.rs:1087`

## Описание

DrawBorder использовал anti_alias:true → tiny-skia hairline_aa::fill_dot8 бьёт debug_assert!(false) для тонких sub-pixel-positioned рамок (inner span округляется в 0) → паника в debug-профиле; fix: anti_alias:false для axis-aligned border quads
