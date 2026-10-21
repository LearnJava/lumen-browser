# BUG-1665 — `fetch(url, {method:'HEAD'})` уходит на сервер как GET; ответ на HEAD и 204/304 с `Content-Length` читается как тело

**Статус:** FIXED 2026-10-21 (Chat)
**Заведён:** 2026-10-21 (Chat; найден пробным прогоном модели MiniMax M3.1 Flash через Kilo Gateway, проверен и доведён Claude)
**Область:** network (`crates/network/src/lib.rs` — `fetch_with_redirect`, `fetch_request_impl`, `fetch_cors`; `crates/network/src/http1/response.rs` — `read_response`; `crates/network/src/http1/chunked.rs` — `read_response_streamed`, `BodyFraming`)

## Симптом

1. `fetch(url, {method:'HEAD'})` отправлял на провод `GET`: `fetch_with_redirect` брал метод из тела запроса или из CORS-контекста, а у `fetch()` без тела нет ни того, ни другого — срабатывал `.unwrap_or("GET")`.
2. HTTP/1-ответ без тела, но с `Content-Length` читался как тело:
   - ответ на `HEAD` с `Content-Length: N` — клиент ждал N байт, которых сервер не шлёт: `ConnectionClosed` («read body: failed to fill whole buffer») или таймаут сокета;
   - `304 Not Modified` / `204 No Content` с `Content-Length` — ветка «нет тела» стояла **после** ветки `Content-Length`, поэтому на keep-alive соединении клиент съедал начало следующего ответа.
3. HEAD после редиректа превращался в GET: метод на redirect-hop не переносился.

## Проба

Mock-сервер в `lumen-network` (тесты ниже):

| запрос | ответ сервера | до | после |
|---|---|---|---|
| `fetch_sync(url, "HEAD")` | `200`, `Content-Length: 4`, без тела | `Err(ConnectionClosed "read body: failed to fill whole buffer")` | `Ok`, тело пустое |
| HEAD, затем GET на одном keep-alive | `200 CL:4` без тела, потом `200 CL:6 second` | `Err(ConnectionTimedOut "read body…")` | HEAD пустой, GET = `second` |
| request-line `fetch_sync(url, "HEAD")` | — | `GET /doc` | `HEAD /doc` |

## Причина

- Метод запроса без тела не передавался в `fetch_with_redirect` вовсе — только тело (`RequestBody::method`) и `CorsContext::method` несли метод.
- `read_response` / `read_response_streamed` выбирали рамку тела только по заголовкам и статусу, не зная метода запроса; проверка 204/304 шла после `Content-Length`.

## Исправление (2026-10-21, Chat)

- `fetch_with_redirect` получил параметр `method: &str`; метод actual-запроса: тело → CORS-контекст → `method`. Все вызовы передают свой метод (`fetch()` — `method_upper`, `fetch_cors` — метод `CorsRequest`, навигация и подресурсы — `"GET"`).
- Метод следующего hop-а по Fetch §4.4: 307/308 сохраняют; 301/302 сохраняют, кроме POST; 303 сохраняет GET/HEAD, остальное → GET.
- `http1::chunked::response_has_no_body(status, method)` — один предикат для буферизованного и streaming-путей: ответ на HEAD и статусы 1xx/204/304 (RFC 9110 §9.3.2, §15.3.5, §15.4.5). Он проверяется **до** `Transfer-Encoding`/`Content-Length`; новый `BodyFraming::None`.
- HTTP-кэш не затронут: путь `fetch()` в него не пишет, поэтому пустое тело HEAD-ответа не может подмениться под GET.

## Как проверить

`cargo test -p lumen-network --lib` — тесты `request_method_reaches_the_wire`, `head_response_body_is_empty_despite_content_length`, `head_response_does_not_consume_the_next_response`, `not_modified_response_body_is_empty_despite_content_length`, `no_content_response_body_is_empty_despite_content_length`, `head_method_survives_a_307_redirect`, `head_method_survives_a_303_redirect`, `http1::tests::no_body_detection_covers_head_and_no_body_statuses`. До фикса падали (наблюдалось) `head_response_*`.
