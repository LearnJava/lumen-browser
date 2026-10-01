# BUG-1189 — нет интерфейса `PerformanceEntry`

**Статус:** FIXED 2026-10-01 (P3)
**Заведён:** 2026-09-26 (P6, при закрытии [BUG-648](BUG-648-FIXED.md)).
**Область:** js — [`crates/js/src/shim/performance_shim.js`](../crates/js/src/shim/performance_shim.js)
(записи `mark`/`measure`/`navigation`/`resource`/`paint` — простые объектные литералы).

## Симптом

`typeof PerformanceEntry` → `'undefined'`; `performance.getEntries()[0] instanceof PerformanceEntry`
бросает `ReferenceError`. WPT `performance-timeline/idlharness.any.html`: после починки BUG-648
`idl_test setup` проходит и доходит до проверок интерфейсов; все 13 подтестов `PerformanceEntry
interface: …` падают на `self does not have own property "PerformanceEntry"` (раньше эти подтесты не
выполнялись вовсе). Chrome: интерфейс есть, у записей — `PerformanceMark`/`PerformanceMeasure`/
`PerformanceNavigationTiming`/`PerformanceResourceTiming`/`PerformancePaintTiming` с общим предком.

## Что требуется

Performance Timeline L2 §3: `[Exposed=(Window,Worker)] interface PerformanceEntry` с `name`,
`entryType`, `startTime`, `duration`, `id`, `navigationId` (геттеры на прототипе) и `toJSON()`; все
записи, которые отдаёт `performance.getEntries*()` и `PerformanceObserverEntryList`, — экземпляры
его наследников. Критерий: подтесты `PerformanceEntry interface: …` в `idlharness.any.html`
проходят, `performanceentry-tojson.any.html` не регрессирует.

## Как исправлено (P3, 2026-10-01)

Базовый `PerformanceEntry` в [`performance_shim.js`](../crates/js/src/shim/performance_shim.js) (акcессоры `id`/`name`/`entryType`/`startTime`/`duration`/`navigationId` без сеттеров, `[Default] toJSON`, `new` бросает TypeError); прототипы Mark/Measure/PaintTiming/LCP/LayoutShift/ResourceTiming (и через него NavigationTiming) наследуют от него. Поля записей теперь создаются `_perf_put` (defineProperty), т.к. присваивание поверх унаследованного акcессора без сеттера молча не создаёт поле. Не тронуты `PerformanceLongTaskTiming`/`TaskAttributionTiming` (`long_tasks.rs`, присваивание в конструкторе). Тест `performance_entry_is_shared_base_of_entries`. WPT-прогон `idlharness` не выполнялся.
