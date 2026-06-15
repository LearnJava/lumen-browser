# BUG-087

**Статус:** FIXED 2026-06-09
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

sized/positioned/repeated gradient layers ignored background-size/position/repeat (filled whole box) — TEST-45: 17.29%; CSS Backgrounds L3 §3.3-3.5. Multiple layers WERE rendered; the gap was gradient tiling. Fix: gradient_tile_rects + gradient_paint_rects emit per-tile gradient commands clipped to painting area. Percent background-size still falls back to Auto (separate gap, BUG-115).
