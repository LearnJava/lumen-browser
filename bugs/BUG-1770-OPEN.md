# BUG-1770 — `sendBeacon` с https на http возвращает `true`, запрос молча блокируется как mixed content

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-HOST-WPT — `http://localhost` был потенциально доверенным, `http://web-platform.test` — нет)
**Область:** js/network (`crates/js/src/v8_runtime/install/net.rs` — `_lumen_send_beacon`; `crates/network/src/lib.rs` — mixed-content проверка в `fetch_with_redirect`)

## Симптом

`beacon/headers/header-referrer-unsafe-url.https.html` — TIMEOUT. Страница на `https://web-platform.test` шлёт `navigator.sendBeacon("http://web-platform.test:…/…")`, получает `true` и ждёт, пока сервер запишет запрос; запрос до сервера не доходит.

Mixed Content: такой запрос blockable и блокируется (в Lumen — `policy.evaluate` в `fetch_with_redirect`). Что именно ждёт тест (что beacon уйдёт, или что `sendBeacon` вернёт `false`) — не разобрано: сверить с Chrome перед правкой. Минимум — не отдавать `true` для запроса, который заведомо не уйдёт, и не терять ошибку молча.

## Причина

`_lumen_send_beacon` ([net.rs:1116-1135](../crates/js/src/v8_runtime/install/net.rs#L1116)) перед `spawn` проверяет только `check_connect_src` (CSP) и возвращает `true`. POST на отдельном потоке доходит до mixed-content проверки в `fetch_with_redirect` ([lib.rs:2683](../crates/network/src/lib.rs#L2683)), ошибка теряется в `let _ = p.fetch_with_body_sync(…)`.

## Что делать

1. Классифицировать mixed content синхронно, до `spawn`, тем же правилом, что в `fetch_with_redirect` (вынести в общую функцию, не дублировать), и при блокировке вернуть `false` + сообщение в консоль.
2. Сверить с Chrome, что именно делает тест (`header-referrer-unsafe-url.https.html`: куда шлётся beacon и чего ждёт проверка), и записать в баг, если ожидание теста иное, чем `false`.

## Как проверить

- `run_smoke.py` на `/beacon/headers/header-referrer-unsafe-url.https.html`.
- `beacon/` — без регрессий.
