# BUG-1076 — в глобальной области выделенного воркера нет `Worker`: вложенные воркеры не работают, `ReferenceError: Worker is not defined`

**Статус:** FIXED 2026-09-25 (P1, WORKER-2) — до того OPEN (ДОРАБОТКА → WORKER-2): WORKER-1 записал баг закрытым, но вложенный Worker не делал ни один срез (сверка 2026-09-25)
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
- [BUG-1071](BUG-1071-FIXED.md) — тот же класс дефекта для `WebSocket` (`[Exposed=(Window,Worker)]`, в воркере нет).
- `docs/tasks/p2-test-track.md#test-3-срез-46-2026-09-21`.

## Не проверялось

- `SharedWorker`-область (`new Worker` из `SharedWorkerGlobalScope`) — перечисленные id идут в основном через выделенный воркер.
- Поведение `terminate()` цепочки из нескольких вложенных воркеров: пока не проверить, нужен сам конструктор.

## Решение (WORKER-2, 2026-09-25)

`run_worker_thread_v8` (`crates/js/src/worker.rs`) после воркерных глобалов ставит в изолят воркера тот же
`WORKER_SHIM`, что и страница, — через `install_worker_constructor_v8` (тело прежнего
`install_worker_bindings_v8`; тот стал обёрткой, создающей счётчик id `MessagePort`) поверх собственного
`NestedWorkers`: реестр детей и очереди сообщений / ошибок / портов. Шелл-тика у воркера нет, поэтому его
цикл задач сам дренирует эти очереди (`deliver_worker_queues` — общая с `V8JsRuntime::pump_workers`
процедура) и, пока ребёнок жив или в очередях что-то лежит, спит не дольше `WORKER_SOCKET_POLL` (10 мс).
При выходе из цикла (`close()`, `terminate()`, закрытый канал) `NestedWorkers::terminate_all` шлёт
`Terminate` каждому ребёнку — каскад по цепочке. Ребёнок получает те же `JsFetchProvider`, провайдер
`WebSocket`, детерминизм, хранилище blob и счётчик id портов, что и родитель; относительный URL
разрешается от скрипта родителя (`_lumen_document_base_url` из `worker_net_shim.js`).

Проверка: 5 юнит-тестов `worker::tests_v8::v8_nested_worker_*` (конструктор есть, эстафета родитель↔потомок,
база URL, `error` при провале загрузки, каскадный `terminate`). WPT одиночным прогоном слота:
`nested_worker.worker.html`, `nested_worker_close_self.worker.html`, `nested_worker_importScripts.worker.html`,
`nested_worker_sync_xhr.worker.html`, `nested_worker_terminate_from_document.html`,
`nested_worker_close_from_parent_worker.html`, `baseurl/alpha/worker-in-worker.html` — FAIL → PASS,
их `.ini` сняты; полный прогон `workers --check` добавил ещё два снятых ожидания —
`semantics/interface-objects/001.worker.html` «The Worker interface object should be exposed.» и
`semantics/multiple-workers/exposure.any.worker.html` «Worker exposure». Остальные «регрессии» того
прогона сверены с бинарником main одиночным прогоном — совпадают (TIMEOUT `.https.`/serviceworker-id
от окружения, флаки под нагрузкой); прочие UNEXPECTED-PASS там — наследие WORKER-1, baseline не мой.

Остаток, не входящий в этот баг:

- `modules/dedicated-worker-import.any.worker.html` по-прежнему TIMEOUT (0/9), но уже не из-за
  `Worker`: модульный ребёнок во вложенном воркере работает (юнит-проба со статическим `import` и
  `self instanceof DedicatedWorkerGlobalScope` проходит), а время съедает
  [BUG-1149](BUG-1149-OPEN.md) — каждая загрузка с `localhost` стоит ~2 с, а у этого id их пять подряд
  до первого `postMessage`. Оконный вариант того же файла — TIMEOUT 1/9 по той же причине. То же у
  `dedicated-worker-import-data-url.any.worker.html`: на main он падал мгновенно (OK, 0/9 FAIL), теперь
  доходит до загрузок и упирается в таймаут. Доказательство — `run_report.py --root workers/modules
  --timeout-multiplier 4`: в обоих id «Static import.», «(redirect)», «Nested static import.» — PASS,
  дальше «Static import and then dynamic import.» — TIMEOUT, как и в оконном варианте (отдельная
  проблема динамического `import()`). `.ini` обоих переписаны под таймаут по умолчанию
  (`[TIMEOUT, PASS]` у первого подтеста — в полном прогоне он проходил).
- `Worker` в `SharedWorkerGlobalScope` не ставится (спека экспонирует его и туда).
- CSP `worker-src` для ребёнка — политика страницы (провайдера), а не пришедшая с собственным скриптом
  родительского воркера.
