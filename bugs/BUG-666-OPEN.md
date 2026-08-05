# BUG-666 — `getDisplayMedia()` never validates its constraints argument and never checks user activation

**Статус:** OPEN
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
arguments — [BUG-646](bugs/BUG-646-OPEN.md) (`PaymentRequest` constructor), [BUG-656](bugs/BUG-656-OPEN.md)
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
