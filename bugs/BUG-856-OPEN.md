# BUG-856 — конструктор `WebSocket` блокирует документ до конца хэндшейка: сервер, который принял TCP и молчит, замораживает страницу навсегда

**Статус:** OPEN (ДОРАБОТКА → [GAP-WSASYNC](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-WSASYNC` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
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

## Срез 1 (2026-09-20, `p6-gap-wsasync`) — асинхронный `connect()`, главный симптом закрыт

`_lumen_ws_connect` (`crates/js/src/v8_runtime/install/net.rs::install_websocket`)
больше не зовёт `provider.connect()` в потоке JS. Хэндл выдаётся немедленно
(новая обёртка `PendingWsSession`, реализующая `JsWebSocketSession` поверх
`Arc<Mutex<PendingWsState>>`), а сам хэндшейк уходит в фоновый поток; когда
он завершается — `Open`/`Error` доставляются через тот же `_lumen_ws_poll`,
которым страница уже пользовалась для входящих кадров. `connect-src`-блок
(GAP-CSPENF срез 11) теперь тоже доставляется асинхронно через пару
`error`+синтетический `close(1006, '', wasClean=false)` в шиме — раньше это
было единственным путём, где `_lumen_ws_connect` мог вернуть `0`
синхронно; теперь `!h` означает только «нет `WebSocketProvider`».

Прямое измерение (`verify_focus_mutation_animation_gaps.py`, dev-release,
Windows, `--seconds 6`):

| вариант | тики (было → стало) | ключевой маркер |
|---|---|---|
| `ws-connect-hang` | 0 → **8** | `wsh-after-ctor readyState=0` — конструктор не блокирует |
| `ws-connect-refused` | 9 → 8 | `wsr-after-ctor readyState=0`, затем `wsr-error`, `wsr-close code=1006 clean=false` (было `readyState=3` сразу из конструктора) |
| `ws-close-connecting` | 0 → 6 | `wsc-before readyState=0`, `wsc-send-throws InvalidStateError`, `wsc-after-close readyState=2` |

**Не в этом срезе:**
- `close()`, вызванный во время хэндшейка к серверу, который **не отвечает
  дольше `FETCH_READ_TIMEOUT` (60 с)** (`crates/network/src/lib.rs`), не
  переводит `readyState` в `CLOSED` раньше этого таймаута — фоновый поток
  ждёт внутри `websocket::WebSocket::connect_deflate`
  (`upgrade::perform_with_deflate`), и `close_requested` проверяется только
  после того, как тот вызов вернётся. У WPT-теста `close-connecting.html`
  сервер отвечает паузой 10 с (`/sleep_10_v13`), так что тест это не заденет,
  но правильная фикса — отменяемый хэндшейк (токен отмены до
  `TcpStream`/`read_exact`, по образцу `AbortWatchdog`), не заведённая здесь.
- [BUG-869](BUG-869-OPEN.md) (синхронный `send()`, бэкпрешер) — отдельная
  половина той же `GAP-WSASYNC`, не тронута.
- [BUG-862](BUG-862-FIXED.md) (`send(null)` кидает `TypeError` в
  `_lumen_ws_bytelen`) — увидено попутно в `ws-echo`, уже заведено, не
  дублируется. Закрыт P6, 2026-09-20.

Не проверялось: реальный прогон `run_report.py --root websockets` (только
живой probe выше).

## Срез 4 (2026-09-20, `p6-gap-wsasync-srez4`) — отменяемый хэндшейк, остаток среза 1 закрыт

`close()`, вызванный, пока `readyState` ещё `CONNECTING`, теперь прерывает
фоновый хэндшейк немедленно вместо ожидания `FETCH_READ_TIMEOUT` (60 с) —
ровно тот остаток, что срез 1 сознательно не тронул.

Механизм — тот же, что уже использует `do_request` для отмены `fetch()` в
полёте (`AbortToken`/`AbortScope`/`AbortWatchdog`, `crates/network/src/lib.rs`),
доведённый до WebSocket-хэндшейка:

- `JsWebSocketProvider` (`crates/core/src/ext.rs`) получил новый метод
  `connect_cancellable(url, protocols, token: &AbortToken)` с реализацией по
  умолчанию, делегирующей в `connect()` (моки в
  `crates/js/src/dom/tests/v8_ws_sse.rs` не блокируются, поэтому дефолт для
  них корректен без правок).
- `HttpClient::connect_cancellable` (`crates/network/src/lib.rs`) устанавливает
  токен на текущий поток через `AbortScope` на время синхронного
  `connect_ws_impl` (общая приватная реализация, вынесенная из старого тела
  `connect()`).
- `websocket::WebSocket::connect_deflate` (`crates/network/src/websocket/mod.rs`)
  читает токен обратно через `current_abort_token()` и оборачивает только
  блокирующее чтение Upgrade-ответа (`upgrade::perform_with_deflate`) в
  `AbortWatchdog`, который делает `shutdown()` сокета при отмене — тот же
  приём, что `do_request` уже применяет к чтению тела ответа.
- `PendingWsSession::close()` (`crates/js/src/v8_runtime/install/net.rs`)
  при `Connecting` теперь не только запоминает `close_requested`
  (код/причина для случая гонки с успешным подключением), но и сразу зовёт
  `cancel.abort()`; фоновый поток вызывает `provider.connect_cancellable`
  вместо `provider.connect`.

Прямое измерение (`verify_focus_mutation_animation_gaps.py --variant
ws-close-connecting`, dev-release, Windows, `--seconds 8`): против `/sleep`
(сервер пробы принимает TCP и не отвечает на Upgrade вовсе — тот же сервер,
что и `ws-connect-hang`) `close()`, позванный на 1000 мс, теперь доводит до
`wsc-error readyState=2` и `wsc-close readyState=3 code=1006 clean=false` в
пределах 8-секундного окна теста; до этого среза оба маркера появились бы
только после полных 60 с `FETCH_READ_TIMEOUT`, то есть не появились бы в
этом окне вовсе. `ws-connect-hang`/`ws-echo` перемерены без регрессии.

Не в этом срезе:
- Реальный прогон `run_report.py --root websockets --recursive` — ни один
  срез 1–4 его не делал, только живые probe.
- [BUG-869](BUG-869-OPEN.md) фактически закрыт срезами 2–3 (`GAP-WSASYNC`),
  но статус `GAP-WSASYNC` в `ROADMAP.md` остаётся `planned` до WPT-прогона
  выше.
- [BUG-862](BUG-862-FIXED.md) — закрыт P6 отдельным срезом, 2026-09-20.
