# BUG-024

**Статус:** FIXED 2026-05-21
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

box-sizing: content-box — border not added to outer size; height% resolved against width

## Детали

TEST-07: content-box боксы в Lumen уже чем в Edge на `2 × border_width`.

**Где смотреть:** `crates/engine/layout/src/box_tree.rs` — вычисление `rect.width` / `rect.height` для `content-box`.
