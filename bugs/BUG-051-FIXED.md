# BUG-051

**Статус:** FIXED 2026-05-31
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs:3698`

## Описание

abs-pos с top+bottom+height:auto (inset:0) схлопывался в height 0 — lay_out_abs_children резолвил ширину из left+right, но симметричной высоты из top+bottom не было (CSS Position L3 §6); страница 30 backdrop-filter рендерилась без фона
