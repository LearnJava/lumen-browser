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
| 15 | `writing-mode: vertical-*` axis swap | L | layout engine |
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
