# BUG-1071 — в глобальной области выделенного воркера нет `WebSocket`: `.any.worker.html`-варианты `websockets/` падают до первого ассерта

**Статус:** FIXED 2026-09-24 (P1, WORKER-1 срез 3)
**Тип:** пробел реализации — `WebSocket` есть в главном потоке и отсутствует в воркерной области; какой из двух классов (точечный дефект или доработка вроде [GAP-WSASYNC](../ROADMAP.md)) — решает триаж P3 по правилу [docs/probe-method.md §8](../docs/probe-method.md).
**Заведён:** 2026-09-20 (P2, WPT-RUN-7 срез 43, `websockets`)
**Область:** воркерный рантайм — `crates/js/src/worker.rs` (прелюдия воркерного изолята); нативы `_lumen_ws_*` регистрируются для главного рантайма (`crates/js/src/v8_runtime/install/net.rs`), в воркер не попадают. Не проверялось: где именно проходит граница (нативы не установлены, шим не подключён или и то и другое).
**Владелец:** P3.

## Симптом

Прогон `websockets` (786 id, `--update-expected`, 2026-09-20): у `.any.worker.html`-вариантов с `?default` и `?wss` harness-статус `ERROR`,
все подтесты `NOTRUN`. В логе движка на каждый такой файл одна строка:

```
[worker-0] v8 script error: Runtime("assert_true: Browser does not support WebSocket expected true got false")
[worker-0] v8 script error: Runtime("WebSocket is not defined")
```

Первая форма — 86 строк (`IsWebSocket()` в `websockets/constants.sub.js:30` — `if (!self.WebSocket) assert_true(false, …)`), вторая — 4–5 (тесты, которые
обращаются к `WebSocket` без этой проверки). Всего в baseline 108 секций `expected: ERROR` у `.any.worker.html?default|?wss`; 90–91 из них
подтверждены строкой лога выше (строки лога не привязаны к id пофайлово — счёт, а не сопоставление), причина остальных ~17 не разбиралась.

В главном потоке та же идиома проходит: `.any.html`-варианты тех же файлов доходят до подтестов.

## Ожидание

`typeof WebSocket === 'function'` в `DedicatedWorkerGlobalScope` (WHATWG HTML §10.3 / WebSockets — интерфейс `[Exposed=(Window,Worker)]`);
`new WebSocket(url)` внутри воркера открывает соединение так же, как в окне.

## Связанное

- [GAP-WSASYNC](../ROADMAP.md) / [BUG-856](BUG-856-FIXED.md) — синхронная модель `WebSocket` в главном потоке; воркеру, у которого свой поток, синхронный
  коннект вреден меньше, так что порядок работ между двумя задачами не очевиден.
- [GAP-WORKERSCOPE](../ROADMAP.md) — у воркерной области нет интерфейсных объектов вообще; `WebSocket` в том же ряду, но не в списке BUG-872.

## Не проверялось

- `SharedWorker` и `ServiceWorker`: `.any.sharedworker.html`/`.any.serviceworker.html`-варианты в этом же прогоне падают по другим причинам
  (`?wpt_flags=h2` — TLS, [BUG-1069](BUG-1069-FIXED.md)), так что чистого замера нет.
- `WebSocketStream` (`websockets/stream/`) в воркере.

## Исправление (WORKER-1 срез 3, 2026-09-24)

Граница проходила в обоих местах сразу: в воркерной области не было ни шима `WebSocket`, ни нативов `_lumen_ws_*`, ни самого
`Event` (он жил в `web_api_shim_head.js`, только в окне), без которого шим не создаёт ни одного события.

- `Event`/`CustomEvent` и блок `WebSocket`+`CloseEvent` вырезаны дословными срезами (`crates/js/src/shim/event_shim.js`,
  `crates/js/src/shim/websocket_shim.js`) и входят в `dom::worker_exposed_shim()`; страничный шим собирается из тех же кусков в
  прежнем порядке. Внутри среза `WebSocket` страничные помощники зовутся через `typeof`-охрану / `_lumen_et_report`.
- Нативы строит одна функция `v8_runtime::install::net::websocket_natives` — её регистрируют и страница, и воркер.
  `dom::install_worker_exposed_v8` ставит их без провайдера; dedicated- и shared-воркер перепривязывают их к провайдеру страницы
  (`dom::bind_worker_websocket_v8`), сервис-воркер остаётся без провайдера — `error` + `close(1006)`, как страница без провайдера.
- Цикл задач воркера (`worker::run_worker_tasks`) поллит сокеты раз за ход и ограничивает сон 10 мс, пока сокет жив. Живость
  спрашивается после задач хода: сокет, открытый из таймера, иначе усыплял поток в `recv()` навсегда
  (`worker_ws_tests.rs::v8_dedicated_worker_websocket_opened_from_a_timer`).

WPT `websockets/` (786 id, `--update-expected`, бинарь ветки): воркерные варианты — 91 секция ERROR→OK, 44 ERROR→TIMEOUT, 18 OK→TIMEOUT, 2 TIMEOUT→OK; 390 подтестов, прежде перечисленных как FAIL/NOTRUN, проходят; ни одного нового FAIL. TIMEOUT-переходы — тесты теперь доходят до сокета: в `?wpt_flags=h2` соединение не устанавливается (оконный h2-вариант тех же файлов тоже FAIL), причина TIMEOUT у четырёх секций `stream/tentative/*` не разбиралась. Оконные секции не изменились: страничный шим собран из тех же кусков. `events/020.html` оставлен с флаки-ожиданием `[OK, TIMEOUT]` из среза 62, `constructor/004.html?wss` перепрогнан отдельно (в общем прогоне упал по внешнему таймауту в конце 61-минутной части).
