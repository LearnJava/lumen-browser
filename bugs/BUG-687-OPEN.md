# BUG-687: `performance.mark()`/`performance.measure()` entries are plain objects, not `PerformanceMark`/`PerformanceMeasure` instances

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:8227` — `performance.mark()`; `crates/js/src/dom.rs:8247` — `performance.measure()`)
**Найден:** P2, WPT-VENDOR-timing-entrytypes-registry, 2026-08-06

## Симптом

`timing-entrytypes-registry` (скоуп ⬜, кандидат) — вендорена и прогнана
целиком (`run_report.py --all --root timing-entrytypes-registry
--recursive`, ~45 с, 2 файла/2 id): **0/2 harness OK, 2/8 сабтестов**.

`registry.any.js` observes `mark`/`measure`/`resource` via a
`PerformanceObserver`, then does `performance.mark('mymark')` and
`performance.measure('mymeasure')`. Both deliveries fire and the
observer callback runs, but:

```
FAIL 'mark' entries should be observable - assert_equals: Class name of
entry should be PerformanceMark. expected "[object PerformanceMark]" but
got "[object Object]"
FAIL 'measure' entries should be observable - assert_equals: Class name
of entry should be PerformanceMeasure. expected "[object PerformanceMeasure]"
but got "[object Object]"
```

`registry.window.js` shows the same for `navigation` (already
[BUG-673](bugs/BUG-673-OPEN.md) — not a new finding here).

## Причина

`crates/js/src/dom.rs` builds mark/measure entries as plain object
literals instead of instances of a registered interface constructor:

```js
// line 8227
var entry = { entryType: 'mark', name: String(name), startTime: start, duration: 0 };
// line 8247
var entry = { entryType: 'measure', name: String(name), startTime: start, duration: end - start };
```

Neither `PerformanceMark` nor `PerformanceMeasure` exists as a global
constructor in the shim (`typeof window.PerformanceMark ===
"undefined"`, likewise `PerformanceMeasure`) — `Object.prototype.toString.call(entry)`
therefore falls back to the generic `[object Object]` tag instead of
`[object PerformanceMark]`/`[object PerformanceMeasure]`.

Same defect class as [BUG-645](bugs/BUG-645-OPEN.md)
(`PerformancePaintTiming`) and [BUG-673](bugs/BUG-673-OPEN.md)
(`PerformanceResourceTiming`/`PerformanceNavigationTiming`) — WebIDL
interface objects for `PerformanceEntry` subtypes are systematically
absent as globals even though the delivery mechanism itself
(`_perf_entries`/`PerformanceObserver`) is genuinely wired and working.
This extends the same gap to the two entry types created entirely
client-side by `performance.mark()`/`.measure()` (User Timing L3),
rather than by a native hook.

## Вторичные находки (реконфирмация, не новые)

- `registry.any.js`'s `resource` subtest: NOTRUN, whole test TIMEOUT.
  The category's own `fetch(self.location.href + "?" + Math.random())`
  never produces a `resource` entry — reconfirmation of
  [BUG-520](bugs/BUG-520-OPEN.md) (Resource Timing hook exists but the
  network layer never calls it for real loads).
- `registry.window.js`'s `paint`/`longtask` subtests: both NOTRUN, whole
  test TIMEOUT. `paint` entries are only delivered once, on the first
  non-empty display list of a page load (see BUG-645's root-cause
  description of `crates/shell/src/main.rs`'s `deliver_paint_timing`
  call sites) — a later DOM mutation (`document.head.parentNode.appendChild(...)`)
  never produces a second one. `longtask` is listed in
  `supportedEntryTypes` but never actually generated — reconfirmation of
  [BUG-354](bugs/BUG-354-OPEN.md).

## Как воспроизвести

```
tests/wpt/run_report.py --binary <lumen.exe> --all --root timing-entrytypes-registry --recursive
```
или живая проба: `eval("performance.mark('m'); typeof window.PerformanceMark")`
→ `"undefined"` на любой странице.
