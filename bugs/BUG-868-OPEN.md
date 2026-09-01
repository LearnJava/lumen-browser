# BUG-868 — `MessagePort` не пересекает границу воркера ни в одну сторону: список transfer отбрасывается, а `MessageChannel` в воркерной области не определён

**Статус:** OPEN (ДОРАБОТКА → [GAP-WORKERSCOPE](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-WORKERSCOPE` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 26 — живой замер, вариант `worker-port`)
**Область:** `crates/js/src/worker.rs:918` — `Worker.prototype.postMessage(data, transfer)` зовёт `_lumenSerializeWithTransfers`, который понимает только `OffscreenCanvas` (`__canvas_id__`); `crates/js/src/worker.rs:366` — воркерный `globalThis.postMessage = function(data)` объявлен **без** второго параметра и сериализует `JSON.stringify(data)`; `MessageChannel`/`MessagePort` определены только в шиме страницы (`crates/js/src/dom.rs:12010`, `:12019` — `globalThis.MessageChannel = MessageChannel` внутри `WEB_API_SHIM`, который в воркер не попадает)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
var w = new Worker("w.js"), ch = new MessageChannel();
w.postMessage("port", [ch.port1]);   // в воркере: e.ports.length === 0
// в воркере:
new MessageChannel();                // ReferenceError: MessageChannel is not defined
self.postMessage("made-port", [p]);  // на странице: e.ports === undefined
```

Транспорт сообщений при этом исправен — строка доходит в обе стороны;
теряется ровно список `transfer` и вместе с ним любой канал связи
«страница ↔ воркер» помимо самого `Worker`.

## Прямое измерение

`tests/wpt/verify_worker_port_storage_gaps.py --variant worker-port`
(2026-08-23, dev-release, Linux, `main` = `c14b8068c`, `--seconds 6`):

```
wp-worker-error MessageChannel is not defined
wp-sent-with-transfer
wp-from-worker data="saw-ports:0" ports=undefined
wp-pinged-page-port
wp-checked
```

Первая строка — воркер, пытающийся создать канал у себя (это делает
`support/Worker-messageport.js` из WPT); третья — воркер отвечает, что в
доставленном ему сообщении портов ноль, а на самой странице поле `ports`
у события от воркера отсутствует как таковое. Порт, созданный и
использованный **внутри** страницы, работает: соседний вариант
`port-lifecycle` печатает `pl-p1 to-p1` и корректно молчит после
`close()`.

## Отношение к соседям

Это третий известный путь, где `transfer` теряется: [BUG-717](BUG-717-OPEN.md)
записал половину «окно → окно» (`window.postMessage` с `transfer` не
порождает `e.ports`). Здесь другой код (`Worker.prototype.postMessage` и
воркерный `globalThis.postMessage`), поэтому баг отдельный, но фикс
разумно делать одной структурой сериализации. Измеренный сосед, не
заводимый отдельно: у `MessagePort` нет `onclose`/события `close`
(`pl-has-onclose false`) — это tentative-часть спеки, ей соответствует
1 id остатка (`webmessaging/message-channels/close-event/
garbage-collected.tentative.any.html`).

## Масштаб

4 id остатка снимка WPT-RUN-5: `workers/Worker-messageport.html`
(2 зависших подтеста — «Test getting messages from a worker on a port»,
«Test sending many messages to workers using ports»),
`workers/Worker-termination-with-port-messages.html`,
`webmessaging/message-channels/worker-post-after-close.any.html`,
`webmessaging/message-channels/close-event/garbage-collected.tentative.any.html`.

## Направление починки (не предписание)

Воркерная область уже получает общие фрагменты шима через
`worker::install_worker_scope_globals_v8` (BUG-401) — `MessagePort`/
`MessageChannel` естественно вынести туда же. Для самого переноса нужен
идентификатор порта в полезной нагрузке (как `__canvas_id__` у
OffscreenCanvas) и таблица портов на стороне воркера, чтобы
`e.ports[0].postMessage` уходил в ту же очередь, что и `postMessage`
воркера.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_worker_port_storage_gaps.py
   --variant worker-port` — ожидается `wp-from-worker … ports=1` и
   `wp-via-worker-port`/`wp-page-port pong` в обе стороны.
2. WPT: `run_report.py --all --root workers --recursive` и
   `--root webmessaging/message-channels --recursive`.
