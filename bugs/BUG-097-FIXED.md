# BUG-097

**Статус:** FIXED 2026-06-09
**Компонент:** layout/paint
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

`<video>` placeholder: posterless video painted grey placeholder; Edge renders empty media transparent → suppress DrawImage when no poster
