# BUG-524: CSS Scroll Anchoring (`overflow-anchor`) is entirely unimplemented
— property not parsed, no anchor-selection/adjustment logic anywhere in layout

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser (property missing) + layout (no anchoring algorithm)
**Найден:** WPT-RUN-3 срез 24 (`ROADMAP.md`) — массовый прогон `css/css-scroll-anchoring`

## Механизм

`grep -rn "overflow-anchor\|overflow_anchor" crates/engine/css-parser/src/` —
zero hits: the property isn't parsed at all (any value, valid or not, is
silently accepted/ignored by the generic inline-style passthrough, i.e. the
[BUG-484](BUG-484-OPEN.md) pattern). `grep -rln "ScrollAnchor\|scroll.anchor"
crates/ -i` finds a single hit, a doc-comment in
`crates/engine/layout/src/style.rs:896` noting the engine has "no support for
`overflow-anchor`" — there is no anchor-node-selection algorithm, no
suppression-heuristic, no scroll-offset-adjustment-on-relayout logic anywhere
in `lumen-layout`. This is a whole CSS module absent, not a partial/buggy
implementation.

## Симптом

Every `css/css-scroll-anchoring` test that actually reaches the anchoring
behavior itself (as opposed to failing earlier on
[BUG-523](BUG-523-OPEN.md)'s async-scrollTop gap or
[BUG-525](BUG-525-OPEN.md)'s missing `document.scrollingElement`) would still
fail even with those two fixed: nothing in layout adjusts scroll position
when content shifts above the visible viewport, which is the entire premise
of the spec. Two direct `e.style['overflow-anchor'] = '...'` parsing
assertions also fail (`= "all"`/`= "auto none"` should be rejected as
invalid) — those are already covered by the generic BUG-484 pattern, not
listed as new.

Filed by track policy (same as BUG-507 `css-exclusions`/BUG-517
`css-rhythm`): a WPT category whose corresponding CSS module has zero
implementation gets one bug for "whole module absent", separate from the
narrower BUG-523/BUG-525 findings that happen to dominate the *raw* failure
count in this specific category's log.

## Фикс (не сделан)

Full CSS Scroll Anchoring L1 implementation: parse `overflow-anchor`
(`auto`/`none`), select a per-scroll-container anchor node on layout,
suppress adjustment on the heuristics the spec defines (position-change,
`overflow-anchor: none`, etc.), and apply the compensating scroll delta
during relayout. Sizeable layout feature — likely its own multi-slice task
once picked up, not a quick property-table addition.
