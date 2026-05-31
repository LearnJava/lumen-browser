# STATUS-P4 — CSS Properties

**Developer:** Программист 4 (CSS implementation ONLY)

---

## In progress
_(none)_

## Workflow

1. **Check for "Needs wiring" section below** — P1/P2 algorithms ready for CSS connection
2. **Read CSS-SPECS.md** P4 Priority Queue for next property to implement
3. **Create branch:** `git checkout -b p4-<property-name>`, e.g. `p4-overflow-scroll`
4. **Implement end-to-end:**
   - Add field to `ComputedStyle` (lumen-layout/src/style.rs)
   - Add parsing in `apply_declaration()` 
   - Wire to `lay_out()` or `build_display_list()` as needed
   - Add 3-5 unit tests
   - Add visual test in `graphic_tests/`

5. **Merge:** After clippy + tests pass, merge to main
   - Update this STATUS-P4.md: move from "Needs wiring" to "Recent"
   - Update CSS-SPECS.md: mark property as ✅

---

## Next

Ordered by priority from CSS-SPECS.md. Items verified against CSS-SPECS.md 2026-05-29 state.

| # | Property / Feature | Effort | Blocker |
|---|-------------------|--------|---------|
| 1 | `::first-letter` / `::first-line` wiring | M | none (stubs ready — see "Needs wiring") |
| 2 | `overflow: scroll` scrollable containers | L | shell scroll event |
| 3 | `image-set()` / `cross-fade()` — CSS Images L4 | M | none |
| 4 | `text-align-last` | S | none |
| 5 | `perspective()` + `transform-style: preserve-3d` (3D Transforms L2) | L | none (P2 matrix primitive ready — see "Needs wiring") |
| 6 | `@counter-style` custom counter definitions | M | none |
| 7 | `justify-items` / `justify-self` for grid (Box Alignment L3) | S | none |
| 8 | `column-rule` rendering + `column-span` + `column-fill` | S | none |
| 9 | Scroll snap shell integration (`scroll-snap-type` / `scroll-snap-align`) | M | shell scroll |
| 10 | `::selection` pseudo-element | S | none |
| 11 | `::marker` rendering | S | none |
| 12 | `cq*` container query units (`cqw`/`cqh`/`cqi`/`cqb`/`cqmin`/`cqmax`) | M | none |
| 13 | `attr()` with type (CSS Values L4) | M | none |
| 14 | `mask-image` CSS wiring | L | P2 GPU compositing pass |
| 15 | `writing-mode: vertical-*` axis swap | L | ~~layout engine~~ **stub ready** (P1 2026-05-31, `vertical.rs`) |
| 16 | `subgrid` track inheritance | XL | grid engine |

---

## Needs wiring (algorithm ready, CSS not connected)

**P1/P2 have implemented the algorithm. P4 wires CSS property to it.**

### CSS 3D transforms — `perspective()` + 3D functions (P2 feature p2-css-3d-transforms)
- **Status:** GPU/matrix primitive ready. `Mat4` has 3D constructors (`perspective(d)`, `rotate_x/rotate_y/rotate_z/rotate_3d`, `translate_3d`, `scale_3d`, `from_3d` for `matrix3d`, `project_point` for 4×4 + perspective divide, `is_2d_affine` fast-path flag) in `lumen-layout/src/property_trees.rs`. The renderer (`paint/src/renderer.rs`, `apply_affine_to_verts` / `apply_affine_to_rrect_verts`) now projects any **non-2D-affine** `PushTransform` matrix perspective-correctly (w-divide), so 3D matrices render as a flattened projection. Existing 2D output is bit-identical (fast path).
- **P4 task:**
  1. Add 3D variants to `TransformFn` (style.rs): `RotateX(f32)`, `RotateY(f32)`, `RotateZ(f32)`, `Rotate3d(f32,f32,f32,f32)`, `TranslateZ(f32)`, `Translate3d(f32,f32,f32)`, `ScaleZ(f32)`, `Scale3d(f32,f32,f32)`, `Perspective(f32)`, `Matrix3d([f32;16])`. Parse them in `apply_declaration()` for `transform`.
  2. Map each new variant to its `Mat4` constructor in the `forward_box_transform` match (see `// CSS:` comment in `property_trees.rs`) **and** in `transform_fns_to_matrix` (animation path).
  3. **Parent `perspective` property** (field `ComputedStyle.perspective` already parsed): a non-`None` perspective on an element applies `Mat4::perspective(d)` to the space its children are drawn in. Wire this in `display_list.rs` where child `PushTransform` matrices are composed — premultiply the parent perspective (offset by `perspective-origin`) into each child's matrix. Add `perspective_origin` field to ComputedStyle (default `50% 50%`).
  4. **`transform-style`**: add `TransformStyle { Flat, Preserve3d }` field + `apply_declaration("transform-style")` (values `flat` / `preserve-3d`, default `flat`, **not** inherited). The **depth-sort primitive is ready** (P2 feature `p2-css-3d-depth-buffer`): `paint/src/display_list.rs` has `depth_sorted_child_order` (back-to-front painter's-algorithm sort of children by transformed z), gated behind `establishes_3d_rendering_context(b)`. To wire: change that helper's body from `false` to `b.style.transform_style == TransformStyle::Preserve3d` (single edit, marked with a `// CSS: transform-style` comment). The **GPU depth buffer is also ready** (`FillVertex.z` field populated by `apply_affine_to_verts` via `project_point_z` for 3D transforms; fill pipeline has `DepthStencilState` with `CompareFunction::LessEqual`; depth texture attached to frame render pass). Intersecting 3D planes will occlusion-test correctly once `preserve-3d` is wired.
- **Entry points:** `lumen-layout/src/property_trees.rs` `forward_box_transform` (match arm `// CSS:` comment) + `transform_fns_to_matrix`; `Mat4` 3D constructors in the same file.
- **CSS comment location:** `property_trees.rs` `forward_box_transform` transform-loop match.

### `position: sticky` scroll-driven offset (P1 feature p1-sticky-layout)
- **Status:** `StickyBox`, `collect_sticky_boxes()`, `compute_sticky_offset()` implemented in `lumen-layout/src/lib.rs`. Layout treats sticky as normal flow; offset computed separately.
- **P4 task:**
  1. `top/right/bottom/left` are already parsed (style.rs) and stored in `ComputedStyle`. No new CSS parsing needed.
  2. After each re-layout, call `collect_sticky_boxes(root)` to get the list.
  3. At each scroll event, call `compute_sticky_offset(sticky, scroll_x, scroll_y, vp_w, vp_h)` per entry and apply the returned `(dx, dy)` as a paint-layer translate (or `TransformNode` offset in the property trees).
  4. Non-px insets (`em`, `%`) currently yield `None` — wire resolved-px values from `lay_out_block()` context if full support is needed (optional for Phase 3).
- **Entry point:** `lumen-layout/src/lib.rs` — `collect_sticky_boxes()` + `compute_sticky_offset()`
- **CSS comment location:** `box_tree.rs` after `Position::Relative` block (end of `lay_out_block`)

### `writing-mode: vertical-rl / vertical-lr` axis swap (P1 feature p1-clickable-nodes, 2026-05-31)
- **Status:** `lay_out_vertical_block()` in `lumen-layout/src/vertical.rs`. Dispatched from `lay_out()` in `box_tree.rs` when `style.writing_mode` is `VerticalRl` or `VerticalLr`. `WritingMode` enum + field `writing_mode` already exists in `ComputedStyle` (style.rs). CSS parsing already wired.
- **P4 task:**
  1. No new CSS parsing or `ComputedStyle` changes needed — `writing_mode` field and `apply_declaration("writing-mode")` are already in `style.rs`.
  2. The dispatch already reads `b.style.writing_mode` (box_tree.rs `lay_out()`) — no wiring needed there either.
  3. **Optional extension:** `sideways-rl` / `sideways-lr` variants in `WritingMode` enum — parse them in `apply_declaration` and handle in `lay_out_vertical_block` (currently falls through to `VerticalRl`).
  4. Inline text flow inside vertical containers (character rotation, vertical text metrics) — deferred to a future P1 inline-vertical task.
- **Entry points:** `crates/engine/layout/src/vertical.rs:1` — `lay_out_vertical_block`; `crates/engine/layout/src/box_tree.rs` — dispatch at `lay_out()` writing-mode check (search `// CSS: writing-mode`).
- **CSS comment location:** `box_tree.rs` at the writing-mode dispatch block.

### ::first-letter pseudo-element (P1 feature p1-css-first-line-letter)
- **Status:** Structural markers ready in InlineRun
- **P4 task:**
  1. Look up `::first-letter` rule via `compute_pseudo_element_style()`
  2. Override segment.style for first grapheme
  3. Wire in `lay_out()` (box_tree.rs) after wrap_inline_run()
  4. Split first grapheme if font-size changes at display-list time

### ::first-line pseudo-element (P1 feature p1-css-first-line-letter)
- **Status:** Structural markers ready in InlineRun.lines[0]
- **P4 task:**
  1. Look up `::first-line` rule via `compute_pseudo_element_style()`
  2. Override frag.style for first line (inheritable properties only)
  3. Wire in `lay_out()` (box_tree.rs) after wrap_inline_run()

### :host / ::slotted pseudo-classes (Shadow DOM)
- **Status:** Selector matching needed in composed tree
- **P4 task:**
  1. Implement `:host` matching in `matches_complex()` (from inside shadow tree)
  2. Implement `::slotted()` pseudo-element matching
  3. Wire in `build_box()` (box_tree.rs)

### `font-variation-settings` TextMeasurer wiring (P1 feature p1-font-variation-wiring)
- **Status:** `Font::advance_width_varied(glyph_id, hmtx, coords)` реализована в `lumen-font/src/face.rs`. `rasterize_and_insert` в renderer.rs применяет HVAR delta при растеризации. gvar outline deltas уже работали ранее.
- **P4 task:**
  1. Добавить `font_variation_settings: Vec<([u8; 4], f32)>` в `ComputedStyle` (style.rs). Парсинг CSS значения типа `"wght" 600` → `Vec<([u8;4], f32)>`.
  2. Расширить `TextMeasurer` трейт методом `char_width_varied(&self, ch, font_size_px, axes: &[([u8;4], f32)]) -> f32` в `lumen-layout/src/lib.rs`. Реализовать в `FontMeasurer` (paint/src/lib.rs) через `Font::advance_width_varied`.
  3. Обновить `measure_text_w` и вызовы в box_tree.rs для передачи `variation_axes` из `ComputedStyle`.
- **Entry points:** `lumen-layout/src/lib.rs:88` (`TextMeasurer` трейт, комментарий `// CSS: font-variation-settings`), `lumen-layout/src/box_tree.rs:4606` (`measure_text_w`, аналогичный комментарий)
- **CSS comment locations:** `lib.rs:88`, `box_tree.rs:4606`

### CSS Scroll Snap — snap container + snap target algorithm (P1 feature p1-scroll-snap)
- **Status:** `SnapPoint`, `SnapContainer`, `collect_snap_containers(root)`, `find_snap_target(container, current_scroll, target_scroll)` implemented in `lumen-layout/src/lib.rs`. CSS parsing already done (`scroll_snap_type`, `scroll_snap_align`, `scroll_snap_stop` in `ComputedStyle`). 10 unit tests pass.
- **P4 / P3 task (this item #9 in Next, blocker is shell):**
  1. No new CSS parsing needed — fields already in `ComputedStyle`.
  2. **P3 shell integration**: after every `relayout_page()`, call `collect_snap_containers(root)` and cache the list. At each scroll event, call `find_snap_target(container, current_scroll, target_scroll)` per container; if `Some((sx, sy))` returned, animate/clamp scroll to that position.
  3. The main-frame viewport scroll can be modelled as a synthetic container with `rect = Rect { x: 0, y: 0, width: vp_w, height: vp_h }` and the root layout box's snap-type. For `overflow: scroll` sub-containers, use their border-box rect from the layout tree.
  4. `scroll-snap-align` inline axis → `snap_x`; block axis → `snap_y`. Container's `axis` field restricts which is used in `find_snap_target`.
- **Entry points:** `lumen-layout/src/lib.rs` — `collect_snap_containers()` + `find_snap_target()` (search `// CSS: scroll-snap-type` comment in lib.rs)

### `overflow: scroll` / `overflow: auto` scroll layer (P2 feature p2-scroll-layer)
- **Status:** Full scroll layer infrastructure ready.
  - `LayoutBox.scroll_x / scroll_y` (f32, default 0.0) — per-element scroll offset. `lumen-layout/src/box_tree.rs:920`.
  - `collect_scroll_containers(root) -> Vec<ScrollContainer>` — enumerates all scroll containers. `lumen-layout/src/lib.rs`.
  - `set_scroll_position(root, node, x, y) -> bool` — updates scroll offset with clamping. `lumen-layout/src/lib.rs`.
  - `DisplayCommand::PushScrollLayer { clip_rect, scroll_x, scroll_y }` / `PopScrollLayer` — clips to padding-box + translates by `(-scroll_x, -scroll_y)`. `paint/src/display_list.rs`.
  - Renderer handles `PushScrollLayer` as clip+translate. `paint/src/renderer.rs`.
  - `walk` (display list builder) already emits `PushScrollLayer` when `overflow_x/y` is `Scroll|Auto`.
- **P4 task:**
  1. `overflow` is already parsed to `Overflow::Scroll | Overflow::Auto` in `apply_declaration()` — no new parsing needed for basic scroll.
  2. The display list emitter (`walk` in `display_list.rs`) already emits `PushScrollLayer` when the parsed `overflow_x/y` equals `Scroll|Auto`. So P4 does **not** need to change the display list emitter — just ensure the CSS parsing is correct (it already is).
  3. P3 (shell) still needs to wire scroll events: on `MouseWheel`, find the container via `collect_scroll_containers()` + point-in-rect, call `set_scroll_position()`, rebuild display list.
  4. `overflow: scroll` already removes the "scroll" blocker for P4's Next #2 entry.
- **Entry points:** `lumen-layout/src/lib.rs` (collect / set API), `paint/src/display_list.rs:2736` (emitter), `paint/src/renderer.rs` (PushScrollLayer handler after PopTransform).
- **CSS comment location:** `display_list.rs:2727` `// CSS: overflow — P4 wires:...` comment.

### `scrollbar-width` / `scrollbar-color` (P2 feature p2-scrollbar-rendering)
- **Status:** `DisplayCommand::DrawScrollbar { track_rect, thumb_rect, vertical }` implemented. Renderer draws track + thumb as two semi-transparent fill quads. Default appearance: 12px gutter, track rgba(0,0,0,0.08), thumb rgba(0,0,0,0.38).
- **P4 task:**
  1. Add `scrollbar_width: ScrollbarWidth` to `ComputedStyle` (values: `auto | thin | none`, default `auto`). Parse in `apply_declaration("scrollbar-width")`.
  2. Add `scrollbar_color: Option<(CssColor, CssColor)>` (thumb, track pair). Parse `scrollbar-color: <color> <color>` in `apply_declaration("scrollbar-color")`.
  3. In `display_list.rs` `walk()`: when emitting `DrawScrollbar`, if `b.style.scrollbar_width == None` skip entirely (no scrollbar). Thread `scrollbar_color` through to `DrawScrollbar` fields so renderer can use it instead of hard-coded constants.
  4. In `renderer.rs` `DrawScrollbar` handler: read the per-command color fields instead of `TRACK_COLOR`/`THUMB_COLOR` constants.
- **Entry points:** `paint/src/display_list.rs` — `scrollbar_rects()` helper + `walk()` emit block after `PopScrollLayer`. `paint/src/renderer.rs` — `DrawScrollbar` match arm. `SCROLLBAR_WIDTH: f32 = 12.0` const controls default gutter width.

### CSS Scroll-Driven Animations L1 — `ScrollTimeline` / `ViewTimeline` (P1 feature p1-scroll-driven-animations)
- **Status:** Algorithm ready. `ScrollTimeline`, `ViewTimeline`, `NamedScrollTimeline`, `NamedViewTimeline`, `ScrollAxis`, `Viewport` in `lumen-layout/src/scroll_timeline.rs`. Progress resolvers: `resolve_scroll_progress()` + `resolve_view_progress()`. Collection stubs: `collect_named_scroll_timelines()` + `collect_named_view_timelines()`. All exported from `lumen-layout`. 15 unit tests.
- **P4 task** (CSS Scroll-Driven Animations L1):
  1. Add `scroll_timeline_name: Option<String>` + `scroll_timeline_axis: ScrollAxis` to `ComputedStyle`. Parse `scroll-timeline-name` + `scroll-timeline-axis` in `apply_declaration()`. Wire to `collect_named_scroll_timelines()` — iterate layout tree, emit `NamedScrollTimeline` for each node with a non-`none` `scroll_timeline_name`.
  2. Add `view_timeline_name: Option<String>` + `view_timeline_axis: ScrollAxis` to `ComputedStyle`. Parse `view-timeline-name` + `view-timeline-axis`. Wire to `collect_named_view_timelines()`.
  3. Add `animation_timeline: AnimationTimeline` enum (`Auto | ScrollFn(ScrollTimeline) | ViewFn(ViewTimeline) | Named(String)`) to `ComputedStyle`. Parse `animation-timeline` (`auto`, `scroll()`, `view()`, `<custom-ident>`).
  4. In the animation scheduler (`AnimationScheduler` / shell tick loop): resolve `animation_timeline` to a progress fraction using `resolve_scroll_progress` / `resolve_view_progress`, then drive `CompositorAnimFrame` progress from it instead of wall-clock time.
- **Entry points:** `lumen-layout/src/scroll_timeline.rs` (all public API), `lumen-layout/src/style.rs` (ComputedStyle), `lumen-layout/src/animation.rs` (AnimationScheduler).

### SVG path stroke advanced properties (P2 feature p2-svg-stroke-path)
- **Status:** Stroke tessellation implemented. `tessellate_stroke(contours, half_width)` in `paint/src/svg_path.rs`. `emit_svg_shape` in `paint/src/display_list.rs` now reads `svg_stroke` + `svg_stroke_width` from `ComputedStyle` and emits a second `DrawSvgPath` for the stroke band (miter join, butt cap). Stroke works end-to-end for any SVG `<path>`.
- **P4 task** (CSS Fill & Stroke L3):
  1. `svg_fill_rule` field in `ComputedStyle` (values: `nonzero | evenodd`, default `nonzero`). Parse `fill-rule` in `apply_declaration()`. Wire to `tessellate_fill` call in `emit_svg_shape` by passing a `FillRule` enum (multi-contour even-odd still needs stencil GPU pass — for now, wiring the enum is enough; single-contour paths produce correct output regardless).
  2. `stroke_linecap: StrokeLinecap { Butt, Round, Square }` field (default `Butt`). Parse `stroke-linecap` in `apply_declaration()`. Wire: `tessellate_stroke` currently produces butt caps. P4 can add `round_cap` / `square_cap` logic to `stroke_contour` in `svg_path.rs` or emit half-circle/half-square cap triangles in `emit_svg_shape` after the main stroke band.
  3. `stroke_linejoin: StrokeLinejoin { Miter, Round, Bevel }` field (default `Miter`). Parse `stroke-linejoin`. Wire to `miter_offset` in `svg_path.rs` — `Round` and `Bevel` variants need separate join triangle fan code.
  4. `stroke_miterlimit: f32` (default `4.0`). Parse `stroke-miterlimit`. Wire: replace the hard-coded `4.0 * half_w` clamp in `miter_offset()` with the parsed value.
  5. `stroke_dasharray: Vec<f32>` + `stroke_dashoffset: f32` (default: empty/0). Parse. Wire: at `emit_svg_shape`, before calling `flatten_path`/`tessellate_stroke`, implement dash pattern by splitting each polyline segment into painted/unpainted sub-segments according to the dash array.
- **Entry points:** `paint/src/svg_path.rs:548` (`tessellate_stroke` — `// CSS:` comment inline), `paint/src/display_list.rs:3263` (`emit_svg_shape` `SvgShapeKind::Path` branch — `// CSS:` comments inline).

---

## Recent merges

| Date | Property | Notes |
|------|----------|-------|
| 2026-05-29 | `var()` full recursive substitution | expand_vars() recursive + @property + env(); 40 unit tests + graphic test 50 |
| 2026-05-29 | `font-optical-sizing` | auto→opsz=font-size in variation axes; none skips; 5 tests |

---

## Notes

- **No algorithm work:** Don't write layout/paint algorithms — that's P1/P2
- **CSS-only:** No shell integration, no runtime — strictly property definition
- **One property per commit** to keep history clean
- **Graphic tests required:** Every property needs a visual test in `graphic_tests/`
- **Check CSS-SPECS.md:** For full property roadmap and spec references

See CLAUDE.md §"CSS ownership: P4 only" for full workflow details.
