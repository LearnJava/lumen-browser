# BUG-564: `document.fonts.ready` is missing — `FontFaceSet` has no `ready` property at all

**Статус:** FIXED 2026-08-23 (P1)
**Компонент:** js (`crates/js/src/dom.rs::_lumen_get_fonts` — the `fontSet` object
literal returned for `document.fonts` has `length`/`item`/`entries`/`forEach`/
`Symbol.iterator` but no `ready` property)
**Найден:** P2, WPT-RUN-3 срез 43 (`css/css-anchor-position`), 2026-08-04

## Симптом

Any script that awaits font loading before measuring layout —
`document.fonts.ready.then(() => …)` — throws synchronously:

```
script error: JS runtime error: Cannot read properties of undefined (reading 'then')
```

`document.fonts` itself exists (`typeof document.fonts === 'object'`), but
`document.fonts.ready` evaluates to `undefined` rather than a `Promise`, so
`.then` throws `TypeError`. When this line sits in the last `<script>` block of
a `testharness.js` file with `setup({explicit_done: true})` (or simply as the
only path that would ever call `done()`), the throw prevents `done()` from
ever being scheduled and the harness only completes via its own ~10s
(`--timeout-multiplier`-scaled since the WPT-RUN-3 срез 42 fix) default
file-level timeout — `harness_status: TIMEOUT` with whatever subtests
happened to register on
earlier, non-throwing script blocks (0, 1, or more depending on the file).

Confirmed by direct source read (`crates/js/src/dom.rs`, `_lumen_get_fonts`,
~line 6779) and live `--mcp-live-port` probe against
`css/css-anchor-position/anchor-getComputedStyle-002.html` and
`anchor-name-inline-001.html`: `typeof document.fonts.ready` is `"undefined"`.

## Причина

The CSS Font Loading Module (Level 3, `readonly attribute Promise<FontFaceSet>
ready`) requires `FontFaceSet.ready` — resolved once the set's pending font
loads settle, and already-resolved if there are none pending. Lumen's
`document.fonts` binding (`_lumen_get_fonts` in the JS shim) is a static
snapshot object built once per access (`_lumen_fonts_size`/`_lumen_fonts_get`
native calls, no live loading-state tracking) and simply never adds a `ready`
property to the returned object literal. Two sibling constructs in the same
file already use the exact fallback this needs —
`this.ready = Promise.resolve();` (`dom.rs:8805`) and
`this.ready = Promise.resolve(self);` (`dom.rs:15403`) — so the fix is a
one-line addition to the `fontSet` object literal, not new engine machinery;
Lumen doesn't track in-flight `@font-face` loads through this shim, so a
resolved-Promise stub is the correct minimum (matches upstream engines'
behavior once all fonts referenced by the document have already settled,
which is the state `document.fonts` snapshots at call time here).

## Масштаб

Found via WPT-RUN-3 srez 43's individual triage of `css-anchor-position`'s
remaining TIMEOUT cluster (31 files left unexplained after срез 42): explains
2 of 31 — `anchor-getComputedStyle-002.html` (1/2 subtests had already
registered before the throw) and `anchor-name-inline-001.html` (0 subtests —
the only script block is the throwing one). Not scoped further outside this
category; any test on any page waiting on `document.fonts.ready` will hit the
same throw.

**Re-measured corpus-wide, WPT-RUN-6 (2026-08-20/21), against the completed
Windows WPT-RUN-5 snapshot (479/479 shards, `docs/wpt/runs/2026-08-20-windows-partial.json`
+ raw per-shard reports in `.tmp/wpt-corpus/`):** 660 files in the vendored
corpus (`tests/wpt/`) reference `document.fonts.ready` (`grep -rl`), most
following the same `<body onload="document.fonts.ready.then(() => {
runTests(); })">` idiom seen in `css/css-grid/alignment/*` (Ahem-font
baseline/layout checks — `runTests()`/`done()` never fires). Of the 452 of
those 660 ids that were actually executed in the Windows run (the rest fell
in shards budget-killed before reaching them or in categories the harness
never started), **384 timed out — 85.0%**, against a 15.2% baseline TIMEOUT
rate across the whole run (6205/40850). This is the single largest identified
TIMEOUT mechanism outside the Worker family (see [BUG-778](BUG-778-FIXED.md)),
spans dozens of categories (`css-grid`, `css-flexbox`, `css-align`,
`css-anchor-position`, `css-display`, and more — not isolated to
`css-anchor-position` as originally found), and per the "Причина" section
above is still a one-line fix. 452/660 is a floor, not the true blast
radius: 166 of 479 shards in this run produced no per-test data at all
(budget-killed before writing a report or crashing near-instantly — see
`docs/tasks/p2-wpt-runner-throughput.md`), so the true count of affected ids
is higher once those categories are covered by a future run. Audit script
(scratch, not committed): `.tmp/fonts_ready_overlap.py` in the `p2-wpt-run-6`
session — grep the vendored corpus for the string, cross-reference against
`test_end` status entries in `.tmp/wpt-corpus/*.json`/`*.raw.jsonl`.

## Исправлено (P1, 2026-08-23)

Ровно one-line fix из раздела «Причина»: `fontSet` object literal
(`_lumen_get_fonts`, `crates/js/src/dom.rs`) получил `ready: Promise.resolve()`
тем же паттерном, что уже использовался в двух соседних местах шима
(`dom.rs:8805`/`dom.rs:15403`). Lumen не отслеживает висящие загрузки
`@font-face` через этот шим, поэтому уже-resolved промис — корректный
минимум (совпадает с поведением апстрим-движков, когда все шрифты документа
уже устоялись к моменту обращения к `document.fonts`, а именно это состояние
и снимает шим при вызове). Юнит-тест
`dom::tests::v8_ws_sse::document_fonts_ready_is_a_thenable_promise`
(`crates/js/src/dom.rs`) проверяет `document.fonts.ready instanceof Promise`
и наличие `.then`.

Масштаб фикса не переизмерялся (нужен новый прогон WPT-RUN); ожидаемый эффект
— 384 замеренных TIMEOUT (WPT-RUN-6, раздел «Масштаб» выше) должны стать
быстрыми PASS/FAIL вместо ~10с таймаута на файл.
