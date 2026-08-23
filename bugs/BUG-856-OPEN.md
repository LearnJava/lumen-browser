# BUG-856 — конструктор `WebSocket` блокирует документ до конца хэндшейка: сервер, который принял TCP и молчит, замораживает страницу навсегда

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 25 — живой замер, маркеры `ws-connect-hang`, `ws-connect-refused`, `ws-close-connecting`)
**Область:** `crates/js/src/v8_runtime.rs:3502` — комментарий модели прямо говорит «Phase 0 model: **synchronous connect**»; `_lumen_ws_connect` (`v8_runtime.rs:3517`) зовёт `provider.connect(&url, &protos)` в потоке JS и возвращает хэндл только после ответа. Ниже по стеку — `crates/network/src/lib.rs::HttpClient::connect` (impl `JsWebSocketProvider`) → `crates/network/src/websocket/mod.rs::connect_deflate`
**Владелец:** P1/P3 (движок: `lumen-js` + `lumen-network`). Заведён P2 в ходе WPT-задачи, здесь не чинится.
**Родственный:** [BUG-772](BUG-772-FIXED.md) — тот же синхронный `connect`, увиденный через список «заблокированных портов» (92 порта × ~2.5 с реального `ECONNREFUSED` = ~230 с на файл). Здесь измерена **неограниченная** половина: если порт принимает соединение и не отвечает, ожидание не кончается никогда.

## Симптом

```js
console.log('before');                                  // печатается
var ws = new WebSocket('ws://127.0.0.1:PORT/sleep');    // управление не возвращается
console.log('after', ws.readyState);                    // никогда
```

Замирает не только этот скрипт: не выполняются ни последующие теги
`<script>`, ни таймеры, ни рендеринг — документ мёртв. `WebSocket` по спеке
(WHATWG §the-websocket-interface, «establish a WebSocket connection» —
параллельная задача) обязан вернуть объект в состоянии `CONNECTING`
немедленно и сообщать об исходе событиями.

## Прямое измерение

`tests/wpt/verify_focus_mutation_animation_gaps.py` (2026-08-23, dev-release,
Linux, `main` = `530d0a444`, `--seconds 5`); собственный минимальный
RFC 6455-сервер пробы: `/echo` отвечает на хэндшейк и эхо-транслирует кадры,
любой другой путь принимает соединение и **молчит**.

| вариант | тики страницы | маркеры |
|---|---|---|
| `ws-connect-hang` (сервер молчит) | **0** | только `wsh-before-ctor` — `wsh-after-ctor` нет |
| `ws-close-connecting` (то же, через тест WPT) | **0** | ни одного маркера, даже `script-start` шаблона |
| `ws-connect-refused` (порт 9 закрыт) | 9 | `wsr-after-ctor readyState=3`, затем `wsr-error`, `wsr-close code=1006 clean=false` |
| `ws-echo` (сервер отвечает) | 9 | `ws-created readyState=0`, `ws-open readyState=1` |

Ноль тиков `setInterval` — независимое свидетельство того, что стоит весь
event loop страницы, а не только этот скрипт. На отказе (`refused`) виден
второй, меньший дефект той же природы: `readyState` сразу `3` (CLOSED)
вместо `0` (CONNECTING), потому что исход известен уже к моменту возврата из
конструктора.

## Масштаб

Механизм `websocket-connect-blocks` в `tests/wpt/timeout_audit.py` — **6 id**
остатка снимка WPT-RUN-5: `websockets/interfaces/WebSocket/close/close-connecting.html`
(`?default`, `?wss`), `websockets/keeping-connection-open/001.html`
(оба варианта), `websockets/unload-a-document/003.html`,
`websockets/send-many-64K-messages-with-backpressure.any.html`. Первые два —
ровно эта форма: тест открывает соединение к `/sleep_10_v13` (хэндлер
`wptserve`, который держит паузу 10 с) и проверяет `close()` в состоянии
CONNECTING; страница замерзает до того, как выполнится первая строка теста.

Важнее числа то, что это ещё и **источник механизма `hung-browser`**: браузер
в шарде не перезапускается после таймаута, поэтому одна такая страница
забирает весь остаток шарда (см. готчу в `CLAUDE.md`, срез 11).

## Направление починки (не предписание)

Перевести `_lumen_ws_connect` на ту же модель, что уже используется для
входящих кадров: отдать хэндл сразу, вести хэндшейк в фоновом потоке и
доставлять `open`/`error`/`close` через существующий `_lumen_ws_poll`.
Тогда же станет корректным `readyState === CONNECTING` сразу после
конструктора и заработает `close()` во время хэндшейка (`wasClean === false`).

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_focus_mutation_animation_gaps.py
   --variant ws-connect-hang --variant ws-close-connecting` — ожидается
   `wsh-after-ctor readyState=0` и ненулевое число тиков в обоих.
2. WPT: `run_report.py --all --root websockets --recursive`.
