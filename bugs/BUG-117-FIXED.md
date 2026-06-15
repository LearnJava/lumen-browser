# BUG-117

**Статус:** FIXED 2026-06-09
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs:5021`

## Описание

multi-column greedy assignment two bugs (TEST-33 16.14%): (1) in balance mode an item taller than the balanced target (total/n_cols) triggered height_overflow on the EMPTY column 0 and was pushed to column 1, leaving column 0 blank — column-span:all segment items (group 5) landed in columns 1&2 instead of 0&1. (2) column-fill:auto wrongly applied the per-column count cap (a balance-only anti-starvation guard), forcing one item per column instead of height-based sequential fill (group 6). Fix in lay_out_multicol_children: never advance past an empty column (col_nonempty guard); count cap gated behind `balance`. 2 regression tests + CPU snapshot 33 regenerated.
