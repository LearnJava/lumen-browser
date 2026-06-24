# Ph3 — Variable fonts (axes runtime)

**Developer:** P2 · **Branch:** `p2-ph3-variable-fonts` · **Size:** L · **Crates:** `lumen-font`, `lumen-paint` (femtovg backend), `lumen-css-parser` (P4 handoff for `font-variation-settings`)

---

## Status

Phase 3 (v1.0) future roadmap item. Source: `docs/plan/phases.md:131` —
**"Variable fonts axes runtime [P2] — font-variation-settings"**.

> **IMPORTANT — most of this is already built.** During research (2026-06-22)
> the variable-font runtime was found to be largely implemented across
> `lumen-font` (table parsers + outline interpolation), `lumen-layout`
> (CSS parsing + cascade + measurement), and `lumen-paint` (wgpu renderer +
> CPU raster path). The remaining real gap is the **femtovg backend (the
> default on-screen window path)**, which renders text natively via femtovg
> and ignores variation coordinates entirely. This task is therefore *not*
> a greenfield implementation but **closing the femtovg gap + verification +
> hardening**, with the spec/architecture below kept for completeness.

---

## Goal

Support OpenType variable fonts (`fvar`/`gvar`/`avar` + the metrics-variation
tables `HVAR`/`VVAR`/`MVAR`) driven by CSS `font-variation-settings` and the
registered axes (`wght`/`wdth`/`slnt`/`ital`/`opsz`), so that a single variable
font file renders at any point in its design space **in every rasterization
path, including the default on-screen window**. Also map the high-level CSS
font properties (`font-weight` → `wght`, `font-stretch` → `wdth`,
`font-style: oblique <angle>` → `slnt`, `font-optical-sizing` → `opsz`) to the
corresponding registered axes per CSS Fonts L4 §6.

---

## Current state (real file:line)

### Font-table parsing (`lumen-font`) — DONE

- `crates/engine/font/src/fvar.rs` — `Fvar`, `VariationAxis`, `NamedInstance`
  fully parsed (axes min/default/max, flags, named instances with/without
  PostScript id). `Fvar::axis(tag)`, `Fvar::is_variable()`,
  `Fvar::instance_by_name_id()`.
- `crates/engine/font/src/gvar.rs` — `Gvar`, `GlyphVariationData`,
  `TupleVariation`, `PointNumbers`, `tuple_scalar()` (tent function over
  peak/intermediate).
- `crates/engine/font/src/avar.rs` — `Avar::normalize(axis_idx, value)`
  piecewise-linear segment-map remapping.
- `crates/engine/font/src/hvar.rs`, `vvar.rs`, `mvar.rs` — per-glyph and
  global metric variation deltas. Shared `ItemVariationStore`
  (`item_variation.rs`) + `DeltaSetIndexMap` (`delta_set_index_map.rs`).
- `crates/engine/font/src/lib.rs:46-84` — all of the above re-exported.

### Glyph-outline interpolation (`lumen-font`) — DONE

- `crates/engine/font/src/variation.rs:80` —
  `apply_variations_to_simple_outline(contours, variations, coords)` applies
  scaled gvar deltas with IUP (Interpolation of Untouched Points), per axis.
- `crates/engine/font/src/face.rs:369` —
  `Font::glyph_resolved_with_coords(glyph_id, coords)` — variable-fonts
  variant of `glyph_resolved`; caches `gvar`, recurses composites (depth 8).
  Outline reading itself: `Font::glyf()` (`face.rs:169`),
  `Outline::Simple(contours)` / `Outline::Composite` (`glyf.rs`).
  **Known limit (documented at `face.rs:357-365`):** component-level gvar
  variations (varying composite-component anchor/origin) are *not* applied —
  only base-glyph stroke thickness varies; composite positioning stays at the
  default instance. CFF2 (variable PostScript outlines) deferred (`face.rs:374`).
- `crates/engine/font/src/variation_coords.rs:45` —
  `VariationCoords::from_css_settings(fvar, avar, css_settings)` normalizes
  user-space axis values → `[-1,1]` relative to default, applies avar.

### CSS property (P4 territory) — DONE

- Parsing: `crates/engine/layout/src/style.rs:9429`
  `parse_font_variation_settings()`; applied at `style.rs:11474`;
  `font-optical-sizing` at `style.rs:11479`.
- `ComputedStyle` fields: `style.rs:2294` `font_variation_settings:
  Vec<FontVariationSetting>`, `style.rs:2297` `font_optical_sizing:
  FontOpticalSizing`. Both **inherited** (cascade `style.rs:5324`, `6457`,
  reset `style.rs:14566`).
- `@font-face` descriptor: `crates/engine/css-parser/src/parser.rs:1118`
  `variation_settings: Option<String>` (parsed `parser.rs:2479`).
- Property registered in `crates/engine/css-parser/src/lib.rs:159,166`.

### Layout measurement — DONE

- `crates/engine/layout/src/box_tree.rs:8656`
  `measure_text_w_varied(...)` — line-wrap width honouring
  `font_variation_settings` (HVAR advance deltas). Used at `box_tree.rs:8960,
  9052, 9077, 9097, 9130, 9181`.

### Paint — display list + wgpu renderer + CPU raster — DONE

- Display list carries axes: `crates/engine/paint/src/display_list.rs:394`
  `font_variation_axes: Vec<([u8;4], f32)>`; populated from
  `font_variation_settings` + opsz injection at `display_list.rs:2229-2240,
  2368, 2913, 4612`.
- wgpu renderer: `crates/engine/paint/src/renderer.rs:7322`
  `push_text_glyphs(..., font_variation_axes, ...)`; normalization
  `normalize_variation_axes()` (`renderer.rs:7305-7308`) → cached per face_id
  (`renderer.rs:7362-7364`) → `ensure_glyph(..., coords)` →
  `glyph_resolved_with_coords`.
- CPU raster path: `crates/engine/paint/src/cpu_raster.rs` consumes the same
  display-list field.
- Glyph atlas is variation-aware (key includes coords):
  `crates/engine/paint/src/atlas.rs:19`.

### Text-path fork (memory `text_render_paths_fork`) — THE GAP

Two engines rasterize text:

1. **femtovg backend** — `crates/engine/paint/src/backends/femtovg_backend.rs`,
   the **default on-screen window** path (memory `femtovg_default_backend`).
   `draw_text()` (`femtovg_backend.rs:1212`) calls
   `self.canvas.fill_text(...)` — femtovg shapes and rasterizes the glyphs
   itself from a `femtovg::FontId` (`femtovg_backend.rs:314`). The
   `DrawText` match arm (`femtovg_backend.rs:1996`) **destructures away**
   `font_variation_axes` (`..`) and never passes axis coordinates to femtovg.
   **Result: variable fonts render at their default instance in the actual
   browser window.** This is the primary deliverable of the task.
2. **lumen-font (CPU snapshot / wgpu)** — already variation-aware (above).

### Tests / docs status

- Graphic test: `graphic_tests/68-font-variation-settings.html` exists;
  `graphic_tests/run.py` entry; CPU snapshot baseline
  `crates/driver/tests/cases/snapshot_vs_edge.rs:122`
  `("68-font-variation-settings", 0.5)`.
- Real-font integration tests: `crates/engine/font/tests/inter_real_font.rs`
  (Inter-Regular is static — `gvar().is_err()` asserted at line 222; the
  bundled font has **no** variations, so end-to-end variation rendering is
  not exercised against real gvar deltas).
- Docs already mark it ✅: `CSS-SPECS.md:220,223`, `CAPABILITIES.md:94`,
  `graphic_tests/COVERAGE.md:239`. **These overstate reality** for the window
  path — see Definition of done about reconciling them.

---

## Architecture

Target pipeline (mostly built; femtovg leg is new):

```
font-variation-settings (CSS)               registered-axis mapping (CSS Fonts L4 §6)
  + @font-face descriptor                      font-weight    → wght
        │                                       font-stretch   → wdth
        ▼                                       font-style:oblique <a> → slnt
ComputedStyle.font_variation_settings          font-optical-sizing → opsz (= font-size px)
        │  (user-space [tag, value] list; author fvs overrides mapped axes)
        ▼
display_list DrawText.font_variation_axes  (Vec<([u8;4], f32)>)
        │
        ├── wgpu / CPU path ──► normalize (fvar+avar → [-1,1]) ──► VariationCoords
        │                         └► glyph_resolved_with_coords ──► gvar deltas + IUP
        │                         └► HVAR/VVAR advance + MVAR global metrics
        │                                                       ──► rasterize at instance
        │
        └── femtovg path (NEW) ──► either:
              (a) pass normalized coords to femtovg if its API supports
                  variable-font instancing, OR
              (b) bypass femtovg glyph shaping for variable faces: rasterize
                  via lumen-font `glyph_resolved_with_coords` into femtovg
                  images/paths (same coords source as wgpu path).
```

Registered-axis mapping rule (CSS Fonts L4 §6.x): a value present in the
author's `font-variation-settings` for a given tag **wins** over the value
derived from the corresponding high-level property. `opsz` injection already
follows this rule (`display_list.rs:2238` "unless the author already set it").
Apply the same precedence for `wght`/`wdth`/`slnt`.

---

## Cross-team boundary

- **P4 owns `font-variation-settings` parsing + cascade + ComputedStyle
  fields.** Already done (see Current state). If the registered-axis mapping
  (`font-weight`/`font-stretch`/`font-style`→axes) needs new ComputedStyle
  plumbing or a new cascade step, that is **P4 work** — file it as a
  `crates/...:line` pointer in `STATUS-P4.md` with a `// CSS: font-weight→wght axis`
  comment at the call site rather than editing `apply_declaration()` here.
- **P2 owns** the `lumen-font` runtime and the `lumen-paint` femtovg backend
  integration. Do not modify `ComputedStyle`/`apply_declaration()` in
  `style.rs`.

---

## Entry points (real file:line; *proposed* marked)

- `crates/engine/paint/src/backends/femtovg_backend.rs:1212` `draw_text()` —
  **modify** to honour variation axes (the gap).
- `crates/engine/paint/src/backends/femtovg_backend.rs:1996` `DrawText` match
  arm — **modify** to extract and forward `font_variation_axes`.
- `crates/engine/paint/src/backends/femtovg_backend.rs:314,333-338` font
  loading (`font_id`, `loaded_fonts`, `fallback_chain`) — *proposed* add a
  variable-face rasterization path (option (b) above) keyed by coords.
- `crates/engine/font/src/face.rs:369` `glyph_resolved_with_coords` — reuse
  as the coord-driven outline source for the femtovg bypass path.
- `crates/engine/paint/src/renderer.rs:7305` `normalize_variation_axes` —
  *proposed* lift/share a normalization helper so the femtovg backend uses the
  identical fvar+avar normalization (avoid divergent code).
- `crates/engine/font/src/face.rs:357-365` composite-component gvar limit —
  *proposed* address if a target variable font needs varied component anchors.
- `crates/engine/font/tests/inter_real_font.rs:222` — *proposed* add a real
  **variable** font fixture (Inter-Regular is static) to exercise gvar deltas
  end-to-end.

---

## Steps

1. **Reconcile claims first.** Confirm by running the actual window
   (`cargo run -p lumen-shell -- <page with a variable font + fvs>`) that
   on-screen text does *not* vary while the CPU snapshot does. This is the
   bug the task closes; capture before/after.
2. **femtovg backend — forward axes.** In `femtovg_backend.rs:1996`, stop
   discarding `font_variation_axes`; thread it into `draw_text()`
   (`:1212`).
3. **femtovg rendering strategy.** Decide (a) vs (b):
   - (a) if the pinned `femtovg` version exposes per-draw variable-font
     coordinates, set them on the paint/font;
   - (b) otherwise, for faces where `font.fvar().is_variable()` and coords are
     non-default, bypass `canvas.fill_text` for that run and rasterize glyphs
     via `glyph_resolved_with_coords` (same coords as wgpu path) into femtovg
     images/paths positioned on the baseline. Reuse `normalize_variation_axes`.
4. **Registered-axis mapping.** Ensure `wght`/`wdth`/`slnt`/`opsz` derived
   from `font-weight`/`font-stretch`/`font-style`/`font-optical-sizing` reach
   `font_variation_axes` with author-fvs precedence. `opsz` already does;
   add `wght`/`wdth`/`slnt` where missing (coordinate with P4 if it requires
   `style.rs` changes — file under boundary above).
5. **Composite limit (optional).** If a chosen test font relies on varied
   composite-component anchors (`face.rs:357-365`), implement component-level
   gvar; otherwise document and leave deferred.
6. **Caching.** Verify the femtovg path caches per (face, glyph, size,
   coords) — do not regress window perf (atlas/cache parity with
   `atlas.rs:19`).

## Tests

- **Unit (lumen-font):** existing `fvar.rs`/`variation.rs`/`variation_coords.rs`
  tests stay green. Add a real **variable** font fixture and assert
  `glyph_resolved_with_coords` produces a different bbox at `wght=900` vs the
  default (Inter-Regular cannot do this — `inter_real_font.rs:222`).
- **Visual (window):** the before/after from Step 1 — `font-variation-settings:
  "wght" 900` must visibly thicken strokes in the on-screen window, not only in
  the CPU snapshot.
- **Graphic test:** `graphic_tests/68-font-variation-settings.html` must pass
  at ≤ 0.5% on the path the gate uses; do **not** change the threshold
  (`snapshot_vs_edge.rs:122`). If the window path is now also validated, ensure
  no regression.
- **Clippy/tests:** `cargo clippy -p lumen-font --all-targets -- -D warnings`,
  `cargo clippy -p lumen-paint --all-targets -- -D warnings`,
  `cargo test -p lumen-font`, `cargo test -p lumen-paint`.

## Definition of done

- Variable fonts render at the requested instance in **all** rasterization
  paths, including the default femtovg on-screen window (the previously
  missing leg).
- `wght`/`wdth`/`slnt`/`opsz` registered-axis mapping from the high-level CSS
  properties works with author-`font-variation-settings` precedence.
- A real variable-font integration test exercises gvar deltas end-to-end
  (not just synthetic bytes / the static Inter bundle).
- `graphic_tests/68-font-variation-settings.html` passes ≤ 0.5%; thresholds
  unchanged.
- Docs reconciled with reality: `CAPABILITIES.md:94`, `CSS-SPECS.md:220,223`,
  `graphic_tests/COVERAGE.md:239` updated to reflect that the window path is
  now covered (they currently claim ✅ while the femtovg window path was not
  variation-aware). `SYMBOLS.md` regenerated if any public API changed.
- Composite-component gvar limitation (`face.rs:357-365`) and CFF2
  (`face.rs:374`) either implemented or explicitly recorded as remaining
  deferred items.
