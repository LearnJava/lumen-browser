# BUG-1076 — в глобальной области выделенного воркера нет `Worker`: вложенные воркеры не работают, `ReferenceError: Worker is not defined`

**Статус:** OPEN
**Тип:** пробел реализации — конструктор `Worker` определён только в оконной прелюдии; какой из двух классов (точечный дефект или доработка вроде [GAP-WORKERSCOPE](../ROADMAP.md)) — решает триаж P3 по правилу [docs/probe-method.md §8](../docs/probe-method.md).
**Заведён:** 2026-09-21 (P2, WPT-RUN-7 срез 46, `workers`)
**Область:** воркерный рантайм — `crates/js/src/worker.rs` (`globalThis.Worker = Worker` на строке 1646 внутри IIFE, начинающейся на строке 1317 — «IIFE that defines `globalThis.Worker`»; прелюдия воркерного изолята его, судя по `ReferenceError`, не устанавливает). Не проверялось: хватит ли для вложенного воркера нынешнего потока-на-воркер и `JsFetchProvider`, либо нужна отдельная маршрутизация `postMessage` через родителя.
**Владелец:** P3.

## Симптом

`new Worker(url)` внутри выделенного воркера бросает `ReferenceError: Worker is not defined`. Прогон `workers` (337 id, `--update-expected`, 2026-09-21):
в логе движка 30 строк `Worker is not defined` (две формы: `[worker-0] v8 script error: Runtime("Worker is not defined")` и текст подтеста).
Строки лога не привязаны к id пофайлово при `--processes 4`, поэтому счёт ниже — по именам подтестов, которые уникальны для файла, и по одиночному прогону:

- `nested_worker.worker.html`, `nested_worker_close_self.worker.html`, `nested_worker_importScripts.worker.html`, `nested_worker_sync_xhr.worker.html` —
  подтесты `Nested worker`, `Nested work that closes itself`, `Nested worker that calls importScripts()`, `Nested worker that issues a sync XHR`:
  `FAIL … Worker is not defined`;
- `nested_worker_terminate_from_document.html`, `nested_worker_close_from_parent_worker.html` — `assert_equals: expected "Pass" but got "Fail: ReferenceError: Worker is not defined"`;
- `modules/dedicated-worker-import.any.worker.html` — 9 подтестов (`Static import.`, `Dynamic import.`, `eval(import()).` …) в baseline `FAIL`; в логе
  `Unhandled rejection with value: object "ReferenceError: Worker is not defined"` (тест запускается *внутри* воркера и сам создаёт модульный воркер);
- `baseurl/alpha/worker-in-worker.html` — одиночный прогон (`--root workers/baseurl --processes 1`, 2026-09-21): `[worker-0] v8 script error: Runtime("Worker is not defined")`,
  `Subtests passed 0/1`. Соседи `importScripts-in-worker.html` / `xhr-in-worker.html` / `import-in-moduleworker.html` в том же прогоне проходят 1/1 — то есть дело именно во вложенности.

Итого не менее **8 id**; вероятно больше — тот же корень у остальных `.any.worker.html`-вариантов, которые создают `Worker` (пофайлово не сопоставлялось).

## Ожидание

`typeof Worker === 'function'` в `DedicatedWorkerGlobalScope` и `SharedWorkerGlobalScope` (WHATWG HTML, Web workers — интерфейс `Worker` экспонируется в `Window`, `DedicatedWorker` и `SharedWorker`);
`new Worker(url)` из воркера создаёт дочерний воркер, `postMessage`/`terminate()`/`onerror` работают так же, как из окна; `close()` родителя останавливает детей.

## Связанное

- [GAP-WORKERSCOPE](../ROADMAP.md) — интерфейсные объекты в воркерной области; `Worker` в его список не входил.
- [BUG-1071](BUG-1071-OPEN.md) — тот же класс дефекта для `WebSocket` (`[Exposed=(Window,Worker)]`, в воркере нет).
- `docs/tasks/p2-test-track.md#test-3-срез-46-2026-09-21`.

## Не проверялось

- `SharedWorker`-область (`new Worker` из `SharedWorkerGlobalScope`) — перечисленные id идут в основном через выделенный воркер.
- Поведение `terminate()` цепочки из нескольких вложенных воркеров: пока не проверить, нужен сам конструктор.
