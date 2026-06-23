# P2-masonry — CSS Masonry full spec (masonry-auto-flow)

**Developer:** P1 (with P4 handoff for the `masonry-auto-flow` CSS property)
**Branch:** `p1-p2-masonry-full`
**Size:** L
**Crates:** `lumen-layout` (`box_tree.rs`, `masonry.rs`, `style.rs`), `lumen-css-parser` (P4 handoff)

## Goal

Implement a genuine CSS Masonry layout per **CSS Grid Layout Level 3 §14** (waterfall
placement): the masonry axis packs items greedily into the shortest running track while
the perpendicular ("grid") axis sizes tracks normally, with `masonry-auto-flow`
(`definite-first | next | ordered`) controlling placement order. This re-introduces a
real masonry placement path that F2-3 deliberately removed, **without breaking the
Edge-parity grid-fallback path** that F2-3 established. Because no stable browser ships
masonry, the real path is **spec-ahead** and must be opt-in / separately validated, not
gated by the existing Edge graphic-test comparison.

## Critical context / trade-off (read first)

F2-3 (✅ 2026-06-22) discovered that **Edge does not support CSS masonry in any form**:
`display: masonry` and `grid-template-rows/columns: masonry` are invalid values that
Edge/Chrome silently drop, so the axis falls back to `none` (a regular auto-sized grid),
or authors fall back to multicol. F2-3 made Lumen **match that ground truth**: it strips
the `masonry` sentinel from the effective track list and falls through to the normal grid
placement algorithm. TEST-63 and TEST-75 now PASS against the Edge fallback.

**The central tension:** real masonry produces a *different* visual result than Edge's
grid fallback. The moment Lumen renders a true waterfall for `grid-template-rows: masonry`,
its output **diverges from the Edge baseline** that F2-3 locked in. Therefore:

- Real masonry **cannot be gated by `graphic_tests/run.py` TEST-63/75** (0.5% vs Edge). If
  real masonry is rendered for those pages, the diff against Edge will *grow*, not shrink —
  that is expected and correct, not a regression.
- **Do not** "fix" this by editing the test pages, raising thresholds, or adding a
  KNOWN_DEBTOR entry that pretends Edge is the reference. Edge is the wrong reference for a
  feature Edge does not have.

**Validation strategy for this task:**

1. **Primary gate = dedicated `lumen-layout` unit tests** asserting waterfall geometry
   (shortest-track placement, `masonry-auto-flow` order, gaps, grid-axis track sizing).
   These are deterministic and need no browser.
2. **Manual visual check against a masonry-supporting reference** — Firefox (where masonry
   is behind `layout.css.grid-template-masonry-value.enabled`) or Safari Technology Preview,
   **not Edge**. Screenshot comparison is informational only.
3. **Keep the Edge-parity path the default** so the existing TEST-63/75 gate stays green.
   Real masonry must be reachable only via an explicit opt-in (see Steps) so a normal
   `graphic_tests` run still measures Lumen against Edge's fallback and passes.

## Current state

Most of the machinery already exists from an earlier (now-orphaned) masonry attempt; F2-3
cut the *dispatch* but left the algorithm and the CSS plumbing in place.

**Algorithm (present, but unreachable):**
- `crates/engine/layout/src/masonry.rs` — `lay_out_masonry(container_w, gap, children, track_count) -> f32`
  (greedy shortest-track waterfall) + `min_track_idx()`. Fully unit-tested
  (`masonry.rs:99-161`). **Currently dead code**: nothing in `box_tree.rs` calls it.
- `crates/engine/layout/src/lib.rs:44` — `pub mod masonry;` (module is wired into the crate).

**Track-list sentinel (present):**
- `crates/engine/layout/src/style.rs:4282-4287` — `GridTrackSize::Masonry` variant
  (stored as `vec![GridTrackSize::Masonry]` in `grid_template_columns` / `grid_template_rows`).
- `style.rs:4316-4319` — `GridTrackSize::is_masonry()`.
- `style.rs:4362-4372` — `parse_track_list`: `grid-template-*: masonry` → `vec![Masonry]`.
- `style.rs:4296` — `Masonry` returns `None` from the fixed-size resolver (caller handles it).

**`masonry-auto-flow` (parsed, but unused in layout):**
- `style.rs:4518-4542` — `MasonryAutoFlow` enum (`DefiniteFirst | Next | Ordered`) + `parse()`.
- `style.rs:2806-2808` — `ComputedStyle.masonry_auto_flow` field (non-inherited, default `DefiniteFirst`).
- `style.rs:11307-11311` — `apply_declaration` for `"masonry-auto-flow"`.
- `style.rs:5219`, `5549` — defaults in `root()` / test style.
- `style.rs:28955-29016` — parse + non-inheritance unit tests.
- **No layout consumer:** `masonry_auto_flow` is never read in `box_tree.rs`.

**The F2-3 fallback (the lines this task must change):**
- `crates/engine/layout/src/box_tree.rs:7910-7918` — inside `lay_out_grid` (signature at
  `box_tree.rs:7847`). Detects masonry on either axis, then **strips it to an empty track
  list** so the rest of the grid algorithm runs as a plain auto grid:
  ```rust
  let col_is_masonry = eff_col_template.first() == Some(&GridTrackSize::Masonry);
  let row_is_masonry = s.grid_template_rows.first() == Some(&GridTrackSize::Masonry);
  let eff_col_template: &[GridTrackSize] = if col_is_masonry { &[] } else { eff_col_template };
  let eff_row_template: &[GridTrackSize] = if row_is_masonry { &[] } else { &s.grid_template_rows };
  ```
  There is no longer any dispatch to `lay_out_masonry` here — F2-3 removed it.
- `box_tree.rs:14439-14509` — two integration tests locking the Edge-parity fallback:
  `grid_masonry_fallback_respects_order` and `grid_masonry_fallback_source_order`
  (helper `masonry_grid_children` at `box_tree.rs:14444`). **These must keep passing** —
  they encode the default behaviour, not a bug.

**Not present:**
- No `Display::Masonry` variant (`style.rs:89-127`); masonry is only expressible via
  `grid-template-*: masonry`, matching the spec's grid-integrated form. No need to add a
  `display` value.
- No `align-tracks` / `justify-tracks` (CSS Masonry §10) — out of scope here.

**Dispatch site:** `lay_out_grid` is called from `box_tree.rs:5257-5261`
(`Display::Grid | Display::InlineGrid`). Related single-axis packing reference:
`balanced_column_height` at `box_tree.rs:6941` (binary-search column balancer used by
multicol — useful intuition for track packing, not directly reused).

## Cross-team boundary (P4)

The CSS-parsing surface for masonry is already implemented and lives in P4 territory; this
task should **consume** it, not re-implement it. If any parsing gap is found:

- `masonry-auto-flow` parsing + `ComputedStyle` field = **P4** (already done at
  `style.rs:4518`, `2806`, `11307`).
- `grid-template-rows/columns: masonry` track parsing = **P4** (already done at
  `style.rs:4362`).
- This task adds the **layout consumer** of those values (P1 work). Where the layout path
  needs a CSS knob that is not yet parsed (e.g. `align-tracks` / `justify-tracks` if you
  decide to honour them), add a `// CSS: align-tracks, justify-tracks` comment at the call
  site in `box_tree.rs` and file it under "Needs wiring" in `STATUS-P4.md` — do **not**
  add the field to `ComputedStyle` yourself.

## Entry points

- `crates/engine/layout/src/box_tree.rs:7847` — `fn lay_out_grid(...)` — the grid container
  layout entry; masonry detection currently lives inside it.
- `crates/engine/layout/src/box_tree.rs:7910-7918` — the F2-3 masonry-strip fallback that
  must be made conditional (keep as default; branch to real masonry under opt-in).
- `crates/engine/layout/src/box_tree.rs:5257-5261` — the `Display::Grid` dispatch into
  `lay_out_grid`.
- `crates/engine/layout/src/masonry.rs:33` — `pub fn lay_out_masonry(...)` — the existing
  greedy waterfall to revive (extend for grid-axis track sizing + `masonry-auto-flow`).
- `crates/engine/layout/src/masonry.rs:64` — `pub fn min_track_idx(...)`.
- `crates/engine/layout/src/style.rs:4286` — `GridTrackSize::Masonry` sentinel.
- `crates/engine/layout/src/style.rs:2808` — `ComputedStyle.masonry_auto_flow`.
- `crates/engine/layout/src/box_tree.rs:14444-14509` — Edge-parity fallback tests
  (must stay green).

## Steps

1. **Decide the opt-in mechanism (gate first, code second).** Real masonry must NOT become
   the default for `grid-template-*: masonry`, or TEST-63/75 break. Pick one and document it
   in the module doc of `masonry.rs`:
   - **(preferred)** an env / build flag, e.g. `LUMEN_REAL_MASONRY=1` read once into a
     `OnceLock<bool>` in `box_tree.rs`. Default off → Edge-parity fallback runs → graphic
     tests stay green. On → real waterfall.
   - or a non-standard opt-in property the test pages never use. Avoid inventing CSS that
     could collide with the spec.

2. **Make the F2-3 strip conditional.** At `box_tree.rs:7910-7918`, keep the
   sentinel detection (`col_is_masonry` / `row_is_masonry`) but, when the opt-in is active
   **and** exactly one axis is masonry, branch to a new `lay_out_grid_masonry` path instead
   of stripping to `&[]`. When the opt-in is off, behaviour is byte-for-byte the current
   fallback.

3. **Add `lay_out_grid_masonry` in `box_tree.rs`** (a sibling of `lay_out_grid`, or a guarded
   branch within it). Responsibilities:
   - Identify the **grid axis** (the non-masonry axis) and size its tracks with the existing
     grid track-sizing logic (reuse the `eff_*_template` resolution + `fr`/`auto`/`fixed`
     sizing already in `lay_out_grid`). The number of grid-axis tracks = `track_count`.
   - Identify the **masonry axis** (the axis carrying `GridTrackSize::Masonry`) — items pack
     waterfall-style along it.
   - Order items per `s.masonry_auto_flow` (`style.rs:2808`):
     `DefiniteFirst` → place items with a definite grid-axis line first, then auto items in
     source order; `Next` → strict source order; `Ordered` → sort by CSS `order` (note
     `item_idxs.sort_by_key(... .style.order)` already exists at `box_tree.rs:7876`).
   - Lay out each item once to get intrinsic size, then call the waterfall placer.

4. **Generalise `masonry.rs::lay_out_masonry` for the grid-axis dimension.** Today it
   assumes column tracks of equal width and packs vertically (`masonry.rs:33-58`). Extend it
   (or add `lay_out_masonry_tracks`) to accept **per-track grid-axis sizes/offsets** (so it
   works when the grid axis uses `fr`/`auto`/fixed, not just equal columns) and to support
   **row-masonry** (masonry axis = horizontal) as well as column-masonry. Keep
   `min_track_idx` as the shortest-track selector. Preserve the existing public signature or
   deprecate it cleanly so current unit tests still compile.

5. **Honour gaps and container height.** Use `col_gap` / `row_gap` already resolved at
   `box_tree.rs:7884-7889`. The container's masonry-axis size = max running track length
   minus the trailing gap (as `lay_out_masonry` already returns). Set `b.rect.height`
   accordingly in the masonry branch (mirror the height handling at `box_tree.rs:5262`).

6. **Leave the fallback tests untouched.** `grid_masonry_fallback_*`
   (`box_tree.rs:14459`, `14485`) describe the default (opt-in off). Do not modify or delete
   them — they are the Edge-parity contract.

7. **Wire `masonry-auto-flow` end-to-end.** It is parsed but unused; the real path must read
   `s.masonry_auto_flow`. Add a unit test proving each variant changes placement.

8. **(Optional, file for P4 if pursued)** If you want track alignment, add
   `// CSS: align-tracks, justify-tracks` at the placement call and note it in `STATUS-P4.md`.
   Do not add the `ComputedStyle` fields yourself.

## Tests / verification

**Primary (deterministic, no browser) — these are the real gate:**

- Extend `crates/engine/layout/src/masonry.rs` tests (`masonry.rs:73-162`) for the new
  generalised placer: per-track grid-axis offsets, row-masonry orientation, non-equal track
  sizes.
- Add a new `#[cfg(test)]` block in `box_tree.rs` (kept **separate** from the
  `grid_masonry_fallback_*` tests) that drives the opt-in path and asserts true waterfall
  geometry. Use a helper mirroring `masonry_grid_children` (`box_tree.rs:14444`) but with the
  opt-in enabled. Assert, e.g.:
  - 3 columns, items of unequal height → the 4th item lands in the **shortest** column
    (verify `rect.x` and `rect.y`), not in source-order row position.
  - `masonry-auto-flow: ordered` reorders placement by `order`.
  - column gaps reflected in `rect.x`; container height = tallest track minus trailing gap.
- Run: `export PATH="/c/Users/konstantin/.cargo/bin:$PATH"`
  then `cargo test -p lumen-layout masonry`
  and `cargo clippy -p lumen-layout --all-targets -- -D warnings`.

**Edge-parity regression (must NOT change):**

- `cargo test -p lumen-layout grid_masonry_fallback` — both fallback tests stay green with
  the opt-in off.
- `python graphic_tests/run.py --only 63` and `--only 75` — must still PASS against Edge
  with the opt-in off (the default). If either fails, the opt-in leaked into the default
  path — fix that, do not touch the test or threshold.

**Spec-ahead visual check (informational only):**

- Build with the opt-in on, render a masonry test page, and compare against **Firefox**
  (masonry pref enabled) or **Safari Technology Preview** — NOT Edge. This is a sanity check,
  not a gate; no screenshots are committed.

## Definition of done

- [ ] Opt-in mechanism implemented and documented in `masonry.rs` module docs; default OFF.
- [ ] `box_tree.rs:7910-7918` strip is conditional: opt-in OFF → identical to F2-3 fallback.
- [ ] Real waterfall path (`lay_out_grid_masonry` / guarded branch) sizes the grid axis with
      existing track-sizing and packs the masonry axis shortest-track-first.
- [ ] `masonry.rs::lay_out_masonry` generalised for per-track grid-axis sizes and both
      orientations (row- and column-masonry); existing unit tests still pass.
- [ ] `masonry-auto-flow` (`DefiniteFirst | Next | Ordered`) read from `ComputedStyle` and
      affecting placement, with a unit test per variant.
- [ ] New, separate layout unit tests assert true waterfall geometry (shortest-track, gaps,
      container height) under the opt-in.
- [ ] `grid_masonry_fallback_respects_order` / `grid_masonry_fallback_source_order` unchanged
      and green.
- [ ] `graphic_tests` TEST-63 and TEST-75 still PASS against Edge with opt-in OFF (no test
      edits, no threshold changes, no new KNOWN_DEBTOR).
- [ ] `cargo clippy -p lumen-layout --all-targets -- -D warnings` clean;
      `cargo test -p lumen-layout` green.
- [ ] No `// CSS:` handoff left dangling; if `align-tracks`/`justify-tracks` were touched,
      a "Needs wiring" entry exists in `STATUS-P4.md`.
- [ ] `CAPABILITIES.md` updated to note masonry is implemented as a **spec-ahead, opt-in**
      feature (Edge-divergent; not gated by the Edge comparison).
