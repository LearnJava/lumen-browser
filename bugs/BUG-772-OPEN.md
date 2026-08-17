# BUG-772 — `WebSocket` не реализует спековый список «заблокированных портов»: конструктор реально коннектится к TCP-порту вместо синхронного `SecurityError`

**Статус:** OPEN
**Компонент:** network (`crates/network/src/lib.rs::HttpClient::connect` — impl `JsWebSocketProvider`, ~строка 4609; `crates/network/src/websocket/mod.rs::connect`/`connect_deflate`, ~строки 73/127)
**Найден:** P2, WPT-VENDOR-websockets, 2026-08-18 — `run_report.py --all --root websockets --recursive`

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

## Предлагаемый фикс

Добавить статический список заблокированных портов (можно взять один в
один из `Create-blocked-port.any.js` — 92 значения) и проверку в начале
`HttpClient::connect`/`websocket::WebSocket::connect` (до резолва DNS и
до открытия сокета): если порт из URL входит в список — сразу вернуть
`Err(Error::Network(...))` (маппится в `SecurityError`/`onerror` на JS-
стороне так же, как остальные ошибки подключения в `_lumen_ws_connect`).
Тот же список стоит переиспользовать и для `fetch()`/`XMLHttpRequest`
(там сейчас тоже нет проверки, но остальные категории её ещё не
проверяли — не подтверждено этой сессией, отдельная проверка).

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
