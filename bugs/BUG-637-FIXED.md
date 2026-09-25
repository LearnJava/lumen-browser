# BUG-637: global `Window` interface object doesn't exist — `typeof Window === "undefined"`

**Статус:** FIXED 2026-09-25 (P3)
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM`, V8 global scope install)
**Найден:** P2, WPT-VENDOR-merchant-validation, 2026-08-05, проба `--mcp-live-port`

## Симптом

Confirmed live (`--mcp-live-port`, `eval`, retry-until-ready protocol):

```
typeof Window       = "undefined"
typeof Document      = "function"
typeof Element        = "function"
typeof Navigator     = "undefined"   (BUG-624, same class)
```

Per WebIDL §3.7 every `[Exposed=Window]` interface — including `Window`
itself — must appear as a named property on the global object holding its
interface object. Lumen exposes `Document`/`Element` (and other DOM classes)
this way, but never installs a `Window` binding at all, unlike `Document`/
`Element`, which do exist as global constructors.

Directly observed from a real WPT test: `tests/wpt/merchant-validation/
constructor.tentative.http.html` runs the common feature-detection idiom
`assert_false("MerchantValidationEvent" in Window)` and gets
`ReferenceError: Window is not defined` instead of a clean `false` — the
harness reports a `FAIL` with a JS exception rather than evaluating the
assertion, because the identifier itself doesn't resolve.

## Масштаб

Affects any WPT test using the standard `"X" in Window` / `"X" in self`
(when aliased) idl-presence idiom, or `window instanceof Window`,
`Object.getPrototypeOf(window) === Window.prototype`, or idlharness checks
against the `Window` interface itself — not specific to `merchant-validation`
(PaymentRequest, 🚫-scope); the underlying gap is engine-wide since `Window`
never exists as a binding regardless of which category's test references it.
Same class of missing-interface-object defect as [BUG-624](BUG-624-FIXED.md)
(`Navigator`) and [BUG-589](BUG-589-FIXED.md) (the `window` instance itself
isn't a proper WebIDL exotic object) — three symptoms of the same root cause:
the `window` global is installed as a plain object/`WindowProperties`-style
bag rather than backed by an actual `Window` interface prototype chain. Not
investigated: whether fixing this requires also wiring `window instanceof
Window` and `Window.prototype` to the existing `window` global, or whether a
minimal shim (bare `class Window {}` global with no prototype linkage) is
sufficient to unblock the common `"X" in Window` idiom used across the WPT
corpus.

## Исправление (P3, 2026-09-25)

К моменту разбора симптом уже не воспроизводился: `Window` заводит IIFE
BUG-589 в `crates/js/src/shim/web_api_shim_tail_b.js` (2026-09-16), так что
`"X" in Window` вычисляется. Но форма интерфейсного объекта была неверной:
`globalThis.Window = Window` делал его *перечисляемым* свойством глобала,
`Window.prototype` оставался перезаписываемым/конфигурируемым, а
`[[Prototype]]` самого `Window` был `Function.prototype`, а не `EventTarget`
(WebIDL §3.7.1: объект интерфейса наследует объект родительского интерфейса).
Теперь `Window` ставится через `defineProperty` (writable, не enumerable,
configurable), `prototype` — `{writable:false, enumerable:false,
configurable:false}`, `Object.setPrototypeOf(Window, EventTarget)`.

Регрессия — `crates/js/src/dom/tests/v8_bug637_window_interface.rs`
(3 теста: идиома WPT, форма по WebIDL, отсутствие `Window` в
`Object.keys(globalThis)`). Весь `cargo test -p lumen-js --features
v8-backend --lib`: 4343/4345, два упавших (`credentials::…installed_provider`,
`frame_bridge::…does_not_mark_dirty`) — флейки глобального состояния при
параллельном прогоне, изолированно зелёные и с фиксом, и без.
