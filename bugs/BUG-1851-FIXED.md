# BUG-1851 — WebSocket-хендшейк не передаёт cookies: same-origin WS получает 401

**Статус:** FIXED 2026-11-26 — `HttpClient` вычисляет `Cookie` из банки тем же способом, что и fetch, и передаёт в рукопожатие через `extra_headers`; cookie-гейт бэкенда открывается, WS подключается вместо поллинга.
**Заведён:** 2026-11-26 (ZCode, живая сессия portal.nextera.ru, Lumen dev-release 9b4e0a6131)
**Компонент:** network ([`websocket/mod.rs`](../crates/network/src/websocket/mod.rs) — `WsCookies`/`cookie_headers`, `connect`/`connect_deflate`; [`websocket/upgrade.rs`](../crates/network/src/websocket/upgrade.rs) — `perform`/`perform_with_deflate`; [`js_providers.rs`](../crates/network/src/js_providers.rs) — `ws_cookie_context`)

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
`login-actions/authenticate` при живом документе портала.

В Chrome 158 на том же аккаунте WS работает (консоль чистая, стрим мгновенный).

## Причина

`WebSocket::connect`/`connect_deflate` не принимали ни cookie-jar, ни заголовки:
`send_upgrade_request` слал только Host/Upgrade/Connection/Key/Version (+ заголовок
протокола/расширения). Цепочка от JS (`_lumen_ws_connect` → `JsWebSocketProvider::connect_cancellable`
→ `connect_deflate`) не имела доступа к банке — и не пыталась. По WHATWG (fetch-стиль
рукопожатия) same-origin WS обязан нести `Cookie`. У `HttpClient` банка есть: fetch прикрепляет
куки через `jar.get_for_request_in_context(host, path_and_query, is_tls, cookie_partition, site_ctx)`,
сеттер — `with_cookie_jar`. SSE-аналог решён раньше тем же способом (`SseCookies`, BUG-1222).

## Фикс

- `websocket::WsCookies { jar, top_level_site, partition }` — контекст cookies документа;
  `cookie_headers()` строит значение `Cookie` fetch-стилем: `cookie_site_context(host,
  top_level_site, false, "GET")` → `get_for_request_in_context`. WS — никогда не top-level
  навигация, поэтому чужой сайт даёт консервативный `CrossSite` (без Lax-исключения).
- Заголовок прокинут через `connect`/`connect_deflate` → `perform`/`perform_with_deflate`
  (новый параметр `extra_headers`) → `send_upgrade_request` — дописывается после фиксированных
  строк RFC 6455. Оба провайдерских пути строят контекст: `WebSocketProvider::connect_ws`
  и `JsWebSocketProvider::connect_ws_impl` (новый хелпер `HttpClient::ws_cookie_context`,
  зеркало `SseCookies`-провода).
- Заголовок `Origin` намеренно не добавлен: на этом пути у `HttpClient` нет надёжного
  источника origin документа (`self_origin` есть только под CSP); для nextera дефект не
  блокирует (бэкенд проверяет cookie-сессию). Отдельный вопрос при появлении origin-контекста.
- `Set-Cookie` из 101-ответа не обрабатывается (как и в SSE); в nextera сессию держит
  cookie из обычных ответов.

## Как проверить

Юнит: `cargo test -p lumen-network ws_handshake_carries_document_cookies` — сервер читает
рукопожатие и утверждает `Cookie: sid=abc`, jar-мок требует `get_for_request_in_context`.
Живой портал, залогиненный: в журнале исчезает `ws: upgrade failed … 401`, появляется
`WebSocketConnected`; «Анализирую…»-стрим завершается мгновенно; форс-редиректы на
Keycloak прекращаются.

Смежные: BUG-307/FETCH_READ_TIMEOUT (та же зона чтения рукопожатия), BUG-1222 (SSE-аналог).
