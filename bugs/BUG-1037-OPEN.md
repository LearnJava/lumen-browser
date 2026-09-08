# BUG-1037 — невидимый `position:fixed`/`sticky` replaced-элемент оставляет `Begin*Layer` без пары

**Статус:** OPEN
**Заведён:** 2026-09-08 (P1), при работе над LAYOUT-2 срез 9 (явный стек для `display_list/walk.rs::walk`)
**Область:** paint (`crates/engine/paint/src/display_list/walk.rs`)

## Симптом

`walk`'s `FormControl`/`Image`/`Video`/`Canvas`/`Audio`/`Iframe` arms each start with an early
`return` when the element is invisible/zero-size (`!is_paint_visible(b)`, or additionally
`b.rect.width <= 0.0 || b.rect.height <= 0.0` for `Audio`/`Iframe`). That `return` exits the whole
`walk` function — but `BeginFixedLayer`/`BeginStickyLayer` (pushed earlier in the same call, based
purely on `b.style.position`, before the `match` on `b.kind`) are only matched by the corresponding
`EndFixedLayer`/`EndStickyLayer` at the very end of the function, textually *after* the `match`. An
early return from inside one of these six arms skips that closing code entirely.

Concretely: a `<canvas style="visibility:hidden; position:fixed">` (or any of the other five
replaced-element kinds, combined with `position:fixed` or `position:sticky` and either
`visibility:hidden` or, for `<audio>`/`<iframe>`, a zero-size box) pushes `BeginFixedLayer`
(or `BeginStickyLayer`) into the display list with no matching `End*Layer` anywhere after it.

## Почему это важно

`BeginFixedLayer`/`EndFixedLayer` and `BeginStickyLayer`/`EndStickyLayer` are partition metadata
the compositor scroll-blit (ADR-016 M3.2.1c) and the sticky scroll-clamp offset logic read to
find where a fixed/sticky layer's content starts and ends in the flat command stream. An unmatched
`Begin*Layer` — depending on how the consumer scans for its matching `End*` — risks either
silently absorbing every subsequent sibling's commands into the open (but logically empty) fixed/
sticky partition, or a matching-bracket panic/`unwrap` if the consumer assumes balance. Not
verified against a live repro yet (found by code inspection while converting `walk`'s per-child
recursion to an explicit stack, LAYOUT-2 срез 9 — the conversion **reproduces this pre-existing
quirk bit-for-bit** rather than incidentally fixing it, to keep that срез's A/B display-list-
neutrality claim honest).

## Where to look

`display_list/walk.rs`'s `dispatch` function (was inlined directly in `walk` before LAYOUT-2 срез
9) — the six `if !is_paint_visible(b) { return ...; }` (`FormControl`) /
`if !is_paint_visible(b) { return ...; }` (`Image`/`Video`/`Canvas`) /
`if !is_paint_visible(b) || ... { return ...; }` (`Audio`/`Iframe`) early-return sites. Likely fix:
close `is_fixed`/`is_sticky` before each of these six returns (or restructure so the common
`is_fixed`/`is_sticky` wrapping is a single choke point every arm passes through, invisible or
not) — needs a regression test asserting `Begin`/`End` counts stay balanced for each of the six
kinds under `visibility:hidden`/zero-size + `position:fixed`/`sticky`, and a check of whether
`fill_buckets`/`emit_box_self` (`box_layer.rs`, the ordered/anim-aware paint path) has the same
shape independently.
