In progress: webp-decoder  branch: p2-webp-decoder
Next step: ImageDecoder trait в lumen-core::ext + image-webp dep + decode_webp()  crates/engine/image/src/lib.rs

Bug fixes rule: P2 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

Next:
- webp-decoder: WebP decoder (VP8 lossy baseline + VP8L lossless, pure-Rust); за trait ImageDecoder в lumen-core::ext; интегрировать в lumen-image dispatch
- gif-decoder: GIF87a/89a decoder (LZW + frame loop); статичные кадры (frame 0); анимация — Wave 3
- font-stretch-matcher: font-stretch percentage matching в FontRegistry::find_best_match (CSS Fonts L4 §5.2 stretch selection algorithm)
- font-variable-opsz: opsz (optical-size) variation axis wiring — читать font-optical-sizing из ComputedStyle (P4), передавать в VariationCoords при font lookup
- icc-color-profiles: базовый ICC профиль из JPEG APP2/PNG iCCP — sRGB passthrough + gamma correction; без полного CMS

Queue (Wave 3+):
- avif-decoder: AVIF/AV1 декодер через rav1d (provisional dep); Phase 2
- webgl-context: WebGL 1.0 контекст поверх wgpu (WebGL API → wgpu calls); Phase 2+
- font-hinting: TrueType bytecode hinting в rasterizer; Phase 2
- subpixel-text: subpixel LCD rendering — RGB-stripe фильтр; toggleable через prefers-reduced-motion
- svg-rasterizer: SVG basic shapes рендеринг (path/circle/rect) через paint pipeline; Phase 2

Recent: picture-srcset-gpu 2026-05-27, bug037-filter-uniform 2026-05-26, bug015-img-alt 2026-05-25, gradient-rendering 2026-05-22, border-radius-sdf 2026-05-22, bug033-box-shadow-blur 2026-05-22, animation-transition-engine 2026-05-22, multi-column-rendering 2026-05-22, @font-face-loading 2026-05-22, canvas2d-context 2026-05-22, woff2-decoder 2026-05-22, clip-path-rendering 2026-05-22, css-filter-pipeline 2026-05-22, bug017-018-closed 2026-05-22
