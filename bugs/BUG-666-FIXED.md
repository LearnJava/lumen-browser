# BUG-666 — `getDisplayMedia()` never validates its constraints argument and never checks user activation

**Статус:** FIXED 2026-09-17 (P3)
**Компонент:** js (`crates/js/src/media_devices.rs:326`-`386`, the `getDisplayMedia` JS shim method — Phase 1 PH3-17 Screen Capture stub)
**Найден:** P2, WPT-VENDOR-screen-capture (2026-08-05), live `--mcp-live-port` probe (the WPT run itself gave zero signal — all 15 selected ids are `.https.` and TIMEOUT on the already-documented TLS gap `UnknownIssuer`, per `docs/wpt-status.md`'s `UnknownIssuer` class)

## Live run signal

```
tests: 0/15 harness OK; subtests: 0/0 passed
```

All 15 selected ids TIMEOUT before reaching any JS — same TLS-handshake gap already tracked
elsewhere (`network error: TLS handshake: invalid peer certificate: UnknownIssuer`). Per the
established convention (`eyedropper`/`fedcm` precedent in
`reference_wpt_run_report_invocation_recipe`), a 🚫-scoped category that gives zero run
signal is still worth a direct `--dump-layout`/`--mcp-live-port` probe when its API is
actually implemented — `crates/js/src/screen_capture.rs` + the shim method below are a real,
non-stub implementation (Phase 1, not a Phase 0 placeholder), so this probe was run.

## Probe and result

`navigator.mediaDevices.getDisplayMedia` is a real, present function
(`typeof === 'function'`). Four calls, no prior click on any element:

```js
navigator.mediaDevices.getDisplayMedia()                            // no args
navigator.mediaDevices.getDisplayMedia({})                          // empty options
navigator.mediaDevices.getDisplayMedia({video: false, audio: false})
navigator.mediaDevices.getDisplayMedia({video: true})                // never clicked anything first
```

All four **resolve** with a live `MediaStream`. Per the upstream test file itself
(`tests/wpt/screen-capture/getdisplaymedia.https.html`, vendored this session), the spec
requires two independent checks the shim performs neither of:

1. **`getDisplayMedia() must require user activation`** — the returned promise must already
   be rejected with `InvalidStateError` if called without the calling script having transient
   activation (a real user gesture, e.g. `test_driver.click(button)` in the upstream test).
   Lumen's shim (`media_devices.rs:326`) never reads any activation state at all — it goes
   straight to `__lumen_screen_capture_start('')`.
2. **`getDisplayMedia(constraints) must fail with TypeError`** for `{video: false}` and every
   constraints object whose `video` member is not truthy (`{}`, no argument, `{video: false,
   audio: false}`, plus a battery of malformed `video`-constraint-dictionary shapes the
   upstream test also expects to reject with `TypeError`) — Screen Capture API §4.1 step 3.
   Lumen's shim never inspects its `options` parameter at all (the parameter is unused in the
   whole function body except being ignored); it unconditionally calls
   `__lumen_screen_capture_start('')` and resolves.

Same defect class already filed for other Phase 0/1 stubs with unchecked constructor/method
arguments — [BUG-646](BUG-646-FIXED.md) (`PaymentRequest` constructor), [BUG-656](BUG-656-FIXED.md)
(`PresentationRequest` constructor).

## Что НЕ является причиной этого бага

- The 15-id WPT run's own TIMEOUT wall — pure TLS gap (`UnknownIssuer`, already tracked, not
  re-filed here), unrelated to the shim logic above; the probe above is the actual finding,
  independently reproduced outside the WPT harness.
- The complete absence of a picker/consent UI (the shim silently grants access to the OS
  screen-capture provider with no user-facing dialog at all) — this is the file's own
  documented Phase 1 design ("resolves with a live MediaStream when ScreenCaptureProvider is
  installed... rejects when no provider is registered or the provider denies access") and a
  separate, larger scope question (privacy/consent model), not a narrow argument-validation
  defect like the two above.

## Предлагаемый фикс

Both checks are small, localized additions to the top of `getDisplayMedia` in
`media_devices.rs` before the `__lumen_screen_capture_start` call: (1) reject with
`InvalidStateError` when the calling context lacks transient user activation (needs a
user-activation tracking primitive shared with other gesture-gated APIs, if one does not
already exist in the shim); (2) reject with `TypeError` when `options` is missing, or its
`video` member is `false`/absent while `audio` is also `false`/absent, mirroring the
`constraints.video`/`constraints.audio` truthiness check the spec requires. The
per-constraint-shape `video: {advanced: [...]}` / `width.exact` / etc. TypeError cases from
the same test file are a further, separable layer of `MediaTrackConstraints` shape validation
— not required to close the two checks above, but worth a follow-up pass once real
`applyConstraints()`/constraint enforcement exists (currently `getSettings()` returns fixed
capture dimensions regardless of any `video` constraints passed in).

## Исправлено

Ревизия при фиксе: пункт 2 выше был сформулирован неверно относительно реального
поведения спецификации. Сам же завендоренный `tests/wpt/screen-capture/getdisplaymedia.https.html`
(строки 38-51) требует, чтобы `{}`/без аргумента/`{audio:false}` **успешно резолвились**
с видео-треком — отсутствующий/`undefined` `video` по спеке по умолчанию `true`, а не
повод для `TypeError`. `TypeError` спека требует только для явно ложного `video`
(`{video:false}`, строка 53) и для отдельного слоя валидации формы
`MediaTrackConstraints` (`advanced`/`width`/`height`/`frameRate` — строки 54-61),
который эта заявка сама пометила как отдельный, не обязательный для закрытия слой.

Добавлены в `getDisplayMedia` (`media_devices.rs:326`), синхронно, до вызова
`__lumen_screen_capture_start`:
1. Проверка transient activation через `navigator.userActivation.isActive` — тот же
   источник и паттерн, что `filesystem_access.rs::requireUserActivation`/
   `local_font_access.rs::requireTransientActivation`. Отклоняет с `InvalidStateError`
   через `Promise.reject(...)` напрямую (не внутри `.then()`), иначе промис не будет
   уже отклонённым к моменту гонки с `Promise.race([p, Promise.resolve()])` в тесте.
2. Проверка `options.video === false` → `Promise.reject(new TypeError(...))`.
   Отсутствующий/`undefined`/непустой `video` не отклоняется.

Более глубокая валидация формы `MediaTrackConstraints` (advanced/width/height/frameRate)
осталась вне скоупа этого фикса — как и отмечено в исходной заявке, это отдельный слой,
требующий реальной модели ограничений (сейчас `getSettings()` всегда возвращает
фиксированные размеры захвата).

Новые тесты в `crates/js/src/media_devices.rs::tests`:
`get_display_media_requires_transient_activation`,
`get_display_media_rejects_explicit_false_video`,
`get_display_media_defaults_video_when_unspecified` (проверяет, что `{}`/без
аргумента/`{audio:false}` не дают `TypeError`, а идут дальше до `NotAllowedError`
из-за отсутствующего провайдера захвата в тестовом окружении).

`cargo test -p lumen-js --lib media_devices --features v8-backend` — 27/27,
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` — чист.
Изменение только в JS-шиме (raw-строке `MEDIA_DEVICES_SHIM` в `media_devices.rs`),
пиксели не затронуты — `graphic_tests`/`scoped-test.sh` не требуются сверх точечного
`cargo test`/`clippy`.
