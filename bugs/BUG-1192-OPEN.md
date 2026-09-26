# BUG-1192 — `atob`/`btoa` в Service Worker не по спецификации

**Статус:** OPEN
**Заведён:** 2026-09-26 (P6, по ходу BUG-1133 — вне выданного пункта).
**Область:** js — [`crates/js/src/sw_worker.rs`](../crates/js/src/sw_worker.rs) `install_sw_globals_v8`,
регистрация нативов `atob`/`btoa` (`base64_decode` + `String::from_utf8`, `base64_encode(s.as_bytes())`).

## Симптом (по коду, живой SW не проверялся)

HTML LS §8.3: `atob` — Infra «forgiving-base64 decode» с результатом-двоичной строкой (один
Latin-1 символ на байт), `btoa` — только Latin-1 вход, иначе `DOMException InvalidCharacterError`.
В `ServiceWorkerGlobalScope`:

- `atob` декодирует байты как UTF-8 — `atob(btoa('\xff'))` не возвращает `'\xff'`;
- на невалидном входе натив отдаёт `None` (пустое значение), а не бросает `InvalidCharacterError`;
- `base64_decode` пропускает `=` в любой позиции и не проверяет длину — `atob('YQ==YQ==')` = `'aa'`;
- `btoa` кодирует UTF-8 байты строки и не бросает на символах вне Latin-1.

Окно и dedicated/shared-воркеры исправлены в [BUG-1133](BUG-1133-FIXED.md): там
`worker.rs::b64_decode` — forgiving-base64, `atob_native_v8`/`btoa_native_v8` + `WORKER_ATOB_BTOA_SHIM`.

## Что сделать

Поставить SW те же `atob`/`btoa`, что у dedicated-воркера (`_lumen_atob_impl`/`_lumen_btoa_impl` +
`WORKER_ATOB_BTOA_SHIM`). Осторожно: SW-шим несёт свой base64→байт-строка декодер для тел
ответов именно потому, что нативный `atob` отвечает UTF-8 ([`subsystems/js.md`](../subsystems/js.md)
§SW, пункт 3) — проверить, что ничего не опирается на UTF-8-поведение `atob`/`btoa`.
`sw_worker::base64_decode` используется ещё `filesystem_access` и телами кэша — его не трогать
или трогать с их тестами.
