# BUG-081

**Статус:** FIXED 2026-06-11
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

vertical-align: sub-pixel 0.99% deviation — snap dy.round() перед shift_y_box (P1 PS-1)
