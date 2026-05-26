In progress: —
Next step: —

Next:
- fix-opacity-edge-aa: BUG-023 P2-часть — edge antialiasing deviation ~1.6% в TEST-13; исследовать compositor.rs opacity pass, возможно premultiplied alpha mismatch
- fix-scrollbar-rendering: BUG-020 — overflow:scroll/auto scrollbar UI не рендерится; реализовать scrollbar track + thumb как DisplayCommand-ы в display_list.rs
- fix-border-dashed-paint: TEST-21 border-style dashed/dotted — реализовать dash pattern в paint pipeline (draw_dashed_border); скоординировать с P1 (layout stroke helper)
- picture-srcset-gpu: <picture>/srcset P2-часть — GPU texture upload для picked source + интеграция с shell ImageLoader (P1-парсер готов, P3-shell hook готов)

Queue (Wave 2):
- webp-decoder: WebP decoder (libwebp-free pure-Rust: VP8 lossy baseline + VP8L lossless); за trait ImageDecoder в lumen-core::ext; интегрировать в lumen-image dispatch
- gif-decoder: GIF87a/89a decoder (LZW + frame loop); статичные кадры (frame 0); анимация — Wave 3
- font-stretch-matcher: font-stretch percentage matching в FontRegistry::find_best_match (CSS Fonts L4 §5.2 stretch selection algorithm)
- font-variable-opsz: opsz (optical-size) variation axis wiring — читать font-optical-sizing из ComputedStyle (P4 вешает), передавать в VariationCoords при font lookup
- icc-color-profiles: базовый ICC профиль из JPEG APP2/PNG iCCP — sRGB passthrough + gamma correction; без полного CMS (Phase 2)
- subpixel-text: subpixel LCD rendering для rasterizer — RGB-stripe фильтр; toggling через media-query prefers-reduced-motion

Queue (Wave 3+):
- avif-decoder: AVIF/AV1 декодер через rav1d (provisional dep); Phase 2
- webgl-context: WebGL 1.0 контекст поверх wgpu (WebGL API → wgpu calls); Phase 2+
- font-hinting: TrueType bytecode hinting в rasterizer; Phase 2
- svg-rasterizer: SVG basic shapes рендеринг (path/circle/rect/text) через paint pipeline; Phase 2

Recent: bug037-filter-uniform 2026-05-26, bug015-img-alt 2026-05-25, gradient-rendering 2026-05-22, border-radius-sdf 2026-05-22, bug033-box-shadow-blur 2026-05-22, animation-transition-engine 2026-05-22, multi-column-rendering 2026-05-22, @font-face-loading 2026-05-22, canvas2d-context 2026-05-22, woff2-decoder 2026-05-22, clip-path-rendering 2026-05-22, css-filter-pipeline 2026-05-22, bug017-018-closed 2026-05-22, bug032-area-avg 2026-05-22
