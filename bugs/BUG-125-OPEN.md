# BUG-125

**Статус:** OPEN
**Компонент:** layout/paint
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

CSS Motion Path L1 (offset-path/offset-distance/offset-rotate) rendering diverges — TEST-76: 3.18% (thr 0.5%); boxes along horizontal/diagonal/cubic-bezier paths misplaced
