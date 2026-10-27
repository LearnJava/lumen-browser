# BUG-1759 — `Sec-Fetch-*` уходят на не потенциально доверенные URL (`http://`)

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-FETCH-SCOUT, кластер M в [`docs/wpt-vendor-notes/fetch.md`](../docs/wpt-vendor-notes/fetch.md))
**Область:** network (`crates/network/src/fetch_metadata.rs` — `fetch_metadata_headers`; навигационный блок профиля Chrome в fingerprint-наборе)

## Симптом

Fetch Metadata §2 (каждый из `Sec-Fetch-Dest/-Mode/-Site/-User`, шаг 1): «если URL запроса не потенциально доверенный — вернуть», то есть на `http://example.com/` Chrome заголовков `Sec-Fetch-*` не шлёт. `fetch_metadata_headers` ([fetch_metadata.rs:188](../crates/network/src/fetch_metadata.rs#L188)) проверяет только профиль и режим навигации — схема цели не учитывается, и подресурс по `http://` получает `Sec-Fetch-Site/-Mode/-Dest`. Это отличие от Chrome в отпечатке при профиле `Chrome`/`Edge`/`Strict` на любом http-сайте.

В WPT это видно в `fetch/metadata/generated/*.sub.html` (не-`https`): сабтесты `Not sent to non-trustworthy ... destination` — `assert_not_own_property: unexpected property "sec-fetch-site"` (88 сабтестов, 15 id). **Оговорка:** раннер сейчас ходит на `http://localhost`, а `localhost` — потенциально доверенный (Secure Contexts §3.2), так что на этих же тестах и Chrome прислал бы заголовки; честно засчитать починку WPT сможет только после `WPT-HOST-RESOLVE` (хост `web-platform.test`).

## Что делать

1. В `fetch_metadata_headers` (и в навигационном блоке профиля): цель не потенциально доверенная (`http`/`ws` кроме `localhost`/`127.0.0.0/8`/`::1`, не `file:`) — заголовки не добавлять.
2. Юнит-тест: `http://example.com/x.png` — пусто, `http://localhost/x.png` и `https://example.com/x.png` — три заголовка.

## Как проверить

- `cargo test -p lumen-network fetch_metadata`.
- После `WPT-HOST-RESOLVE`: `run_smoke.py /fetch/metadata/generated/element-img.sub.html` — без `unexpected property "sec-fetch-*"`; ожидаемый прирост до +4.7 балла.
