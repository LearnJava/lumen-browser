# BUG-401 — `performance` полностью отсутствует в Worker global scope

**Статус:** OPEN
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
[BUG-400](bugs/BUG-400-FIXED.md) это станет актуальнее — EventTarget-
наследование и `toJSON()` тоже придётся продублировать или вынести).

## Связанные

* [BUG-400](bugs/BUG-400-FIXED.md) — тот же API (`Performance`), другой
  root cause: там метод/наследование неполны на `window.performance`,
  здесь объект отсутствует на `self`/`globalThis` воркера целиком.
* `worker_global_shim` уже содержит несколько намеренно урезанных
  Phase 0 заглушек (`setInterval` = single-shot, `importScripts` без
  сетевых `http(s):` URL) — этот пробел не документирован там же ни
  комментарием, ни в `CAPABILITIES.md`.
