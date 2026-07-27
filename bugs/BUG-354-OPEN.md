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

## Возможный фикс (не реализован в этой сессии)

Two independent halves, both small:

- Trim the literal to the implemented seven (`largest-contentful-paint`,
  `layout-shift`, `mark`, `measure`, `navigation`, `paint`, `resource`), and
  add each back in the same commit that implements its entry type. Better still,
  build the array from the same table `_lumen_deliver_perf_entry` dispatches on,
  so the two cannot drift again.
- Make `observe()` reject/no-op-with-warning consistently for a type not in that
  list, so `supportedEntryTypes` and `observe()` agree by construction.

Note this is a *narrowing* change: it makes `PerformanceElementTiming`-gated
code take its fallback path instead of a dead path. Implementing Element Timing
itself (`PerformanceElementTiming`, the `elementtiming` attribute, image/text
paint timestamps) is a separate, much larger piece of work — not this bug.

Not fixed in this session — P2-wpt vendors and surveys, code fixes are P3's lane
(`CLAUDE.md` developer assignments).
