# BUG-862 — `WebSocket.send()` бросает `TypeError` на любом значении, кроме строки и буфера: `null`, число, объект, функция не приводятся к строке

**Статус:** FIXED 2026-09-20 (P6)
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 25 — живой замер, маркер `ws-echo`)
**Область:** `crates/js/src/shim/web_api_shim_mid_b2.js` (`_lumen_ws_bytelen`, `WebSocket.prototype.send`) — файл переехал из `dom.rs` (устаревшая ссылка `crates/js/src/dom.rs:9853`–`9881` в исходной заявке) в шим до этого фикса.
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, закрыт P6 (неотложная полоса).

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

## Починено (P6, 2026-09-20)

`_lumen_ws_bytelen`/`WebSocket.prototype.send` (`crates/js/src/shim/web_api_shim_mid_b2.js`) теперь явно
различают WebIDL-объединение `(BufferSource or Blob or USVString)`: новый хелпер `_lumen_ws_is_buffer_source(data)`
(`data instanceof ArrayBuffer || ArrayBuffer.isView(data)`) распознаёт буфер, `data instanceof Blob` — блоб
(поведение блоба не менялось — он и раньше уходил в бинарную ветку, отдельный дефект передачи содержимого блоба
сюда не входит), а всё остальное (`null`/`undefined`/число/обычный объект/функция) приводится через `String(data)`
к USVString вместо чтения `.byteLength` у `null`/`undefined`, что и роняло `TypeError`.

Проверено:
- Живой проб `verify_focus_mutation_animation_gaps.py --variant ws-echo`: было `ws-send-throws i=0 TypeError:
  Cannot read properties of null (reading 'byteLength')`, стало восемь `ws-message` с `data === String(value)`
  подряд (`null`→`"null"`, `undefined`→`"undefined"`, `1`→`"1"`, `{}`→`"[object Object]"`, `[]`→`""`,
  `function(){}`→`"function () {}"`, `new Error()`→`"Error"`, `"plain"`→`"plain"`).
- Реальный WPT: `run_report.py --all --root websockets/interfaces/WebSocket/send` — файл `010.html?default`
  перешёл из harness `ERROR`/зависших подтестов в 11 `UNEXPECTED-PASS` (`Constructor succeeds`, `sending
  non-strings` на `null`/`undefined`/`1`/`[object Window]`/`[object Object]` ×3/`()`/`function(){}`/`Error`);
  `send/002.html` дал ещё 2 `UNEXPECTED-PASS` (`readyState is CONNECTING`), `send/003.html` — 2 (surrogate pairs),
  `send/005.html` — 1 (`sending null`, отдельный минимальный файл). Baseline `.ini` не перегенерирован в этом
  срезе — переходит следующему, кто трогает эту категорию (не входило: генерация `--update-expected`).
- `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` — чист (Rust-код не менялся,
  только `.js`-шим).

Не входило: передача реального содержимого `Blob` по WebSocket (сейчас `new Uint8Array(blob)` даёт пустой
0-байтовый фрейм — статус-кво, не регрессия этого фикса, отдельный незаведённый дефект).
