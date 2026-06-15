# BUG-116

**Статус:** FIXED 2026-06-09
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs:4670`

## Описание

auto table column widths: CSS 2.1 §17.5.2 content-based auto sizing. Added box_min_max_content_w (recursive InlineRun/Block traversal), cell_min_max_border_box_w, scan_row_content_widths (rowspan-aware per-column pass). compute_table_col_widths now takes measurer: each auto column gets ≥min-content; extra distributed proportional to max-content weight. Without measurer: equal distribution fallback preserved.
