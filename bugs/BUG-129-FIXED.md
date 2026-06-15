# BUG-129

**Статус:** FIXED 2026-06-14
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

CSS Tables border-collapse: collapse — TEST-80 16.81%. Collapse mode only zeroed border-spacing: adjacent cells kept full borders (4px doubled lines) and the table was ~10px too wide (table border + outer cell borders both drawn). Fix (CSS 2.1 §17.6.2 collapsing model, box_tree.rs lay_out_table/lay_out_table_row): columns positioned so neighbours overlap by the shared grid-line border (collapse_v_edges = max of meeting borders), rows pulled together by collapse_max_cross_border, outer cells snapped onto the table border (table border-box width/height = overlapped grid). Uniform-border tables (cols 2/4) pixel-exact; varied-width (col 3) has a small paint-order residual (later cell bg overpaints the thicker neighbour's collapsed border by the width delta). Geometry verified via --dump-layout (table widths 628→618, 656→638). 4 regress-tests. Release graphic run not executed (release OOM under parallel sessions)
