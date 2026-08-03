# BUG-540: `getBoundingClientRect()` ignores the `offset-path` motion-path transform (paint-only, not reflected in geometry queries)

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** layout/paint (`offset-path` applied via `resolve_motion_transform` in paint's `property_trees.rs`, per `CSS-SPECS.md` — never reaches the layout-box rect that geometry queries read)
**Найден:** WPT-RUN-3 срез 28 (`ROADMAP.md`) — массовый прогон `css/motion`

## Механизм

`CSS-SPECS.md` (Motion Path L1 row) documents `offset-path`/`offset-distance`/
`offset-rotate` as wired end-to-end via `resolve_motion_transform` in the
*paint* property-trees stage — i.e. motion-path repositioning is applied the
same way as a `transform`, as a paint-time matrix, not as a layout-box
position change. Confirmed via `--dump-layout` on a minimal repro
(`position:absolute; offset-path: path('M 20 20 L 220 20')` — a straight
horizontal path starting at the box's own top-left): the box's `rect` stays
at its untransformed layout position for both x and y, unmoved by the path.

For a real `transform`, browsers still report the *transformed* box from
`getBoundingClientRect()` — the paint-only application is fine as long as
geometry queries composite the same matrix. Here they evidently don't for
`offset-path` specifically (untested whether plain `transform` has the same
gap in this engine — out of scope for this slice's `css/motion` triage).

## Симптом

```
FAIL Bounding client rect for #blue - assert_equals: #blue client rect.x expected 220 but got 0
FAIL Bounding client rect for #purple - assert_equals: #purple client rect.x expected -30 but got 0
```

`css/motion/offset-path-bounding-client-rect.html`, 2/2 subtests. Low
subtest count but a real, distinct geometry defect — not folded into
[BUG-536](BUG-536-OPEN.md) (CSS Transitions/Web Animations no-op), which is
about *animated* interpolation never being observable; this is about the
*static* (non-animated) `offset-path` transform never reaching geometry
queries at all.

## Как исправить (не входит в объём P2)

Whatever code path makes `getBoundingClientRect()`/`collect_layout_rects`
account for a live `transform` (if it does) should also composite the
motion-path matrix from `resolve_motion_transform`; if `transform` has the
same gap, this may be one fix rather than two.
