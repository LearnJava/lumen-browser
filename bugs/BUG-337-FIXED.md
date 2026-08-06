# BUG-337: femtovg fallback backend has the same nested-`position:sticky` offset bug as BUG-336 (wgpu)

**Статус:** FIXED 2026-08-06
**Компонент:** paint (femtovg `backends/femtovg_backend.rs`, `DisplayCommand::BeginStickyLayer`)
**Найден:** P1, CC-CSS-3 2026-07-24 (while fixing BUG-336 in the default wgpu backend)

## Симптом

Same defect as [BUG-336](BUG-336-FIXED.md), in the femtovg backend instead of wgpu: a
`position:sticky` element nested inside a scrollable ancestor (`overflow:auto`/`scroll`)
scrolls away with its container instead of pinning, because `BeginStickyLayer`'s
compensation (`crates/engine/paint/src/backends/femtovg_backend.rs:5592-5608`) only reads
the single page-level `self.scroll_x`/`self.scroll_y` fields and the global
`viewport_css_w`/`viewport_css_h` bound — with no notion of any nearer `PushScrollLayer`'s
own scroll translate, which femtovg composes via its own internal `Canvas` transform/save
stack rather than the Rust-side `Vec<Mat4>`/`Vec<Rect>` stacks `renderer.rs` uses.

**Not user-visible today**: femtovg is the wgpu-init-failure fallback / explicit
`LUMEN_BACKEND=femtovg` override (see `crates/shell/Cargo.toml`), not the default live
window (wgpu, per the "Ph-wgpu-default" ADR) — so `about:chrome-preview`'s `.net-table
th`/`.site-nav` sticky headers render correctly by default.

## Why not fixed alongside BUG-336

The wgpu fix reused existing machinery (`clip_stack: Vec<Rect>`, `transform_stack:
Vec<Mat4>`, both already screen-space, plus `Mat4::invert_2d_affine()`) that has no direct
analogue in femtovg_backend.rs. A correct port needs `self.canvas.transform()` (already
used elsewhere in the file for similar screen-space mapping, e.g. `is_command_culled`) and
`femtovg::Transform2D::inversed()` (confirmed to exist and to fail safe — falls back to
identity on a near-singular matrix rather than producing NaN/garbage) — but computing the
*bound* additionally needs a new Rust-side "innermost active clip, in screen space" stack,
since femtovg's own clip/scissor state isn't queryable as a plain `Rect`. That stack has to
account for `PushClipRoundedRect`/`PushClipPath`'s existing bbox-offscreen-FBO-layer
machinery (BUG-272 slice 11/12: `screen_bbox_device_px`/`acquire_bbox_layer`), which
switches the active render target and therefore the meaning of "current transform" mid-
subtree — `renderer.rs`'s `PushClipRoundedRect`/`PushClipPath` don't have this complexity
(they're a plain bbox-scissor fallback per BUG-140), so the wgpu fix's approach doesn't
port over as-is. Scoped out of CC-CSS-3 to keep that task to its wgpu/live-window-default
DoD; left for whoever next touches femtovg sticky/scroll rendering.

## Suggested fix direction

Mirror BUG-336's `sticky_bound()` concept: at `BeginStickyLayer`, take
`self.canvas.transform()` as the ambient forward transform, invert it
(`.inversed()`), and map a newly-tracked "innermost clip, screen space" rect back through
that inverse to get the pre-transform bound `sticky_offset_dy`/`dx` need. The new clip-
bound stack must be pushed/popped at the same points as `clip_stack: Vec<ClipEntry>`
already is (`PushClipRect`/`PushClipRoundedRect`/`PushClipPath`/`PushScrollLayer`) — for
the two offscreen-FBO variants, push the bound in *screen* space captured **before** the
FBO switch (same moment `transform` is captured for their `ClipEntry::*Layer` variants),
not the bbox-local space used once rendering *inside* the FBO.

**Correction (2026-08-06, P3):** the claim above that `femtovg::Transform2D::inversed()`
"fails safe to identity on a near-singular matrix" does not hold for femtovg 0.9.2:
`Transform2D::inverse()` resets `*self` to identity on a near-zero determinant, but then
falls through unconditionally and overwrites every component again from the *pre-reset*
matrix using `1.0 / det` — for `det` in `(-1e-6, 1e-6)` that's a huge-but-finite or
literally-infinite `invdet`, clobbering the identity reset with garbage rather than
returning it. The fix below guards the determinant itself before calling `.inversed()`
(`try_invert_transform`) instead of relying on the crate's own fallback.

## Fix

Ported wgpu's `sticky_bound`/`sticky_offset_dy`/`dx` concept to femtovg, adapted to its
continuously-accumulated `canvas.transform()` model instead of a parallel `Vec<Mat4>`
stack:

- New `sticky_bound_stack: Vec<Rect>` field, pushed (screen-space, intersected with the
  current top) by all four scrollport-opening commands — `PushClipRect`,
  `PushClipRoundedRect`, `PushClipPath` (both the offscreen-FBO and the plain-scissor
  fallback branch, captured once before the branch so both agree), and `PushScrollLayer`
  (captured **before** that layer's own `-scroll` translate joins the canvas, so a
  container's own scrollport rect stays invariant to *its own* scroll — only ancestors
  can move it). Popped by `PopClip`/`PopScrollLayer`.
- `sticky_bound()` (femtovg analogue of `renderer.rs::sticky_bound`) inverts the *current*
  ambient `canvas.transform()` (via the new `try_invert_transform`, see the correction
  above) and maps the stack's top back into the current pre-ambient page space — this
  naturally absorbs any number of nested `PushScrollLayer`s and `PushTransform`s, unlike
  the old code's manual `sdy + self.scroll_x/self.scroll_y` cancellation, which only
  undid the single page-level scroll.
- `sticky_offset_dy`/`dx` switched from a `-scroll_y`/`-scroll_x` baseline (wgpu's
  convention, needed because wgpu's `transform_stack` excludes page-level scroll) to a
  `0.0` baseline: femtovg's `ambient` already carries *all* scroll (page-level and
  nested), so "no correction" means "let the naturally-scrolled position stand" and a
  correction only kicks in when it would violate `bound`.
- `BeginStickyLayer` now does a plain `self.canvas.translate(sdx, sdy)` on top of
  `ambient` — femtovg's `translate()` premultiplies (`new_transform(p) = old_transform(p +
  (tx, ty))`, verified against `Transform2D::multiply`/`premultiply`/`Canvas::translate`
  in femtovg-0.9.2), so this composes to exactly `ambient(p + (sdx, sdy))` for any
  subsequently-drawn local point `p` — matching wgpu's `T(child_raw_page_pos + (sdx,
  sdy))` formula with no extra scroll-cancellation arithmetic needed.

New unit tests (`sticky_bound_*`, `sticky_offset_dy_unclamped_is_zero`,
`sticky_nested_in_scroll_container_pins_within_local_scrollport`) mirror the wgpu
BUG-336 test suite in `renderer.rs`; the last one reproduces this bug's exact regression
scenario (`.net-table th { position:sticky; top:0 }` inside a scrolled `.dt-panel
{ overflow-y:auto }`) and asserts the header pins at the panel's own scrollport edge
instead of riding away with the panel's scroll. Not covered by the pixel-diff pipeline
(`graphic_tests/run.py`) since the live default backend is wgpu (BUG-336's own fix), not
femtovg — `graphic_tests/dump_golden.py` confirms the change is display-list-neutral for
the default backend (unaffected, since the fix touches only `femtovg_backend.rs`, not
`display_list.rs`).
