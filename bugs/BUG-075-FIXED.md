# BUG-075

**Статус:** FIXED 2026-06-08
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs:4103`

## Описание

display:table без явной ширины растягивается до ширины контейнера вместо shrink-to-fit. TEST-69 (border-spacing) failing 42.62% — таблица должна быть ~228px, рендерится 982px
