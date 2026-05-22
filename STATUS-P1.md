In progress: display:flow-root BFC + display:contents elimination  branch: flow-root-bfc

Next step: add BoxKind::FlowRoot + BoxKind::Contents, flatten_contents()  box_tree.rs:258

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Next:
display: flow-root BFC + contents elimination box_tree.rs    ~2h

Queue (Wave 3+):
display: list-item marker box                 box_tree.rs    ~1h
Shadow DOM cascade + composed tree                           ~3h

Recent: BUG-011 FIXED (::marker box + list-item layout) 2026-05-22, BUG-026 FIXED (img renders at correct CSS size) 2026-05-22, BUG-025 FIXED (InlineSpace shrink-to-fit) 2026-05-22, Forms ValidityState :valid/:invalid 2026-05-22, BUG-013 display:none breaks inline context 2026-05-22, bug-024-box-sizing 2026-05-21, table-layout 2026-05-21, forms-layout 2026-05-21, css-nesting 2026-05-21
