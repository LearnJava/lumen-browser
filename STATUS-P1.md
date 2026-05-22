In progress: ::before / ::after inline pseudo-elements in collect_inline_segments  branch: p1-pseudo-inline

Next step: modify collect_inline_segments in box_tree.rs:626 to inject pseudo-element segments  crates/engine/layout/src/box_tree.rs:626

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Next:
(все основные layout-задачи выполнены)

Queue (Wave 3+):

Recent: BUG-034 FIXED transform-origin 50% 50% не резолвился → rotate/scale вращались вокруг (0,0) 2026-05-22, Shadow DOM composed tree (FlatTree) + layout wiring 2026-05-22, display:flow-root BFC + display:contents elimination 2026-05-22, BUG-011 FIXED (::marker box + list-item layout) 2026-05-22, BUG-026 FIXED (img renders at correct CSS size) 2026-05-22, BUG-025 FIXED (InlineSpace shrink-to-fit) 2026-05-22, Forms ValidityState :valid/:invalid 2026-05-22, BUG-013 display:none breaks inline context 2026-05-22, bug-024-box-sizing 2026-05-21, table-layout 2026-05-21
