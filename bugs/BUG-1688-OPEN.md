# BUG-1688 — в воркерах нет `crypto`: 110 тестов WebCryptoAPI `*.any.worker.html` падают с ReferenceError

**Статус:** OPEN
**Заведён:** 2026-10-23 (сессия Chat, по вердиктам WPT-RUN-16 — `docs/wpt/pass-rate.md`, строка `WebCryptoAPI` таблицы падений)
**Область:** js (`crates/js/src/worker.rs` — глобалы воркера; `crates/js/src/v8_runtime/install/platform.rs` — нативы Web Crypto)

## Симптом

В выделенном воркере `self.crypto` не определён: `crypto.getRandomValues(...)`,
`crypto.subtle.digest(...)` и т. д. бросают `ReferenceError: crypto is not defined`. На странице
(window) те же вызовы работают.

В прогоне 2026-10-22 так падают **110** тестов `tests/wpt/WebCryptoAPI/**/*.any.worker.html` (из
313 в категории, балл 14.62); то же было и на `d913a39b9` (2026-10-05), то есть это не регрессия,
а пробел. По спецификации `crypto` есть в `WindowOrWorkerGlobalScope`.

Репро:

```
tests/wpt/run_corpus.py --binary <dev-release> --prefixes WebCryptoAPI/getRandomValues.any.worker.html
```

## Почему (проверено по коду 2026-10-23)

- Шим Web Crypto — IIFE `crates/js/src/shim/web_api_shim_tail_b.js:182-509`: ставит
  `window.crypto` (`:498`), `.subtle` и `CryptoKey` — только при `_lumen_secure_context !== false`
  (`:505-507`). Обращается к `window` и `_lumen_secure_context`, которых в воркере нет.
- Его нативы (`_lumen_get_random_bytes`, `_lumen_sha_digest`, 16 `_lumen_subtle_*`)
  регистрирует `install_crypto_and_typed_om` (`crates/js/src/v8_runtime/install/platform.rs:1453-1630`),
  а её вызывает только рантайм страницы (`crates/js/src/v8_runtime.rs:686`); сигнатура тянет
  состояние страницы (`doc`, `dom_dirty`, вычисленные стили).
- Глобалы воркера собирает `install_worker_globals_v8` (`crates/js/src/worker.rs:1275-1434`) и общий
  для dedicated/shared/service воркеров `install_worker_scope_globals_v8` (`worker.rs:425-535`) —
  туда уже перенесены MessageChannel, WebAssembly streaming и Trusted Types (BUG-1086, `:533`), а
  crypto — нет.
- Реализация на Rust от страницы не зависит: ключи — `thread_local!` в `subtle_crypto.rs:106-110`,
  у воркера свой поток. `isSecureContext` в воркере есть (`worker_shims.rs:152`), `DOMException` —
  тоже (`worker.rs:1398-1399`).

## Что сделать (направление, не предписание)

1. Вынести регистрацию нативов Web Crypto из `install_crypto_and_typed_om` в отдельную функцию без
   зависимостей от страницы и звать её и из рантайма страницы, и из
   `install_worker_scope_globals_v8`.
2. Шим — на `globalThis` вместо `window`; условие для `.subtle`/`CryptoKey` — от `isSecureContext`
   (в воркере) / `_lumen_secure_context` (на странице).
3. Вычислить шим в `install_worker_scope_globals_v8`.

## Как проверить

- Юнит-тест в `crates/js` (рядом с `crates/js/src/dom/tests/v8_webcrypto.rs`): в выделенном воркере
  `crypto.getRandomValues(new Uint8Array(8))` и `crypto.subtle.digest('SHA-256', …)` работают.
- `tests/wpt/run_corpus.py --prefixes WebCryptoAPI --processes 6` — ни одного
  `crypto is not defined` в `*.any.worker.html`; балл категории выше 14.62 (тихий перепрогон
  WPT-RUN-16 — 22.23).
