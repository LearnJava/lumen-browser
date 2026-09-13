# BUG-540: `getBoundingClientRect()` ignores the `offset-path` motion-path transform (paint-only, not reflected in geometry queries)

**Статус:** FIXED 2026-09-13 (P3)
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

## Срез P3 2026-09-13 (закрытие)

Проверено: у обычного `transform` (не только `offset-path`) была та же дыра —
`collect_layout_rects_rec` читал `b.rect` без композиции с
`forward_box_transform` вообще, для любого transform-источника. Исправлено:
`collect_layout_rects_rec` (`crates/engine/layout/src/lib.rs`) теперь считает
`forward_box_transform(b)` (та же матрица, что уже применяет paint) и
заворачивает основной rect бокса через новый общий хелпер `transformed_aabb`
(4 угла → AABB), вынесенный из уже существовавшего `child_scrollable_bounds`
(BUG-504) — та же самая операция там уже делалась для scrollable-overflow.

Аккумуляция трансформации по цепочке предков (трансформированный контейнер
двигает repored-rect нетрансформированного потомка) сознательно вне объёма —
заявленный репро трансформирует сам запрашиваемый бокс, не контейнер над ним.

2 новых регресс-теста (`layout_rects_composite_own_transform`,
`layout_rects_composite_offset_path`,
`crates/engine/layout/src/tests/fixtures_and_core_selectors.rs`),
`cargo test -p lumen-layout --lib` 3955/3955, `cargo clippy -p lumen-layout
--all-targets -- -D warnings` чист. `dump_golden.py --build` — те же
предсуществующие 4/12 (BUG-1008-класс), изменение не трогает display list.
