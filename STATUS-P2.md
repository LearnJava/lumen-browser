In progress: border-radius rendering (FillRoundedRect SDF)  branch: p2-border-radius
Next step: add CornerRadii + FillRoundedRect to display_list.rs  crates/engine/paint/src/display_list.rs

CSS rule: P2 does NOT implement CSS properties. P4 owns all CSS.
  P2 writes rendering primitives and GPU pipelines only.
  When a new pipeline needs a CSS property → add // CSS: <prop> comment and
  add a line to STATUS-P4.md "Needs wiring".

Next:

Queue (Wave 3+):

Recent: bug033-box-shadow-blur 2026-05-22, animation-transition-engine 2026-05-22, multi-column-rendering 2026-05-22, @font-face-loading 2026-05-22, canvas2d-context 2026-05-22, woff2-decoder 2026-05-22, clip-path-rendering 2026-05-22, css-filter-pipeline 2026-05-22, bug017-018-closed 2026-05-22, bug032-area-avg 2026-05-22
