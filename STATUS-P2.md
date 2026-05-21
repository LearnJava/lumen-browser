In progress: —

Next (Wave 1):
BUG-017+018  text-decoration-style+color      display_list.rs/renderer.rs  ~1h
BUG-032      mipmap for large downscale       renderer.rs                  ~2h

Next (Wave 2, after P4 finishes @font-face parse):
@font-face loading: fetch URL + font register font/ + shell                ~3h  depends on P4 @font-face
CSS filter pipeline (blur, grayscale, etc.)   renderer.rs                  ~2h
mix-blend-mode wgpu blend states              renderer.rs                  ~2h

Queue (Wave 3+):
Animation scheduler (@keyframes frame loop)                  ~2h  depends on P4 animation wire-up
Transition engine (smooth interpolation)                     ~2h  depends on P4 transition wire-up
clip-path rendering (inset, circle, polygon)                 ~2h
WOFF2 decoder                                                ~2h
Multi-column layout rendering                                ~2h  depends on P4 multi-column
Canvas 2D basic context                                      ~3h

Recent: image-third-party 2026-05-21, overflow-clip 2026-05-21, img-in-span 2026-05-21, dotted-circles 2026-05-21, image-cpu-resize 2026-05-21, bug023-analysis 2026-05-21
