# BUG-647: `linear-gradient(to <corner>, …)` used a fixed 45/135/225/315° angle regardless of box aspect ratio

**Статус:** FIXED 2026-08-05
**Компонент:** layout (`crates/engine/layout/src/style.rs::parse_linear_gradient_angle`) — shared by paint (`cpu_raster.rs`, `renderer.rs`, `femtovg_backend.rs`)
**Найден:** P3, BUG-277 срез 19, 2026-08-05 (while re-measuring the wgpu-live-window-vs-headless-CPU delta for the KNOWN_DEBTORS record «76»)

## Симптом

`graphic_tests/76-motion-path.html`, `.track-diag` box (960×160,
`background: linear-gradient(to bottom right, transparent calc(50% -
2px), #30363d calc(50% - 2px), #30363d calc(50% + 2px), transparent
calc(50% + 2px))` — a 4px-wide diagonal guide line). Edge renders the
line spanning the full 960px width, sloping gently (~170.5° in
CSS-angle terms — nearly vertical, tilted slightly toward the right).
Lumen rendered a short, steep segment centred near the middle of the
box instead of a full-width shallow line — identically wrong on **both**
the wgpu-live window and the deterministic headless-CPU path (0.87%/
1.03% vs Edge, near-identical to each other), which is what flagged this
as *not* a wgpu-backend defect (BUG-277's actual scope) but a shared
gradient-math bug.

## Причина

`parse_linear_gradient_angle` (`style.rs`) hardcoded the `to <corner>`
keywords to the diagonal angle of a **square** box:

```rust
"top right" | "right top" => 45.0,
"bottom right" | "right bottom" => 135.0,
"bottom left" | "left bottom" => 225.0,
"top left" | "left top" => 315.0,
```

Per CSS Images L3 §3.1, a corner keyword's true gradient-line angle
depends on the box's aspect ratio: the line is defined to be
**perpendicular to the diagonal connecting the two corners the keyword
does *not* name** — e.g. for `to bottom right` that diagonal runs
between the top-right and bottom-left corners, direction `(-width,
height)`, so the gradient line itself runs along `(height, width)`,
giving base angle `atan2(height, width)` (**height first** — the
opposite ratio from the naive "tilts toward the long axis" guess: on a
box much wider than tall, this angle is *small*, i.e. the line tilts
toward vertical, not horizontal). Verified against a real Edge render of
the 960×160 test box: formula predicts 170.5°, Edge's rendered line
measures ~170.5° (pixel-sampled).

The angle was resolved once at style-parse time, with no box context —
only for a square box does the fixed 45/135/225/315° happen to be
correct, which is presumably why this went unnoticed through 18 prior
BUG-277 slices (most gradient regression targets found so far were
either explicit-`<angle>` gradients or happened to sit on near-square
boxes).

## Фикс

- `ParsedGradient::Linear` gained a `corner: Option<GradientCorner>`
  field alongside the existing `angle_deg: f32` (which for a corner
  keyword now only carries the square-box placeholder).
- New `GradientCorner` enum + `angle_deg(width, height)` method
  implementing the formula above for all four corners.
- `parse_linear_gradient_angle` returns `(f32, Option<GradientCorner>)`.
- The two `display_list.rs` sites that turn a `ParsedGradient::Linear`
  into a `DisplayCommand` (`DrawLinearGradient` and
  `PushMaskLinearGradient`) resolve the true angle from the actual paint
  rect's `width`/`height` when `corner` is `Some`, before handing a plain
  `angle_deg: f32` to the (unchanged) backends — same pattern already
  used for `radial_gradient_radii` a few lines above. All three paint
  backends (`cpu_raster.rs`, `renderer.rs`, `femtovg_backend.rs`) consume
  `DisplayCommand::DrawLinearGradient.angle_deg` as a true CSS angle
  already (confirmed correct independently in BUG-277 срез 9's
  `box_aspect` fix for the wgpu path), so no backend-specific change was
  needed — fixing the single upstream angle resolves all three at once.

TEST-76: wgpu-live 0.62%→**0.02%** (record removed from `KNOWN_DEBTORS`,
was BUG-277's), headless-CPU 0.63%→**0.02%** (parity). TEST-45 (the only
other corpus page using a `to <corner>` gradient) unaffected (0.87%/
1.03%, unchanged) — its `.track-diag`-equivalent boxes are square-ish
enough that the old and new angle differ by under a pixel; its residual
KNOWN_DEBTORS entry is font-parity (BUG-128 class), not gradient
geometry.
