In progress: CSS filter pipeline (blur, grayscale, etc.)  branch: css-filter-pipeline
Next step: filter: blur() → Gaussian на CPU в image/  renderer.rs

Next (Wave 2):
@font-face loading: fetch URL + font register font/ + shell                ~3h  depends on P4 @font-face
mix-blend-mode wgpu blend states              renderer.rs                  ~2h

Queue (Wave 3+):
Animation scheduler (@keyframes frame loop)                  ~2h  depends on P4 animation wire-up
Transition engine (smooth interpolation)                     ~2h  depends on P4 transition wire-up
clip-path rendering (inset, circle, polygon)                 ~2h
WOFF2 decoder                                                ~2h
Multi-column layout rendering                                ~2h  depends on P4 multi-column
Canvas 2D basic context                                      ~3h

Recent: bug017-018-closed 2026-05-22, bug032-area-avg 2026-05-22, image-third-party 2026-05-21, overflow-clip 2026-05-21, img-in-span 2026-05-21
