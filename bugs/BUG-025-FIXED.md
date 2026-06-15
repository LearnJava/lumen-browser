# BUG-025

**Статус:** FIXED 2026-05-22
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

max-height does not clamp block height; InlineSpace not included in shrink-to-fit width

## Детали

TEST-11: При `height: 160px; max-height: 80px` блок рендерится 160px (max-height игнорируется).

**Где смотреть:** `crates/engine/layout/src/box_tree.rs` — после вычисления `height`, найти применение `min_height`/`max_height`.
