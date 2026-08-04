# BUG-610: `MessageEvent.userActivation` attribute missing

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `MessageEvent`/`MessageEventInit` constructor path has no `userActivation` field)
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
