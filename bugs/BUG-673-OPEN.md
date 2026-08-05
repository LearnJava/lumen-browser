# BUG-673: `window.PerformanceResourceTiming`/`window.PerformanceNavigationTiming` interface objects don't exist — entries are plain objects, not instances of anything

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `PerformanceObserver`/`_perf_entries` shim, ~line 8409-8460)
**Найден:** P2, WPT-VENDOR-server-timing, 2026-08-06

## Симптом

`server-timing` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root server-timing --recursive`, ~3 мин 33 с, 9
отобранных id): **0/9 harness OK, 0/0 сабтестов**, все 9 — `.https.`,
TIMEOUT на уже задокументированном TLS-гэпе `UnknownIssuer`
(`docs/wpt-status.md:25-28`) — ни один тест не дошёл до навигации, весь
API категории (`PerformanceResourceTiming.serverTiming`/
`PerformanceNavigationTiming.serverTiming`) недостижим через wptrunner
целиком.

Живая проба (`--mcp-live-port`, `file://` страница с `<script>`, без
сети/`.https.`) на общем контейнере — `Performance*Timing` — нашла
независимый, не заблокированный TLS-гэпом дефект:

```js
typeof window.PerformanceServerTiming   // "undefined" — ожидаемо, API не реализован
typeof window.PerformanceResourceTiming // "undefined"
typeof window.PerformanceNavigationTiming // "undefined"
PerformanceObserver.supportedEntryTypes  // содержит 'navigation' и 'resource'
```

`supportedEntryTypes` честно перечисляет `'navigation'` и `'resource'`
(доставка реально подключена — `_lumen_record_resource_timing`/
`deliver_nav_timing` вызываются из живого пайплайна), но нигде в шиме
нет глобальных конструкторов `PerformanceResourceTiming`/
`PerformanceNavigationTiming` (`interface PerformanceResourceTiming :
PerformanceEntry` / `interface PerformanceNavigationTiming :
PerformanceResourceTiming`, W3C Resource Timing L2 §4 / Navigation
Timing L2 §4) — записи остаются "утиными" plain-object значениями
(`crates/js/src/dom.rs:8412` `_lumen_record_resource_timing`,
`:8444` `_lumen_deliver_perf_entry`), а не инстансами этих интерфейсов.

## Причина

Тот же класс дефекта, что [BUG-645](bugs/BUG-645-OPEN.md)
(`PerformancePaintTiming`), [BUG-624](bugs/BUG-624-OPEN.md)
(`Navigator`), [BUG-637](bugs/BUG-637-OPEN.md) (`Window`) и
[BUG-589](bugs/BUG-589-OPEN.md) (`window` сам не WebIDL-объект) —
WebIDL-интерфейсные объекты систематически отсутствуют как глобалы,
хотя поведение самих shim-функций местами уже реализовано. Здесь он
блокирует ровно то же поле, что уже названо (но не объяснено на уровне
интерфейса) в [BUG-640](bugs/BUG-640-OPEN.md), которое перечисляет
`serverTiming` в списке отсутствующих полей `PerformanceNavigationTiming`
— BUG-640 объясняет пустой `detail_json`, но не то, что даже при
заполненном `detail_json` результат не был бы `instanceof
PerformanceNavigationTiming`, потому что такого конструктора не
существует вовсе. Каждая полноценная реализация Server-Timing
(`server_timing_header-parsing.https.html` и т.п.) требует ОБА
исправления: `serverTiming` в `detail_json` (BUG-640 scope) и
`PerformanceResourceTiming`/`PerformanceNavigationTiming` как реальные
конструкторы (этот баг).

## Масштаб

Блокирует `assert_implements`/`instanceof`-проверки в любом WPT-тесте
`resource-timing`/`navigation-timing`/`server-timing`, которые
опираются на существование интерфейсного объекта, а не только на форму
записи — не только `server-timing` (недостижим целиком через TLS-гэп),
но и потенциально ещё не проверенные сабтесты соседних, уже
прогнанных категорий (`resource-timing`, `navigation-timing`).

## Как воспроизвести

```
eval("JSON.stringify({r: typeof window.PerformanceResourceTiming, n: typeof window.PerformanceNavigationTiming})")
```
на любой странице с `<script>` (JS-рантайм требует хотя бы один тег) →
`{"r":"undefined","n":"undefined"}`.

## Дальше

Fix scope: как и в BUG-645 — зарегистрировать `PerformanceResourceTiming`
и `PerformanceNavigationTiming` как настоящие WebIDL-конструкторы
(`interface PerformanceResourceTiming : PerformanceEntry`, `interface
PerformanceNavigationTiming : PerformanceResourceTiming`) и делать
`_lumen_record_resource_timing`/`_lumen_deliver_perf_entry('navigation',
…)` инстанцировать записи через них, а не как объектные литералы —
общий рефакторинг с BUG-645/BUG-640, один P3-фикс может закрыть все три.
