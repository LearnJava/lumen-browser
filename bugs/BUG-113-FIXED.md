# BUG-113

**Статус:** FIXED 2026-06-09
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs:5229`

## Описание

TEST-53 row-2 vertical drift ~24px: single-line row flex container leaked the trailing `row-gap` (from `gap:24px`) into its own cross size (height). `lay_out_flex` adds `line_cross + cross_gap` per line but only removed the surplus trailing gap when `n_lines > 1`; single-line containers kept it. Fix: always drop one trailing `cross_gap`. Row-2 moved up 24px; 15 single-line-flex+gap CPU snapshots regenerated. Residual TEST-53 ~4px = BUG-114 (`font` shorthand size).
