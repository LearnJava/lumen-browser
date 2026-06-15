# BUG-040

**Статус:** FIXED 2026-05-27
**Компонент:** layout
**Файл:** `layout/src/lib.rs:9996`

## Описание

table layout unit tests assume direct `<tr>` children of `<table>`; html-full-tree-builder now injects implicit `<tbody>` breaking them
