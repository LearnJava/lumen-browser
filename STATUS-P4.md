# STATUS-P4 — CSS Properties

**Developer:** Программист 4 (CSS implementation ONLY)

---

## In progress
_(none)_

## Workflow

1. **Check for "Needs wiring" section below** — P1/P2 algorithms ready for CSS connection
2. **Read CSS-SPECS.md** P4 Priority Queue for next property to implement
3. **Create branch:** `git checkout -b p4-<property-name>`, e.g. `p4-opacity-css`
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

Ordered by priority from CSS-SPECS.md P4 Work Queue. Pick the first unblocked item.

| # | Property / Feature | Effort | Blocker |
|---|-------------------|--------|---------|
| 1 | `transition` interpolation (per-frame lerp) | M | easing functions (linear() ✅ done) |
| 2 | `@keyframes` AnimationScheduler::tick wiring | L | transitions done (item 1) |
| 3 | CSS Nesting — nested rule parser | L | none |
| 4 | `position: sticky` layout + scroll listener | M | none |
| 5 | `list-style-type` marker rendering | S | none |
| 6 | `:is()` / `:where()` / `:has()` matching | M | none |
| 7 | `@media` resize hook re-evaluation | S | shell event |
| 8 | `filter` GPU offscreen pass | L | wgpu pipeline |
| 9 | `clip-path` basic shapes (inset/circle/ellipse/polygon) | M | none |
| 10 | `::marker` rendering | S | float/list (both ✅ done) |
| 11 | `@font-face` actual file loading | L | network/P3 |
| 12 | `min-content` / `max-content` / `fit-content` | L | layout engine |
| 13 | `overflow: scroll` scrollable containers | L | shell scroll |
| 14 | `column-rule` rendering | S | paint |
| 15 | Scroll snap shell integration | M | scroll event |
| 16 | `@container` 2nd-pass execution | L | container-type done |
| 17 | `backdrop-filter` GPU compositing pass | L | wgpu pipeline |
| 18 | `writing-mode: vertical-*` axis swap | L | layout engine |
| 19 | `subgrid` track inheritance | XL | grid engine |

---

## Needs wiring (algorithm ready, CSS not connected)

**P1/P2 have implemented the algorithm. P4 wires CSS property to it.**

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
