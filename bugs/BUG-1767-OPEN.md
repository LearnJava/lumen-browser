# BUG-1767 — cookies: `credentials: "omit"` сохраняет и шлёт cookie; host-only cookie уходит на поддомены

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-HOST-WPT — на хосте `localhost` тест до проверки не доходил)
**Область:** network (`crates/network/src/lib.rs` — `fetch_with_redirect`, `credentials_allowed`), storage (`crates/storage/src/cookies.rs` — `get_for_request`, `domain_matches`)

## Симптом

`fetch/api/cors/cors-cookies.any.html` / `.any.worker.html`, сабтест «Include mode: 1 cookie»: ожидается `a=1`, приходит `g=7; a=1`. Cookie `g=7` выставлена ответом на запрос с `credentials: "omit"` к другому хосту семьи `web-platform.test`.

## Причина

Две независимые ошибки, обе нужны для симптома:

(a) **`omit` не соблюдается.**
- Хранение: в `fetch_with_redirect` ([lib.rs:3066-3075](../crates/network/src/lib.rs#L3066)) `jar.process_set_cookie` зовётся на каждом hop-е без учёта `cors_ctx.credentials_mode`. Fetch §4.6 «HTTP-network fetch» шаг 14: `Set-Cookie` сохраняются, только если *include credentials* истинно — при `omit` (и при `same-origin` для кросс-ориджин запроса) ответные cookie игнорируются.
- Отправка: `credentials_allowed` ([lib.rs:2884-2888](../crates/network/src/lib.rs#L2884)) — `true` для любого запроса без кросс-ориджин цели, то есть same-origin `fetch(…, {credentials: "omit"})` всё равно несёт `Cookie`.

(b) **Нет флага host-only.** Cookie без атрибута `Domain` (RFC 6265 §5.3 шаг 6: host-only-flag = true) хранится с `domain = <хост>`, а `domain_matches` ([cookies.rs:360](../crates/storage/src/cookies.rs#L360)) принимает любой суффикс `.<domain>`. Поэтому host-only cookie `web-platform.test` уходит на `www1.web-platform.test`. RFC 6265 §5.4 шаг 1: host-only cookie — только при точном совпадении канонизированного хоста.

## Что делать

1. Протащить режим учётных данных до `fetch_with_redirect`: при `omit` (и `same-origin` + кросс-ориджин) не звать `process_set_cookie` и не прикладывать `Cookie`; поведение без `cors_ctx` (навигация, обычные подресурсы) не менять.
2. Добавить в таблицу `cookies` признак host-only (миграция схемы; существующие строки — по наличию ведущей точки/атрибута, если это различимо, иначе host-only = false), выставлять его в `process_set_cookie` при отсутствии `Domain`, учитывать в `get_for_request` и в замене/удалении cookie (ключ RFC 6265 §5.3 шаг 11 — name+domain+path; host-only и domain-cookie одного имени — разные записи только в смысле флага, проверить по Chrome).
3. Юнит-тесты в `crates/storage` (host-only не матчит поддомен, domain-cookie матчит) и в `crates/network` (omit не хранит и не шлёт).

## Как проверить

- `run_smoke.py` на `/fetch/api/cors/cors-cookies.any.html` и `.any.worker.html`.
- `cookies/` и `fetch/api/credentials/` — без регрессий.
