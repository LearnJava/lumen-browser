# BUG-658 — `offsetWidth`/`offsetHeight`/`getBoundingClientRect()` return `0` (not the real box, not a thrown error) when read from an inline `<script>` that runs during the initial HTML parse — same timing gap as BUG-555, different API surface

**Статус:** DUPLICATE → [BUG-443](BUG-443-FIXED.md)
**Компонент:** js (`crates/js/src/dom.rs:3196` `get offsetWidth()`/sibling `offsetHeight`/`getBoundingClientRect()`, all backed by the native `_lumen_get_bounding_rect(nid)`), shell (`crates/shell/src/main.rs::apply_loaded_page` — same layout-snapshot publish path as BUG-555/BUG-382)
**Найден:** P2, WPT-VENDOR-quirks (2026-08-05), `run_report.py --all --root quirks --recursive` real run + `--dump-layout` cross-check
**Закрыт как дубликат:** P3, 2026-09-17

## Механизм

[BUG-555](BUG-555-DUPLICATE.md) documented that `getComputedStyle()` reads `""`
for every property when called from a synchronous inline `<script>` that runs
during the initial HTML parse — the engine thread has not yet published a
layout snapshot for the page's first layout pass, and `getComputedStyle` does
not force a synchronous recompute; it just reads whatever snapshot already
exists (none, at that point). That bug's own "Impact" section predicted this
would generalize to other reads gated on the same snapshot and asked for a
"targeted re-triage pass" — this is that pass turning up a second, distinct
API hitting the identical mechanism.

`offsetWidth`/`offsetHeight`/`getBoundingClientRect()` are all backed by the
same single native, `_lumen_get_bounding_rect(nid)` (`dom.rs:3196`), which
depends on the identical engine-thread-published snapshot as
`getComputedStyle`. A `<script>` executing mid-parse — the ordinary execution
model for any non-deferred inline script — reads it before that snapshot
exists, and unlike `getComputedStyle` (which reads back `""`, a value at
least distinguishable from "computed to nothing"), `offsetWidth` reads back
`0` — indistinguishable from "genuinely zero-width box", so callers cannot
even detect the race the way an empty-string sentinel hints at one.

## Live evidence (WPT run, not `--dump-layout` alone)

`quirks/table-cell-width-calculation-applies-to.html` (no DOCTYPE, so a
quirks-mode document — irrelevant to this bug, just the vendored test that
surfaced it) has an explicit-width table immediately followed by an inline
`<script>` that calls `checkLayout('table')`
(`tests/wpt/resources/check-layout-th.js:72`,
`assert_tolerance(node.offsetWidth, expectedWidth, ...)`) — the canonical
"script right after the styled markup" idiom BUG-555 already flagged as
common in the vendored `css/` corpus:

```
FAIL table 1 - assert_equals:
width expected 80 but got 0
```

Cross-checked with a headless `--dump-layout` on the identical file (which
runs the dump *after* the whole document + layout has settled, sidestepping
the mid-parse timing window entirely): the table's `Table rect=(8.00, 8.00,
80.00, ...) w=80.00` — the geometry is computed correctly; only the live
DOM read at the wrong moment sees `0`. Confirms this is a publish-timing
gap, not a table/cascade computation bug (ruling out the more obvious
suspect given `layout/src/table.rs` is a known-dead parallel table-layout
implementation — see project memory `project_dead_table_rs` — the live path
is `box_tree.rs::lay_out_table`, which this reproduction shows is correct).

## Impact

Any page (WPT test or real site) whose inline script measures an element's
box (`offsetWidth`/`offsetHeight`/`getBoundingClientRect`) immediately after
writing it — a pattern at least as common as the `getComputedStyle` idiom
BUG-555 already covers, and the one `check-layout-th.js` uses throughout the
`css/`-adjacent and `quirks/` WPT corpora — silently measures a `0×0` box
instead of the real one. Worth grouping with BUG-555 for the fix (both are
symptoms of the same missing "publish snapshot synchronously as soon as the
first layout pass completes, before resuming script for that navigation"
step) rather than patching each read site independently.

## Что НЕ является причиной

Not a table-layout defect (`box_tree.rs::lay_out_table` computes the correct
80px width, confirmed via `--dump-layout` on the same file) and not
quirks-mode-specific (the WPT test happens to be an unrelated quirks-mode
fixture; the underlying script-timing race is document-mode-agnostic — see
BUG-555's own reproduction, which uses a standards-mode page).

## Предлагаемый фикс

Same as BUG-555's proposal: publish an initial layout snapshot synchronously
right after the page's first layout pass completes, before resuming/
continuing script execution for that navigation, rather than only after
full-document `apply_loaded_page` — fixing the snapshot-publish timing fixes
both `getComputedStyle` and `_lumen_get_bounding_rect` at once, since they
share the same publish path.

## Ревизия P3 2026-09-17

Заявка (2026-08-05) предсказала this exact mechanism would also hit
`offsetWidth`/`offsetHeight`/`getBoundingClientRect`, sharing the publish path
with [BUG-555](BUG-555-DUPLICATE.md). [BUG-443](BUG-443-FIXED.md) (найден P1
2026-07-29, исправлен P3 2026-08-30 — **после** подачи этой заявки, but never
back-marked against it) reordered `build_page_cascade` + the first
`layout_page`/`JsLayoutSnapshot` ahead of `run_scripts_with_dom` and publishes
the snapshot right after `install_dom` — before the page's first line of
script — which covers `_lumen_get_bounding_rect` exactly as it covers
`getComputedStyle`, since both read the same published snapshot.

New regression test
`crates/shell/src/tests/page_pipeline.rs::parse_time_script_reads_offset_width_and_height`
reproduces this заявка's own repro shape (`offsetWidth`/`offsetHeight` read by
an inline `<script>` immediately after a styled element, in the same parse
tick) and is green on current `main`
(`cargo test -p lumen-shell --profile dev-release --features v8
parse_time_script_reads_offset_width_and_height` → `ok`, `300x120` instead of
`0x0`). Live check of the заявка's own repro
(`quirks/table-cell-width-calculation-applies-to.html`) would need a WPT
re-run to confirm the harness result, but the mechanism-level test above
pins the same native (`_lumen_get_bounding_rect`) the failing assertion used
(`node.offsetWidth`), so this is closed as a duplicate rather than left open
awaiting a slow re-run.

## Связанные

* [BUG-443](BUG-443-FIXED.md) — реальный фикс порядка фаз (каскад/раскладка до
  скриптов), закрывает механизм этой заявки.
* [BUG-555](BUG-555-DUPLICATE.md) — тот же механизм на `getComputedStyle`,
  уже закрыт дубликатом BUG-443.
