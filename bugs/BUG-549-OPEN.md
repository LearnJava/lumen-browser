# BUG-549: 7 Phase-0 stub Web APIs are entirely undefined under the default V8 build (were present-but-rejecting under QuickJS)

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js — `crates/js/src/{contacts,background_sync,periodic_sync,push_api,background_fetch,payment_request,media_stream_recording}.rs`
**Найден:** P1, S12b-G0 триаж (13 модулей без V8-порта)

## Механизм

Seven modules install a JS-visible constructor/property whose entire body is
a Phase-0 stub (constructs successfully, then every operation rejects with
`NotSupportedError` or is a fixed-value no-op) — none has a native backend
to port, only a JS shim. All seven are called exclusively from
`QuickJsRuntime::install_dom`; none has a `_v8` variant or any call from
`v8_runtime.rs`:

| Модуль | Install site (`lib.rs`) | Что отсутствует под V8 |
|---|---|---|
| `contacts.rs` | :937 | `navigator.contacts` (Contact Picker API stub — `select()`/`getProperties()` both reject) |
| `background_sync.rs` | :995 | `ServiceWorkerRegistration.prototype.sync` (`register(tag)`/`getTags()`) |
| `periodic_sync.rs` | :1002 | `ServiceWorkerRegistration.prototype.periodicSync` (`register(tag, {minInterval})`/`unregister()`/`getTags()`) |
| `push_api.rs` | :1009 | `ServiceWorkerRegistration.prototype.pushManager` (`subscribe()`/`getSubscription()`/`permissionState()`) |
| `background_fetch.rs` | :988 | `ServiceWorkerRegistration.prototype.backgroundFetch` (`fetch()`/`get()`/`getIds()`) |
| `payment_request.rs` | :944 | `window.PaymentRequest` constructor (`.show()`/`.canMakePayment()`/`.abort()` all reject) |
| `media_stream_recording.rs` | :1199 | `window.MediaRecorder` constructor + `BlobEvent` |

`ServiceWorkerRegistration.prototype` itself (bare constructor, defined in
`content_index.rs`, V8-ported) exists and real registrations are created —
only these four sub-APIs attached to it are missing.

## Симптом

None of these are claimed in `CAPABILITIES.md` (no doc overclaim to fix),
but all seven change observable behavior for page scripts that
feature-detect: under QuickJS, `'PaymentRequest' in window` /
`'MediaRecorder' in window` / `'sync' in ServiceWorkerRegistration.prototype`
etc. are `true` (the API exists, then rejects when called) — under V8 they
are `false` (the API doesn't exist at all). Sites that gate a UI affordance
on presence-detection (e.g. "show Apple/Google Pay button only if
`window.PaymentRequest` exists") silently hide that affordance instead of
offering it and getting a rejection. Lowest individual severity of the
S12b-G findings since none of the seven ever did real work even under
QuickJS, but the surface area (7 distinct globals) is the largest.

## Фикс (не сделан)

Port per the standard S12b-G group procedure
(`docs/tasks/p1-s12b-cleanup-queue.md` §4, slots S12b-G1/G3/G4): each is a
pure `ctx.eval(SHIM)` with no native bindings — replace
`rquickjs::Ctx::eval` with `lumen_core::ext::JsRuntime::eval` per module and
register via `install_v8!`, per the doc's "Модули без нативов" fast path.
Combined budget ≈1600 lines / 40 tests across all seven — fits comfortably
in the existing G1/G3/G4 batch pairs already planned.
