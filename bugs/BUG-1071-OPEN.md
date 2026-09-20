# BUG-1071 — в глобальной области выделенного воркера нет `WebSocket`: `.any.worker.html`-варианты `websockets/` падают до первого ассерта

**Статус:** OPEN
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

- [GAP-WSASYNC](../ROADMAP.md) / [BUG-856](BUG-856-OPEN.md) — синхронная модель `WebSocket` в главном потоке; воркеру, у которого свой поток, синхронный
  коннект вреден меньше, так что порядок работ между двумя задачами не очевиден.
- [GAP-WORKERSCOPE](../ROADMAP.md) — у воркерной области нет интерфейсных объектов вообще; `WebSocket` в том же ряду, но не в списке BUG-872.

## Не проверялось

- `SharedWorker` и `ServiceWorker`: `.any.sharedworker.html`/`.any.serviceworker.html`-варианты в этом же прогоне падают по другим причинам
  (`?wpt_flags=h2` — TLS, [BUG-1069](BUG-1069-OPEN.md)), так что чистого замера нет.
- `WebSocketStream` (`websockets/stream/`) в воркере.
