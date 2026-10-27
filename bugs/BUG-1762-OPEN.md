# BUG-1762 — заголовок `Host` теряет нестандартный порт

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-LUMEN-RESOLVE — найден интеграционным тестом `crates/network/tests/cases/lumen_resolve.rs`)
**Область:** network (`crates/network/src/lib.rs` — вызовы `fetch_single(…, &host_ascii, …)`, параметр `request_host_header`; `http/headers.rs` — `headers.add("Host", host)`)

## Симптом

`HttpClient::new().fetch_page("http://x.invalid:54321/")` шлёт на провод:

```
GET / HTTP/1.1
Host: x.invalid
```

RFC 9110 §7.2: `Host` = authority целевого URI без userinfo, то есть `uri-host [":" port]`; нестандартный порт обязан входить в значение. Chrome шлёт `Host: x.invalid:54321`. В `fetch_single` значение заголовка — тот же `host_ascii`, что идёт в DNS/TLS ([lib.rs:2947](../crates/network/src/lib.rs#L2947) и соседние вызовы), порт к нему не дописывается.

Последствия:
- серверы, строящие абсолютные URL из `Host` (Werkzeug/Flask `url_for(_external=True)`, редиректы многих dev-серверов), отправляют на порт 80/443 — редирект с `localhost:3000` уходит в никуда;
- виртуальные хосты и проверки `Host` (Vite `server.allowedHosts`, Django `ALLOWED_HOSTS` с портом) видят другое значение, чем у Chrome;
- отличие от Chrome в отпечатке запроса на любом нестандартном порту.

WPT задевает слабо: wptserve при `Host` без порта подставляет порт своего сервера (`tools/wptserve/wptserve/request.py:278`), страдают только обработчики, возвращающие сырой `Host` (эхо заголовков).

## Что делать

1. Собирать значение `Host` (и `:authority` H2/H3 — проверить, `h3::client_transport::authority_for` уже опускает только 443) как `host_ascii` + `:port`, если порт не стандартный для схемы (80 для `http`/`ws`, 443 для `https`/`wss`); IPv6-литерал — в скобках.
2. Проверить все вызовы `fetch_single`/`do_request` (навигация, подресурсы, preflight CORS, WebSocket upgrade, EventSource).
3. Снять оговорку в `crates/network/tests/cases/lumen_resolve.rs` — проверять `host: x.invalid:{port}`.

## Как проверить

- `cargo test -p lumen-network --test all lumen_resolve` с усиленным утверждением.
- Юнит-тест: `http://a.test:8080/` → `Host: a.test:8080`, `http://a.test:80/` → `Host: a.test`, `https://[::1]:8443/` → `Host: [::1]:8443`.
