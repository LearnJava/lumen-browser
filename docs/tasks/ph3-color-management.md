# Ph3 — Color management (Display P3 / Rec2020 / wide gamut)

**Developer:** P2 · **Branch:** `p2-ph3-color-management` · **Size:** L · **Crates:** `lumen-paint`, `lumen-image`, `lumen-css-parser` (P4 handoff for CSS Color 4)

---

## Status

**Phase 3 (v1.0) — future.** Roadmap item `docs/plan/phases.md:132`: *"Color management + Display P3 / Rec2020 / ICC [P2]"*.

The **ICC slice (ICC-1…6, RGB + CMYK) is already DONE** (project memory `project_icc_color_management_slice`). This task covers the **remaining** color-management pieces: wide-gamut *output* (Display P3 / Rec2020 surface), keeping CSS Color 4 wide-gamut values wide instead of collapsing them to sRGB, and display-profile-aware compositing.

Do **not** start before Phase 2 closes. This file is a forward design note so the work is grounded when it is picked up.

---

## Goal

End-to-end color-managed paint to a **wide-gamut display**: a Display-P3 or Rec.2020 pixel authored via CSS `color(display-p3 …)`, or decoded from a wide-gamut image, reaches the screen at its real chromaticity instead of being clamped into sRGB at parse/decode time. On an sRGB-only display the result must be byte-identical to today (gamut-map to sRGB, no regression).

---

## Current state

### What is already done (ICC + parsing)

- **ICC profile parser** — `crates/core/src/icc.rs`. Parses header, tag table, `rXYZ/gXYZ/bXYZ`, `rTRC/gTRC/bTRC`, `wtpt`, `A2B0/B2A0`. Read-only, panic-free.
  - `IccProfile::color_space()` — `crates/core/src/icc.rs:329` — classifies an RGB profile into `ColorSpace::{Srgb,DisplayP3,Rec2020}` by colorant chromaticities.
  - `IccProfile::build_rgb_transform()` — `crates/core/src/icc.rs:383` — compiles a matrix-shaper transform. **Output target is hardcoded sRGB** via `XYZ_D65_TO_SRGB` (`crates/core/src/icc.rs:943`) + `srgb_encode()` (`crates/core/src/icc.rs:951`).
  - `IccProfile::build_cmyk_transform()` — `crates/core/src/icc.rs:426` — CMYK `A2B0` LUT path, also outputs sRGB (`CmykTransform::apply`, `crates/core/src/icc.rs:465`).
  - `cached_rgb_transform()` / `cached_cmyk_transform()` — `crates/core/src/icc.rs:904` / `:926` — process-wide transform cache keyed on profile bytes.
- **PCS maths** — `crates/core/src/pcs.rs`. `Xyz`, `Lab`, CIE Lab conversion, Bradford D50↔D65 (`Xyz::adapt`, `:83`). Reusable building blocks for any *output* primaries.
- **`ColorSpace` enum** — `crates/core/src/color.rs:4` — `Srgb`, `DisplayP3`, `Rec2020`, `Lab`. `detect_color_space_from_icc()` at `:36`. (Memory notes +3 exhaustive matches downstream — any new variant ripples.)
- **Image decode → tone-map** — `crates/engine/image/src/lib.rs`. `apply_icc_rgb_transform()` (`:436`), `detect_color_space()` (`:371`), `apply_tone_mapping()` (`:500`). Wide-gamut images carrying an embedded RGB profile are converted **to sRGB** at decode time.
- **CSS Color 4 wide-gamut parsing (P4 territory, already extensive)** — `crates/engine/layout/src/style.rs`:
  - `ColorFloat` struct (`:771`) holds float RGB + a `ColorSpace` tag, used for `color(display-p3 …)`, `color(rec2020 …)`, `color(srgb …)`.
  - `CssColor::Wide(ColorFloat)` variant (`:1083`).
  - `predefined_to_srgb_linear()` (`:867`) handles `srgb-linear`, `a98-rgb`, `prophoto-rgb`, and the Lab/LCH/Oklab/Oklch families — gamut-mapped to sRGB **at parse time**.
  - `color-mix()` algorithm — `crates/engine/layout/src/color_mix.rs` (mixes in srgb/lab/lch/oklab/oklch, returns sRGB).
- **Graphic test** — `graphic_tests/128-icc-color-management.html` + `graphic_tests/gen_icc_images.py`.

### The two collapse points (the actual gap)

Every path above terminates in **sRGB 8-bit**. Wide-gamut information is destroyed at two doors:

1. **`ColorFloat::to_srgb_color()`** (`crates/engine/layout/src/style.rs:782`) and **`ColorFloat::to_linear_srgb()`** (`:814`) — both run `p3_linear_to_srgb_linear` / `rec2020_linear_to_srgb_linear` and clip out-of-gamut channels. A P3 red authored in CSS becomes the nearest sRGB red before it ever reaches paint.
2. **ICC `build_rgb_transform` / `build_cmyk_transform`** — output matrix and encode are fixed to sRGB primaries.

### Output surface (sRGB-only)

- `crates/engine/paint/src/renderer.rs:1609` — wgpu surface format is chosen as the first **non-sRGB** candidate from `surface.get_capabilities`, falling back to `caps.formats[0]`. In practice this is an 8-bit `Bgra8Unorm`/`Rgba8Unorm` sRGB-gamut surface. **No wide-gamut format** (`Rgb10a2Unorm`, `Rgba16Float`) and **no color-space hint** is requested.
- femtovg backend — `crates/engine/paint/src/backends/femtovg_backend.rs` — all image registration is `femtovg::PixelFormat::Rgba8` (`:1257`, `:1402`, `:2026`, …). 8-bit sRGB-gamut throughout. (Memory `project_femtovg_default_backend`: femtovg is the default windowed backend; `project_text_render_paths_fork`: femtovg vs lumen-font/CPU are separate paint paths.)
- CPU raster — `crates/engine/paint/src/cpu_raster.rs` / `cpu_backend.rs` — 8-bit `Rgba8` snapshot path used by graphic tests / `--screenshot`.

### Display-profile detection — none

No OS query for the active monitor's color profile / gamut exists anywhere (grep for `EDID` / `display.*profile` / `wide_gamut` / `HDR` finds only unrelated hits). The browser cannot currently tell whether it is on an sRGB or a P3 panel. **This is the foundational gap** — without it, wide-gamut output is unconditional and would over-saturate on sRGB monitors.

---

## Architecture

Target pipeline (additive — sRGB path stays the default and the fallback):

```
wide-gamut color VALUE                       OUTPUT-aware conversion                  wide-gamut SURFACE
─────────────────────                        ──────────────────────                  ──────────────────
CSS color(display-p3 …) ─┐                                                          ┌─ Rgb10a2Unorm / Rgba16Float
CSS color(rec2020 …)     ├─► keep as ColorFloat ──► to_display(target_space) ──────►│   (P3 / Rec2020 gamut)
image w/ P3/Rec2020 ICC ─┘   (NO premature                  │                       └─ OR sRGB fallback (today)
                              sRGB collapse)                 ▼
                                              display profile (target primaries)
                                              from OS query (NEW) or sRGB default
```

Three layers, each with an sRGB fallback so nothing regresses on sRGB hardware:

1. **Preserve wide-gamut values.** Stop collapsing `ColorFloat` to sRGB at parse time. Carry the `ColorSpace` tag through the display list to paint. Add `ColorFloat::to_display(target: ColorSpace)` alongside the existing `to_srgb_color` / `to_linear_srgb` (which remain the fallback). Same for the ICC transforms: add `build_rgb_transform_to(target_primaries)` / a target-parametrised encode, generalising the hardcoded `XYZ_D65_TO_SRGB` + `srgb_encode` (`crates/core/src/icc.rs:943`/`:951`) using the existing `pcs::Xyz` Bradford + a per-target XYZ→linear matrix.

2. **Color-managed compositing.** The compositor must know the *destination* gamut. Blending/anti-aliasing should happen in a consistent space; a P3 value composited onto an sRGB background on a P3 display must convert the sRGB background into the P3 encoding (or composite in a shared linear space) rather than forcing the P3 value down to sRGB.

3. **Wide-gamut output surface.** Request a wide-gamut surface when the display supports it: a 10-bit (`Rgb10a2Unorm`) or 16-bit float (`Rgba16Float`) format plus the appropriate wgpu surface color space, at `crates/engine/paint/src/renderer.rs:1609`. Fall back to the current 8-bit sRGB selection when unsupported.

### Per-backend plan

| Backend | File | Work |
|---|---|---|
| **wgpu (`renderer.rs`)** | `crates/engine/paint/src/renderer.rs:1609` | Request `Rgb10a2Unorm`/`Rgba16Float` + wide-gamut color space when available; pass target primaries into shaders; keep 8-bit sRGB fallback. Primary target for true wide-gamut output. |
| **femtovg (default window)** | `crates/engine/paint/src/backends/femtovg_backend.rs` | Constrained: femtovg pixel formats are 8-bit (`Rgba8`). True wide gamut likely needs the wgpu renderer path; femtovg keeps the sRGB gamut-mapped output (document as a known limitation, or gate wide-gamut behind the wgpu backend). |
| **CPU raster** | `crates/engine/paint/src/cpu_raster.rs`, `cpu_backend.rs` | 8-bit sRGB snapshot for graphic tests / `--screenshot`. Keep sRGB (tests diff against Edge sRGB PNGs). Optionally emit the target color space as metadata only. |

### Display-profile detection (new, foundational)

Add an OS query for the active display's color profile / gamut, behind a `lumen-core::ext` trait so the platform impl lives in `shell`/`driver` (per `docs/plan/architecture.md` extension-trait policy). Windows: WCS / `GetICMProfile` / DXGI `DXGI_OUTPUT_DESC1` color space + EDID. Result feeds the target `ColorSpace` for layers 1–3. Default to `ColorSpace::Srgb` when unknown — that default makes the whole feature a no-op on sRGB panels, preserving current output.

---

## Cross-team boundary (P4: CSS Color 4)

CSS color **parsing** is P4's domain and is **largely already implemented** in `crates/engine/layout/src/style.rs` (`ColorFloat`, `CssColor::Wide`, `predefined_to_srgb_linear`, `color_mix.rs`). P2 must **not** edit color *parsing*.

The handoff for P4 (add a `crates/...:line` pointer in `STATUS-P4.md` with `// CSS:` markers, do **not** implement here):

- Today `predefined_to_srgb_linear()` (`style.rs:867`) and `ColorFloat::to_*` (`:782`/`:814`) bake the sRGB collapse into the parse/used-value step. For wide-gamut output, the *value* must survive in a wide space down to paint. P2 provides the wide-carry types and `to_display(target)`; **P4 wires** `color()` / `lab()` / `lch()` / `oklab()` / `oklch()` / `color-mix()` to emit the preserved wide value instead of an eager sRGB `Color`, keeping the sRGB collapse only as the fallback when the target is sRGB.
- Mark the connection points with `// CSS: wide-gamut color() output (Ph3 color management)` at the `ColorFloat` construction sites.

---

## Entry points (real file:line; *(proposed)* = to be added)

- `crates/core/src/color.rs:4` — `ColorSpace` enum (extend variants? *proposed:* keep set, add target-primaries lookup).
- `crates/core/src/icc.rs:943` — `XYZ_D65_TO_SRGB` matrix · `:951` `srgb_encode` → *(proposed)* generalise to `xyz_d65_to_<target>` + `<target>_encode`.
- `crates/core/src/icc.rs:383` — `build_rgb_transform` → *(proposed)* `build_rgb_transform_to(target: ColorSpace)`.
- `crates/core/src/pcs.rs:83` — `Xyz::adapt` (reuse for output-white adaptation).
- `crates/engine/layout/src/style.rs:771` — `ColorFloat` → *(proposed)* `to_display(target: ColorSpace)` method (P4 wires callers).
- `crates/engine/image/src/lib.rs:436` — `apply_icc_rgb_transform` / `:500` `apply_tone_mapping` → *(proposed)* target-aware variants.
- `crates/engine/paint/src/renderer.rs:1609` — surface format selection → *(proposed)* wide-gamut format + color-space request.
- `crates/engine/paint/src/backends/femtovg_backend.rs:1257` — `PixelFormat::Rgba8` registration sites (limitation note).
- *(proposed)* `lumen-core::ext` — `DisplayColorProfile` trait; impl in `crates/shell` or `crates/driver`.

---

## Steps

1. **Display-profile detection trait (foundational).** Add `DisplayColorProfile` to `lumen-core::ext`; implement OS query in `shell`/`driver` (Windows WCS/DXGI/EDID); default `Srgb`. Plumb the result to paint as the target `ColorSpace`. Unit-test the default path.
2. **Target-parametrised ICC transforms.** Generalise `build_rgb_transform` / encode in `crates/core/src/icc.rs` to an arbitrary target gamut using `pcs::Xyz` + per-target XYZ→linear matrices; keep the sRGB path identical. Add `to_display(target)` to `ColorFloat`. Tests: P3-in → P3-out is identity-ish; any-in → sRGB-out matches current output exactly (no regression).
3. **Carry wide-gamut values through the display list** (with the existing sRGB collapse kept as the `target == Srgb` fallback). Add `// CSS:` markers + a `crates/...:line` pointer in `STATUS-P4.md` for the parser side (P4 wires).
4. **Wide-gamut output surface (wgpu).** At `renderer.rs:1609`, request `Rgb10a2Unorm`/`Rgba16Float` + wide-gamut surface color space when supported; gate behind the detected display profile; fall back to today's 8-bit sRGB selection.
5. **Color-managed compositing.** Composite/blend in a consistent space relative to the destination gamut; convert sRGB content into the target encoding (not vice-versa) on a wide-gamut display.
6. **Image path.** Target-aware `apply_icc_rgb_transform` / `apply_tone_mapping` in `crates/engine/image/src/lib.rs` so a P3/Rec2020 photo stays wide when the display is wide.
7. **femtovg limitation.** Document/handle femtovg's 8-bit constraint: either gate wide-gamut behind the wgpu backend or accept sRGB gamut-mapping there.
8. Update `CAPABILITIES.md`, `subsystems/paint.md` / `subsystems/image.md` / `subsystems/core.md`, and the roadmap status. Bump `phases.md:132` once shipped.

---

## Tests

- **Unit (`lumen-core`):** target-parametrised ICC transform — P3 profile → P3 target is near-identity; → sRGB target matches the existing `build_rgb_transform` output byte-for-byte (regression guard on the current sRGB path).
- **Unit (`lumen-layout`):** `ColorFloat::to_display(Srgb)` equals the current `to_srgb_color()` / `to_linear_srgb()`; `to_display(DisplayP3)` of an in-gamut P3 value preserves channels.
- **Unit (display detection):** unknown / no-profile path returns `ColorSpace::Srgb` (whole feature is a no-op on sRGB).
- **Graphic test:** extend `graphic_tests/128-icc-color-management.html`. Note the sRGB-PNG diff harness and the 0.5% threshold are **immutable** (memory `feedback_test_thresholds_immutable`); the CPU/sRGB snapshot path must stay byte-stable, so wide-gamut output is only exercisable on a real wide-gamut display or via a dedicated non-pipeline check. File any deferred wide-gamut verification as a `KNOWN_DEBTOR` with a BUG-NNN if it can't reach 0.5% on the sRGB pipeline.
- **No-regression sweep:** run the full graphic-test pipeline; sRGB output must be unchanged.

---

## Definition of done

- OS display-profile detection exists behind a `lumen-core::ext` trait, defaulting to sRGB; the platform impl lives in `shell`/`driver`.
- ICC transforms and `ColorFloat` can target a wide-gamut output space; the sRGB path is byte-identical to today.
- The wgpu surface requests a wide-gamut format + color space when the display supports it, with an 8-bit sRGB fallback.
- A Display-P3 CSS color and a P3-profiled image reach a P3 display at real chromaticity; on an sRGB display the output is unchanged.
- femtovg's 8-bit limitation is documented or wide-gamut is gated behind the wgpu backend.
- P4 handoff for CSS Color 4 wide-value carry is filed in `STATUS-P4.md` (a `crates/...:line` pointer) with `// CSS:` markers; **no color parsing was edited by P2**.
- `cargo clippy -p lumen-core -p lumen-image -p lumen-paint --all-targets -- -D warnings` clean; crate tests pass; full graphic pipeline shows no sRGB regression.
- `CAPABILITIES.md`, the relevant `subsystems/*.md`, and `docs/plan/phases.md:132` updated in the merge commit.
