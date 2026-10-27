# BUG-1755 — `fetch()` отвергает корректные запросы: метод без тела, нестандартный метод, `data:;base64`, 3xx без `Location`, битое сжатие

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-FETCH-SCOUT, кластер N в [`docs/wpt-vendor-notes/fetch.md`](../docs/wpt-vendor-notes/fetch.md))
**Область:** network (`crates/network/src/js_providers.rs` — `fetch_request_impl`, проверка метода; разбор `data:`; обработка 3xx и `Content-Encoding`)

## Симптом

Проба (временная `tests/wpt/fetch/zz-scout-probe2.html`, удалена; `dev-release` от `main` `2a00e31ec`) — каждый вызов ниже отклоняется `TypeError: fetch: network error`, хотя должен разрешиться:

| Вызов | Ожидание |
|---|---|
| `fetch(url, {method: 'POST'})` (без тела), так же `OPTIONS`, `DELETE`, `{method:'POST', body:null}` | ответ сервера (200/204) |
| `fetch(url, {method: 'SUPER', body: 'x'})` и любой метод-токен кроме POST/PUT/PATCH/DELETE с телом | ответ сервера |
| `fetch('data:;base64,WA')` (пустой MIME-тип) | 200, тело `X` |
| `fetch('/fetch/api/resources/redirect.py?redirect_status=301')` (3xx без `Location`) | ответ 301 как есть |
| `fetch('/fetch/content-encoding/gzip/resources/bad-gzip-body.py')` | промис разрешается, ошибка — при чтении тела |

`GET`/`PUT` с телом проходят. В логе браузера запросов POST без тела нет вовсе — отказ до сокета. В прогоне `fetch` на `network error`/`Failed to fetch` падают 393 сабтеста в 76 id (`api/redirect/redirect-location`, `redirect-origin`, `redirect-count`, `api/basic/request-headers`, `response-null-body`, `header-value-combining`, `api/cors/cors-preflight-star`, `cors-preflight`, `data-urls/base64`, `data-urls/processing`, `content-encoding/*/bad-*-body`).

## Причина

[js_providers.rs:998](../crates/network/src/js_providers.rs#L998):

```rust
match (req.body.is_some(), method_upper.as_str()) {
    (false, "GET" | "HEAD") | (true, "POST" | "PUT" | "PATCH" | "DELETE") => {}
    (false, m) => return Err(... "fetch: Phase 0 supports GET/HEAD only, got {m}"),
    (true, m)  => return Err(... "fetch_with_body: unsupported method {m}"),
}
```

Ограничение «Phase 0» пережило реализацию тел запросов. Остальные три пункта — отдельные места того же пути (разбор `data:` с пустым MIME перед `;base64`; редирект без `Location` считается ошибкой, хотя Fetch §4.4 «HTTP-redirect fetch» шаг 3 возвращает сам ответ; ошибка декодирования `Content-Encoding` поднимается при получении заголовков, а не при чтении тела).

## Что делать

1. Разрешить любой метод-токен (Fetch §2.2: `method` — token, кроме forbidden `CONNECT`/`TRACE`/`TRACK`, их уже отсекает конструктор `Request`), с телом и без; для POST/PUT/PATCH без тела слать `Content-Length: 0`.
2. `data:` — по алгоритму «data: URL processor» (Fetch §4.5.1): пустой MIME → `text/plain;charset=US-ASCII`.
3. 3xx без `Location` (или с пустым) — вернуть ответ как есть.
4. Ошибку декодирования тела отдавать в поток тела (`read()` реджектится), а не в промис `fetch()`.

## Как проверить

- `run_smoke.py /fetch/api/basic/response-null-body.any.html /fetch/api/redirect/redirect-location.any.html /fetch/data-urls/base64.any.html /fetch/content-encoding/gzip/bad-gzip-body.any.html` — без `network error`.
- Ожидаемый прирост `fetch` — от +8 (сабтесты с методом в имени) до +40 баллов (все `network error`).
