# BUG-1007 — canvas background-color propagation destructively mutates `<body>`'s own `ComputedStyle`, corrupting `getComputedStyle(body)`/`getComputedStyle(html)`

**Статус:** OPEN
**Заведён:** 2026-09-05 (P3, побочно при ревизии [BUG-514](BUG-514-OPEN.md))
**Компонент:** layout (`crates/engine/layout/src/box_tree/entry.rs::propagate_canvas_background`,
`canvas_background_color`)

## Механизм

CSS Backgrounds L3 §2.11.2 / HTML "Rendering" §the-page: when `<html>` has no
background, the canvas is painted using `<body>`'s background instead. This is
a **rendering/used-value** effect only — `getComputedStyle()` on either
`<html>` or `<body>` must keep reporting each element's own author-specified
value, unaffected by the propagation.

Lumen implements the propagation by **mutating the shared `Arc<ComputedStyle>`**
that the layout box tree holds for `<body>`:

```rust
// crates/engine/layout/src/box_tree/entry.rs:598-603
let body_style = Arc::make_mut(&mut body.style);
let bg_color = body_style.background_color.take();      // <- removes it from body
let bg_layers = std::mem::take(&mut body_style.background_layers);
let html_style = Arc::make_mut(&mut html_box.style);
html_style.background_color = bg_color;                  // <- moves it onto html
html_style.background_layers = bg_layers;
```

`LayoutBox::style` is the exact object later serialized into the
`computed_styles` cache that backs JS `getComputedStyle()`
(`crates/engine/layout/src/lib.rs::collect_computed_styles`, running over the
box tree *after* this mutation). The move therefore leaks into CSSOM on both
ends: `body` loses a value it should still report, and `html` gains one it
never had.

## Симптом

Confirmed live (`--mcp-live-port`, `LUMEN_NO_ENGINE_THREAD=1`), minimal repro
— a page with nothing but:

```html
<style>body { background-color: rgb(9, 9, 9); }</style>
```

`getComputedStyle(document.body).getPropertyValue('background-color')` →
`"rgba(0, 0, 0, 0)"` (should be `"rgb(9, 9, 9)"`). `color`/`margin-top` set on
the same rule read back correctly — only `background-color` (and, by the same
code path, `background-image`/`background-layers`) is affected. Symmetric
bug on the other side: `getComputedStyle(document.documentElement)` would
report the propagated color as `html`'s own, which is equally wrong (not
separately reproduced with a live probe, but follows from the same
`html_style.background_color = bg_color` assignment).

This is **not** the same defect as [BUG-493](BUG-493-OPEN.md)/CSSOM-4 (stale
cache on a script-mutated-in-the-same-tick node) — reproduces on a
statically-parsed `<body>` that was never touched by script, and `color`/
`margin-top` on the identical element read back fine in the same call.

## Масштаб

Found via 2 of [BUG-514](BUG-514-OPEN.md)'s five `css/css-env` files
(`at-supports.tentative.html`, `fallback-nested-var.tentative.html` — both
assert on `getComputedStyle(document.body)`'s `background-color`, unrelated to
`env()` itself; the `env()` value happens to be what's assigned, but the
underlying read is broken for *any* value). Not measured beyond that: this
mechanism runs on every document with an `<html>`/`<body>` pair, so any page
whose script or devtools reads `background-color` off `document.body` (or
`document.documentElement`) after CSS gives `<body>` a background is affected.
Not investigated: whether `background-image`/gradients on `body` show the
same CSSOM corruption (same code path moves `background_layers` the same
way — very likely yes, just not separately confirmed with a probe).

## Почему это не point-fixed в этой ревизии

The naive fix (stop moving the value — copy it to `html` instead of taking
it from `body`) is **not safe**: painting has no separate "used value"
concept here, so if `body`'s own box paint step is left untouched it would
re-paint its own background on top of the canvas clear. For opaque solid
colors that's a harmless idempotent double-paint, but for a translucent
`background-color` (e.g. `rgba(255,0,0,0.5)`) it double-composites and
visibly darkens the color — a real regression, not just a CSSOM nuance.

A correct fix needs the canvas-clear color to be computed **without**
touching either element's `ComputedStyle` (`canvas_background_color()` can
walk `html`/`body`'s *unmutated* styles directly — its only two call sites,
`crates/shell/src/frames.rs` and `.../window_event/redraw_requested.rs`, don't
require the box tree to have been pre-mutated), plus a **separate,
non-`ComputedStyle` marker** on `body`'s `LayoutBox` telling the paint pass to
skip repainting the propagated background at `body`'s own box (so translucent
colors don't get composited twice). That's a `LayoutBox`-shape change
(currently no such flag exists — see `crates/engine/layout/src/box_tree/types.rs`),
plus updates to the 5 existing propagation unit tests in
`crates/engine/layout/src/tests/table_grid_presentational.rs` (`html.style.background_color`/
`body.style.background_color` assertions there directly assert today's
mutation, i.e. the bug), plus wherever the paint backend turns a box's own
`background_color` into a fill command — wider than a single-file fix,
deferred rather than rushed.

## Воспроизведение

```
python .tmp/probe_bug514_env.py   # ad-hoc probe written for this investigation,
                                   # not committed — see BUG-514-OPEN.md's revision
                                   # note for the exact minimal HTML/JS
```

Minimal HTML:

```html
<style>body { background-color: rgb(9, 9, 9); }</style>
```

`getComputedStyle(document.body).backgroundColor` reads `rgba(0, 0, 0, 0)`
instead of `rgb(9, 9, 9)`.
