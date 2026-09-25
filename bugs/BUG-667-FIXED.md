# BUG-667 — `navigator.getScreenDetails()` never checks permission state or user activation

**Статус:** FIXED 2026-09-17 (P3)
**Компонент:** js (`crates/js/src/window_management.rs:112`-`136`, `navigator.getScreenDetails` JS shim — Phase 0 Window Management stub)
**Найден:** P2, WPT-VENDOR-screen-details (2026-08-05), live `--mcp-live-port` probe (the WPT run itself gave zero signal — all 3 selected ids are `.https.` and TIMEOUT on the already-documented TLS gap `UnknownIssuer`, per `docs/wpt-status.md`'s `UnknownIssuer` class)

## Live run signal

```
tests: 0/3 harness OK; subtests: 0/0 passed
```

All 3 selected ids (`getScreenDetails.tentative.https.window.html`,
`isExtended.tentative.https.window.html`, `permission.https.window.html`) TIMEOUT before
reaching any JS — same TLS-handshake gap already tracked elsewhere (`network error: TLS
handshake: invalid peer certificate: UnknownIssuer`). Per the established convention
(`eyedropper`/`fedcm`/`screen-capture` precedent in `reference_wpt_run_report_invocation_recipe`),
a 🚫-scoped category (multi-monitor OS integration, out of Lumen's reader-engine scope) that
gives zero run signal is still worth a direct `--mcp-live-port` probe when its API is
actually implemented — `crates/js/src/window_management.rs` is a real Phase 0 shim (not a
missing stub), so this probe was run.

## Probe and result

Live probe against a page with a real `<script>` tag (about:blank has no JS context to eval
into), no prior click on anything, no `test_driver.set_permission` call available (Lumen's
Window Management permission is not wired to any real permission store — see below):

```js
navigator.permissions.query({name:'window-management'})   // → {state: "granted"} (not in the
                                                            //   hardcoded `_perm_denied` list,
                                                            //   so always 'granted' — BUG-386's
                                                            //   generic "no name validation"
                                                            //   defect, reconfirmed here, not a
                                                            //   new finding by itself)
navigator.getScreenDetails()                               // resolves immediately, screens.length === 1
```

Two independent checks the upstream test files
(`getScreenDetails.tentative.https.window.js`, vendored this session) require and the shim
performs neither of:

1. **`getScreenDetails() must reject with NotAllowedError when the `window-management`
   permission is denied`** — `window_management.rs`'s `navigator.getScreenDetails` function
   (line 112-136) never reads `navigator.permissions` at all; it goes straight to
   `_buildPhase0ScreenDetails()` (or the Phase 1 `_lumen_get_screen_details` native hook,
   equally unconditional) regardless of any permission state. Confirmed live: the promise
   resolves successfully even though the whole permission model behind it is inert — `query()`
   for `window-management` is not backed by any writable store, so there is no way to ever
   observe a `'denied'` state from JS at all, and even if there were, `getScreenDetails()`
   would ignore it.
2. **`getScreenDetails() must require transient user activation`** (W3C Multi-Screen Window
   Placement §3.2 step 2) — the promise resolves with zero prior user gesture (no
   `test_driver.click`, no synthesized click of any kind). Same defect class as
   [BUG-666](BUG-666-FIXED.md) (`getDisplayMedia` — user-activation gate + constraints
   validation both unchecked) and [BUG-646](BUG-646-FIXED.md)/[BUG-656](BUG-656-OPEN.md)
   (unchecked constructor arguments) — a recurring pattern across Phase 0/1 stubs of gesture-
   or permission-gated Web APIs: the JS shim implements the happy-path return shape but skips
   every precondition check the spec attaches to it.

## Что НЕ является причиной этого бага

- The 3-id WPT run's own TIMEOUT wall — pure TLS gap (`UnknownIssuer`, already tracked, not
  re-filed here), unrelated to the shim logic above; the probe above is the actual finding,
  independently reproduced outside the WPT harness.
- `screen.isExtended` always being `false` — this is the file's own documented Phase 0 design
  (single-screen stub, correctly reported as `false`, matching `isExtended.tentative.https.window.js`'s
  only non-permission assertion `typeof self.screen.isExtended === 'boolean'`), not a defect.
- `navigator.permissions.query({name:'window-management'})` always answering `'granted'` — this
  was the pre-existing, already-filed [BUG-386](BUG-386-FIXED.md) defect (no permission-name
  validation, and by extension no real per-permission state store at all); reconfirmed here as
  the same generic gap, not a `window-management`-specific bug on its own.
  **Stale since 2026-08-10** — BUG-386 is fixed and `window-management` now answers `'denied'`
  (no window placement exists, so that is the truthful answer). The probe transcript above
  records the old behaviour.

## Предлагаемый фикс

Both checks are small, localized additions to the top of `navigator.getScreenDetails` in
`window_management.rs` before building/resolving `ScreenDetails`: (1) reject with
`DOMException('...', 'NotAllowedError')` when the `window-management` permission state is not
`'granted'` — no longer blocked on BUG-386 as of 2026-08-10 (`window-management` is a
recognised name answering `'denied'`), but note the consequence: a literal gate makes
`getScreenDetails()` reject unconditionally until there is a way for the user to grant the
permission, so gate and grant path have to land together; (2) reject with
`DOMException('...', 'InvalidStateError')` when the calling context lacks transient user
activation, mirroring whatever activation-tracking primitive BUG-666's fix introduces for
`getDisplayMedia` — the two bugs share the same missing primitive and should likely land
together.

## Исправлено

Добавлены в `navigator.getScreenDetails` (`window_management.rs`), перед резолвом
`ScreenDetails` (и Phase-0 стабом, и Phase-1 нативным хуком `_lumen_get_screen_details` —
оба идут через один и тот же промис-чейн теперь), два синхронных геймплей-независимых
чека, тем же паттерном, что `media_devices.rs::getDisplayMedia` (BUG-666) и
`local_font_access.rs::queryLocalFonts`:

1. `requireTransientActivation()` — `navigator.userActivation.isActive === false` →
   `throw DOMException('...', 'InvalidStateError')` внутри `Promise.resolve().then(...)`
   (WebIDL: promise-возвращающая операция сообщает об отказе прекондиции через
   rejection, не через синхронный throw).
2. `requireWindowManagementPermission()` — `navigator.permissions.query({name:
   'window-management'})`; любое состояние, кроме `'granted'` (включая отсутствие
   `navigator.permissions` вовсе), → `DOMException('...', 'NotAllowedError')`. Тот же
   fail-closed паттерн, что `requireLocalFontsPermission`.

Порядок — сначала activation, затем permission, как в `local_font_access.rs`; в этом
движке оба чека почти всегда одновременно валят вызов (`window-management` статически
`denied` в таблице разрешений — реального per-permission стора всё ещё нет, см. BUG-386),
так как порядок здесь не наблюдаем ни одним завендоренным тестом.

Ревизия при фиксе: завендоренный `tests/wpt/screen-details/getScreenDetails.tentative.https.window.js`
не проверяет требование transient activation вовсе (WPT `test_driver.set_permission` не
эмулирует клик), только permission-гейт (`denied` → `NotAllowedError`, строки 21-26).
Activation-чек добавлен по прямому тексту спеки (W3C WMWPA §3.2 шаг 2) и по прецеденту
BUG-666/queryLocalFonts, но не покрыт живым WPT-сигналом — риск отмечен явно, а не скрыт.

Новые тесты в `crates/js/src/window_management.rs::tests`:
`get_screen_details_requires_transient_activation`,
`get_screen_details_requires_granted_permission`,
`get_screen_details_resolves_when_gates_pass` (happy-path регрессия — оба гейта открыты).
Существующий тестовый хелпер `with_window_management` расширен дефолтным
`navigator.userActivation`/`navigator.permissions` (оба "granted"), чтобы старые тесты
остались на happy-path, а не начали молча падать в `NotAllowedError`.

`cargo test -p lumen-js --lib window_management --features v8-backend` — 11/11,
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` — чист.
Изменение только в JS-шиме (raw-строке `WINDOW_MANAGEMENT_SHIM`), пиксели не затронуты —
`graphic_tests`/`scoped-test.sh` не требуются сверх точечного `cargo test`/`clippy`.
