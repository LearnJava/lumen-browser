# BUG-609: `HTMLOptionsCollection.length` setter doesn't grow `<select>` for valid large values

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_make_nid_collection`'s Proxy; `crates/js/src/shim/web_api_shim_tail_b.js` — `_lumen_options_set_length`, `HTMLOptionsCollection`'s `options` getter, `HTMLSelectElement.prototype.length`)
**Найден:** P2, WPT-VENDOR-html-misc, 2026-08-04

## Симптом

```
FAIL select options.length too large 3 - assert_equals: Length of <select> should be 100,000 expected 100000 but got 3
FAIL select options.length too large 4 - assert_equals: Manual expansion still works expected 100002 but got 5
```
(`select/options-length-too-large.html`)

## Причина

Per the DOM/HTML `HTMLOptionsCollection` `length` setter algorithm, setting
`select.options.length = N` for an in-range `N` (`0 <= N <= 100000`) must
grow the collection by appending empty `<option>` elements up to length `N`
(and shrink/truncate for smaller in-range `N`). Out-of-range values
(negative, `> 100000`) must be silently ignored, leaving the collection
unchanged. Lumen's setter correctly ignored the three out-of-range cases,
but the growth path was a no-op — `_lumen_make_nid_collection`'s Proxy had
no `set` trap at all, so `options.length = N` silently landed on the empty
plain `proto` object behind the Proxy and never touched the DOM.

## Масштаб

Self-contained, 1 file, 2/5 subtests (the 3 out-of-range-rejection subtests
already passed).

## Срез P3 2026-09-16

`_lumen_make_nid_collection` (`web_api_shim_mid.js`) gained an optional
trailing `lengthSetFn` parameter and a `set` trap that calls it for
`prop === 'length'`, falling back to a plain property write otherwise. A new
shared helper `_lumen_options_set_length(select_nid, v)`
(`web_api_shim_tail_b.js`) implements the actual HTML LS §4.10.7 length
setting algorithm: out-of-range `v` (not `0 <= v <= 100000`) is ignored;
in-range, it truncates via `.remove()` (unchanged from before) or grows by
appending `new Option()` up to `v`. The `options` getter wires this in as
the new `lengthSetFn`. `HTMLSelectElement.prototype.length`'s setter — the
spec-identical twin algorithm per HTML LS, previously its own hand-rolled
truncate-only implementation with an explicit "growing is a no-op" comment —
now delegates to the same helper instead of duplicating (and under-)
implementing it.

**Residual, not part of this defect:** this engine caps the whole DOM arena
at `lumen_dom::MAX_DOM_NODES = 50_000` (BUG-418, pre-existing architectural
limit). The WPT file's own magnitude (`length = 100000`) exceeds that cap,
so `options-length-too-large.html` subtests 4/5 still won't pass end-to-end
in this engine — growth now runs correctly up to the cap and then throws
`QuotaExceededError` (the same overflow signal every other `createElement`
call gives past the arena limit), rather than silently doing nothing. That
is a distinct, already-tracked constraint, not a residue of this bug.

New tests `crates/js/src/dom/tests/v8_bug609_options_length_growth.rs`
(3/3 green, magnitudes chosen to stay well under the arena cap so they
isolate the growth-algorithm defect from BUG-418): out-of-range rejection
stays a no-op, in-range growth appends bare `<option>`s and interacts
correctly with manual `appendChild`/truncation, and `select.length`'s setter
shares the same growth behavior. `cargo test -p lumen-js --features
v8-backend` green (3737/3737 excluding 5 known-flaky, order-dependent
worker/threading tests that fail under full-suite parallelism and pass in
isolation — unrelated to this change). `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` clean. `scripts/scoped-test.sh`
green except the pre-existing, unrelated `cpu_snapshots_match_references`
golden drift (BUG-1008, same 7-file signature already on `main`).
