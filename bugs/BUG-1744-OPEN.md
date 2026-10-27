# BUG-1744 — перехват fetch сервис-воркером: из ответа `respondWith` доходит только тело, `preventDefault`/reject не дают network error

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-SW-SCOUT)
**Область:** network/js (`crates/core/src/ext.rs` — `SwFetchRequest`, `FetchInterceptor`; `crates/network/src/lib.rs:4724-4731`, `:4848`; `crates/storage/src/sw_interceptor.rs`; `crates/js/src/sw_worker.rs` — `_sw_fire_fetch`)

## Симптом

```js
// sw.js
self.onfetch = e => e.respondWith(new Response("x", {status: 404, headers: {"X-A": "1"}}));
// страница
const r = await fetch("intercepted");
r.status;               // 200 — должно быть 404
r.headers.get("X-A");   // null
```

- Канал ответа — `SyncSender<Option<Vec<u8>>>` (`ext.rs` `SwFetchRequest::response_tx`), `FetchInterceptor::intercept` возвращает `Option<Vec<u8>>`. Сетевой клиент собирает из этого `PageResponse { status: 200, headers: Vec::new(), .. }` (`network/src/lib.rs:4731`). Status, statusText, заголовки, `type`, `url` и redirected-флаг ответа воркера теряются (`fetch-cors-exposed-header-names`, `xhr-content-length`, `xhr-response-url`, `mime-sniffing`, `navigation-redirect-resolution`).
- `None` значит «перехвата не было», поэтому `event.preventDefault()`, `respondWith(Promise.reject())`, `respondWith(не Response)` и `Response.error()` уходят в сеть, а не дают network error (`fetch-event-network-error`: «prevent-default: expected network error but loaded», `fetch-event-respond-with-argument`). Ошибка чтения тела ответа (битый chunked) не доходит до `read()` (`fetch-event-respond-with-response-body-with-invalid-chunk`).
- `SwFetchRequest` несёт только `url` и `method`, а `sw_interceptor.rs` всегда шлёт `"GET"` (`:119`). У `FetchEvent.request` нет тела, заголовков, `mode`, `credentials`, `destination`; у события нет `clientId`, `resultingClientId`, `handled`, `preloadResponse` (`fetch-with-body`: `assert_false: expected false got undefined`, `request-end-to-end`: «Unexpected end of JSON input»).
- Синхронный XHR и запросы из Dedicated Worker перехватываются не тем путём или не перехватываются вовсе (`fetch-request-xhr-sync`: «expected 200 but got 404», `dedicated-worker-service-worker-interception`).
- Тейнтинг: ответ no-cors через SW не помечается opaque, поэтому canvas/audio/CSS-подмена ведут себя как same-origin (`fetch-response-taint`, `css-sw-same-origin-substitution`, `fetch-canvas-tainting-*` — TIMEOUT).

## Как найдено

WPT-SW-SCOUT, кластер D (`docs/wpt-vendor-notes/service-workers.md`): 21 id (OK 15, TIMEOUT 5, FAIL 1), потеряно 19.8 балла (6.0 п.п. категории). Представители: `fetch-event-network-error.https.html`, `fetch-cors-exposed-header-names.https.html`.

## Что делать

1. Заменить `Option<Vec<u8>>` в `SwFetchRequest`/`FetchInterceptor` перечислением: `Fallback` (не перехвачено), `NetworkError`, `Response { status, status_text, headers, body, url, response_type }`.
2. `_sw_fire_fetch` (`sw_worker.rs:321`) сериализует весь `Response`; `preventDefault` без `respondWith`, reject и не-`Response` дают `NetworkError`.
3. `network/src/lib.rs:4731` и `:4851` собирают `PageResponse` из полей ответа; `NetworkError` → ошибка запроса (`TypeError` в `fetch`, `error` у XHR).
4. `SwFetchRequest` передаёт метод, заголовки, тело, mode/credentials/destination и id клиента; `sw_interceptor.rs` берёт метод из запроса.

## Как проверить

`run_corpus.py --prefixes service-workers/service-worker/fetch- --processes 6`: `fetch-event-network-error`, `fetch-event-respond-with-argument`, `fetch-cors-exposed-header-names`, `xhr-content-length` без провалов. Кластер D в заметке пересчитать.
