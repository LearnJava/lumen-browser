In progress: —

Next step: —

Next (Wave 1):
BUG-025  max-height clamp                     box_tree.rs    ~1h
BUG-026  <img> CSS width/height ignored       box_tree.rs    ~1h
BUG-013  adjacent spans stack vertically      box_tree.rs    ~1h

Next (Wave 2, after D finishes content+pseudo):
::before/::after layout integration           box_tree.rs    ~2h  depends on P4 content
BUG-011  list markers (disc/decimal/::marker) box_tree.rs    ~2h  depends on pseudo
display: flow-root BFC + contents elimination box_tree.rs    ~2h

Queue (Wave 3+):
Table layout algorithm                        box_tree.rs    DONE 2026-05-21 (BUG-006)
display: list-item marker box                 box_tree.rs    ~1h
Shadow DOM cascade + composed tree                           ~3h
Forms: ValidityState + validation pseudo                     ~2h

Recent: bug-024-box-sizing 2026-05-21, table-layout 2026-05-21, forms-layout 2026-05-21, css-nesting 2026-05-21, aspect-ratio 2026-05-21, flex-order 2026-05-21, css-units 2026-05-21, logical-properties 2026-05-21, quirks-table-cell-width 2026-05-21
