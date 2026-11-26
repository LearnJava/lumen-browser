# BUG-1849 — WebSocket-хендшейк не передаёт cookies: same-origin WS получает 401

**Статус:** OPEN
**Заведён:** 2026-11-26 (ZCode, живая сессия portal.nextera.ru, Lumen dev-release 9b4e0a6131)
**Область:** network (`websocket/upgrade.rs` — `write_upgrade_request`/`perform*`; `websocket/mod.rs` — `WebSocket::connect`/`connect_deflate`; `js_providers.rs` — `JsWebSocketProvider::connect_cancellable`)

## Симптом

Портал nextera после входа держит поток событий через WebSocket; в Lumen рукопожатие падает и
сайт пожизненно живёт на длинном поллинге:

```
[JS WebSocket] connect error: network error: ws: upgrade failed (expected 101): HTTP/1.1 401 Unauthorized
→ GET https://portal.nextera.ru/api/portal/dm?peer=…&after=…   ← 200 (фолбэк-поллинг)
```

Следствия у пользователя: индикатор стрима ответа агента («Анализирую реестр агентов…») висит,
пока поллинг не принесёт ответ с задержкой; сессия периодически уходит в форс-редирект на
Keycloak (`approval_prompt=force`), из-за чего окно оказывалось на
`login-actions/authenticate` при живом документе портала (см. BUG-1850).

В Chrome 158 на том же аккаунте WS работает (консоль чистая, стрим мгновенный).

## Причина

`WebSocket::connect`/`connect_deflate` (`websocket/mod.rs:82,179`) не принимают ни cookie-jar,
ни заголовки: `upgrade::write_upgrade_request` шлёт только Host/Upgrade/Connection/Key/Version
(+ пустой `extra_headers`). Цепочка от JS (`_lumen_ws_connect` →
`JsWebSocketProvider::connect_cancellable` (`js_providers.rs:1582`) → `connect_deflate`) не
имеет доступа к банке — и не пытается. По WHATWG (fetch-стиль рукопожатия) same-origin WS
обязан нести `Cookie`. У `HttpClient` банка есть: fetch прикрепляет куки через
`jar.get_for_request_in_context(host, path_and_query, is_tls, cookie_partition, site_ctx)`
(`lib.rs:3031-3044`), setter — `with_cookie_jar` (`lib.rs:4267`).

## Что делать

В `HttpClient::connect_cancellable` вычислить `Cookie` из `self.cookie_jar` тем же способом,
что и fetch, и передать в `WebSocket::connect_deflate` → `upgrade::perform_with_deflate`
(у `write_upgrade_request` уже есть `extra_headers`). Попутно решить, нужен ли заголовок
`Origin` (Chrome шлёт). Провайдерский трейт `JsWebSocketProvider` менять не требуется.

## Как проверить

Живой портал, залогиненный: в журнале исчезает `ws: upgrade failed … 401`, появляется
`WebSocketConnected`; «Анализирую…»-стрим завершается мгновенно; форс-редиректы на Keycloak
прекращаются. Юнит: сервер-эхо из `ws_permessage_deflate_roundtrip` + проверка, что
`Cookie: k=v` дошёл в рукопожатии.

Смежные: BUG-1850 (раскол документа/адреса при редирект-цепочке — вероятно следствие этой
петли), BUG-307/FETCH_READ_TIMEOUT (та же зона чтения рукопожатия).
