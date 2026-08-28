# BUG-696: `performance.mark()`/`performance.measure()` perform zero argument validation

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:8366-8392` — `performance.mark`/`performance.measure` in `WEB_API_SHIM`)
**Найден:** P2, WPT-VENDOR-user-timing, 2026-08-09

## Симптом

`user-timing` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root user-timing --recursive`, ~1:42, 44 файла,
36 отобранных id): **27/36 harness OK, 66/176 сабтестов**. The dominant
failure cluster (61 of 88 unexpected subtests, spread across
`mark-errors.any.html`, `mark_exceptions.html`, `measure-exceptions.html`,
`measure_exception.html`, `measure_exceptions_navigation_timing.html`,
`measure_syntax_err.any.html`, `invoke_with_timing_attributes.html`,
`invoke_without_parameter.html`) is a single root cause: neither `mark()`
nor `measure()` validates its arguments at all.

```js
performance.mark("navigationStart");           // must throw SyntaxError, doesn't
performance.mark("mark1", 123);                 // must throw TypeError, doesn't
performance.mark("mark1", NaN);                 // must throw TypeError, doesn't
performance.mark("mark1", "string");             // must throw TypeError, doesn't
performance.mark("mark1", {startTime: -1});     // must throw TypeError, doesn't
performance.measure("m");                        // must throw TypeError (name required), doesn't
performance.measure("m", "NonExistMark1");       // must throw SyntaxError, doesn't
performance.measure("m", "start", "end");        // non-existent marks → must throw SyntaxError, doesn't
performance.measure("m", {start: 0, duration: 2, end: 3}); // conflicting dict → must throw TypeError, doesn't
performance.measure("m", {start: 1, detail: Symbol()});    // undeserializable detail → must throw DataCloneError, doesn't
```

Per User Timing L3 §3.1 `mark(markName, markOptions)`: step 3 must throw
`SyntaxError` if `markName` matches a `PerformanceTiming` navigation
attribute name (the legacy `navigationStart`/`unloadEventStart`/…/
`loadEventEnd` list, still exercised by
`invoke_with_timing_attributes.html`/`invoke_without_parameter.html`), and
step 4 ("run the mark options validity check") must throw `TypeError` when
`markOptions` is neither `undefined` nor a coercible dictionary object, or
when `startTime` is negative. Per §3.3 `measure(measureName,
startOrMeasureOptions, endMark)`: missing/non-existent named marks must
throw `SyntaxError`, and conflicting `{start, duration, end}` combinations
in the options-dict form must throw `TypeError`. None of these checks
exist — `dom.rs:8366-8392` unconditionally builds an entry object from
whatever was passed:

```js
mark: function(name, opts) {
    var start = (opts && typeof opts.startTime === 'number') ? opts.startTime : performance.now();
    var entry = { entryType: 'mark', name: String(name), startTime: start, duration: 0 };
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);
    return entry;
},
measure: function(name, startMark, endMark) {
    var start = 0, end = performance.now();
    if (typeof startMark === 'string') { ... } else if (typeof startMark === 'number') { start = startMark; }
    if (typeof endMark === 'string') { ... } else if (typeof endMark === 'number') { end = endMark; }
    var entry = { entryType: 'measure', name: String(name), startTime: start, duration: end - start };
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);
    return entry;
},
```

`opts.startTime` is read only if it's already a `number` (silently
ignoring `NaN`/`Infinity`/negative/non-numeric values instead of
rejecting them); a non-string/non-number `opts` is simply not inspected
at all instead of being validated as a dictionary; `measure()` treats any
unresolved mark name as start=0/end=now() instead of throwing; `measure()`
never accepts (or validates) the options-dict overload's `detail` field at
all — `structured-serialize-detail.any.js`'s two `detail`-cloning subtests
FAIL/error separately because `mark`/`measure` don't read `opts.detail`
into the entry, so `entry.detail` is always `undefined`.

## Причина

`mark()`/`measure()` in the `WEB_API_SHIM` (`crates/js/src/dom.rs`) are
Phase-0 convenience code — they compute the entry's `startTime`/`duration`
from whatever was passed and never implement User Timing L3's normative
validation algorithm (§3.1 steps 3-4, §3.3's SyntaxError/TypeError
matrix, or the `detail` structured-clone step).

## Реконфирмации (не новые)

- `[object Object]` instead of `[object PerformanceMark]`/
  `[object PerformanceMeasure]`, `PerformanceMark`/`PerformanceMeasure is
  not defined` (`mark-entry-constructor.any.html`,
  `mark-measure-return-objects.any.html`, `user-timing-tojson.html`) —
  same as [BUG-687](BUG-687-OPEN.md) (entries are plain objects, not
  real `PerformanceMark`/`PerformanceMeasure` instances; no `toJSON()`).
- `clearMarks.html`/`clearMeasures.html`/`mark.html`/`measure.html`/
  `measures.html`/`measure_associated_with_navigation_timing.html`/
  `measure_navigation_timing.html` TIMEOUT — all use
  `<body onload="...">`/`<body onload=...>` inline event-handler content
  attributes to kick off the test; same as [BUG-360](BUG-360-FIXED.md)
  (inline event-handler attributes were dead at the time of this run — the
  fix landed in main the same day, in parallel; not re-run against it).
- `idlharness.any.html` TIMEOUT — `/resources/idlharness.js` 404, the
  established not-vendored-`idlharness.js` gap already documented for
  other categories (`touch-events`, `trusted-types`, …).

## Как воспроизвести

```
tests/wpt/run_report.py --binary <lumen.exe> --all --root user-timing --recursive
```
or a live probe:
```js
try { performance.mark("navigationStart"); console.log("no throw"); }
catch (e) { console.log(e.name); }   // spec: SyntaxError. Lumen: "no throw"
```
