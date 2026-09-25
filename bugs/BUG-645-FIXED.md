# BUG-645: `window.PerformancePaintTiming` interface object doesn't exist — blocks nearly all `paint-timing` WPT conformance

**Статус:** FIXED 2026-09-25 (P3)
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

Тот же класс дефекта, что [BUG-624](BUG-624-FIXED.md) (`Navigator`),
[BUG-637](BUG-637-FIXED.md) (`Window`) и
[BUG-589](BUG-589-FIXED.md) (`window` сам не WebIDL-объект) —
WebIDL-интерфейсные объекты систематически отсутствуют как глобалы,
хотя поведение самих shim-функций местами уже реализовано.

## Вторичные находки (не новые, реконфирмация)

- 20/21 harness-level `TIMEOUT` (все `fcp-only/*.html` кроме
  `idlharness.window.html`) — `<script src="../resources/utils.js">` даёт
  сетевой 404 (`../` не схлопывается при резолве относительного URL),
  файл реально вендорен и лежит на диске
  (`tests/wpt/paint-timing/resources/utils.js`) — прямая реконфирмация
  [BUG-346](BUG-346-FIXED.md). Симптом в логе — `script error: JS
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

## Исправление (2026-09-25, P3)

`crates/js/src/shim/web_api_shim_tail.js`: введён интерфейсный объект
`PerformancePaintTiming` — `new` из скрипта бросает `TypeError` (в IDL нет
конструктора), на прототипе WebIDL-`[Default] toJSON()` полей
PerformanceEntry. `_lumen_deliver_paint_entry` строит запись от этого
прототипа (`Object.create`), поля остаются собственными свойствами, как у
остальных типов записей шима. `window.PerformancePaintTiming` выставлен в
`web_api_shim_tail_mc.js` рядом с `LayoutShift`.

`PerformanceEntry` как глобал сознательно **не** выставлен: записи
mark/measure/resource по-прежнему плоские объекты, и `instanceof
PerformanceEntry` отвечал бы для них `false` — ложь хуже отсутствия.

Регресс-тест: `performance_paint_timing_interface_backs_paint_entries`
(`crates/js/src/dom/tests/v8_perf_observers.rs`).

**A/B** (`run_report.py --all --root paint-timing`, верхний уровень, тот же
слот, dev-release): **0/9 → 7/9** сабтестов. Все 9 до правки падали на
`assert_implements`. Оставшиеся два — другой механизм, не этот дефект:

- `first-contentful-paint.html` — `FP only. expected 1 but got 2`: шелл
  выдаёт FCP в том же кадре, что и FP, без проверки «контентности» кадра
  (Phase 0-аппроксимация, `crates/shell/src/app/window_event/redraw_requested.rs`,
  шаг 5). Это нереализованная часть Paint Timing, а не дефект интерфейса.
- `first-contentful-bg-image.html` — TIMEOUT; FCP от фоновой картинки, того
  же класса (что именно считается контентным кадром).

Вторичные находки из шапки (BUG-346 `../`, невендоренный idlharness) этой
правкой не затрагивались.
