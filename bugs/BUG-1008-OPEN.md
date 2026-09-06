# BUG-1008 — snapshot_cpu reference PNGs stale again (5th occurrence)

**Статус:** OPEN
**Компонент:** test/snapshot (`crates/driver/tests/cases/snapshot_cpu.rs`, `graphic_tests/snapshots/cpu/`)
**Найден:** P3 2026-09-06, гейт `/lumen-task-finish` на [BUG-517](BUG-517-FIXED.md)

## Симптом

```
cargo test -p lumen-driver --test all cases::snapshot_cpu -- --nocapture
```

fails on `main` (`e11f78862`, no local changes):

```
CPU snapshot mismatches (regenerate with SAVE_CPU_SNAPSHOTS=1 if intentional):
55-text-rendering: 28713 differing bytes (of 2949120)
57-canvas-2d: 3600 differing bytes (of 2949120)
32-list-markers: 75381 differing bytes (of 2949120)
34-forms: 4734 differing bytes (of 2949120)
45-multiple-backgrounds: 32024 differing bytes (of 2949120)
51-scrollbar-rendering: 10092 differing bytes (of 2949120)
1000000-final: 2604 differing bytes (of 2949120)
```

## Причина (гипотеза)

Same class as [BUG-118](BUG-118-FIXED.md) (2026-06-09), [BUG-149](BUG-149-FIXED.md)
(2026-06-13), [BUG-297](BUG-297-FIXED.md)/[BUG-316](BUG-316-FIXED.md) (2026-07-20) —
`graphic_tests/snapshots/cpu/` reference PNGs go stale whenever a merge shifts the
CPU rasterizer's pixel output on pages the merge itself doesn't visibly touch, and
nobody regenerates the reference set in the same commit. Fifth recurrence despite
the CLAUDE.md "Adding a new CSS property" checklist note added in BUG-297's
resolution.

Not localized to a specific merge yet. Candidate window: the branches merged into
`main` between BUG-297's regeneration (2026-07-20) and now that touch text/paint —
most recently FONTLOAD-16/17 (glyph rasterization overrides, 2026-09-05/06) and the
P3/P1 CSS merges around `67a54fd30`/`e11f78862`. `55-text-rendering` moving is
consistent with the font-rasterization work; `32-list-markers`/`34-forms` also
render text so could share the same root cause; `45-multiple-backgrounds`/
`51-scrollbar-rendering`/`57-canvas-2d` are less obviously text-related and need a
separate look.

## Подтверждение: не регрессия текущей ветки

Verified in an isolated sparse worktree checked out to `main` (`e11f78862`) with no
branch changes applied — identical 7-page mismatch, identical byte counts. `p3-bug517-block-step`
(BUG-517, `block-step-size`/`-insert`/`-align`/`-round` CSS properties) does not
touch any of the 7 affected pages or the rasterizer — unrelated, merged anyway per
the same policy as [BUG-805](BUG-805-OPEN.md) (gate broken independent of the branch
under test, documented and proceeded).

## Что нужно для закрытия

Same recipe as BUG-297: diff each of the 7 pages' current CPU render against its
Edge/GPU reference to confirm the drift is feature-driven (not a rasterizer
regression) — bisecting which merge introduced each page's diff would narrow this
down faster than a blanket regenerate. Then `SAVE_CPU_SNAPSHOTS=1 cargo test
-p lumen-driver --test all cases::snapshot_cpu -- --nocapture` to regenerate; verify
exactly 7 PNGs change on disk, matching this mismatch list.
