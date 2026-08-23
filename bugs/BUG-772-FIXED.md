# BUG-772 — `WebSocket` не реализует спековый список «заблокированных портов»: конструктор реально коннектится к TCP-порту вместо синхронного `SecurityError`

**Статус:** FIXED 2026-08-24
**Компонент:** network (`crates/network/src/bad_port.rs` — новый модуль; `crates/network/src/lib.rs::require_http_scheme`; `crates/network/src/websocket/mod.rs::require_ws_scheme`)
**Найден:** P2, WPT-VENDOR-websockets, 2026-08-18 — `run_report.py --all --root websockets --recursive`
**Исправлен:** P1, 2026-08-24

## Симптом

WPT-тест `websockets/Create-blocked-port.any.js` перебирает 92 порта из
[списка «port blocking» спеки Fetch](https://fetch.spec.whatwg.org/#port-blocking)
(1, 7, 9, 11, 13, …, 6667, 6697, 10080 — telnet, smtp, pop3, irc и т. п.) и
ожидает, что `new WebSocket('ws://host:<port>/')` синхронно (в рамках
текущей задачи микротаска) откроет соединение с ошибкой (`onerror`), не
дожидаясь сетевого таймаута — конструктор обязан отклонить заблокированный
порт **до** попытки TCP-подключения.

В Lumen `HttpClient::connect` (`crates/network/src/lib.rs:4609`, impl
`JsWebSocketProvider` для `_lumen_ws_connect`) передаёт URL напрямую в
`websocket::WebSocket::connect_deflate` (`crates/network/src/websocket/mod.rs:127`)
без какой-либо проверки порта — ни в `HttpClient::connect`, ни в
`connect`/`connect_deflate` самого модуля `websocket`. `grep -rn
"blocked_port\|bad_port\|BLOCKED_PORTS\|is_bad_port" crates/network/src/`
даёт **ноль совпадений** — списка заблокированных портов в кодовой базе нет
вовсе, ни для WebSocket, ни для обычного `fetch()`/`XMLHttpRequest`.

Следствие — каждый вызов `CreateWebSocketWithBlockedPort(N)` реально
пытается установить TCP-соединение на `127.0.0.1:N`. Для портов, на которых
локально ничего не слушает, попытка стоит ~2.4–2.9 с (реальный ОС-таймаут
`ECONNREFUSED`/`WSAECONNREFUSED`, см. лог: `connect 127.0.0.1:1` → 2.86 с,
`127.0.0.1:7` → 2.4 с, и т. д.) — 92 порта последовательно дают ~230 с,
что многократно превышает внешний таймаут `wptrunner`-раннера (~25 с) и
роняет файл в `TIMEOUT` на всех трёх вариантах (`?default`, `?wss`,
`?wpt_flags=h2`) вместо содержательного `FAIL`/`PASS` по каждому порту.

## Почему это не только тестовый шум

Список «блокированных портов» в Fetch/WebSocket-спеке существует как
защита от cross-protocol-атак (страница инициирует подключение к
telnet/smtp/irc-порту, пытаясь заставить браузер сформировать пакет,
похожий на валидный протокольный запрос этого сервиса). Отсутствие
проверки в Lumen означает, что **любая веб-страница может открыть
WebSocket-подключение к произвольному локальному или внутрисетевому порту**
(включая перечисленные выше «опасные» службы) без какого-либо
клиентского ограничения — минорный, но реальный security-пробел, не
только несоответствие тестовому харнесу.

## Как исправлено (P1, 2026-08-24)

Новый модуль [`crates/network/src/bad_port.rs`](../crates/network/src/bad_port.rs):
отсортированный массив `BAD_PORTS: [u16; 83]` (список спеки целиком, сверен
с `tests/wpt/websockets/Create-blocked-port.any.js` — 83 значения, включая
`0`, `4190` и `6679`, которых в устаревших копиях списка нет) и
`is_bad_port(port) -> bool` бинарным поиском. Порядок массива — инвариант,
его держит собственный тест: без сортировки `binary_search` молча
перестал бы находить часть списка, и проверка выключилась бы незаметно.

Точки проверки — **валидация формы URL**, а не место открытия сокета:

* `require_http_scheme` (`crates/network/src/lib.rs`) — покрывает `http`/`https`
  целиком: `fetch_with_redirect` (значит `fetch()`, `XMLHttpRequest`,
  подресурсы, навигацию **и каждый redirect-hop**, потому что hop идёт через
  ту же функцию) и `EventSource::open_connection` (`sse.rs`);
* `require_ws_scheme` (`crates/network/src/websocket/mod.rs`) — покрывает обе
  точки входа `WebSocket::connect` и `connect_deflate`.

Так проверка гарантированно оказывается до DNS-резолва и до `TcpStream::connect`
(это и есть требование спеки: само подключение к чужой службе уже является
атакой), и её нельзя обойти, добавив ещё один вызов `connect()`.

Класс отказа выбран тот же, что у bad scheme (`ftp://`): `Err(Error::Network)`
без событий `RequestStarted`/`RequestCompleted`/`RequestBlocked` — форма
запроса невалидна, байт не улетал. На JS-стороне ошибка `_lumen_ws_connect`
уже даёт `readyState = 3` + асинхронные `error` и `close(1006)`, чего WPT и
ждёт (спека требует не `throw` из конструктора, а «fail the WebSocket
connection»: блокировка порта происходит внутри «establish a WebSocket
connection», уже после шагов конструктора).

**Не покрыто намеренно:** DoH/DoT/прокси и прочий внутренний трафик браузера
не проходит через `require_*_scheme` — список спеки ограничивает веб-контент,
а не конфигурацию самого браузера.

## Гейт

Юнит-тесты (`crates/network`): 4 в `bad_port.rs` (сортировка, блокируемые,
обычные порты — включая порты WPT-сервера из `tests/wpt/config.json`, соседи
границ) и 3 интеграционных в `lib.rs`. Два последних доказывают главное
свойство напрямую: слушатель поднимается **на самом заблокированном порту**
(10080/6667/6666/4190/6566, первый свободный) и после отказа `accept()`
обязан вернуть `WouldBlock` — то есть TCP-соединения не было; плюс отказ
`WebSocket` укладывается в 500 мс вместо ОС-таймаута. Третий тест держит
обратную сторону — обычный порт по-прежнему ходит в сеть.

Живой прогон WPT, `tests/wpt/run_smoke.py --binary <dev-release>
"/websockets/Create-blocked-port.any.html?default"`:

| | до фикса | после |
|---|---|---|
| Собрано сабтестов | 0 | 84 |
| Результат файла | TIMEOUT (обрезан раннером на 34 с) | OK, все 84 PASS |
| Время | > 34 с | 2.3 с |

(84 = 83 порта + сабтест `Basic check`, проверяющий, что обычный порт
теста не заблокирован.)

## Не расследовано в этой сессии

Другие TIMEOUT/ERROR-паттерны той же категории (не связаны с блокировкой
портов, отдельные вероятные дефекты, не заведены как баги — не хватило
времени на root-cause в рамках одной вендоринг-сессии):
`cookies/003.html` (HttpOnly cookie не долетает до заголовков рукопожатия
WS?), `interfaces/WebSocket/close/close-connecting.html` (закрытие в
состоянии CONNECTING), `send/005.html`/`send/010.html`,
`send-many-64K-messages-with-backpressure.any.js` (`bufferedAmount`-
backpressure), `remove-own-iframe-during-onerror.window.html` (пересекается
с BUG-480, нет отдельного browsing context у iframe), `unload-a-document/003
html`/`004.html`, `Create-on-worker-shutdown.any.js`. См. полный список в
`docs/wpt-vendor-notes/websockets.md`.
