# BUG-1812 — `window.postMessage` не переносит `MessagePort`: список передачи молча отбрасывается, `event.ports` всегда пуст

**Статус:** OPEN
**Заведён:** 2026-11-15 (P3, срез 2 [BUG-1647](BUG-1647-OPEN.md))
**Область:** js (`crates/js/src/frame_bridge.rs` — JS-шим фасада окна, натив
`_lumen_f_post_message`, `_lumen_frame_pump_messages`, `_lumen_deliver_frame_message`;
`crates/js/src/window_messaging.rs` — собственный `window.postMessage` страницы;
`crates/js/src/shim/message_channel_shim.js` — готовая машинерия переноса)

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

## Что сделать

1. Принять `transfer` в обоих `postMessage` (фасад окна и собственное окно), вызвать
   `_lumen_port_prepare_transfer(transfer, null)` и положить id в конверт
   (`PendingFrameMessage`/`PendingWindowMessage` — новое поле `ports`).
2. На приёме (`_lumen_frame_pump_messages`, pump собственного окна) реифицировать порты
   `_lumen_port_reify_list`, отдать их в `_lumen_deliver_frame_message`/`_lumen_deliver_message`
   и выставить `Event.ports` (плюс разыменовать sentinel
   `__lumen_sentinel__:'__lumen_message_port__'` внутри `data` — идентичность объекта, как в
   worker-пути).
3. Портовые сообщения между изолятами кадров: id → документ-получатель (по образцу
   worker-реестра), доставка на тике помпы в `_lumen_port_deliver(id, json)` →
   `_lumenPortRegistry[id]._deliverOrQueue`.
4. Тесты в `crates/js`: (a) page ↔ cross-origin frame эхо через переданный порт; (b) объект
   порта внутри `data` приходит тем же объектом, что `event.ports[0]`; (c) `ev.ports.length`
   для сообщения без списка передачи — 0; (d) перенос того же порта дважды —
   `DataCloneError`.
5. Проверка: `.tmp/xo2_1647.py` → `ports: 1` и ответ по порту; живой стенд
   `python scripts/captcha_stand.py --arm lumen --only recaptcha-v3-score`.

## Попутно (не чинилось здесь)

`targetOrigin`, который не разбирается как URL, должен бросать `SyntaxError` (HTML LS §9.2.9
шаг 3), а сейчас молча ничего не доставляет. Замечено при срезе 2 [BUG-1647](BUG-1647-OPEN.md)
вместе с исправленной нормализацией порта по умолчанию в сравнении происхождений.
