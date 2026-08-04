# BUG-559: `window.screen.width`/`height` (and siblings) are silently writable, `window.screenLeft`/`screenTop`/`screenX`/`screenY` are missing entirely

**Статус:** OPEN
**Дата:** 2026-08-04
**Компонент:** js (`crates/js/src/navigator_bindings.rs:213-225` — the `_screen` object literal and its `Object.defineProperty(globalThis, 'screen', …)`)
**Найден:** P2, WPT-RUN-3 срез 39 (`css/cssom-view`), 2026-08-04

## Симптом

```
FAIL immutability test - assert_equals: window.screen.width should be
  immutable expected 1920 but got 0
FAIL immutability test - assert_equals: window.screen.height should be
  immutable expected 1080 but got 0
FAIL screenLeft - assert_equals: screenLeft type expected "number" but got
  "undefined"
FAIL screenTop - assert_equals: screenTop type expected "number" but got
  "undefined"
```

`window-screen-width-immutable.html`/`-height-immutable.html`: read
`window.screen.width` (real configured value, e.g. 1920), assign
`window.screen.width = 0`, re-read — expected unchanged, got `0` (the
assignment stuck). `screenLeftTop.html`: `window.screenLeft`/`screenTop`
are `undefined` rather than numbers.

## Причина

Two separate gaps in `navigator_bindings.rs`:

1. **`screen` object properties aren't readonly.** The binding does:
   ```js
   var _screen = { width: {screen_width}, height: {screen_height}, … };
   Object.defineProperty(globalThis, 'screen', {
     value: _screen, writable: false, configurable: true, enumerable: true
   });
   ```
   `writable: false` only protects the `globalThis.screen` **binding**
   (reassigning `window.screen = somethingElse` would fail) — it says
   nothing about `_screen`'s own properties, which are ordinary object-
   literal data properties (default `writable: true`). `window.screen.width
   = 0` therefore mutates `_screen.width` directly, no error, no-op
   protection. CSSOM View §4.1 requires every `Screen` attribute
   (`width`/`height`/`availWidth`/`availHeight`/`colorDepth`/`pixelDepth`) to
   be `readonly` — needs either `Object.freeze(_screen)` or converting each
   field to a non-writable property descriptor / getter.

2. **`window.screenLeft`/`screenTop`/`screenX`/`screenY` don't exist at
   all.** `grep -n "screenLeft\|screenTop\|globalThis.screenX"
   crates/js/src/*.rs crates/shell/src/*.rs` finds nothing — these are
   CSSOM View §5 `Window` attributes (position of the browser window on the
   physical screen; `screenLeft`/`screenTop` are legacy aliases of
   `screenX`/`screenY`), unrelated to the `Screen` interface's own
   `width`/`height` and not covered by the `_screen` object above at all.

## Масштаб находки

4 subtests this slice (`window-screen-width-immutable.html`,
`-height-immutable.html`, `screenLeftTop.html` ×2). Low subtest count but a
basic, easily fixed spec-conformance gap.

## Что нужно

1. `Object.freeze(_screen)` (or give each field a `writable: false`
   descriptor) in `navigator_bindings.rs` before the `globalThis.screen`
   `defineProperty` call.
2. Add `window.screenLeft`/`screenTop`/`screenX`/`screenY` — plausibly `0`
   or the actual OS window position if available through the windowing
   backend; `screenLeft`/`screenTop` must equal `screenX`/`screenY`
   respectively.
