# BUG-1189 — нет интерфейса `PerformanceEntry`

**Статус:** OPEN
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
