# BUG-1755 — `fetch()`: `Response.type` всегда `"default"`, отфильтрованных ответов нет

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-FETCH-SCOUT, кластер T в [`docs/wpt-vendor-notes/fetch.md`](../docs/wpt-vendor-notes/fetch.md))
**Область:** js (`crates/js/src/shim/fetch_body_shim.js` — `_lumen_response_from_fetch_cache`, `responseSlots`), network (`crates/network/src/js_providers.rs` — что `fetch_request` знает о tainting)

## Симптом

```js
const r = await fetch('/fetch/api/resources/top.txt');   // same-origin
r.type;   // "default", ожидается "basic"
```

Кросс-origin `mode: 'cors'` должен давать `"cors"`, `mode: 'no-cors'` — `"opaque"` (status 0, пустые заголовки, тело `null`), `redirect: 'manual'` — `"opaqueredirect"`. В прогоне `fetch` 2026-10-27 — 546 сабтестов в 45 id падают на `assert_equals: Response's type is basic expected "basic" but got "default"` (`api/basic/*` — `accept-header`, `request-headers`, `mode-same-origin`, `api/redirect/redirect-origin`, `api/cors/*`). Проба `zz-scout-probe2.html` (временная, удалена) подтвердила для простого same-origin GET.

## Причина

Сетевой путь собирает ответ через `responseSlots()` — у него `type: 'default'` (значение для `new Response()`), и `_lumen_response_from_fetch_cache` его не переписывает ([fetch_body_shim.js:419](../crates/js/src/shim/fetch_body_shim.js#L419)). Response tainting (Fetch §4.1 «main fetch», шаги 12–14: `basic`/`cors`/`opaque`) не вычисляется нигде; соответственно нет и фильтрации: у `cors`-ответа видны заголовки вне CORS-safelisted и `Access-Control-Expose-Headers`, у `opaque` — статус и тело.

## Что делать

1. Нативная сторона (`fetch_request`) возвращает tainting: `basic` для same-origin (после редиректов — по origin финального URL с учётом флага «был кросс-origin редирект»), `cors` для кросс-origin в `mode: cors`, `opaque` для кросс-origin `no-cors`.
2. Шим ставит `st.type` и фильтрует: `cors` — только safelisted + exposed заголовки; `opaque` — `status 0`, `statusText ''`, пустые `headers`, `body null`, `url ''`.
3. `redirect: 'manual'` — `opaqueredirect` (вместе с [BUG-1758](BUG-1758-OPEN.md)).

## Как проверить

- `run_smoke.py /fetch/api/basic/accept-header.any.html /fetch/api/basic/mode-same-origin.any.html /fetch/api/redirect/redirect-origin.any.html` — без `got "default"`.
- `run_corpus.py --prefixes fetch` — ожидаемый прирост до +36 баллов (546 сабтестов; часть тех же сабтестов держат и другие дефекты).
