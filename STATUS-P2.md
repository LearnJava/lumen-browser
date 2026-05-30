# STATUS-P2 — Feature Development

**Developer:** Программист 2 (Feature development — any subsystem from roadmap)

---

## In progress

**10D.3 Cross-tab caches unified eviction API** — branch: `p2-cross-tab-cache`
Next step: add `EvictableCache` trait + `CacheRegistry` to `lumen-core/src/ext.rs`

---

## Next

Ordered by impact. Pick the first unblocked item; update "In progress" before coding.

| # | Task | Crate(s) | Effort | Blocker |
|---|------|----------|--------|---------|
| 1 | Box model renderer-side rendering — P3 wires `BoxModelOverlay` to the DevTools CDP overlay endpoint (7E.3 следующий шаг) | `devtools`, `shell` | M | P3 devtools |
| 2 | ~~3D depth buffer (pixel-exact пересекающиеся плоскости / BSP)~~ — выполнено в p2-css-3d-depth-buffer | `paint` | — | done |

---

## Recent merges

- **p2-depth-z-all-vertices** ✅ 2026-05-30 — GPU depth buffer расширен на `TextVertex`/`ImageVertex`/`RRectVertex`. Каждая из трёх вершин получила поле `z: f32` (CSS depth px); WGSL TEXT/IMAGE/RRECT шейдеры мапят z через ту же формулу `clamp(0.5 - z/20000, 0, 1)`, что и FillVertex; их pipeline'ы получили `DepthStencilState { Depth32Float, LessEqual }`. `VertexPos::set_depth` реализован для всех трёх — `apply_affine_to_verts` автоматически прокидывает projected z через 3D-путь. `apply_affine_to_rrect_verts` 3D-ветка использует `project_point_z` и пишет в `RRectVertex.z`. Теперь cross-type depth testing полный: 3D-transformed текст/картинки/SDF-rrect корректно перекрываются с background-rect под `preserve-3d`. 8 новых unit-тестов (423 total). Прежнее ограничение «depth только для FillVertex» снято.
- **p2-background-origin** ✅ 2026-05-30 — `background-origin` rendering: `background_origin_rect()` в `paint/src/display_list.rs` вычисляет positioning area (border/padding/content-box). `DrawBackgroundImage.origin_rect: Rect` — отдельный positioning rect независимо от clip. Renderer использует `oarea` для cover/contain ratio и `background-position` % offset, `area` (clip) только для x_end/y_end тайл-границ. 4 unit-теста. Graphic test 53.
- **p2-print-pages-renderer** ✅ 2026-05-30 — Print PDF renderer side: `DisplayCommand::PageBreak` маркер страницы, `build_print_display_list(pages: &[Page]) -> DisplayList` (фрагменты → page-local координаты через `PushTransform`), `split_at_page_breaks(cmds) -> Vec<Vec<DisplayCommand>>` (разбивает DL по маркерам), `Renderer::render_print_pages(font_bytes, pages, w, h) -> Result<Vec<Image>, _>` (headless render per page). 6 новых unit-тестов. Всего lumen-paint: 411 unit + 21 snapshot. P3 handoff: shell `--print-to-pdf` → собирает `Vec<Image>` → PDF через pdf-writer crate.
- **p2-memory-pressure** ✅ 2026-05-30 — `MemoryPressureSource` trait + `MemoryPressureLevel { Low, Medium, High }` + `NullMemoryPressureSource` в `lumen-core::ext`. Платформенные реализации в `core/src/memory_pressure.rs`: `Win32MemoryPressureSource` (`GlobalMemoryStatusEx`, `cfg(windows)`) + `LinuxMemoryPressureSource` (`/proc/pressure/memory` PSI avg10, `cfg(linux)`). Кэши подписаны: `ImageDecodeCache::on_memory_pressure` (4 теста), `GlyphAtlas::on_memory_pressure` (3 теста), `LayerCache::on_memory_pressure` (3 теста). 13 новых unit-тестов. Pending: macOS impl (10H.4) + shell integration (poll loop + tier trigger).
- **p2-text-shadow-blur** ✅ 2026-05-30 — `text-shadow` blur рендеринг: `emit_text_shadows()` в `paint/src/display_list.rs` оборачивает `DrawText` в `PushFilter{Blur(sigma)}/PopFilter` когда `shadow.blur > 0` (sigma = blur/2.0). Повторно использует Gaussian blur GPU-пасс из box-shadow и CSS `filter:blur()`. Порядок сохранён (обратный CSS-список). 3 unit-теста. Graphic test 52.
- **p2-scrollbar-rendering** ✅ 2026-05-30 — Визуальные скроллбары для `overflow:scroll/auto`: `DisplayCommand::DrawScrollbar{track_rect, thumb_rect, vertical}` в `paint/src/display_list.rs`. `scrollbar_rects()` helper (12px gutter, 20px min thumb, thumb position proportional to scroll offset). `walk()` emit после `PopScrollLayer` — не переводится с контентом. Renderer: 2 fill quad (track rgba(0,0,0,0.08) + thumb rgba(0,0,0,0.38)). 5 unit-тестов. Graphic test 51. P4 handoff: `scrollbar-width`/`scrollbar-color` (CSS Scrollbars L1) → `ComputedStyle` fields → override `SCROLLBAR_WIDTH` const + colors in renderer.
- **p2-image-decode-cache** ✅ 2026-05-30 — `ImageHandle` (`Arc<Image>`) + `ImageKey` + `ImageDecodeCache` (LRU, 256 MB default budget) в `lumen-image/src/decode_cache.rs` (ADR-008 §10E.1+10E.2). `insert()` + `get()` + `decode_or_get(key, closure)` cache-aside. Автоматический `evict_to_budget()` при превышении бюджета. `lru_candidates()` для внешнего управления вытеснением. Callers держат `ImageHandle`; eviction не освобождает данные пока живы внешние Arc. Экспортировано из `pub use decode_cache::{...}`. 9 unit-тестов. P3 handoff: 10E.4 (scroll-discard) — wire `gate_image_requests` + free handle when >3 screens from viewport.
- **p2-scroll-layer** ✅ 2026-05-29 — Scroll layer инфраструктура для `overflow:scroll`/`auto`: `LayoutBox.scroll_x/scroll_y`, `collect_scroll_containers()` + `set_scroll_position()` в `lumen-layout`, `PushScrollLayer{clip_rect, scroll_x, scroll_y}`/`PopScrollLayer` в `DisplayCommand`, walk emitter меняет `PushClipRect` → `PushScrollLayer` для Scroll|Auto overflow, renderer обрабатывает clip+translate. 11 тестов display list + 6 тестов layout. P4 handoff в STATUS-P4.md. P3 (shell): wire wheel events через `collect_scroll_containers()` + `set_scroll_position()`.
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
