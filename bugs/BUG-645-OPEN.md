# BUG-645: `window.PerformancePaintTiming` interface object doesn't exist — blocks nearly all `paint-timing` WPT conformance

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `PerformanceObserver`/`_perf_entries` shim, ~line 8253-8432)
**Найден:** P2, WPT-VENDOR-paint-timing, 2026-08-05

## Симптом

`paint-timing` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root paint-timing --recursive`, ~4 мин): **34/56
harness OK, 1/36 сабтестов**.

Доминирующая причина отказа (20 из 36 сабтестов, все исполнившиеся файлы
вне `fcp-only/`): каждый тест начинается с

```js
assert_implements(window.PerformancePaintTiming, "Paint Timing isn't supported.");
```

(`tests/wpt/paint-timing/resources/utils.js:55` и `basetest.html:14`) —
и падает немедленно, потому что `window.PerformancePaintTiming` в
Lumin'е не существует вовсе (`typeof window.PerformancePaintTiming ===
"undefined"`). Тест никогда не доходит до собственно проверки доставки
paint-таймингов.

## Причина

`crates/js/src/dom.rs` реализует доставку paint-записей (`first-paint`/
`first-contentful-paint`) как обычные объектные литералы:

```js
// line ~8340
var entry = { entryType: 'paint', name: String(name), startTime: start_ms, duration: 0 };
_perf_entries.push(entry);
```

`PerformanceObserver.supportedEntryTypes` (line 8265-8272) честно
перечисляет `'paint'` в списке — и сам механизм доставки реально
подключён к живому рендер-пайплайну: `crates/shell/src/main.rs:15551-15566`
(`#[cfg(feature = "v8")]`, гейт `self.js_present`) зовёт
`j.deliver_paint_timing("first-paint", …)` и
`j.deliver_paint_timing("first-contentful-paint", …)` на первом непустом
display list каждой загрузки страницы — то есть данные для paint-timing
действительно текут. Но нигде в шиме нет глобального конструктора
`PerformancePaintTiming` (`interface PerformancePaintTiming :
PerformanceEntry` по W3C Paint Timing §2) — записи остаются "утиными"
plain-object значениями, а не инстансами этого интерфейса, поэтому
`window.PerformancePaintTiming` = `undefined`.

Тот же класс дефекта, что [BUG-624](bugs/BUG-624-OPEN.md) (`Navigator`),
[BUG-637](bugs/BUG-637-OPEN.md) (`Window`) и
[BUG-589](bugs/BUG-589-OPEN.md) (`window` сам не WebIDL-объект) —
WebIDL-интерфейсные объекты систематически отсутствуют как глобалы,
хотя поведение самих shim-функций местами уже реализовано.

## Вторичные находки (не новые, реконфирмация)

- 20/21 harness-level `TIMEOUT` (все `fcp-only/*.html` кроме
  `idlharness.window.html`) — `<script src="../resources/utils.js">` даёт
  сетевой 404 (`../` не схлопывается при резолве относительного URL),
  файл реально вендорен и лежит на диске
  (`tests/wpt/paint-timing/resources/utils.js`) — прямая реконфирмация
  [BUG-346](bugs/BUG-346-OPEN.md). Симптом в логе — `script error: JS
  runtime error: test_fcp is not defined` (хелпер из недогруженного
  `utils.js`), затем внешний таймаут wptrunner.
- 6/36 `ReferenceError: assert{No,}FirstContentfulPaint is not defined` —
  тот же корень (BUG-346), другой недогруженный хелпер из того же
  `utils.js`.
- `idlharness.window.html` TIMEOUT — известный невендоренный
  `/resources/idlharness.js`+`WebIDLParser.js` (тот же класс, что у
  `FileAPI`/`animation-worklet`/`netinfo`, см. `STATUS-PN` записи по
  `page-lifecycle`).

## Как воспроизвести

```
tests/wpt/run_report.py --binary <lumen.exe> --all --root paint-timing --recursive
```
или живой probe: `eval("typeof window.PerformancePaintTiming")` →
`"undefined"` на любой странице.
