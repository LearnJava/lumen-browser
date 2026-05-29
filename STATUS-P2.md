# STATUS-P2 — Feature Development

**Developer:** Программист 2 (Feature development — any subsystem from roadmap)

---

## In progress
Scroll layer infrastructure for overflow:scroll  branch: p2-scroll-layer
Next step: PushScrollLayer/PopScrollLayer display commands + LayoutBox.scroll_x/scroll_y  box_tree.rs:903

---

## Next

Ordered by impact. Pick the first unblocked item; update "In progress" before coding.

| # | Task | Crate(s) | Effort | Blocker |
|---|------|----------|--------|---------|
| 1 | Box model renderer-side rendering — P3 wires `BoxModelOverlay` to the DevTools CDP overlay endpoint (7E.3 следующий шаг) | `devtools`, `shell` | M | P3 devtools |
| 2 | ~~3D depth buffer (pixel-exact пересекающиеся плоскости / BSP)~~ — выполнено в p2-css-3d-depth-buffer | `paint` | — | done |

---

## Recent merges

- **p2-css-3d-depth-buffer** ✅ 2026-05-29 — GPU depth buffer для CSS 3D transforms: `FillVertex.z` (CSS depth px), FILL_SHADER NDC depth mapping `0.5 - z/20000`, `fill_pipeline` з `DepthStencilState(LessEqual)`, `depth_texture/depth_view` в Renderer (recreated on resize), depth attachment в frame render pass. `apply_affine_to_verts` використовує `project_point_z` для 3D матриць + `VertexPos::set_depth`. 5 нових unit-тестів (66 renderer total). P4 handoff оновлено — як тільки P4 дротує `transform-style: preserve-3d`, GPU occlusion для перетинних площин буде коректним.
- **p2-css-preserve-3d** ✅ 2026-05-29 — True depth-sorted 3D для `transform-style: preserve-3d` (Transforms L2 §6.2). Depth-sort компоновщик в `paint/src/display_list.rs`: `depth_sorted_child_order` (стабильная back-to-front painter's-сортировка детей по transformed z), `child_z_depth`, gated за `establishes_3d_rendering_context` (`// CSS: transform-style` — P4 флипнёт `false`→`b.style.transform_style == Preserve3d`). z-aware методы `Mat4::project_point_z` / `transform_z` в `layout/property_trees.rs`. Интегрировано в `walk` и `walk_with_anim`; flat-путь побитово идентичен (document order). 11 unit-тестов (5 layout + 6 paint). Pixel-exact пересечения плоскостей (depth buffer/BSP) — Next #2. P4 handoff обновлён.
- **p2-css-3d-transforms** ✅ 2026-05-29 — CSS 3D transforms (Transforms L2 #24): Mat4 3D-конструкторы (`perspective`/`rotate_x/y/z`/`rotate_3d`/`translate_3d`/`scale_3d`/`from_3d`/`project_point`/`is_2d_affine`) в `layout/property_trees.rs` (18 unit-тестов). Renderer проецирует 3D/перспективные матрицы через `project_point` с делением на w (flattened: rotateX/Y, card flip, perspective-наклоны), 2D affine — прежний быстрый путь (3 теста). P4 handoff для 3D transform-функций + `perspective` + `transform-style`. Depth buffer и `preserve-3d` отложены (см. Next #1).
- **p2-mask-image-layer** ✅ 2026-05-29 — `MaskMode { Alpha, Luminance }` + `PushMaskLayer/PopMaskLayer` в `DisplayCommand` (CSS Masking L1 §5). WGSL shader `fs_alpha`/`fs_luma` (ITU-R BT.709 luminance). Два пайплайна с REPLACE blend: scratch×mask → parent layer at element rect. 4 unit-теста. Graphic test 26 обновлён.
- **p2-boxmodel-overlay** ✅ 2026-05-29 — `DisplayCommand::BoxModelOverlay { margin, border, padding, content }` (7E.3): DevTools box model overlay. Renderer разворачивает в 4 полупрозрачных FillRect (Chrome DevTools палитра). 2 unit-теста.
- **p2-svg-path-rendering** ✅ 2026-05-29 — SVG `<path>` GPU рендеринг: tessellator (lyon или аналог) + `DrawSvgPath` в `paint/src/display_list.rs`. Поддержка всех path-команд (M/L/C/Q/A/Z).
- **p2-webp-decoder** ✅ 2026-05-27 — WebP декодер (provisional `image` crate): растеризует WebP в RGBA, загружается через `ImageDecoder` trait.
- **p2-picture-srcset-gpu** ✅ 2026-05-27 — `<picture>`/`srcset` GPU upload: текстуры через wgpu texture pipeline; P1 парсер уже готов.
- **p2-gradient-rendering** ✅ 2026-05-22 — GPU градиенты: linear/radial/conic через wgpu compute shader.
- **p2-border-radius-sdf** ✅ 2026-05-22 — `border-radius` через SDF в fragment shader.
- **p2-animation-transition-engine** ✅ 2026-05-22 — `TransitionScheduler` + `AnimationScheduler` с `tick()` wired в shell `RedrawRequested`.
- **p2-multi-column-rendering** ✅ 2026-05-22 — Multi-column layout: column-count/column-width + column gap rendering.
- **p2-canvas2d-context** ✅ 2026-05-22 — Canvas 2D context: fill/stroke/path/image/text через wgpu.
- **p2-woff2-decoder** ✅ 2026-05-22 — WOFF2 декодер (brotli decompress + sfnt extract).
- **p2-clip-path-rendering** ✅ 2026-05-22 — `clip-path` basic shapes (inset/circle/ellipse/polygon) через stencil.
- **p2-css-filter-pipeline** ✅ 2026-05-22 — CSS filter + backdrop-filter GPU offscreen pass: `PushBackdropFilter/PopBackdropFilter`, 4 display-list tests + graphic test 30.

---

## Notes

- **Coordinate with P1:** Check STATUS-P1.md before starting cross-domain work
- **CSS workflow:** If your algorithm needs a CSS property, add `// CSS: <property>` comment and note in STATUS-P4.md "Needs wiring"
- **Bug discovery:** Don't fix bugs — add to BUGS.md with next BUG-NNN number, continue feature work
- **All tasks tracked:** Use git branch prefix `p2-<task-name>` so parallel sessions don't duplicate

See CLAUDE.md for full workflow details.
