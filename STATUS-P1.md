In progress: BUG-034 fix transform-origin % resolution  branch: fix-transform-origin
Next step: verify --dump-display-list 22-transform.html shows correct rotate pivot  crates/engine/layout/src/style.rs:9646

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Next:
(все основные layout-задачи выполнены)

Queue (Wave 3+):

Recent: Shadow DOM composed tree (FlatTree) + layout wiring 2026-05-22, display:flow-root BFC + display:contents elimination 2026-05-22, BUG-011 FIXED (::marker box + list-item layout) 2026-05-22, BUG-026 FIXED (img renders at correct CSS size) 2026-05-22, BUG-025 FIXED (InlineSpace shrink-to-fit) 2026-05-22, Forms ValidityState :valid/:invalid 2026-05-22, BUG-013 display:none breaks inline context 2026-05-22, bug-024-box-sizing 2026-05-21, table-layout 2026-05-21, forms-layout 2026-05-21, css-nesting 2026-05-21
