In progress: Quirks mode application — body percentage height  branch: p1-quirks-body-height
Next step: add doc: &Document to lay_out, thread through call sites  box_tree.rs:1299

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Next:
(все основные layout-задачи выполнены)

Queue (Wave 3+):

Recent: text-wrap-mode: nowrap + overflow-wrap: break-word + word-break: break-all/break-word wired to wrap_inline_run, char_break_offset() helper, 6 тестов 2026-05-24, margin collapsing CSS 2.1 §8.3.1 FIXED (prev_block_mb в child-loop box_tree.rs + 3 тест) 2026-05-24, BUG-020 per-axis overflow clip FIXED (box_layer_ops BIG-сентинели) + 3 regression tests 2026-05-24, BUG-023 P1-часть FIXED strut в InlineBlockRow без текста (Edge не добавляет font-strut в строках только из inline-block) + 3 regression tests 2026-05-24, BUG-004 FIXED height on inline elements (display:inline-block applies, display:inline ignores per CSS 2.1 §10.6.1) + 3 regression tests 2026-05-24, ::before/::after inline (collect_inline_segments) 2026-05-22, BUG-034 FIXED transform-origin 50% 50% не резолвился → rotate/scale вращались вокруг (0,0) 2026-05-22, Shadow DOM composed tree (FlatTree) + layout wiring 2026-05-22, display:flow-root BFC + display:contents elimination 2026-05-22, BUG-011 FIXED (::marker box + list-item layout) 2026-05-22, BUG-026 FIXED (img renders at correct CSS size) 2026-05-22, BUG-025 FIXED (InlineSpace shrink-to-fit) 2026-05-22, Forms ValidityState :valid/:invalid 2026-05-22, BUG-013 display:none breaks inline context 2026-05-22, bug-024-box-sizing 2026-05-21
