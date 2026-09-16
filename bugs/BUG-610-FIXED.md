# BUG-610: `MessageEvent.userActivation` attribute missing

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid_b.js` — `MessageEvent` constructor)
**Найден:** P2, WPT-VENDOR-html-misc, 2026-08-04

## Симптом

```
FAIL MessageEventInit user activation not set - assert_equals: userActivation attribute expected (object) null but got (undefined) undefined
```
(`user-activation/message-event-init.tentative.html` — the sibling
subtest, "user activation set", coincidentally passes because both sides
of its `assert_equals(ev.userActivation, navigator.userActivation)` are
independently `undefined`, masking the gap on its own)

## Причина

The HTML LS `MessageEventInit` dictionary carries an optional
`userActivation` member (`UserActivation?`, default `null`), exposed as a
read-only `MessageEvent.prototype.userActivation` IDL attribute. Lumen's
`MessageEvent` constructor doesn't recognize the dictionary member at all
(silently ignored, per normal WebIDL unknown-member behavior) and there's
no getter on the prototype, so `ev.userActivation` is always `undefined`
instead of either the passed-in value or the spec default `null`.

## Масштаб

Self-contained, 1 file, 1/2 subtests (the constructor-with-explicit-value
case only accidentally reads as passing — see above).

## Исправление

`MessageEvent`'s object-literal constructor (`crates/js/src/shim/web_api_shim_mid_b.js`)
now reads `init.userActivation` and assigns it to `this.userActivation`,
defaulting to `null` when the dictionary member is absent — same pattern as
`origin`/`lastEventId`, just previously missing. New tests
`crates/js/src/dom/tests/v8_ws_sse.rs::message_event_user_activation_defaults_to_null`
and `::message_event_user_activation_reflects_init` (2/2 зелёных).
`cargo test -p lumen-js --features v8-backend` зелёный, `cargo clippy
--workspace --all-targets -- -D warnings` чист. `scripts/scoped-test.sh`
зелёный кроме постороннего дрейфа эталонов `cpu_snapshots_match_references`
(BUG-1008, та же сигнатура 7 файлов уже на `main`).
