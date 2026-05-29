# STATUS-P2 — Feature Development

**Developer:** Программист 2 (Feature development — any subsystem from roadmap)

---

## In progress
_(none)_

---

## Next

Ordered by impact. Pick the first unblocked item; update "In progress" before coding.

| # | Task | Crate(s) | Effort | Blocker |
|---|------|----------|--------|---------|
| 1 | `mask-image` GPU compositing — CSS Masking #14: `PushMaskLayer/PopMaskLayer` в DisplayCommand + wgpu stencil/alpha compositing pass | `paint` | L | none |
| 2 | CSS 3D transforms — `perspective()` + `transform-style: preserve-3d` (Transforms L2 #24): wgpu matrix stack, depth buffer | `paint` | L | none |
| 3 | Box model renderer-side rendering — P3 wires `BoxModelOverlay` to the DevTools CDP overlay endpoint (7E.3 следующий шаг) | `devtools`, `shell` | M | P3 devtools |

---

## Recent merges

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
