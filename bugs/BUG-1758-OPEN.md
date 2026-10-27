# BUG-1758 — `fetch()`: `redirect: 'error'|'manual'` игнорируется, CORS-проверки на редиректах и preflight мягче спецификации

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-FETCH-SCOUT, кластер R в [`docs/wpt-vendor-notes/fetch.md`](../docs/wpt-vendor-notes/fetch.md))
**Область:** js (`crates/js/src/shim/fetch_body_shim.js` — `Request.redirect`, `crates/js/src/shim/web_api_shim_mid_b3.js` — вызов `_lumen_fetch_async_start`), core (`lumen_core::ext::JsFetchRequest` — нет поля режима редиректа), network (`crates/network/src/lib.rs` — CORS на редиректах, preflight)

## Симптом

В прогоне `fetch` 2026-10-27 — 514 сабтестов в 35 id `api/cors/*` и `api/redirect/*` ждут отказа или `status 0` и получают обычный ответ:

- `api/redirect/redirect-mode.any.html`: `redirect 301 in error redirect and cors mode` — `Should have rejected`; `... in manual redirect ...` — `Response's status is 0 expected 0 but got 200` (редирект пройден).
- `api/cors/cors-redirect-credentials.any.html` (120 сабтестов): кросс-origin редирект на URL с `user:password@` не отвергается (Fetch §4.4 «HTTP-redirect fetch», шаг 8).
- `api/cors/cors-preflight-status.any.html` (27 × 3 варианта): `Preflight request has been made expected "1" but got "0"` — сервер не видит OPTIONS-запроса; при статусе preflight ≠ 2xx запрос не отвергается.
- `api/cors/cors-multiple-origins.any.html`: несколько значений `Access-Control-Allow-Origin` (`"",http://example.com,*`) не отвергаются.
- `api/cors/cors-preflight-redirect`, `cors-redirect-preflight`: редирект на preflight и preflight после редиректа не отвергаются.

## Причина

1. `Request.redirect` сохраняется в состоянии запроса ([fetch_body_shim.js:508](../crates/js/src/shim/fetch_body_shim.js#L508)), но `JsFetchRequest` (`url, method, headers, body, mode, destination, token`) не несёт режима редиректа — сеть всегда делает `follow`.
2. Preflight-логика есть (`crates/network/src/lib.rs:2790`, `cors::needs_preflight`), но для этих тестов сервер её не наблюдает — установить по представителю `cors-preflight-status` (возможные причины: путь `fetch_request_impl` не идёт через CORS-контекст для кросс-origin на другом порту; запрос считается simple).
3. Проверки «редирект на URL с credentials», «несколько ACAO», «редирект preflight-а» — отсутствуют или мягкие.

## Что делать

1. Добавить в `JsFetchRequest` режим редиректа; `error` — network error на первом 3xx, `manual` — `opaqueredirect` ([BUG-1755](BUG-1755-OPEN.md)).
2. Найти, почему preflight не уходит для `cors-preflight-status`; preflight с не-2xx статусом или с редиректом — network error.
3. CORS-проверка ответа: ровно одно значение ACAO; редирект CORS-запроса на URL с credentials — network error; кросс-origin редирект обнуляет origin (`null`) по Fetch §4.4 шаг 12.

## Как проверить

- `run_smoke.py /fetch/api/redirect/redirect-mode.any.html /fetch/api/cors/cors-preflight-status.any.html /fetch/api/cors/cors-redirect-credentials.any.html /fetch/api/cors/cors-multiple-origins.any.html`.
- Ожидаемый прирост `fetch` до +25 баллов.
