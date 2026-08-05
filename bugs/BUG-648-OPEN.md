# BUG-648: `PerformanceObserver.observe()`/notification pipeline implements neither call-signature validation nor the spec's task-queued delivery model

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:8258-8335` — `PerformanceObserver` constructor/`observe`/`disconnect`/`takeRecords`/`_perf_observer_notify`, and every `_lumen_deliver_*`/`performance.mark`/`performance.measure` call site that invokes it)
**Найден:** P2, WPT-VENDOR-performance-timeline, 2026-08-05

## Симптом

`performance-timeline` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root performance-timeline --recursive`, ~4.5 мин,
51 id): **36/51 harness OK, 19/69 сабтестов**. Unusually high signal for
this backlog (most 🚫/⬜ categories in this stretch return 0-2 subtests) —
`PerformanceObserver` is a real, wired-up implementation, so its
conformance gaps are directly observable rather than masked by an
unimplemented API.

Two distinct, both spec-normative, defects account for essentially every
`PerformanceObserver`-specific subtest failure (as opposed to the
already-known unrelated gaps listed under Реконфирмации below):

**1. `observe()` performs no call-signature validation at all**
(`po-observe-type.any.html`, 4/5 subtests FAIL; `po-observe.any.html`,
1/3 subtests FAIL — `entryTypes must be a sequence or throw a
TypeError`):

```js
obs.observe({});                                    // must throw TypeError, doesn't
obs.observe({entryTypes: ["mark"]});
obs.observe({type: "measure"});                      // must throw InvalidModificationError, doesn't
obs.observe({type: "mark", entryTypes: ["measure"]}); // must throw TypeError, doesn't
```

Per Performance Timeline L2 §6.2.2 "register a performance observer",
`observe()` must throw `TypeError` when neither `type` nor `entryTypes`
is present (or a non-array `entryTypes`), and `InvalidModificationError`
when an observer that already registered via one form (single-`type` vs
multi-`entryTypes`) is re-registered via the other. Lumen's implementation
(`dom.rs:8273-8303`) has no throw statement anywhere in the function — any
input silently normalizes to an (possibly empty) type list.

**2. Observer notification runs synchronously, in-line at entry-creation
time, instead of being queued as a task** (`po-disconnect.any.html`
"An observer disconnected after a mark must not have its callback
invoked" FAIL, "Reached unreachable code"; `po-disconnect-removes-
observed-types.any.html` FAIL; `po-callback-mutate.any.html` FAIL;
`po-takeRecords.any.html` "expected 3 but got 5" FAIL;
`buffered-does-not-sync-invoke.html` TIMEOUT; `po-mark-measure.any.html`
TIMEOUT):

```js
mark: function(name, opts) {
    ...
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);   // dom.rs:8194 — fires the callback right here
    return entry;
},
```

`_perf_observer_notify` is called directly from `performance.mark()`
(`dom.rs:8194`), `.measure()` (`dom.rs:8214`), and every `_lumen_deliver_*`
native-entry-point (paint/LCP/layout-shift/…, `dom.rs:8342` onward) — none
of these wrap the call in `queueMicrotask`/a task queue, even though
`queueMicrotask` already exists and is used elsewhere in the same file
(e.g. mutation observers, `dom.rs:6957`). Per §5.1/§10.3 ("queue a
PerformanceObserverCallback"), delivery must happen via a queued task, not
inline in the same synchronous turn that created the entry. Concretely
reproduced by `po-disconnect.any.html`'s second case: `observer.observe();
performance.mark("mark1"); observer.disconnect(); performance.mark
("mark2")` expects the callback to fire **zero** times (the queued task
for mark1's notification hasn't run yet when `disconnect()` executes, so
it's cancelled) — Lumen fires it immediately inside `mark()`, before
`disconnect()` gets a chance to run, hitting `assert_unreached`. The same
in-line-delivery model also explains the `buffered: true` case
(`PerformanceObserver.prototype.observe`, `dom.rs:8295-8302`, calls
`_perf_deliver_to_observer` directly inside `observe()` — literally what
`buffered-does-not-sync-invoke.html`'s title says must not happen) and the
`takeRecords()` overcount (an observer that already received an entry via
synchronous delivery has no "pending, undelivered records" queue distinct
from `_perf_entries`, so `takeRecords()` re-returns entries the callback
already saw).

## Причина

Both defects trace to the same shortcut in `dom.rs:8253-8335`: the shim
implements `PerformanceObserver` as straightforward JS convenience code
(collect types into an array, filter `_perf_entries` by type, call the
callback) rather than the spec's normative algorithm, which requires (a)
validating the *shape* of the `observe()` argument against the observer's
prior registration state, and (b) maintaining a separate queued-task
delivery path with its own pending-records buffer, distinct from the
synchronous `_perf_entries` push.

## Реконфирмации (не новые)

- `PerformanceObserverEntryList is not defined` (`po-observe.any.html`) —
  same class as [BUG-645](bugs/BUG-645-OPEN.md)/[BUG-624](bugs/BUG-624-OPEN.md)/
  [BUG-637](bugs/BUG-637-OPEN.md)/[BUG-589](bugs/BUG-589-OPEN.md): WebIDL
  interface objects absent as globals even where the underlying behavior
  (the plain-object "list" passed to callbacks, `dom.rs:8319-8326`) works.
- `case-sensitivity.any.html` (`resources/square.png?id=1` never loads,
  "fetch error: invalid url: invalid url: missing scheme") — same class
  as [BUG-347](bugs/BUG-347-OPEN.md) (`fetch()`/resource loading doesn't
  resolve relative URLs).
- `timing-removed-iframe.html` (`Cannot read properties of null (reading
  'performance')` on a detached iframe's `contentWindow`) — same class as
  the already-documented `<iframe>` no-separate-browsing-context gap
  (BUG-480 lineage, per `STATUS-PN`/`focus` session notes).
- `navigation-id-*.tentative.html`, `not-restored-reasons/*.window.html`
  (`ReferenceError: RemoteContext is not defined` / `token is not
  defined`) — `/common/dispatcher/dispatcher.js` is category-external and
  not vendored (`tests/wpt/common/` doesn't exist), same established gap
  as `navigation-timing`/`mixed-content`.
- `supportedEntryTypes` listing types with no real delivery mechanism
  (`element`/`event`/`first-input`/`longtask`/`soft-navigation`) — already
  filed as [BUG-354](bugs/BUG-354-OPEN.md).

## Новая, не реконфирмационная находка вне PerformanceObserver

`not-restored-reasons/abort-block-bfcache.window.html` FAILs on
`window.stop is not a function` — `window.stop()` (HTML LS §7.4.1) does
not exist on the `window` shim at all. Unrelated to the two defects above;
noted here rather than filed separately since it's a single-file,
single-line finding with no further investigation performed.

## Как воспроизвести

```
tests/wpt/run_report.py --binary <lumen.exe> --all --root performance-timeline --recursive
```
or a live probe:
```js
var o = new PerformanceObserver(function(){ throw new Error('called'); });
o.observe({entryTypes:['mark']});
performance.mark('m1');
o.disconnect();
// spec: callback must NOT have run by here. Lumen: it already threw.
```
