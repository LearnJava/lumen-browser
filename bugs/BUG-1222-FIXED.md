# BUG-1222 — `EventSource` (SSE) стабильно получает 401 на same-origin эндпоинте, тогда как XHR с той же сессией проходит

**Статус:** FIXED 2026-10-07
**Компонент:** network/js (`crates/network/src/sse.rs`, JS-обвязка `EventSource`)
**Найден:** 2026-09-29, стенд bankruptcy-platform, сборка `main` 22a782d55

## Симптом

После входа (`test_au`) в stderr каждые несколько секунд:

```
[JS SSE] connect error: network error: sse: server returned 401
```

19 раз за одну сессию. При этом `XMLHttpRequest`/`fetch` к `/api/*` с той же cookie-сессией отвечают 200
(`/api/me`, `/api/v1/notifications/inbox`: 23–47 мс). Значит, запрос EventSource, вероятно, уходит без cookie сессии:
для same-origin `EventSource` cookie отправляются по умолчанию (HTML LS §9.2; `withCredentials` влияет только на cross-origin).

Не проверено: какой именно URL открывает страница и какие заголовки уходят — вывод о cookie сделан по косвенным признакам.

## Что делать

Сравнить заголовки XHR- и SSE-запросов; проверить, что `sse.rs` берёт cookie-jar документа для same-origin.
Тест: локальный сервер, требующий cookie, `new EventSource('/stream')` → должен получить 200.

## Исправление

`sse.rs` не отправлял `Cookie:` вовсе (и не обрабатывал `Set-Cookie`). Теперь `HttpClient::connect_sse` передаёт `EventSource` jar документа (`SseCookies`); `Cookie:` уходит на каждое (пере)подключение, если хост — хост документа. Тест `sse::tests::request_carries_document_cookies`. Остаток: `withCredentials` для cross-origin до провайдера не доходит — cookie на чужой хост не шлются.
