# BUG-862 — `WebSocket.send()` бросает `TypeError` на любом значении, кроме строки и буфера: `null`, число, объект, функция не приводятся к строке

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 25 — живой замер, маркер `ws-echo`)
**Область:** `crates/js/src/dom.rs:9853`–`9881` — `_lumen_ws_bytelen(data)` читает `data.byteLength` без проверки на `null`/`undefined`, а `send` для всего нестрокового идёт в `_lumen_ws_send_bin(this._handle, data instanceof Uint8Array ? data : new Uint8Array(data))`
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
ws.send(null);
// TypeError: Cannot read properties of null (reading 'byteLength')
```

По WebIDL аргумент `send` — объединение `USVString | Blob | ArrayBuffer |
ArrayBufferView`; всё, что не Blob/буфер, конвертируется в `USVString`, то
есть `null` уходит строкой `"null"`, `{}` — `"[object Object]"`, функция —
своим исходником. WPT-тест `websockets/interfaces/WebSocket/send/010.html`
проверяет ровно это: шлёт `[null, undefined, 1, window, document.body, {},
[], ws, function(){}, new Error()]` и сверяет эхо с `String(value)`.

## Прямое измерение

`tests/wpt/verify_focus_mutation_animation_gaps.py --variant ws-echo`
(2026-08-23, dev-release, Linux, `main` = `530d0a444`, `--seconds 5`;
собственный RFC 6455-сервер пробы эхо-транслирует кадры):

```
ws-created readyState=0 url=ws://127.0.0.1:PORT/echo
ws-open readyState=1
ws-send-throws i=0 TypeError: Cannot read properties of null (reading 'byteLength')
ws-checked readyState=1 echoed=0
```

То есть соединение устанавливается и остаётся открытым — ломается ровно
первый `send(null)`, и цепочка теста (каждый следующий вызов делается из
`onmessage` предыдущего) не сдвигается ни на шаг. Строковый `send`
проверен рабочим в том же варианте (последний элемент списка), а
`bufferedAmount` считается.

## Масштаб

Механизм `websocket-send-non-string` в `tests/wpt/timeout_audit.py` — **2 id**
остатка снимка WPT-RUN-5 (`websockets/interfaces/WebSocket/send/010.html`
в вариантах `?default` и `?wss`), но это 8 зависших подтестов на файл:
`WebSockets: sending non-strings (null)`, `(undefined)`, `(1)`,
`([object Object])`, `()`, `(function(){})`, `(Error)`, и внешний
`Constructor succeeds`.

## Направление починки (не предписание)

В `send` привести аргумент по WebIDL: `Blob`/`ArrayBuffer`/`ArrayBufferView`
— как сейчас, всё остальное — `String(data)` и текстовый кадр.
`_lumen_ws_bytelen` тогда получает уже приведённое значение и не читает
`byteLength` у `null`.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_focus_mutation_animation_gaps.py
   --variant ws-echo` — ожидается восемь `ws-message` с `data === String(value)`.
2. WPT: `run_report.py --all --root websockets/interfaces/WebSocket/send`.
