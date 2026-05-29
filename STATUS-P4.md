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
| 5 | `perspective()` + `transform-style: preserve-3d` (3D Transforms L2) | L | P2 wgpu 3D pipeline |
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
