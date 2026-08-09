# BUG-354 — `PerformanceObserver.supportedEntryTypes` advertises 5 entry types that are not implemented at all

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:11067-11074` — the hardcoded `supportedEntryTypes` accessor; `observe()` at `dom.rs:11075` for the second half)
**Найден:** P2, WPT-VENDOR-element-timing (2026-07-27), `run_report.py --all --root element-timing --recursive`

## Симптом

`PerformanceObserver.supportedEntryTypes` returns a fixed 12-element list:

```
["element","event","first-input","largest-contentful-paint","layout-shift",
 "longtask","mark","measure","navigation","paint","resource","soft-navigation"]
```

Of those, **five have no implementation whatsoever** — no entry constructor, no
entry ever produced, nothing in `crates/js/`:

| Advertised entry type | Backing in Lumen |
|---|---|
| `element` | none — `PerformanceElementTiming` is `undefined` |
| `event` | none — `PerformanceEventTiming` is `undefined` |
| `first-input` | none (same `PerformanceEventTiming` family) |
| `longtask` | none — `PerformanceLongTaskTiming` is `undefined` |
| `soft-navigation` | none |

The other seven (`largest-contentful-paint`, `layout-shift`, `mark`, `measure`,
`navigation`, `paint`, `resource`) do produce entries.

Verified outside WPT with `--dump-layout` on a probe page:

```
PROBE supported=["element","event","first-input","largest-contentful-paint","layout-shift",
                 "longtask","mark","measure","navigation","paint","resource","soft-navigation"]
PROBE PerformanceElementTiming=undefined PerformanceEventTiming=undefined
      PerformanceLongTaskTiming=undefined
PROBE observe(element) ok, entries=
PROBE getEntriesByType(element)=[]
```

Second half of the defect: `observe({type: 'element'})` **succeeds silently** and
then never delivers a callback. Per Performance Timeline L2 §3.2, `observe()`
with a single `type` the UA does not support must be a no-op *and* the UA
should warn; the fatal part here is not the no-op but that the preceding
feature-detection said the type was supported, so the page has no way to branch.

## Причина

`supportedEntryTypes` is a hand-written literal array, not derived from what the
shim actually registers. It was presumably written as the aspirational full
Performance Timeline list rather than the implemented subset, and has since
drifted as only part of that list got implemented.

## Масштаб

`supportedEntryTypes` exists in the spec *for* feature detection — it is the
canonical way both WPT (`assert_implements(window.PerformanceElementTiming, …)`
is the fallback; RUM libraries use `supportedEntryTypes.includes(t)` directly)
and real-world analytics/RUM code decide whether to install an observer. Every
major RUM library (web-vitals.js and friends) gates on it. A false positive there
means the page installs an observer that never fires and takes the "supported"
branch — worse than a clean absence, which would take the fallback branch.

In the `element-timing` WPT category itself the mis-advertisement is directly
visible: **the single passing subtest of the whole 52-test category** is
`supported-element-type.html` → *"supportedEntryTypes contains 'element'."*,
which passes only because of this bug, while the other 50 subtests fail on
`assert_implements: PerformanceElementTiming is not implemented`.

**Second confirmation, WPT-VENDOR-event-timing (2026-07-28).** The `event-timing`
category is almost entirely `testdriver.js`-bound (67 of its 71 ids SKIP), so
exactly two tests executed — and *both* of them are feature-detection tests that
land on this bug:

* `supported-types.window.html` — `assert_implements(window.PerformanceEventTiming,
  'Event Timing is not supported.')` fails: the interface does not exist, yet the
  list advertises the entry types it backs.
* `supported-types-consistent-with-self.html` — fails `assert_equals: expected
  false but got true`. This test is written specifically to catch *this* class of
  defect: it compares `supportedEntryTypes.includes('first-input')` /
  `.includes('event')` against `'PerformanceEventTiming' in self` and requires the
  two to agree. Lumen answers `true` / `false` — the exact inconsistency described
  above, caught without needing any of the API to work.

So the two categories whose entry types are mis-advertised (`element` from
`element-timing`, `event`+`first-input` from `event-timing`) have now both been
run, and both point at the same literal. `soft-navigation`/`longtask` are still
unvendored — expect the same result there.

## Фикс (P3, 2026-08-09)

Both halves from the "possible fix" section implemented, `crates/js/src/dom.rs`
(`WEB_API_SHIM`, `PerformanceObserver`):

- New `_PERF_SUPPORTED_ENTRY_TYPES` array — single source of truth for both
  halves, so they cannot drift apart again — trimmed to the seven entry types
  an entry constructor actually produces on the live document:
  `largest-contentful-paint`, `layout-shift`, `mark`, `measure`, `navigation`,
  `paint`, `resource`. The `supportedEntryTypes` getter now returns a copy of
  this array instead of the old 12-element literal.
- `observe()` gates admission against the same array per Performance Timeline
  L2 §6.2.2: the single-type form (`observe({type})`) aborts with a
  `console.warn` and no-op when `type` is unsupported; the multi-type form
  (`observe({entryTypes})`) drops each unsupported entry individually (same
  warning) instead of subscribing to it. Delivery to `getEntriesByType()`/the
  entry buffer is untouched — `_lumen_deliver_perf_entry` (a generic Rust→JS
  hook, not itself part of this bug) can still push any `entryType` string
  into the buffer; only the *observer* notification path is now gated.

`element`/`event`/`first-input`/`longtask` were confirmed to have zero
producing code anywhere in `crates/js/` (no `PerformanceElementTiming`/
`PerformanceEventTiming`/`PerformanceLongTaskTiming` constructor exists).
`soft-navigation` was excluded too: `PerformanceSoftNavigationEntry` exists as
a class (`crates/js/src/soft_navigation.rs`) but its delivery hook
`_lumen_deliver_soft_nav` has no caller anywhere in the engine outside unit
tests — tracked separately as [BUG-678](BUG-678-OPEN.md), not fixed here.

One existing unit test (`deliver_perf_entry_notifies_observer`) exercised the
generic `_lumen_deliver_perf_entry` hook through `observe({entryTypes:
['longtask']})`; switched to `'navigation'` (a genuinely supported type) so it
still exercises the same delivery path without asserting on a type `observe()`
now correctly rejects. `cargo test -p lumen-js --features v8-backend --lib`
2506/2506 green, `cargo clippy -p lumen-js --features v8-backend -- -D
warnings` clean.

This is a narrowing change: `PerformanceElementTiming`-gated feature-detection
code now correctly takes its fallback branch instead of a dead "supported"
branch. Implementing Element Timing/Event Timing/Long Tasks/Soft Navigation
themselves is out of scope — separate, larger pieces of work.
