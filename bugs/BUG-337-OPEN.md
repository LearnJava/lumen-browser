# BUG-337: femtovg fallback backend has the same nested-`position:sticky` offset bug as BUG-336 (wgpu)

**Статус:** OPEN
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
