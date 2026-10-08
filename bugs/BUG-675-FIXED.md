# BUG-675 — `navigator.serviceWorker.register()` performs no validation at all on the script URL — cross-origin scripts and `javascript:`-scheme URLs both register successfully instead of rejecting

**Статус:** FIXED 2026-09-26 (P3)
**Компонент:** js — `crates/js/src/shim/web_api_shim_mid_b.js` (`_sw_container.register`, `_sw_check_register_urls`)
**Найден:** P2, WPT-VENDOR-service-workers (2026-08-06), live `--mcp-live-port` probe (same session as [BUG-674](BUG-674-FIXED.md); the category's own WPT run gave no signal — all `.https.` ids TIMEOUT on the documented TLS gap `UnknownIssuer`)

## Механизм

`register: function(scriptUrl, options)` (`dom.rs:4999`) does nothing with `scriptUrl` beyond `String(scriptUrl)` and building a registration object around it — no scheme check, no same-origin check, no MIME-type check (the fetch that eventually happens, `_sw_run_lifecycle`'s `fetch(scriptURL)`, is fire-and-forget with `.catch(function() {})`, so even a failed fetch doesn't reject the registration promise or change its resolved state). Per the Service Worker spec (§4.7 "Start Register" / "Update"), registration must be rejected with `TypeError` for a disallowed URL scheme and `SecurityError` for a script URL whose origin differs from the registering document's origin — Lumen's implementation performs neither check and resolves `Promise.resolve(reg)` unconditionally.

## Живое воспроизведение

Через `--mcp-live-port` (`file://` страница, реальный навигированный origin `file://D`):

```js
navigator.serviceWorker.register('https://evil.example/sw.js', {scope: '/'})
// => resolves: {"ok":true,"url":"https://evil.example/sw.js"}
// spec requires: reject SecurityError (script URL origin != document origin)

navigator.serviceWorker.register('javascript:alert(1)', {scope: '/'})
// => resolves: {"ok":true,"url":"javascript:alert(1)"}
// spec requires: reject TypeError (disallowed URL scheme)
```

Both calls resolved cleanly with the registration's `scriptURL` echoing back whatever was passed, no exception, no rejection.

## Симптом

Any WPT test or real page asserting that `register()` rejects for a cross-origin or non-http(s) script URL fails silently (the promise resolves instead of rejecting) — this is the same "constructor/method performs no argument validation" defect class already filed for other APIs found by this backlog ([BUG-646](BUG-646-FIXED.md) `PaymentRequest`, [BUG-656](BUG-656-FIXED.md) `PresentationRequest`, [BUG-666](BUG-666-FIXED.md) `getDisplayMedia`, [BUG-667](BUG-667-FIXED.md) `getScreenDetails`), now confirmed for `ServiceWorkerContainer.register()` itself.

## Что НЕ является причиной

Not [BUG-674](BUG-674-FIXED.md) — that bug is about the origin *key* the registry stores state under being forgeable; this bug is about `register()` accepting a script URL it should reject regardless of which origin's table it lands in. Not the TLS gap — reproduced on a live `file://` page with no `.https.` navigation involved.

## Предлагаемый фикс

In `_sw_container.register`, before constructing the registration: reject (return a rejected `Promise`) with `TypeError` when `scriptUrl`'s scheme is not `http:`/`https:`, and with a `SecurityError`-shaped rejection when the script URL's origin differs from `_sw_origin` (once [BUG-674](BUG-674-FIXED.md)'s origin-binding fix lands, use the real bound origin for this comparison rather than the forgeable `_sw_origin` global as it exists today).

## Исправление (2026-09-26, P3)

`register()` теперь до создания любого состояния регистрации вызывает `_sw_check_register_urls` (`web_api_shim_mid_b.js`) по SW §3.4.3 + §9.2 Start Register + §9.6 Register:

1. URL скрипта разбирается относительно `document.baseURI` (раньше хранилась сырая строка, `'sw.js'` оставался относительным) — ошибка разбора → `TypeError`; `registration.scriptURL` теперь абсолютный.
2. Схема не `http:`/`https:` или в пути `%2f`/`%5c` (без учёта регистра) → `TypeError`. То же для явного `options.scope`.
3. Origin скрипта, затем scope ≠ `location.origin` документа → `DOMException` `SecurityError`. Порядок как в спеке: сначала обе проверки формы (Start Register), затем обе проверки origin (задача Register).

`file:`-страница больше не регистрирует SW (её `/sw.js` — `file:`-URL, `TypeError`), как в Chrome. Тест `sw_register_calls_lumen_primitive` переведён на `https://`, новые тесты — `v8_events_cache.rs` `sw_register_resolves_script_url_against_document`, `sw_register_rejects_*`.

**Остаток (не в этом баге):** ключ реестра — по-прежнему сырая строка `options.scope` (по умолчанию `'/'`), а не разобранный URL scope; умолчание по спеке — `new URL('./', scriptURL)`, а не корень origin; `getRegistration()` сравнивает префиксы сырых строк. Проверка MIME-типа скрипта (§9.4 Update step 7) не делается — скрипт по-прежнему тянется fire-and-forget `fetch()` после разрешения промиса.
