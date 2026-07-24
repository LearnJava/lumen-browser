# BUG-335: nested `overflow:auto` content vanishes in the live wgpu window under an active chrome transform

**Статус:** FIXED 2026-07-24
**Компонент:** paint (wgpu `WgpuBackend`/`renderer.rs`, `DisplayCommand::PushScrollLayer`)
**Найден:** P1, CC-CSS-2 — новый graphic-тест 149 (вложенные `overflow:auto` scroll-контейнеры) 2026-07-24
**Исправлено:** P1, CC-CSS-2 2026-07-24

## Симптом

An `overflow:auto`/`overflow:scroll` container whose content actually overflows (scrollbar
shown) renders with content missing beyond the first child — sometimes ALL children,
including the first — in the live windowed (wgpu) build. `--dump-display-list` and the
CPU raster path (`--screenshot`) both show the correct, complete set of paint commands for
the same page, so the display list itself is not at fault.

Minimal repro:

```html
<div style="width:260px;height:55px;overflow-y:auto;overflow-x:hidden;">
  <div style="height:30px;background:purple;"></div>
  <div style="height:30px;background:red;"></div>
</div>
```

With content only slightly taller than the box (55px vs 60px of children), the live window
renders neither block — only the container's own background and the scrollbar thumb.
Widening the box so nothing actually overflows makes both children render correctly,
isolating the trigger to the overflow/scrollbar condition itself, not geometry or child
count.

## Root cause

`crates/engine/paint/src/renderer.rs`, `DisplayCommand::PushScrollLayer` handler.

BUG-276 (2026-07-13) fixed `PushClipRect`/`PushClipRoundedRect`/`PushClipPath`: each now
calls `apply_transform_to_clip(scrolled, transform_stack.last())` before intersecting with
`clip_stack`, because a clip rect from the display list is in the *page's* coordinate
space, while `clip_stack` entries must already be in *screen* space (accounting for any
accumulated `PushTransform`, e.g. the shell's own chrome/tab-bar Y-offset applied to page
content).

`PushScrollLayer` — which combines a clip push with a scroll-translate push — was never
updated to do the same. It intersected its `clip_rect` (page space) directly against
`clip_stack.last()` (screen space) whenever an accumulated transform was active. The two
rects being in different coordinate systems produced a bogus intersection: sometimes empty
(then `sync_scissor_to_stack` returns `false` and every draw call under that scissor is
skipped via `continue` — nothing in the container renders, not even the first child),
sometimes a small sliver (only the first child happens to fall inside it).

The live shell always has *some* chrome transform active above page content (tab bar /
toolbar offset), so this reproduces on every `overflow:auto`/`scroll` container with real
overflow in the actual window — headless dumps and the CPU path never apply that chrome
transform, which is why they stayed correct and masked the bug.

## Fix

Mirror the `PushClipRect` pattern: apply `apply_transform_to_clip()` to the scroll layer's
own `clip_rect` using the transform stack *as it stood before this push's own scroll
translate is composed onto it* — same convention as `PushClipRect`, and required so the
new clip lands in the same screen-space frame as everything already on `clip_stack`.

```rust
let scrolled_clip = translate_rect(*clip_rect, dx, dy);
let in_screen = apply_transform_to_clip(scrolled_clip, transform_stack.last());
let new_clip = match clip_stack.last() {
    Some(prev) => intersect_rects(*prev, in_screen),
    None => in_screen,
};
clip_stack.push(new_clip);
```

## Verification

New graphic test `graphic_tests/149-nested-overflow-scroll.html` (CC-CSS-2): three nested
`overflow:auto` scenarios (independent scrollbars, nested `border-radius` clip, `z-index`
element inside a nested scroll layer). Before the fix: 18.01% diff vs Edge (content missing
in all three columns). After: 7.91% — Item 1/2/3 text and the z-index badge now match Edge
pixel-for-pixel outside thin border AA; the residual is the pre-existing static-scrollbar-
vs-Edge-overlay-scrollbar class (BUG-288) plus an unrelated gradient-color-interpolation
gap in the border-radius column (wgpu gradient debt, BUG-277 class), not further nesting-
clip breakage. Registered as `KNOWN_DEBTOR` (`'149': ('BUG-288', 7.91)`) per the same
convention as TEST-14/51/83.
