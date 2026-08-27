# BUG-401 — `performance` полностью отсутствует в Worker global scope

**Статус:** FIXED 2026-08-11 (P3)
**Компонент:** js (`crates/js/src/worker.rs:439-566` — `worker_global_shim`)
**Найден:** P2, WPT-VENDOR-hr-time (2026-07-28), прогон `run_report.py --all --root hr-time --recursive`

## Симптом

Три из 13 файлов категории `hr-time` создают `Worker` и падают TIMEOUT
с одинаковой первопричиной:

```
[worker-0] v8 script error: Runtime("performance is not defined")
```

— `clamped-time-origin.html` (0/1 подтестов), `timeOrigin.html` (1/3,
оба TIMEOUT-подтеста — про воркеры), `window-worker-timeOrigin.window.html`
(0/1). Суммарно они дают 3 из 6 TIMEOUT/unexpected-подтестов прогона.

## Причина

`worker_global_shim` (`worker.rs:439`) строит глобальную область воркера
с нуля: `self`, `postMessage`, `onmessage`/`addEventListener`/
`removeEventListener`, `console`, `importScripts`, `setTimeout`/
`setInterval`/`queueMicrotask`. `performance` в этот список не входит
вовсе — ни `performance.now()`, ни `performance.timeOrigin`.

Спека HR Time L3 явно помечает интерфейс `[Exposed=(Window,Worker)]`
(см. BUG-400) — `performance` обязателен в `WorkerGlobalScope`
(WHATWG HTML §10.2.3, `WorkerGlobalScope includes
WindowOrWorkerGlobalScope`, которая объявляет `performance`).
Любой воркер-скрипт, который просто читает `performance.now()` для
тайминга (частый паттерн — сравнение задержки внутри воркера), падает
`ReferenceError` на первой же строке.

## Что нужно сделать

Внедрить в `worker_global_shim` тот же `performance`-объект, что и в
`WEB_API_SHIM` (`dom.rs:10987`), с собственным `_perf_origin_ms` —
per-worker time origin, а не общий с window (спека: у воркера свой
`timeOrigin`, отсчитываемый от создания воркера, не от создания
страницы; см. подтест `timeOrigin.html` — «Window and worker
timeOrigins differ when worker is created after a delay»). Минимальный
набор методов, покрывающий эту категорию: `now()`, `timeOrigin`;
`mark`/`measure`/`getEntries*` можно перенести тем же кодом, если не
хочется дублировать логику — оценить, стоит ли выносить общий
JS-фрагмент `performance`-конструктора в шаред-строку, используемую и
`WEB_API_SHIM`, и `WORKER_SHIM`, чтобы не разъезжались (после фикса
[BUG-400](BUG-400-FIXED.md) это станет актуальнее — EventTarget-
наследование и `toJSON()` тоже придётся продублировать или вынести).

## Исправление (P3, 2026-08-11)

### Одно место, а не вторая копия

Заявка оставляла выбор: продублировать `performance` в
`worker_global_shim` или вынести общий JS-фрагмент. Дублирование отпадает
по существу задачи — BUG-400 закрылся за день до этого ровно тем, что
переделал `performance` страницы из объектного литерала в настоящий
`Performance : EventTarget`; вторая рукописная копия разъехалась бы с
первой на следующей же правке. Поэтому `WEB_API_SHIM` разрезан на пять
частей (`dom.rs`), две из которых — те, что WHATWG объявляет
`[Exposed=*]` / `[Exposed=(Window,Worker)]` — стали отдельными
`pub(crate)`-константами:

* `EVENT_TARGET_SHIM` — класс `EventTarget`;
* `PERFORMANCE_SHIM` — интерфейс `Performance`, синглтон `performance`,
  `_perf_entries`/`_perf_origin_ms`.

`web_api_shim()` склеивает все пять **в исходном порядке**, так что V8
компилирует ровно ту же одну программу, что и до разреза: подъём
`var`/`function` через границы частей не меняется, ни одного оператора не
переставлено. `worker_exposed_shim()` отдаёт две общие части воркеру.
`EventTarget` пришлось вынести вместе с `Performance`, а не оставить на
месте: без базового класса прототипную цепочку `Performance : EventTarget`
в воркере не построить, а построить её «почти» — значит вернуть тот самый
литерал-двойник. `EventTarget` в воркере и сам по себе обязателен —
WHATWG DOM объявляет его `[Exposed=*]`.

### Куда это ставится

Новая `worker::install_worker_scope_globals_v8(rt)` регистрирует нативные
часы `_lumen_now_ms` и выполняет общий фрагмент. Её зовут **все три**
разновидности воркер-областей, а не только названная в заявке:
`install_worker_globals_v8` (dedicated), `install_shared_worker_globals_v8`
(`SharedWorkerGlobalScope`) и `install_sw_globals_v8`
(`ServiceWorkerGlobalScope`) — по спеке это всё `WorkerGlobalScope`, у всех
трёх `performance` отсутствовал одинаково, и после общего фрагмента
починка каждой стоит одну строку. Оставить две из трёх сломанными значило
бы завести ту же заявку повторно.

`_perf_observer_notify` (`PerformanceObserver`) живёт дальше по шиму
страницы и в воркер не попадает, поэтому оба его вызова из
`mark()`/`measure()` обёрнуты в `typeof`-проверку. Для страницы это
no-op — там та же самая программа с подъёмом функции; для воркера это
единственное, что отличает рабочий `performance.mark()` от
`ReferenceError`.

### Time origin

`_perf_origin_ms` снимается в момент выполнения фрагмента, то есть при
создании конкретной глобальной области, — это и есть требование
HR Time L3 §4.2 и то, что делает осмысленным подтест «Window and worker
timeOrigins differ when worker is created after a delay»: у воркера,
запущенного позже, origin позже. Специального per-worker кода для этого
не понадобилось — общий фрагмент даёт правильный ответ обеим сторонам
именно потому, что читает часы у себя, а не получает значение снаружи.

### Что сознательно не сделано

`_lumen_now_ms` воркера — стенные часы и **не** подчиняется
`--deterministic`. Заморозить только его было бы подделкой: патч
детерминизма выполняется в контексте страницы, поэтому `Date.now()` и
`Math.random()` внутри любого воркера и без того живые, и остаются
такими. Пробел заведён целиком — [BUG-768](BUG-768-OPEN.md).

`PerformanceObserver` в воркер не добавлен (`[Exposed=(Window,Worker)]`,
но это отдельная поверхность со своей доставкой записей), `Event` в
воркере по-прежнему нет — `dispatchEvent` там принимает объект-литерал.
Ни то, ни другое заявкой не названо и на её тесты не влияет.

### Проверка

8 новых тестов, все на настоящих install-путях (`cargo test -p lumen-js
--features v8-backend`, 2859 + 70 зелёных):

* `worker.rs` — наличие `performance` в области (проверяется **цепочка
  прототипов и отсутствие собственных перечислимых свойств**, а не три
  имени: литерал-двойник прошёл бы проверку по именам); `timeOrigin`
  зажат между двумя стенными отсчётами вокруг install — так отличается
  «origin своей области» от нуля и от чужого; User Timing без
  `PerformanceObserver` (тест на `typeof`-guard); сквозной прогон
  настоящего `spawn_worker_v8` со скриптом, у которого `performance.now()`
  стоит первой строкой — тот самый сценарий из заявки;
* `shared_worker.rs`, `sw_worker.rs` — по тесту на область;
* `dom.rs` — склейка пяти частей в исходном порядке, каждая ровно один
  раз, и посимвольное совпадение воркерского фрагмента с куском шима
  страницы (сравнивать строки, а не поведение объектов, — единственный
  способ доказать отсутствие расхождения).

Живая проба не требовалась: тесты бьют по производственным
`install_*_globals_v8`/`spawn_worker_v8`, ничего из чинимого не
заглушается.

Три файла `hr-time`, названные в заявке, перестают падать
`ReferenceError`; зеленеют ли они целиком, прогоном не проверялось —
`clamped-time-origin.html` требует ещё и огрубления значения, которого
движок не делает.

## Связанные

* [BUG-768](BUG-768-OPEN.md) — заведён этим фиксом: `--deterministic`
  не доезжает ни до одной worker-области (все три источника —
  `Math.random`, `Date.now`, `_lumen_now_ms`).
* [BUG-766](BUG-766-OPEN.md) — `isSecureContext` отсутствует в
  `WorkerGlobalScope`: тот же класс пробела, и после этого фикса у него
  есть готовое место — `worker::install_worker_scope_globals_v8`.
* [BUG-400](BUG-400-FIXED.md) — тот же API (`Performance`), другой
  root cause: там метод/наследование неполны на `window.performance`,
  здесь объект отсутствует на `self`/`globalThis` воркера целиком.
* `worker_global_shim` уже содержит несколько намеренно урезанных
  Phase 0 заглушек (`setInterval` = single-shot, `importScripts` без
  сетевых `http(s):` URL) — этот пробел не документирован там же ни
  комментарием, ни в `CAPABILITIES.md`.
