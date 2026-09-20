# BUG-868 — `MessagePort` не пересекает границу воркера ни в одну сторону: список transfer отбрасывается, а `MessageChannel` в воркерной области не определён

**Статус:** FIXED 2026-09-20 (P6, GAP-WORKERSCOPE срез 2)
**Тип:** нереализованная функциональность, не дефект реализованного кода — велась как задача `GAP-WORKERSCOPE` в [ROADMAP.md](../ROADMAP.md), P3 как баг не брала. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 26 — живой замер, вариант `worker-port`)
**Область:** `crates/js/src/worker.rs` (`Worker.prototype.postMessage`/`globalThis.postMessage`/`install_worker_bindings_v8`/`install_worker_globals_v8`/`run_worker_thread_v8`), `crates/js/src/shim/message_channel_shim.js` (`MessagePort`/`MessageChannel`), `crates/js/src/v8_runtime.rs` и `crates/js/src/v8_runtime/runtime.rs` (`pump_workers`, `worker_port_messages`)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи.
**Обновление 2026-09-20 (P6, GAP-WORKERSCOPE срез 1):** [BUG-872](BUG-872-FIXED.md) закрыт —
`MessageChannel`/`MessagePort`/`MessageEvent` (и остальные шесть интерфейсных объектов) теперь
существуют в воркерной области (`worker::install_worker_scope_globals_v8` эвалирует тот же
`MESSAGE_CHANNEL_SHIM`, что и страница). `new MessageChannel()` **внутри** воркера больше не
`ReferenceError` — строка 15 симптома ниже устарела. Осталось ровно то, что заголовок бага
называет: список `transfer` не пересекает саму границу «страница ↔ воркер», а
`MessagePort`-объекты в такой список не входят вовсе.

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

Это третий известный путь, где `transfer` теряется: [BUG-717](BUG-717-FIXED.md)
записал половину «окно → окно» (`window.postMessage` с `transfer` не
порождает `e.ports`) — остаётся отдельным, не тронутым этим срезом. Измеренный
сосед, не заводимый отдельно: у `MessagePort` нет `onclose`/события `close`
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

## Исправлено

Реальная передача `MessagePort` через границу страница↔воркер в обе стороны,
плюс безусловный блокер, который до этого среза скрывал бы результат любой
попытки: у воркерной области не было `structuredClone` вовсе, так что
`MessagePort.postMessage()` внутри воркера — даже локальный, без транзита
через страницу — падал `ReferenceError: structuredClone is not defined`
раньше, чем доходил до какой-либо логики транзита (`install_worker_scope_globals_v8`
теперь эвалирует минимальный, JSON-совместимый `structuredClone` для
воркерной области — не полный клон-спек страницы из `web_api_shim_tail_b.js`,
а то же пространство значений, которому уже следует собственный провод
сообщений воркера через `JSON.stringify`).

Модель переноса (`message_channel_shim.js`, общий файл страницы и воркера):
`_lumen_port_prepare_transfer(transfer, bindWorkerId)` при передаче порта
неутрирует сам передаваемый объект (`_neutered`) и переключает его локального
партнёра (`port._other`) на «удалённый» режим (`_remoteBound`) с
глобальным id моста, взятым из общего счётчика страницы и всех её воркеров
(`_lumen_next_port_id`). На принимающей стороне `_lumen_port_reify_list`/
`_lumen_port_get_or_create` при первом упоминании id создают новый
`MessagePort`, регистрируемый в `_lumenPortRegistry` под тем же id — обе
стороны находят друг друга по этому id, а не по прямой JS-ссылке (её нет:
стороны живут в разных `v8::Isolate`). Порт, встроенный внутрь `data`, а не
только присутствующий в списке `transfer`, получает тот же
сентинел-механизм, что `OffscreenCanvas` (`__lumen_sentinel__:
'__lumen_message_port__'`), применённый ДО прогона через существующий
канвас-walker `_serializeObj` (`worker.rs`) — иначе собственные поля порта
(`_other`, `_queue`, …) утекли бы в JSON как мусор вместо сентинела.

Транспорт (`worker.rs`, `v8_runtime.rs`/`v8_runtime/runtime.rs`): новый
вариант `WorkerInMsg::PortPost(port_id, json)` поверх уже существующего
`mpsc`-канала страница→воркер (не новый канал) и новая параллельная очередь
`WorkerPortMessageQueue` воркер→страница, дренируемая `pump_workers()` через
`_lumen_deliver_port_messages` — тем же циклом, что уже дренирует
`worker_messages`/`worker_errors`. Направление воркер→страница отвечает без
явного адреса получателя (`_lumen_port_post_reply` — у воркера один
родитель); направление страница→воркер — с `workerId` явно
(`_lumen_port_post_to_worker`), поскольку у страницы воркеров может быть
несколько.

Два сквозных теста в `crates/js/src/dom/tests/v8_webworker.rs`
(`worker_message_port_transfer_page_to_worker_round_trip`,
`worker_message_port_transfer_worker_to_page_round_trip`) гоняют настоящий
`Worker` + настоящий `MessageChannel` через `pump_workers()` в обе стороны —
не мок транспорта, а тот же путь, которым идёт живая страница.

Вне объёма этого среза: `window.postMessage`'s `transfer` (BUG-717, другой
код, окно↔окно, не страница↔воркер), `onclose`/`close`-событие `MessagePort`
(tentative-часть спеки), полный клон-спек `structuredClone` для воркера
(Map/Set/типизированные массивы/собственный `transfer`-список — узкий
JSON-совместимый клон закрывает потребности `postMessage`, не более).

`cargo test -p lumen-js --lib --features v8-backend` 3963/3963,
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
чист.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_worker_port_storage_gaps.py
   --variant worker-port` — ожидается `wp-from-worker … ports=1` и
   `wp-via-worker-port`/`wp-page-port pong` в обе стороны.
2. WPT: `run_report.py --all --root workers --recursive` и
   `--root webmessaging/message-channels --recursive`.
3. `cargo test -p lumen-js --lib --features v8-backend worker_message_port_transfer`
   — регрессионные тесты обоих направлений переноса.
