# BUG-1812 — `window.postMessage` не переносит `MessagePort`: список передачи молча отбрасывается, `event.ports` всегда пуст

**Статус:** FIXED 2026-11-15 (P3)
**Заведён:** 2026-11-15 (P3, срез 2 [BUG-1647](BUG-1647-FIXED.md))
**Область:** js (`crates/js/src/frame_bridge.rs` — JS-шим фасада окна, натив
`_lumen_f_post_message`, `_lumen_frame_pump_messages`, `_lumen_deliver_frame_message`;
`crates/js/src/shim/message_channel_shim.js` — машинерия переноса;
`crates/js/src/shim/web_api_shim_mid_b4.js` — собственный `window.postMessage`)

## Симптом

Третий аргумент `postMessage(message, targetOrigin, transfer)` не читается ни в одном из двух
путей (свой `window.postMessage` и фасад `iframe.contentWindow`/`window.parent`):

- `event.ports.length === 0` у получателя, хотя отправитель передал `[port2]`;
- сообщение, отправленное позднее в переданный порт, не приходит никуда;
- в JSON-конверте кадрового транспорта поля `ports` нет вовсе.

Проба `.tmp/xo2_1647.py` (кросс-оригинный родитель/ребёнок, `parent` и `127.0.0.1`):
`f.contentWindow.postMessage({tag:'with-port'}, origin, [ch.port2])` — ребёнок получает
`data`, но `e.ports.length` = 0, ответ по порту не приходит. То же на живом стенде reCAPTCHA
(`.tmp/cap1647c.py`): якорь шлёт родителю сообщение handshake, у получателя `ports: 0`, хотя
бандл передаёт ровно один порт (`recaptcha__en.js`: `m.postMessage(tq, origin, [F.port2])`,
ответы якоря — `handler.postMessage({action:"zzsc", token})`).

**Почему это важно.** Через перенос порта работают handshake reCAPTCHA v3 (api.js ↔ anchor),
Cloudflare Turnstile и hCaptcha: без `event.ports` канал не устанавливается, клиент уходит в
собственный таймаут (`execute-ms=30000` у reCAPTCHA) и отдаёт токен, который Google не
принимает.

## Причина

1. `frame_bridge.rs` (JS-шим фасада окна): `w.postMessage = function(message, targetOrigin)`
   — третьего параметра нет, `arguments[2]` никуда не идёт; натив
   `_lumen_f_post_message(bid, json, origin)` не имеет поля `ports`.
2. `window_messaging.rs` — тот же двухаргументный путь для собственного окна страницы.
3. `_lumen_frame_pump_messages` разворачивает конверт в
   `_lumen_deliver_frame_message(data, origin, source, opaque)` — `ports` не передаётся, в
   `MessageEvent` он не попадает.

Машинерия переноса уже есть и работает на границе page↔worker: `MessagePort` с
`_lumenPortRegistry` и общим id, `_lumen_port_prepare_transfer(transfer, bindWorkerId)`,
`_lumen_port_get_or_create`, `_lumen_port_reify_list`, нативы
`_lumen_port_post_to_worker`/`_lumen_port_post_reply` (`shim/message_channel_shim.js`,
`worker.rs`). В `docs`/`subsystems/js.md` перенос `MessagePort` в `postMessage` помечен как
«Not in scope … separate task».

## Что сделано (2026-11-15, P3)

1. **Список передачи принят в обоих путях.** `w.postMessage = function(message,
   targetOrigin, transfer)` (шим фасада окна) зовёт `_lumen_port_prepare_transfer(transfer,
   null, true)` — общую машинерию page↔worker моста — и передаёт id четвёртым аргументом
   натива `_lumen_f_post_message` (`into_v8_fn4`). Порт внутри `data` уходит sentinel'ом
   (`_lumen_port_walk_serialize`), на приёме разворачивается в тот же объект, что
   `event.ports[k]` (`_lumen_port_walk_deserialize`). Собственный `window.postMessage`
   третьим аргументом принимает список передачи локально: переданный порт отсоединяется, а
   событие несёт новый объект, сцепленный с его прежним партнёром (без моста — другой конец
   в том же изоляте).
2. **Конверт и `Event.ports`.** `PendingFrameMessage.ports: Vec<u32>`, поле `ports` в
   JSON-конверте `_lumen_frame_take_messages`; `_lumen_frame_pump_messages` реифицирует
   список ДО разбора `data`, `_lumen_deliver_frame_message(data, origin, source, opaque,
   ports)` строит `MessageEvent` с `ports` (HTML LS §9.3.4 `MessageEventInit`).
3. **Портовые сообщения между изолятами кадров.** Маршрут
   `port_routes(): (ключ изолята, id порта) → ключ изолята-получателя` пишется обеими
   сторонами при переносе; `_lumen_port_post_frame(id, json)` (натив, зовётся из
   `MessagePort.prototype.postMessage` при `_remoteFrame`) кладёт сообщение в
   `port_outbox`, `_lumen_frame_take_port_messages` отдаёт его адресату на тике его помпы,
   разбор — общим `_lumen_deliver_port_messages` worker-пути → `_deliverOrQueue`. Ключ
   изолята в маршруте обязателен: `_lumen_next_port_id` — счётчик рантайма, поэтому один и
   тот же id вправе носить два независимых канала разных документов.
   `frame_transport_has_for` учитывает и этот ящик (спящий event-loop просыпается).
4. **Ошибки списка передачи.** Повторный перенос того же порта —
   `DataCloneError` (HTML LS §9.2.9).

**Тесты** (`crates/js/src/v8_runtime/tests/bug1812_frame_port.rs`, 4 шт.):
`transferred_message_port_echoes_across_the_frame_boundary` (кросс-оригинная пара,
`ports.length === 1`, `data.port === ports[0]`, ответ по порту доезжает до `port1`),
`message_without_transfer_list_has_no_ports`, `transferring_the_same_port_twice_throws_data_clone_error`,
`self_post_message_transfer_delivers_a_working_port`.

**Живая проверка.** `.tmp/xo1812.py` (родитель `http://localhost:P`, кадр
`http://127.0.0.1:P`, оба направления): лента родителя —
`parent-got:child-hello:ports=1`, `parent-childport-got:child-reply`,
`parent-sent`, `parent-port-got:child-through-port`. До фикса `ports` был 0 и порт не
доезжал. Стенд reCAPTCHA после фикса: порт доходит, но `grecaptcha.execute` по-прежнему
без балла — следующий блокер
[BUG-1813](BUG-1813-FIXED.md) (начальный `about:blank`-кадр не отдаёт глобалы:
`contentWindow.JSON === undefined`).

## Что ещё осталось (не в этом дефекте)

`targetOrigin`, который не разбирается как URL, должен бросать `SyntaxError` (HTML LS §9.2.9
шаг 3), а сейчас молча ничего не доставляет. Замечено при срезе 2 [BUG-1647](BUG-1647-FIXED.md)
вместе с исправленной нормализацией порта по умолчанию в сравнении происхождений.
