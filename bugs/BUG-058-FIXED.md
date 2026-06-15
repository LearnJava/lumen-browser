# BUG-058

**Статус:** FIXED 2026-06-04
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs:3805`

## Описание

display:contents не сглажен перед lay_out: паника «entered unreachable code: display:contents boxes must be flattened before lay_out» при открытии cnn.com
