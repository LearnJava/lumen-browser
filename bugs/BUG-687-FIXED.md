# BUG-687: `performance.mark()`/`performance.measure()` entries are plain objects, not `PerformanceMark`/`PerformanceMeasure` instances

**Статус:** FIXED 2026-09-28 (P3, ветка `p3-bug687-perf-mark`)
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
[BUG-673](BUG-673-FIXED.md) — not a new finding here).

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

Same defect class as [BUG-645](BUG-645-FIXED.md)
(`PerformancePaintTiming`) and [BUG-673](BUG-673-FIXED.md)
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
  [BUG-520](BUG-520-FIXED.md) (Resource Timing hook exists but the
  network layer never calls it for real loads).
- `registry.window.js`'s `paint`/`longtask` subtests: both NOTRUN, whole
  test TIMEOUT. `paint` entries are only delivered once, on the first
  non-empty display list of a page load (see BUG-645's root-cause
  description of `crates/shell/src/main.rs`'s `deliver_paint_timing`
  call sites) — a later DOM mutation (`document.head.parentNode.appendChild(...)`)
  never produces a second one. `longtask` is listed in
  `supportedEntryTypes` but never actually generated — reconfirmation of
  [BUG-354](BUG-354-FIXED.md).

## Как воспроизвести

```
tests/wpt/run_report.py --binary <lumen.exe> --all --root timing-entrytypes-registry --recursive
```
или живая проба: `eval("performance.mark('m'); typeof window.PerformanceMark")`
→ `"undefined"` на любой странице.

## Исправление (2026-09-28, P3)

`crates/js/src/shim/performance_shim.js` (общий для страницы и воркеров):

- `PerformanceMark` — конструируемый интерфейс по User Timing L3 §4.2:
  `new.target`-проверка, `SyntaxError` на имя из `PerformanceTiming` (только в
  Window-скоупе), `TypeError` на не-словарь в `markOptions` и на
  отрицательный/неконечный `startTime`, `detail` — `structuredClone`.
  `performance.mark()` теперь буквально «конструктор + буфер + наблюдатели».
- `PerformanceMeasure` — без конструктора (`Illegal constructor`),
  `measure()` строит запись от его прототипа.
- Форма WebIDL, которую проверяет `user-timing/idlharness`: `detail` — геттер
  на прототипе (значение в неперечислимом слоте), `length` интерфейса и
  операций `mark`/`measure`/`clearMarks`/`clearMeasures` без опциональных
  аргументов, `mark()`/`measure()` без аргументов — `TypeError`,
  неперезаписываемый `prototype`, неперечислимые глобалы (function expression +
  `defineProperty(globalThis, …)`, как `_perf_po_iface`).
- `Symbol.toStringTag` у обоих — именно его читает `registry.any.js`.

Тот же дефект у остальных entry-типов, которые перечисляет
`registry.window.js`: `[object Object]` вместо имени интерфейса. Тег добавлен
`PerformancePaintTiming`/`LargestContentfulPaint`/`LayoutShift`(`Attribution`)/
`PerformanceResourceTiming`/`PerformanceNavigationTiming`
(`web_api_shim_tail.js`, `_lumen_idl_tag`), `PerformanceLongTaskTiming`/
`TaskAttributionTiming` (`long_tasks.rs`), `PerformanceScriptTiming`/
`PerformanceLongAnimationFrameTiming` (`long_animation_frames.rs`),
`PerformanceSoftNavigationEntry` (`soft_navigation.rs`).

WPT (`run_report.py --all --recursive`, бинарь до правки против после):
`timing-entrytypes-registry` **2/9 → 8/9** сабтестов (`mark`/`measure`/
`resource`/`navigation`/`paint`; остаток — `supportedEntryTypes` в воркере,
`PerformanceObserver` там нет); `user-timing` **595/824 → 757/824**. Baseline
обеих категорий (`tests/wpt/metadata/`) переснят этим же коммитом. В нём
появились FAIL, которых раньше не было, — это сабтесты, до которых прогон
прежде не доходил: `measure*.html` (файлы были ERROR) падают на валидации
`measure()` — [BUG-696](BUG-696-OPEN.md); `idlharness` — на отсутствующем
интерфейсе `PerformanceEntry` (не выставлен ни для одного entry-типа).
`idlharness.any.serviceworker.html` записан TIMEOUT вместо ERROR — бинарь до
правки даёт тот же TIMEOUT, старый baseline был устаревшим.
`performance-timeline` — без изменений baseline (`--check`: единственные
отклонения — флейки `idlharness.any.worker.html` TIMEOUT/OK).

Регрессия — `performance_mark_and_measure_interfaces_back_entries` и
`performance_entry_interfaces_have_class_strings`
(`crates/js/src/dom/tests/v8_perf_observers.rs`),
`v8_worker_user_timing_works_without_performance_observer` (`worker.rs`,
интерфейсы в воркерном скоупе).
