# STATUS-P4 — CSS Properties

**Developer:** Программист 4 (CSS implementation ONLY)

---

## In progress
_(none)_ — p4-system-colors влит 2026-06-13

## Workflow

1. **⚠️ СНАЧАЛА проверь секцию "Phase 2" в ## Next** — если есть незачёркнутые задачи (строки без ~~зачёркивания~~), брать их первыми. Не CSS-SPECS.md, не "Needs wiring". **Сейчас Phase 2 пуста — все задачи A–E выполнены.**
2. **Check for "Needs wiring" section below** — P1/P2 algorithms ready for CSS connection (только если Phase 2 пуста)
3. **Read CSS-SPECS.md** P4 Priority Queue for next property to implement (только если Phase 2 и Needs wiring пусты)
4. **Create branch:** `git checkout -b p4-<property-name>`, e.g. `p4-overflow-scroll`
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

> **⚠️ ОБЯЗАТЕЛЬНО:** Пока в этой секции есть незачёркнутые задачи — брать только их. Не CSS-SPECS.md, не "Needs wiring".

| # | Property / Feature | Effort | Blocker | Task file |
|---|-------------------|--------|---------|-----------|
| ~~**A**~~ | ~~**`:host` / `::slotted` (Shadow DOM)**~~ — **выполнено** (p4-host-slotted, 2026-06-10) | M | none | — |
| ~~**B**~~ | ~~**Find in page (Ctrl+F)**~~ — **выполнено** (P3 259b0c1d + regex f0e9f08d + scroll-to-match 62be2e83) | M | — | — |
| ~~**C**~~ | ~~**DevTools / Inspector Phase 0**~~ — **выполнено** (P2 f3cb196e + P3 0aaa77ec + d7d47800; DOM inspector + console + network panel) | L | — | — |
| ~~**D**~~ | ~~**`overflow: scroll` scrollable containers**~~ — **выполнено** (P2 ca59abfa scroll layer; P3 R-1 5a0b240a scroll events) | L | — | — |
| ~~**E**~~ | ~~**`ComputedStyle` JSON export** (lumen-plan §7E.2, P4-часть)~~ — **выполнено** (p4-computed-style-json, 2026-06-10); `computed_style_json` + `computed_style_json_by_selector` в lumen-layout, `InProcessSession::computed_style_json(selector)` в lumen-driver | S | — | — |

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

### ✅ CSS Color 4 system colors — **ВЫПОЛНЕНО** (p4-system-colors, 2026-06-13)
`SystemColor` Copy enum (23 variants); `CssColor::System(SystemColor)` variant; `parse_css_color_legacy` детектирует системные ключевые слова; color-scheme pre-pass в `compute_style()` + `resolve_system_colors_in_style()` post-pass для CssColor-полей; `dark_mode: bool` param в `apply_declaration()` для `color: Color` поля; 7 unit-тестов + graphic test 92.

### `masonry-auto-flow` / `align-tracks` / `justify-tracks` (P1 feature p1-masonry-layout, 2026-06-10)
- **Status:** Full masonry layout algorithm ready in `lumen-layout/src/masonry.rs`.
  - `GridTrackSize::Masonry` variant added to enum (`style.rs:3630`).
  - `parse_track_list("masonry", ...)` returns `vec![GridTrackSize::Masonry]` sentinel.
  - `lay_out_grid` in `box_tree.rs` detects masonry axis and dispatches to inline waterfall algorithm.
  - Greedy waterfall placement: each item goes into the track with minimum running height.
  - `masonry::min_track_idx` helper exposed as `pub` for P4 potential reuse.
  - 7 unit tests pass. Clippy clean.
- **P4 task:**
  1. Add `masonry_auto_flow: MasonryAutoFlow` to `ComputedStyle` in `lumen-layout/src/style.rs`
     (non-inherited, default = `DefiniteFirst`; values: `DefiniteFirst | Next | Ordered`).
  2. Parse `masonry-auto-flow` in `apply_declaration()`.
  3. In `lay_out_grid` masonry dispatch (around `box_tree.rs:5623`), use `masonry_auto_flow` to
     control item ordering: `DefiniteFirst` → items with explicit track first; `Next` → source order;
     `Ordered` → reverse source order.
  4. (Optional) Add `align-tracks` / `justify-tracks` to `ComputedStyle` for cross-axis alignment.
- **Entry points:** `lumen-layout/src/masonry.rs` — `lay_out_masonry`, `min_track_idx`;
  `lumen-layout/src/box_tree.rs:5623` — inline masonry dispatch block (`// CSS: masonry-auto-flow`).
- **CSS comment location:** `box_tree.rs` at masonry dispatch: `// CSS: masonry-auto-flow`.

### `field-sizing: content` form control auto-sizing (P1 feature p1-bb9-field-sizing, 2026-06-13)
- **Status:** Measurement algorithm ready: `field_sizing_content_intrinsic(tag, value_text, font_size_px, line_height_px, m) -> (f32, f32)` in `lumen-layout/src/field_sizing.rs`.
  Computes padding-box size from text content for `"input"` and `"textarea"`. 5 unit tests pass.
- **P4 task:**
  1. Add `FieldSizing` enum (`Fixed` | `Content`, default `Fixed`) and `pub field_sizing: FieldSizing` to `ComputedStyle` in `style.rs`.
  2. Parse `field-sizing: fixed | content` in `apply_declaration()`.
  3. In `apply_ua_form_controls`, skip assigning `style.width` / `style.height` for `"input"` / `"textarea"` when `style.field_sizing == FieldSizing::Content` (leave them `None` so layout picks up content size).
  4. In `lay_out_box` at the `is_replaced && s.width.is_none()` branch (box_tree.rs, `// CSS: field-sizing` comment), call `lumen_layout::field_sizing_content_intrinsic(tag, value_text, s.font_size, resolved_lh, measurer)` and assign `(padding_w + border_widths)` as `b.rect.width`; do same for height.
- **Entry points:** `lumen-layout/src/field_sizing.rs:field_sizing_content_intrinsic`;
  `lumen-layout/src/box_tree.rs` at `// CSS: field-sizing` comment (replaced-element width branch);
  `lumen-layout/src/style.rs:apply_ua_form_controls`.

### ✅ `@starting-style` entry transitions — **ВЫПОЛНЕНО** (p4-starting-style, 2026-06-10)
`compute_style_from_declarations()` в `style.rs`; `StartingStyleTracker` + wiring в shell `relayout()` — для новых нод (не в `prev_styles`) матчит `@starting-style` и вызывает `sync` с starting-style как `old`. 4 unit-теста + graphic test 71.

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

### ✅ `anchor-name` / `position-anchor` / `inset-area` — **ВЫПОЛНЕНО** (p4-anchor-positioning, 2026-06-10)
ComputedStyle.anchor_name/position_anchor/inset_area_row/col; parse_inset_area_keyword (9 keywords + physical aliases); collect_anchors_rec wired; apply_anchor_positions() post-layout pass; 7 unit-тестов + graphic test 77.

### ~~`list-style-type` (custom counter-style)~~ — **ВЫПОЛНЕНО** (p4-list-style-type-custom, 2026-06-08)
`ListStyleType::Custom(Box<str>)` добавлен; `parse()` возвращает `Custom` для нераспознанных idents; `build_list_marker_text()` резолвит через `format_counter_with_registry`; shorthand-парсер исправлен (position до type). 3 unit-теста + graphic test 32.

### ✅ `gap-rule-width`, `gap-rule-style`, `gap-rule-color` — **ВЫПОЛНЕНО** (p4-gap-rule, 2026-06-10)
`gap_rule_*` поля в ComputedStyle; shorthand+longhands в apply_declaration; `collect_gap_segments()` + `emit_gap_rules()` в display_list.rs walk(); 5 unit-тестов + graphic test 73.

### ✅ `font-stretch` — **ВЫПОЛНЕНО** (p4-font-stretch, 2026-06-10)
`FontStretch::NORMAL` (1000) → без инжекции wdth. Не-нормальный stretch → `wdth = stretch.0/10.0` добавляется в `font_variation_axes` в 4 местах DrawText (text frags, ellipsis, text-shadow, emphasis-marks). Explicit wdth из font-variation-settings не перезаписывается. 5 unit-тестов + graphic test 74.

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

### ✅ CSS Scroll-Driven Animations L1 — **ВЫПОЛНЕНО** (p4-scroll-driven-animations, 2026-06-10)
- ComputedStyle: `scroll_timeline_name/axis`, `view_timeline_name/axis`, `animation_timelines: Vec<AnimationTimeline>`
- `AnimationTimeline` enum: `Auto | Scroll{axis, nearest} | View{axis} | Named(String)`
- Shorthands: `scroll-timeline`, `view-timeline` в apply_declaration
- `parse_scroll_axis()`, `parse_animation_timeline_list()`, `parse_scroll_fn()`, `parse_view_fn()`
- `collect_named_scroll_timelines()` + `collect_named_view_timelines()` — полный walk layout tree
- SUPPORTED_PROPERTIES: animation-timeline, scroll-timeline{,-name,-axis}, view-timeline{,-name,-axis}
- 12 unit-тестов (8 CSS parsing + 4 collect); graphic test 78
- Шаг 4 (shell scheduler wiring) — деферировано P3/shell

### ✅ CSS Anchor Positioning L1 — **ВЫПОЛНЕНО** (p4-anchor-positioning, 2026-06-10)
`anchor-name`/`position-anchor`/`inset-area`/`position-area` реализованы. `anchor()` в inset-values — Phase 3+ (требует новый вариант LengthOrAuto::AnchorFn).

### ✅ CSS Motion Path L1 — `offset-path` / `offset-distance` / `offset-rotate` — **ВЫПОЛНЕНО** (p4-motion-path, 2026-06-10)
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
| 2026-06-13 | CSS Color 4 system color keywords | CSS Color 4 §6.2; `SystemColor` Copy enum (23 variants); `CssColor::System(SystemColor)`; `parse_css_color_legacy` детектирует ключевые слова; color-scheme pre-pass + `resolve_system_colors_in_style()` post-pass; `dark_mode: bool` в `apply_declaration()`; 7 unit-тестов + graphic test 92 |
| 2026-06-13 | relative color syntax | CSS Color L5 §4; `rgb/hsl/oklch/oklab/lab/lch(from <origin> c1 c2 c3 [/ a])`; `parse_relative_color()` в style.rs резолвит channel keywords (r/g/b, h/s/l, l/c/h, l/a/b, alpha) через новый `color_mix::relative_origin_channels()`; компоненты поддерживают число/процент/угол/`calc()` (mini-evaluator с +−*/ и скобками); результат реконструируется в обычную color-функцию и переразбирается; CSS Color L5 модуль → ✅; 7 unit-тестов style.rs + graphic test 91 |
| 2026-06-10 | `ComputedStyle` JSON export (DevTools) | lumen-plan §7E.2 (P4-часть); `computed_style_json(&ComputedStyle) -> String` + `computed_style_json_by_selector()` в lumen-layout (детерминированный JSON, отсортированные ключи, ~70 свойств, dependency-free escaping); `InProcessSession::computed_style_json(selector)` в lumen-driver; 5 unit-тестов (layout) + 2 unit-теста (driver); не CSS-свойство — graphic test неприменим |
| 2026-06-10 | `view-transition-name` | CSS View Transitions L1 §10; `ComputedStyle.view_transition_name: Option<Box<str>>` (non-inherited, default None); parse «none»→None, ident→Some; `collect_view_transition_names()` в lib.rs — возвращает [(NodeId, name)] для shell; SUPPORTED_PROPERTIES +1; 5 unit-тестов style.rs + 4 unit-теста lib.rs; graphic test 81 |
| 2026-06-10 | `border-collapse` | CSS Tables L2 §17.6; `BorderCollapse` enum в style.rs; `ComputedStyle.border_collapse` (inherited, default Separate); collapse → spacing=0 в lay_out_table + compute_table_col_widths; `TableContext::from_box()` читает реальные CSS-значения; 5 unit-тестов + graphic test 80 |
| 2026-06-10 | `text-underline-offset` + `text-underline-position` wiring | CSS Text Decoration L4 §5.1/§5.3; `text_underline_offset: Option<f32>` в ComputedStyle; парсинг auto/px/em; wired в push_text_decoration() — Under→fs*0.25; offset добавляется к base; 5 unit-тестов + graphic test 79 |
| 2026-06-10 | `scroll-timeline-name/axis`, `view-timeline-name/axis`, `animation-timeline` | CSS Scroll-Driven Animations L1; `AnimationTimeline` enum (Auto/Scroll/View/Named); `collect_named_scroll_timelines/view_timelines()` полный walk; SUPPORTED_PROPERTIES +7; 12 unit-тестов + graphic test 78 |
| 2026-06-10 | `anchor-name` / `position-anchor` / `inset-area` | CSS Anchor Positioning L1; ComputedStyle 4 fields; parse_inset_area_keyword (9 logical kw + physical aliases); collect_anchors_rec wired; apply_anchor_positions() post-layout pass in box_tree.rs; position-area alias; 7 unit-тестов + graphic test 77 |
| 2026-06-10 | `offset-path` / `offset-distance` / `offset-rotate` | CSS Motion Path L1; forward_box_transform() + PropertyTrees::walk() wiring; resolve_motion_transform() composed before CSS transform; creates_transform() extended; 4 unit-тесты + graphic test 76 |
| 2026-06-10 | `masonry-auto-flow` | CSS Masonry Layout §9; `MasonryAutoFlow` enum (DefiniteFirst\|Next\|Ordered); `sorted_idxs` в masonry dispatch lay_out_grid; Ordered сортирует по CSS `order`; DefiniteFirst ставит grid-positioned items первыми; 10 unit-тестов + graphic test 75 |
| 2026-06-10 | `font-stretch` | CSS Fonts L4 §5.2; wdth axis injection в 4 местах DrawText; FontStretch.0/10.0 = wdth %; explicit wdth не перезаписывается; 5 unit-тестов + graphic test 74 |
| 2026-06-10 | `gap-rule-width/style/color` | CSS Gap Decorations L1; `gap_rule_*` в ComputedStyle (non-inherited); shorthand+longhands в apply_declaration; `collect_gap_segments()` + `emit_gap_rules()` в display_list.rs walk(); 5 unit-тестов + graphic test 73 |
| 2026-06-10 | `:host` / `::slotted` Shadow DOM | CSS Scoping L1 §6.1-6.2; `PseudoClass::Host` в `matches_pseudo_class`; `is_slotted_element()` + `matches_slotted_complex()` + cascade wiring в `compute_style`; 6 unit-тестов + graphic test 72 |
| 2026-06-10 | `@starting-style` entry transitions | CSS Transitions L2 §3.4; `compute_style_from_declarations()` в style.rs; `StartingStyleTracker` + shell `relayout()` — новые ноды матчатся через `resolve_starting_style`; `sync` вызывается с starting-style как `old`; 4 unit-теста + graphic test 71 |
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
