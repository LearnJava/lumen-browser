# STATUS-P4 — CSS Properties

**Developer:** Программист 4 (CSS implementation ONLY)

---

## In progress
_(none)_ — p4-flex-align-content-single влит 2026-06-08

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

Ordered by priority. Задачи с `→ [docs/tasks/…]` имеют подробный пошаговый файл для Haiku.

### Phase 2 — делать первыми (блокируют закрытие фазы)

| # | Property / Feature | Effort | Blocker | Task file |
|---|-------------------|--------|---------|-----------|
| **A** | **`:host` / `::slotted` (Shadow DOM)** — Web Components Phase 2 | M | none | — |
| **B** | **Find in page (Ctrl+F)** — `[P4]` по lumen-plan.md §Фаза 2 | M | none | — |
| **C** | **DevTools / Inspector Phase 0** — DOM tree + computed styles + network log (CDP минимум) — `[P4]` по lumen-plan.md §Фаза 2 | L | none | — |
| **D** | **`overflow: scroll` scrollable containers** | L | P1 shell wiring (добавлено в STATUS-P1.md W-6) | — |

### CSS Properties (после Phase 2)

| # | Property / Feature | Effort | Blocker | Task file |
|---|-------------------|--------|---------|-----------|
| ~~1~~ | ~~`:fullscreen` + `:popover-open` sentinel pseudo-classes~~ — **выполнено** | XS | none | — |
| ~~2~~ | ~~`color-mix()` CSS parsing~~ — **выполнено** (p4-color-mix-parsing, 2026-06-08) | S | none | → [`docs/tasks/p4-color-mix-parsing.md`](docs/tasks/p4-color-mix-parsing.md) |
| ~~3~~ | ~~`text-align-last` wiring в align_lines~~ — **выполнено** (p4-text-align-last, 2026-06-08) | S | none | → [`docs/tasks/p4-text-align-last.md`](docs/tasks/p4-text-align-last.md) |
| ~~4~~ | ~~`::selection` pseudo-element~~ — **выполнено** (p4-selection-pseudo, 2026-06-08) | S | none | — |
| ~~5~~ | ~~`attr()` with type (CSS Values L4)~~ — **выполнено** (p4-attr-typed, 2026-06-08) | M | none | — |
| ~~6~~ | ~~`font-variation-settings` TextMeasurer wiring~~ — **выполнено** (p4-font-variation-settings, 2026-06-08) | M | — | — |
| ~~3~~  | ~~`perspective()` + `transform-style: preserve-3d`~~ — **выполнено P2** (p2-css-3d-wiring, 2026-06-03) | — | — | — |
| ~~4~~  | ~~`@counter-style`~~ — **выполнено P2** (p2-c7-counter-style, 2026-06-03) | — | — | — |
| ~~5~~  | ~~`justify-items`/`justify-self`~~ — **выполнено** (parsing+wiring, 2026-06-03) | — | — | — |
| ~~6~~  | ~~`column-span`/`column-fill`~~ — **выполнено P2** (p2-c8-column-extras, 2026-06-03) | — | — | — |
| ~~9~~  | ~~`::marker` rendering~~ — **выполнено P2** (p2-c9-marker-rendering, 2026-06-03) | — | — | — |
| ~~10~~ | ~~`cq*` container query units~~ — **выполнено P1** (p1-cq-units, 2026-06-03) | — | — | — |
| ~~12~~ | ~~`mask-image`~~ — **выполнено P4+P2** (p4-mask-image, 2026-06-03) | — | — | — |
| ~~13~~ | ~~`writing-mode: vertical-*`~~ — **уже проброшено** (wiring в box_tree.rs готово) | — | — | — |
| ~~14~~ | ~~`subgrid`~~ — **алгоритм готов, P4 работа не нужна** (p1-css-subgrid, 2026-06-03) | — | — | — |

---

## Needs wiring (algorithm ready, CSS not connected)

**P1/P2 have implemented the algorithm. P4 wires CSS property to it.**

### `@starting-style` entry transitions (P1 V-2, 2026-06-10)
- **Status:** Algorithm ready. `StartingStyleTracker` and `resolve_starting_style()` in `lumen-layout/src/starting_style.rs`. `TransitionScheduler::sync` has `// CSS: @starting-style` doc comment at `animation.rs:1104`.
- **P4 task:**
  1. In `TransitionScheduler::sync`, check `tracker.is_entered(node)`. If true, call `resolve_starting_style(node, doc, sheet)` to get entry declarations.
  2. Build a `ComputedStyle` from those declarations (via `apply_declaration` on a fresh `ComputedStyle::default()`).
  3. Pass that style as the `old` argument to `sync` (before-change style) — the transition starts from the `@starting-style` state.
  4. Call `tracker.consume(node)` after transitions are registered.
  5. Call `tracker.mark_entered(node)` from the shell / JS runtime whenever a node is inserted into the DOM or `display` changes from `none` → non-`none`.
- **Entry points:** `crates/engine/layout/src/animation.rs:1104` — `// CSS: @starting-style`; `crates/engine/layout/src/starting_style.rs`.

### ✅ `object-fit` / `object-position` — **ВЫПОЛНЕНО** (p4-object-fit, 2026-06-08)
`compute_object_fit_transform()` добавлена в `box_tree.rs`; при Fill (CSS default) сохраняется поведение SVG `preserveAspectRatio`; для Contain/Cover/None/ScaleDown применяется CSS Images L3 §5.5 семантика. `object-position` управляет выравниванием через free-space фракции. 6 unit-тестов + graphic test 70.

### `::first-letter` / `::first-line` pseudo-elements (P5 audit 2026-06-08)
- **Status:** Algorithm stubs ready. `build_first_letter_segment()` at `box_tree.rs:1205` and `build_first_line_segment()` at `box_tree.rs:1257` have full doc comments with step-by-step wiring instructions. Both call `compute_pseudo_element_style(node, "first-letter"/"first-line")` placeholder.
- **P4 task:**
  1. In `apply_declaration()` / cascade, handle `::first-letter` and `::first-line` pseudo-element rules.
  2. In `compute_style()`, expose `compute_pseudo_element_style(node, pseudo: &str) -> ComputedStyle` that looks up matched pseudo rules and overrides the parent style.
  3. Call these from `build_first_letter_segment` and `build_first_line_segment` at `box_tree.rs:1205/1257`.
- **Entry points:** `lumen-layout/src/box_tree.rs:1205` and `:1257` — `// CSS: ::first-letter` / `::first-line`.

### `border-spacing` (P5 audit 2026-06-08)
- **Status:** Algorithm stub ready. Table cell layout in `box_tree.rs` uses hardcoded `h_spacing = 0.0` at lines 4156, 4258, 4320, 4363, 4488. `lay_out_table_with_spacing()` at 4488 has a `// CSS: border-spacing` comment and accepts an `h_spacing` parameter.
- **P4 task:**
  1. Add `border_spacing_h: f32` and `border_spacing_v: f32` to `ComputedStyle` (non-inherited, default 0.0). Parse `border-spacing` shorthand (1 or 2 lengths) in `apply_declaration()`.
  2. At `box_tree.rs:4156/4258/4320/4363/4488` replace `0.0` / hardcoded `h_spacing` with `style.border_spacing_h` / `style.border_spacing_v`.
- **Entry points:** `lumen-layout/src/box_tree.rs:4488` — `lay_out_table_with_spacing`; `:4156` first `// CSS: border-spacing`.

### `anchor-name` / `position-anchor` / `anchor()` / `inset-area` (P5 audit 2026-06-08)
- **Status:** Full algorithm scaffold ready in `lumen-layout/src/anchor.rs`. `AnchorRegistry`, `AnchorResolver`, `resolve_anchor_position()` are implemented. Comments `// CSS: anchor-name`, `// CSS: position-anchor`, `// CSS: anchor()`, `// CSS: inset-area` at lines 35, 62, 153, 160, 203, 284 mark CSS wiring points. CSS Anchor Positioning L1 spec.
- **P4 task:**
  1. Add `anchor_name: Option<Box<str>>` and `position_anchor: Option<Box<str>>` to `ComputedStyle`; parse in `apply_declaration()`.
  2. Wire `inset-area` grid shorthand: add `inset_area: Option<InsetArea>` to `ComputedStyle`, parse 2-value `<self-position> <other-position>` syntax.
  3. At `anchor.rs:146`, read `child.style.position_anchor` instead of `None`.
  4. At `anchor.rs:153/160`, call `registry.register(node_id, &style.anchor_name)`.
- **Entry points:** `lumen-layout/src/anchor.rs:35` — `AnchorRegistry`; `:146` — `resolve_anchor_position` caller.

### ~~`list-style-type` (custom counter-style)~~ — **ВЫПОЛНЕНО** (p4-list-style-type-custom, 2026-06-08)
`ListStyleType::Custom(Box<str>)` добавлен; `parse()` возвращает `Custom` для нераспознанных idents; `build_list_marker_text()` резолвит через `format_counter_with_registry`; shorthand-парсер исправлен (position до type). 3 unit-теста + graphic test 32.

### `gap-rule-width`, `gap-rule-style`, `gap-rule-color` (P2 feature p2-e5-gap-decorations, 2026-06-07)
- **Status:** Paint-side emit logic ready. `lumen-paint::emit_gap_rules(boxes, gaps, ctx)` in `gap_decorations.rs` takes a `GapDecorationContext {rule_width, rule_style, rule_color}` and a slice of `GapSegment {rect, horizontal}` and returns `Vec<DisplayCommand::DrawBorder>`. Rules are centered in each gap rectangle; column gaps get vertical rules (right-side DrawBorder), row gaps get horizontal rules (bottom-side DrawBorder). Clamped to gap size if rule_width > gap. 6 unit tests pass.
- **P4 task:**
  1. Add `gap_rule_width: f32`, `gap_rule_style: BorderStyle`, `gap_rule_color: CssColor` fields to `ComputedStyle` in `lumen-layout/src/style.rs` (near `column_rule_*` fields, non-inherited, default width=0/style=None/color=currentColor).
  2. Wire `apply_declaration()` for `gap-rule-width`, `gap-rule-style`, `gap-rule-color` shorthand/longhands.
  3. In `lumen-paint/src/display_list.rs` `walk()`, after emitting grid/flex children, build `GapSegment` list from child box positions and call `emit_gap_rules()` with `GapDecorationContext` from style.
- **Entry points:** `lumen-paint/src/gap_decorations.rs` — `emit_gap_rules`, `GapDecorationContext`, `GapSegment`; re-exported from `lumen_paint::`.

### `font-stretch` (P2 feature p2-f3-font-stretch, 2026-06-07)
- **Status:** Algorithm ready. `MultiFontMeasurer::resolve_font_stretch(families, stretch_pct) -> Option<f32>` in `lumen-paint/src/lib.rs`. Searches registered @font-face families for a `wdth` variation axis; if found, clamps `stretch_pct` to `[axis.min, axis.max]` and returns Some. Returns None when no registered family has a `wdth` axis. 4 unit tests pass.
- **P4 task:**
  1. Add `font_stretch: f32` field to `ComputedStyle` in `lumen-layout/src/style.rs` (default 100.0; non-inherited per CSS Fonts L4 §5.2). Parse keyword and percentage forms in `apply_declaration("font-stretch")`: ultra-condensed→50, condensed→75, normal→100, expanded→125, ultra-expanded→200, or `<percentage>` directly.
  2. In `build_display_list` / wherever `DrawText` is emitted, call `measurer.resolve_font_stretch(families, style.font_stretch)` and pass the result as a variation-settings override to the font rasterizer (future: once variable font rendering is wired).
- **Entry points:** `lumen-paint/src/lib.rs` — `MultiFontMeasurer::resolve_font_stretch` (pub); `// CSS: font-stretch` comment on the method.

### `grid-template-columns/rows: subgrid` (P1 feature p1-css-subgrid, 2026-06-03)
- **Status:** Full layout algorithm ready in `lumen-layout/src/subgrid.rs` + `box_tree.rs`.
  - `GridTrackSize::Subgrid` variant added to the enum (`style.rs:3490`).
  - `parse_track_list("subgrid", ...)` returns `vec![GridTrackSize::Subgrid]` sentinel.
  - `lay_out_grid` in `box_tree.rs:4586` reads thread-local `SUBGRID_COL_CTX`/`SUBGRID_ROW_CTX` and uses inherited track sizes when available.
  - Parent grid automatically sets thread-locals for subgrid children before `lay_out` call (RAII `SubgridContextGuard`).
  - `collect_subgrid_items(root) -> Vec<SubgridItem>` — iterates layout tree and returns all subgrid containers.
  - 9 unit tests pass: parse (2), layout (1), collect_subgrid_items (1), SubgridContext API (5).
- **P4 task:** CSS parsing already wired — `apply_declaration` for `grid-template-columns`/`grid-template-rows` calls `parse_track_list` which handles `subgrid`. No new ComputedStyle fields needed. The layout engine now reads `GridTrackSize::Subgrid` sentinel and applies inherited tracks. **No further P4 work required for Phase 1** — the algorithm is end-to-end. To add CSS Grid L2 `<line-name-list>` after `subgrid` keyword (optional), extend `parse_track_list` to collect named lines when `subgrid <ident>+` is detected.
- **Entry points:** `lumen-layout/src/subgrid.rs` — `SubgridContext`, `collect_subgrid_items`; `lumen-layout/src/box_tree.rs:4586` — `lay_out_grid` subgrid entry.

### ✅ `:fullscreen` + `:popover-open` CSS pseudo-classes (p4-sentinel-pseudos, 2026-06-03)
- **Status:** WIRED — `PseudoClass::Fullscreen` и `PseudoClass::PopoverOpen` проверяют sentinel-атрибуты `data-lumen-fullscreen` и `data-lumen-popover-open`. 2 новых теста. Полный рабочий цикл с обоих сторон (JS выставляет атрибут, CSS его читает).

### CSS `image-set()` background image (P1 V-4 + P2 feature p2-css-image-set)
- **Status:** Full algorithm ready. `lumen-layout::image_set` (V-4, p1-v4-image-set) provides typed API: `parse_image_set(value) -> Vec<ImageSetOption>`, `select_image_set_candidate(candidates, dpr, supported) -> Option<&ImageSetOption>` with CSS Images L4 §5 `type()` filtering, `select_image_set_url(value, dpr) -> String` convenience wrapper. `lumen-paint::select_image_set_url` (raw `&str` variant) also exists in `display_list.rs`; `emit_background_layer` calls it automatically. DPR threading: `build_display_list_ordered_dpr` / `build_display_list_ordered_with_anim_dpr` take `dpr`; default `1.0`.
- **P4 task:**
  1. In `style.rs` background-image parsing (`parse_single_bg_layer`, near the `url(...)` / gradient branches, ~line 13345) detect `image-set(` / `-webkit-image-set(` tokens and store the **raw function string** in `BackgroundImage::Url(...)` (do **not** pre-resolve — paint picks per-DPR). Same for `background` shorthand layer parsing.
  2. (Optional) For intrinsic-size resolution, call `lumen_layout::parse_image_set(url_str)` + `select_image_set_candidate` instead of the raw paint helper, to get the typed `ImageSetOption` with `mime_type` support.
  3. (Optional, HiDPI) Shell: pass real window scale factor into `build_display_list_ordered_dpr` instead of `1.0`.
- **Entry points:** `lumen-layout/src/style.rs` `parse_single_bg_layer`; `lumen-layout/src/image_set.rs` (typed API); `lumen-paint/src/display_list.rs` `emit_background_layer` (`// CSS: image-set`).
- **CSS comment location:** `display_list.rs` `emit_background_layer` `// CSS: image-set`.

### ~~`@media (prefers-color-scheme: dark)` visual restyle~~ — **ВЫПОЛНЕНО** (p2-dark-mode-visual, 2026-06-03)
`dark_mode` уже передаётся через весь каскад: `layout_measured_hyp(.., dark_mode)` → `compute_style` → `media_context_from_viewport(viewport, dark_mode)`. Shell форвардит `self.dark_mode`. Задача закрыта.

### ~~CSS 3D transforms — `perspective()` + 3D functions~~ — **ВЫПОЛНЕНО** (p2-css-3d-wiring, 2026-06-03)
`TransformFn` расширен 3D-вариантами; `establish_3d_rendering_context` подключён к `transform_style`; GPU depth buffer готов. Задача закрыта.

### `position: sticky` scroll-driven offset (P1 feature p1-sticky-layout)
- **Status:** `StickyBox`, `collect_sticky_boxes()`, `compute_sticky_offset()` implemented in `lumen-layout/src/lib.rs`. Layout treats sticky as normal flow; offset computed separately.
- **P4 task:**
  1. `top/right/bottom/left` are already parsed (style.rs) and stored in `ComputedStyle`. No new CSS parsing needed.
  2. After each re-layout, call `collect_sticky_boxes(root)` to get the list.
  3. At each scroll event, call `compute_sticky_offset(sticky, scroll_x, scroll_y, vp_w, vp_h)` per entry and apply the returned `(dx, dy)` as a paint-layer translate (or `TransformNode` offset in the property trees).
  4. Non-px insets (`em`, `%`) currently yield `None` — wire resolved-px values from `lay_out_block()` context if full support is needed (optional for Phase 3).
- **Entry point:** `lumen-layout/src/lib.rs` — `collect_sticky_boxes()` + `compute_sticky_offset()`
- **CSS comment location:** `box_tree.rs` after `Position::Relative` block (end of `lay_out_block`)

### ~~`writing-mode: vertical-rl / vertical-lr`~~ — **ВЫПОЛНЕНО** (dispatch уже готов)
`lay_out_vertical_block()` вызывается из `lay_out()` при `WritingMode::VerticalRl/Lr`. CSS-парсинг и dispatch готовы. Задача закрыта.
- **CSS comment location:** `box_tree.rs` at the writing-mode dispatch block.

### ~~::first-letter pseudo-element~~ — **ВЫПОЛНЕНО**
`apply_first_letter_pseudo()` реализована и вызывается из `lay_out()` (`box_tree.rs:2377, 2414`). Задача закрыта.

### ~~::first-line pseudo-element~~ — **ВЫПОЛНЕНО**
`apply_first_line_pseudo_styles()` реализована и вызывается. Задача закрыта.

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

### ~~`scrollbar-width` / `scrollbar-color`~~ — **ВЫПОЛНЕНО** (p2-scrollbar-width-color, 2026-06-03)
`DrawScrollbar` расширен `thumb_color`/`track_color`; emit читает поля стиля. Задача закрыта.

### CSS `color-mix()` function → [`docs/tasks/p4-color-mix-parsing.md`](docs/tasks/p4-color-mix-parsing.md) (P1 feature p1-color-mix, 2026-06-03)
- **Status:** Algorithm ready. `lumen_layout::mix_colors(space, c1, w1, c2, w2) -> [f32; 4]` in `crates/engine/layout/src/color_mix.rs`. Converts both input sRGB colors to the interpolation space, lerps (polar spaces use shortest-arc hue), converts result back to sRGB. Input/output: `[r, g, b, a]` each in `[0.0, 1.0]`. Supported spaces: `MixColorSpace::Srgb | SrgbLinear | Hsl | Hwb | Lab | Lch | Oklab | Oklch | XyzD65 | XyzD50`. `MixColorSpace::from_css(s)` parses the CSS identifier. 25 unit tests.
- **P4 task** (CSS Color L5 §10.2 `color-mix()`):
  1. In `parse_function_color()` (`style.rs:15030`), detect `"color-mix("` prefix before the existing `rgba(` chain (marked with `// CSS: color-mix()` comment).
  2. Parse the `color-mix(in <space>, <color1> [<pct>]?, <color2> [<pct>]?)` syntax:
     - Strip `color-mix(` prefix + `)` suffix.
     - Split by `,` to get: `in <space>`, `<color1> [<pct>]?`, `<color2> [<pct>]?`.
     - Call `MixColorSpace::from_css(space_token)` → `MixColorSpace`.
     - Parse `<color1>` via `parse_color()`, extract optional `<pct>` (percentage or fraction; default: 50%).
     - Parse `<color2>` similarly.
     - Normalize: if one percentage is given, the other = 100% - pct1. If neither given, both = 50%. Convert to fractions `w1, w2 ∈ [0, 1]`.
     - Call `mix_colors(space, c1.to_f32(), w1, c2.to_f32(), w2)` (use `Color::to_f32()` helper or inline `[r/255.0, g/255.0, b/255.0, a/255.0]`).
     - Convert result `[f32; 4]` back to `Color` via `[(r*255.0) as u8, ...]`.
  3. To support `color-mix()` in `CssColor` context (for `color: color-mix(...)`), extend `parse_css_color_legacy()` similarly.
  4. Add 3-4 CSS tests: `color-mix(in srgb, red, blue)` → `(128, 0, 128)`, `color-mix(in oklch, red 40%, blue)` → some saturated color, `color-mix(in hsl, red 100%, blue 0%)` → red.
- **Entry points:** `lumen-layout/src/style.rs:15030` — `parse_function_color` + `parse_css_color_legacy`; `lumen-layout/src/color_mix.rs` — `mix_colors` + `MixColorSpace`.
- **CSS comment location:** `style.rs:15030` `// CSS: color-mix()` comment.

### CSS Scroll-Driven Animations L1 — `ScrollTimeline` / `ViewTimeline` (P1 feature p1-scroll-driven-animations)
- **Status:** Algorithm ready. `ScrollTimeline`, `ViewTimeline`, `NamedScrollTimeline`, `NamedViewTimeline`, `ScrollAxis`, `Viewport` in `lumen-layout/src/scroll_timeline.rs`. Progress resolvers: `resolve_scroll_progress()` + `resolve_view_progress()`. Collection stubs: `collect_named_scroll_timelines()` + `collect_named_view_timelines()`. All exported from `lumen-layout`. 15 unit tests.
- **P4 task** (CSS Scroll-Driven Animations L1):
  1. Add `scroll_timeline_name: Option<String>` + `scroll_timeline_axis: ScrollAxis` to `ComputedStyle`. Parse `scroll-timeline-name` + `scroll-timeline-axis` in `apply_declaration()`. Wire to `collect_named_scroll_timelines()` — iterate layout tree, emit `NamedScrollTimeline` for each node with a non-`none` `scroll_timeline_name`.
  2. Add `view_timeline_name: Option<String>` + `view_timeline_axis: ScrollAxis` to `ComputedStyle`. Parse `view-timeline-name` + `view-timeline-axis`. Wire to `collect_named_view_timelines()`.
  3. Add `animation_timeline: AnimationTimeline` enum (`Auto | ScrollFn(ScrollTimeline) | ViewFn(ViewTimeline) | Named(String)`) to `ComputedStyle`. Parse `animation-timeline` (`auto`, `scroll()`, `view()`, `<custom-ident>`).
  4. In the animation scheduler (`AnimationScheduler` / shell tick loop): resolve `animation_timeline` to a progress fraction using `resolve_scroll_progress` / `resolve_view_progress`, then drive `CompositorAnimFrame` progress from it instead of wall-clock time.
- **Entry points:** `lumen-layout/src/scroll_timeline.rs` (all public API), `lumen-layout/src/style.rs` (ComputedStyle), `lumen-layout/src/animation.rs` (AnimationScheduler).

### CSS Anchor Positioning L1 — `anchor-name` / `position-anchor` / `inset-area` / `anchor()` (P1 feature p1-anchor-positioning, 2026-06-03)
- **Status:** Algorithm ready. `lumen_layout::collect_anchors(root) -> AnchorRegistry` (two-phase collect), `register_anchor(registry, name, node, rect)`, `resolve_anchor_function(registry, name, side, is_horizontal) -> Option<f32>`, `resolve_inset_area(registry, name, row, col, containing_rect) -> Option<AnchoredPosition>` in `lumen-layout/src/anchor.rs`. Types: `AnchorSide` (Top/Right/Bottom/Left/Center/Start/End/Percentage), `InsetAreaKeyword` (Start/Center/End/SpanStart/SpanEnd/SpanAll/SelfStart/SelfEnd/None), `AnchoredPosition { top, left, width, height }`, `AnchorEntry { node, rect }`, `AnchorRegistry { entries: HashMap<String, AnchorEntry> }`. 21 unit tests.
- **P4 task** (CSS Anchor Positioning L1 — <https://drafts.csswg.org/css-anchor-position-1/>):
  1. **`anchor-name`** (§2): Add `anchor_name: Option<String>` to `ComputedStyle`. Parse `anchor-name: --foo` in `apply_declaration()` (stores the raw custom-ident string). **Not inherited.** Wire in `collect_anchors_rec()` in `anchor.rs` — replace the current stub body with:
     ```rust
     if let Some(name) = &lb.style.anchor_name {
         register_anchor(registry, name.clone(), lb.node, lb.rect);
     }
     ```
     Then call `collect_anchors(root)` after layout in `box_tree.rs` before the positioned-layout pass (or as a separate post-pass). Store the result in a `&AnchorRegistry` passed down to `lay_out_absolute()`.
  2. **`position-anchor`** (§3): Add `position_anchor: Option<String>` to `ComputedStyle`. Parse `position-anchor: --foo` in `apply_declaration()`. **Not inherited.** Used in `lay_out_absolute()` to look up the default anchor.
  3. **`anchor()` function in inset values** (§3.1): When evaluating `top`/`right`/`bottom`/`left` for an absolutely-positioned element, if the value is an `anchor()` function token (detect `starts_with("anchor(")`), parse the anchor-element name + side, and call `resolve_anchor_function(registry, name, side, is_horizontal)` to get the px value. Substitute `auto` if `None`.
  4. **`inset-area`** (§5): Add `inset_area_row: InsetAreaKeyword` + `inset_area_col: InsetAreaKeyword` to `ComputedStyle` (both default `None`). Parse `inset-area: center span-all` etc. in `apply_declaration()`. In `lay_out_absolute()`, if both fields are not `None`, call `resolve_inset_area(registry, position_anchor_name, row, col, cb_rect) -> Option<AnchoredPosition>` and apply the returned `top`/`left`/`width`/`height` before the usual inset resolution.
  5. **`position-area`** is an alias for `inset-area` per the spec — parse identically.
- **Entry points:** `lumen-layout/src/anchor.rs` (all algorithm API), `lumen-layout/src/box_tree.rs` `lay_out_absolute()` (wire collect + resolve calls, marked `// CSS: anchor-name, position-anchor, inset-area, anchor()`).
- **CSS comment location:** `anchor.rs:collect_anchors_rec` body + `box_tree.rs` `lay_out_absolute()` (P4 adds `// CSS:` comment).

### CSS Motion Path L1 — `offset-path` / `offset-distance` / `offset-rotate` (P1 feature p1-motion-path, 2026-06-02)
- **Status:** Algorithm ready. `lumen_layout::resolve_motion_transform(path_str, offset_distance_px, rotate) -> Option<MotionTransform>` in `lumen-layout/src/motion_path.rs`. Parses `path("M…")` SVG path strings (all commands M/L/H/V/C/S/Q/T/A/Z, relative and absolute). Returns `MotionTransform { translate_x, translate_y, rotation_deg }`. `OffsetRotate::Auto` tracks tangent, `Reverse` = tangent+180°, `AutoAngle` = tangent+extra, `Angle(deg)` = fixed. Arc commands approximated as cubic Bézier via W3C endpoint→center parameterisation. 15 unit tests.
- **P4 task** (CSS Motion Path L1):
  1. `ComputedStyle` already has `offset_path: Option<String>`, `offset_distance: Length`, `offset_rotate: OffsetRotate` fields (style.rs). **No new CSS parsing needed.**
  2. In `property_trees.rs` `build_property_trees_rec()` at the `creates_transform(style)` branch (search `// CSS: offset-path` comment at `property_trees.rs:802`): after computing the CSS `transform` local matrix, if `style.offset_path.is_some()`, resolve `offset_distance` to px (percentage → fraction of `b.rect` diagonal), call `resolve_motion_transform(path_str, dist_px, style.offset_rotate)`, then compose the result into `local` as an additional `translate(tx, ty) rotate(deg)` pre-transform (multiply on the left).
  3. `offset-anchor` (default `auto` = object's transform-origin): if `style.offset_anchor != "auto"`, shift the element's origin by `(anchor_x - origin_x, anchor_y - origin_y)` before the translate. Can be a Phase 3+ refinement — `auto` covers 90% of real usage.
  4. Deferred path types: `url(#id)`, `ray(angle)`, `circle()`, `ellipse()` — `resolve_motion_transform` returns `None` for these; element stays at normal position.
- **Entry points:** `lumen-layout/src/motion_path.rs` — `resolve_motion_transform()` + `MotionTransform`; `lumen-layout/src/property_trees.rs:802` — `// CSS: offset-path` handoff comment.
- **CSS comment location:** `property_trees.rs` near line 802 (`// CSS: offset-path, offset-distance, offset-rotate, offset-anchor`).

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
| 2026-06-08 | `align-content` single-line flex | CSS Box Alignment L3; убран guard n_lines>1; flex-wrap:wrap с одной строкой теперь реагирует на flex-end/center/space-around/space-evenly; 2 новых unit-теста; TEST-65 ожидается улучшение 23.52%→~0% |
| 2026-06-08 | `object-fit` / `object-position` | CSS Images L3 §5.5; `compute_object_fit_transform()` в box_tree.rs; Fill fallback на SVG preserveAspectRatio; Contain/Cover/None/ScaleDown; object-position free-space фракции; 6 unit-тестов + graphic test 70 |
| 2026-06-08 | `border-spacing` | CSS 2.1 §17.6; `border_spacing_h/v: f32` в ComputedStyle (inherited); парсинг 1-/2-значного shorthand; h_spacing → compute_table_col_widths + lay_out_table_row (новый параметр); v_spacing → lay_out_table; 5 unit tests + graphic test 69 |
| 2026-06-08 | `list-style-type` custom ident | CSS Lists L3 §2.1; `ListStyleType::Custom(Box<str>)`; parse() → Custom для нераспознанных idents; build_list_marker_text() → format_counter_with_registry; 3 unit-теста + graphic test 32 |
| 2026-06-08 | `font-variation-settings` | CSS Fonts L4 §6.3; OwnedVariableFont in lumen-paint; char_width_varied() in TextMeasurer + MultiFontMeasurer; measure_text_w_varied() in box_tree.rs; 6 unit tests + graphic test 68 |
| 2026-06-08 | `attr()` typed | CSS Values L4 §7.7; find_attr_open() + expand_attr_val() in style.rs; unit-suffix/string/color types; fallback; 4 unit tests + graphic test 67 |
| 2026-06-08 | `::selection` | CSS Pseudo-elements L4 §5.6; SelectionHighlight struct; build_display_list_with_selection(); frag_selection_highlight() byte-proportional; 4 unit tests in style.rs; graphic test 66 |
| 2026-06-08 | `text-align-last` | CSS Text L3 §7.2; align_lines wired with 5th arg; 4 unit tests in box_tree.rs |
| 2026-06-08 | `color-mix()` | CSS Color L5 §10.2; parse_color_mix() + parse_color_with_pct() in style.rs; 3 unit tests |
| 2026-06-02 | `image-set()` / `cross-fade()` | CSS Images L4 §5/§4; BackgroundImage::CrossFade; 5 unit tests + graphic test 59; CPU snapshot 58+59 |
| 2026-06-02 | `::first-letter` / `::first-line` | CSS Pseudo-elements L4 §5.3-5.4; segment split + first_line_style; 4 unit tests + graphic test 58 |
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
