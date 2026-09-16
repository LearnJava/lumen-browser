# BUG-615: `IdleDetector` (and sibling `EventTarget`-subclass shims) leak private `this._x` state as own enumerable JS properties

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/idle_detection.rs::IDLE_DETECTION_SHIM`, the pattern also appears in `bluetooth.rs`/`document_pip.rs`/`navigation_api.rs`/`serial.rs`/`webhid.rs`/`webusb.rs`/`webxr.rs`)
**Найден:** P2, WPT-VENDOR-idle-detection, 2026-08-04

## Симптом

`idle-detection`'s 12 selected WPT ids are all `.https.`-only and die on the
already-documented TLS gap (`UnknownIssuer`, self-signed test cert not in
Lumen's trust store — `docs/wpt-status.md:26`), so the run itself gives no
signal (0/12 harness OK). A live `--mcp-live-port` probe of the
`IdleDetector` class (which *is* implemented — `idle_detection.rs`, Phase 1)
found instead:

```js
var d = new IdleDetector();
d.start({ threshold: 60000 });
Object.keys(d)
// → ["_userState", "_screenState", "_started", "_threshold", "_timer"]
JSON.stringify(d)
// → {"_userState":"active","_screenState":"unlocked","_started":true,"_threshold":60000,"_timer":3}
```

Every field the spec models as an internal slot (`[[state]]`, the polling
timer, the threshold) is instead a plain `this._x = ...` assignment in the
constructor/`start()`, which JS makes an **own, enumerable, writable**
instance property by default. Consequences:

- `Object.keys(detector)` / `for...in` / `JSON.stringify(detector)` /
  `structuredClone(detector)` all expose Lumen's private implementation
  fields — no real browser's `IdleDetector` has any own enumerable
  properties at all (state is only reachable through the `userState`/
  `screenState` **prototype** getters, confirmed correctly non-enumerable
  here: `Object.getOwnPropertyDescriptor(IdleDetector.prototype,
  "userState")` → `{enumerable:false, configurable:true}` — only the
  instance-side duplication is wrong).
- The fields are externally **writable**: `detector._threshold = 1` or
  `detector._timer = null` silently corrupts the object's internal
  consistency from page script, with no guard — e.g. overwriting `_timer`
  leaks the real interval id and orphans the running `setInterval`,
  preventing `stop()` from ever clearing it.
- Any WPT idlharness-style test that enumerates own properties of a
  platform object (a common WebIDL-conformance check pattern, e.g.
  `assert_equals(Object.getOwnPropertyNames(obj).length, 0)`) would fail
  here the moment the TLS gap is fixed and `idlharness.https.window.html`
  can actually run — not confirmed against a live idlharness run only
  because the run never gets that far, but the general Web IDL
  requirement is unambiguous: an interface's own instance MUST NOT
  acquire arbitrary enumerable properties beyond what its IDL declares.

## Причина

The `IDLE_DETECTION_SHIM` JS string (`idle_detection.rs`) implements
`IdleDetector` as a hand-written ES class using `this._userState`-style
fields for private state, instead of a closure-captured variable or a
module-private `WeakMap<this, State>` — the common pattern for hiding
state from the public instance in this codebase's JS shims. The same
`this._x` pattern recurs in every other native-shim `class ... extends
EventTarget` implementation grepped (`bluetooth.rs`, `document_pip.rs`,
`navigation_api.rs`, `serial.rs`, `webhid.rs`, `webusb.rs`, `webxr.rs`) —
this bug's fix is scoped to `IdleDetector` (the category actually being
vendored), but the same defect class likely applies to all of them.

## Масштаб

Confirmed via live `--mcp-live-port` probe (`Object.keys`/`JSON.stringify`
on a real `IdleDetector` instance after `start()`), not via the WPT run
itself — all 12 selected ids in this category TIMEOUT/ERROR on the TLS gap
before reaching any assertion. Not a new instance of the TLS gap; a
distinct, previously-unfiled defect surfaced only because the "probe the
class one level deeper than the failing run" step was applied. Fix for
`IdleDetector` alone is small (rewrite the 5 `this._x` fields as closure
variables inside the constructor, exposed only through the existing
getters); auditing the other 7 files for the same pattern is a separate,
broader follow-up.

## Исправление

The 5 `this._x` fields (`_userState`/`_screenState`/`_started`/`_threshold`/
`_timer`) moved out of the `IdleDetector` instance into a module-private
`WeakMap<IdleDetector, State>` (`idleState` in `IDLE_DETECTION_SHIM`,
`crates/js/src/idle_detection.rs`) keyed by `this`, closing over that map
from `start()`/`stop()`/the getters instead of touching `this` directly.
`Object.keys`/`JSON.stringify`/`for...in` now see zero own enumerable
properties on an `IdleDetector` instance, and the fields are no longer
externally writable (`detector._threshold = 1` is now a no-op own-property
assignment that never reaches the real state).

Test coverage: new `idle_detection::tests::instance_has_no_own_enumerable_properties`
asserts `Object.keys(d)` is empty after `start()`. Three existing tests
(`poll_interval_is_half_threshold`, `poll_interval_minimum_is_30s`,
`stop_clears_interval_timer`) inspected the timer id via the now-removed
`d._timer` — updated to read the mock `__timers` array directly instead,
which is what they actually needed.

Scope: fixed only for `IdleDetector`, as filed. The same `this._x` pattern
in `bluetooth.rs`/`document_pip.rs`/`navigation_api.rs`/`serial.rs`/
`webhid.rs`/`webusb.rs`/`webxr.rs` is unaudited and unfixed — a separate
follow-up, not filed as its own bug yet.

`cargo test -p lumen-js --features v8-backend idle_detection` — 18/18
green. `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` clean.
