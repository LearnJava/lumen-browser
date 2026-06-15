# BUG-091

**Статус:** FIXED 2026-06-08
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

background-blend-mode: bottom layer wrapped in PushBlendMode (should be suppressed per CSS Compositing L1 §8.3) — TEST-49: 30.62%
