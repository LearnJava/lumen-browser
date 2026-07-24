# BUG-336: `position:sticky` nested in an `overflow:auto`/`hidden`/`scroll` container doesn't pin — it just scrolls away with its container

**Статус:** FIXED 2026-07-24
**Компонент:** paint (wgpu `renderer.rs`, `DisplayCommand::BeginStickyLayer`)
**Найден:** P1, CC-CSS-3 — code audit while wiring `.net-table th`/`.site-nav` sticky headers in `assets/chrome/chrome.html`
**Исправлено:** P1, CC-CSS-3 2026-07-24

## Симптом

A `position:sticky` element nested inside a scrollable ancestor (`overflow:auto|scroll`,
e.g. a `PushScrollLayer`) behaves like ordinary in-flow content when that ancestor
scrolls — it scrolls out of view along with everything else instead of pinning at the
container's own top/bottom/left/right inset. Top-level page scrolling (no scrollable
ancestor) already worked; only the nested case was broken. Concretely: `.dt-panel
{ overflow-y: auto }` → `.net-table th { position: sticky; top: 0 }` in the chrome asset
(a Network-panel-style table header) does not stay pinned while the panel scrolls.

## Root cause

`crates/engine/paint/src/renderer.rs`, `DisplayCommand::BeginStickyLayer` handler and its
helpers `sticky_offset_dy`/`sticky_offset_dx`.

The offset a sticky element gets (`dy`/`dx`, pushed onto `sticky_stack` and used in place
of `-scroll_y`/`-scroll_x` for its subtree) was always clamped against the **global page
viewport** (`(0, 0, viewport_css_w, viewport_css_h)`) and the **top-level page scroll**
(`scroll_y`/`scroll_x`, the function's own parameters) — with no notion of any nearer
scrolling ancestor. Meanwhile a `PushScrollLayer` (nested `overflow:auto`/`scroll`) pushes
its *own* scroll translate onto `transform_stack`, applied to draw calls *after* the
sticky `dx`/`dy` translate. So for content nested in a scroll container: the sticky offset
was computed as if the page's scroll were the only thing moving it, then the container's
own scroll translate was layered on top unconditionally — meaning the sticky element got
the exact same treatment as an ordinary box: it moved 1:1 with the container's scroll,
never engaging the clamp (since the page itself wasn't scrolling, `dy` stayed `-scroll_y
== 0` regardless of how far the *container* had scrolled).

The femtovg fallback backend (`backends/femtovg_backend.rs`) has the same defect for the
same reason (its `BeginStickyLayer` handler also only reads the single page-level
`self.scroll_x`/`self.scroll_y` and the global viewport) — **not fixed here**, since
femtovg is the init-failure/`LUMEN_BACKEND=femtovg` fallback, not the default live-window
path (wgpu, ADR "Ph-wgpu-default"), and fixing it needs different plumbing (femtovg's
`Canvas::transform()`/`Transform2D::inversed()` instead of the Mat4/Rect stacks used here,
plus care around the FBO-based `PushClipRoundedRect`/`PushClipPath` render-target-switch
machinery there). Tracked as a follow-up.

## Fix

Generalized the clamp bound from a hardcoded global viewport to `sticky_bound()`: the
innermost active `clip_stack` entry (any ancestor `overflow:auto|hidden|scroll` — a clip
means a scroll container per the CSS Overflow spec, whether or not it's the one actually
scrolling) when present, else the full viewport — same fallback as before, so top-level
stickies with no scrollable ancestor are byte-identical to the old behaviour.

`clip_stack` entries are screen-space (post all ambient transforms, same convention as
`PushClipRect`/`PushScrollLayer`'s own clip intersection — BUG-276/BUG-335), while
`sticky_offset_dy`/`dx` need the bound in the *same pre-transform page-space* as
`flow_rect` (their `dy`/`dx` result is applied via `translate_rect` **before**
`transform_stack.last()` runs, exactly like every other draw command). `sticky_bound()`
maps the screen-space clip back through the *inverse* of the current ambient transform
(`Mat4::invert_2d_affine()`) to get there — falling back to the clip rect unchanged when
there's no ambient transform, or it isn't invertibly affine (same conservative BUG-140
policy as `apply_transform_to_clip`).

Only the clamp *bound* changed — the base (unclamped) offset is still exactly `-scroll_y`/
`-scroll_x` (the page-level scroll), since the nested container's own scroll is entirely
handled by the ambient transform applied afterward, uniformly for sticky and non-sticky
content alike.

## Verification

`cargo test -p lumen-paint --features backend-wgpu` — 6 new `renderer::tests::sticky_*`
unit tests exercise the exact composition: `sticky_bound_defaults_to_full_viewport_with_no_clip_or_transform`
(regression guard: unchanged behaviour with no ancestor clip), `sticky_bound_narrows_to_innermost_clip_rect`,
`sticky_bound_maps_screen_clip_back_through_ambient_transform` (the inverse-transform math
itself), and `sticky_nested_in_scroll_container_pins_within_local_scrollport` — the direct
BUG-336 repro: a `PushScrollLayer` translate(0,-120) plus its screen-space clip, verifying
a sticky header whose *unclamped* position would land above the panel's own visible top
(30, off-screen) instead pins exactly at the panel's own scrollport edge (40) once the
ambient transform is re-applied, matching what the renderer actually does at draw time.
Full `lumen-paint` suite: 1055/1055 passed (`--features backend-wgpu`), no regressions.

No graphic test added: `graphic_tests/run.py`'s screenshot pipeline only captures a single
unscrolled frame, and neither of Lumen's scroll-driving automation surfaces can produce a
*nested*-container-scrolled state to capture — the MCP `scroll` tool's `target` parameter
is accepted but ignored (`WinitSession::scroll()` always moves the root/page scroll node
only, `crates/driver/src/winit_session.rs:1014`), and fragment (`#anchor`) navigation only
scrolls the top-level page viewport, not ancestor scroll containers
(`navigate_fragment()`, `crates/shell/src/main.rs:9273`). Filed as a follow-up gap
(BUG-338) — reproducing this fix's exact production scenario end-to-end needs that
automation capability first. Existing sticky/scroll graphic tests confirmed unaffected:
TEST-42 (`position:sticky`, unscrolled/no-clip-ancestor) 0.27% (< 0.5% threshold, was
already passing pre-fix); TEST-51/TEST-149 (unrelated `KNOWN_DEBTOR`s) unchanged at their
recorded baselines (1.72%/BUG-124, 7.91%/BUG-288).
