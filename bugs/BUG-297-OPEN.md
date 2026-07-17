# BUG-297 — snapshot_cpu reference PNGs stale again (30+ pages, third occurrence)

**Статус:** OPEN
**Компонент:** test/snapshot (`crates/driver/tests/cases/snapshot_cpu.rs`, `graphic_tests/snapshots/cpu/`)
**Найден:** P1-imagebitmap (2026-07-17), `scripts/scoped-test.sh` gate before merge

## Симптом

`cargo test -p lumen-driver --features cpu-render cases::snapshot_cpu::cpu_snapshots_match_references`
fails with 31 mismatching pages, none touched by the branch under test (P1-imagebitmap only changed
`crates/js`): `18-images`, `36-border-radius`, `38-z-index`, `39-gradients`, `47-svg-basic`,
`55-text-rendering`, `57-canvas-2d`, `52-text-shadow-blur`, `30-css-filter`, `26-mask-image`,
`46-individual-transforms`, `49-background-blend-mode`, `28-css-containment`, `33-multi-column`,
`32-list-markers`, `27-direction-rtl`, `34-forms`, `45-multiple-backgrounds`, `51-scrollbar-rendering`,
`24-vertical-align`, `53-background-origin`, `58-first-letter-line`, `59-image-set-cross-fade`,
`100-transform-overflow`, `101-radius-overflow`, `104-mask-gradient-radius`, `106-transform-zindex`,
`107-shadow-radius-overflow`, `109-clippath-transform`, `117-quotes`, `119-paint-order`
(plus `1000000-final`, expected — that page gained a new demo card in the same commit).

## Причина

`graphic_tests/snapshots/cpu/` was last regenerated at `801d7640` (2026-06-30, BUG-247/BUG-173 SVG
nonzero-fill AA fix). Every P4 CSS-property merge since then (writing-mode, `@function`,
`@color-profile`, `backface-visibility`, `revert-layer`, `counter-set`, `font-size-adjust`,
`contain-intrinsic-size`, `interpolate-size`, … — see `graphic_tests/COVERAGE.md` entries for tests
110–145) shifted the CPU rasterizer's pixel output on unrelated pages without regenerating this
reference set. Same class of staleness as [BUG-118](bugs/BUG-118-FIXED.md) (2026-06-09) and
[BUG-149](bugs/BUG-149-FIXED.md) (2026-06-13) — third recurrence.

## Repro

```
cargo test -p lumen-driver --features cpu-render cases::snapshot_cpu -- --nocapture
```

## Что нужно для закрытия

Diff each listed page's current CPU render against its Edge/GPU reference to confirm every mismatch
is an *intentional* consequence of the feature that introduced it (not a real regression), then
`SAVE_CPU_SNAPSHOTS=1 cargo test -p lumen-driver --features cpu-render -- --nocapture` to regenerate.
Consider wiring this into the relevant doc-sync step (CLAUDE.md's "Adding a new CSS property"
checklist) so future paint-affecting merges regenerate the reference alongside the property, instead
of drifting until someone notices.
