# BUG-074

**Статус:** FIXED 2026-06-08
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs:4953`

## Описание

height:100% на flex-item не резолвится — available_height=None передаётся в lay_out() при шаге 1 flex-алгоритма, percentage height от definite flex-container height игнорируется. TEST-67 (attr-typed) failing 20.19% — bar/::before с height:100% рендерятся h=0
