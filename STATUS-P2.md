In progress: —
Next step: —

CSS rule: P2 does NOT implement CSS properties. P4 owns all CSS.
  P2 writes rendering primitives and GPU pipelines only.
  When a new pipeline needs a CSS property → add // CSS: <prop> comment and
  add a line to STATUS-P4.md "Needs wiring".

Next:
Animation scheduler: @keyframes frame loop engine                          ~2h
  (P2: AnimationScheduler::tick; P4: wires animation-* properties)
Transition engine: smooth interpolation infrastructure                     ~2h
  (P2: interpolation engine; P4: wires transition-* properties)
Multi-column layout rendering: column primitives                           ~2h
  (P2: draw column separators; P4: wires column-rule-*)

Queue (Wave 3+):

Recent: @font-face-loading 2026-05-22, canvas2d-context 2026-05-22, woff2-decoder 2026-05-22, clip-path-rendering 2026-05-22, css-filter-pipeline 2026-05-22, bug017-018-closed 2026-05-22, bug032-area-avg 2026-05-22
