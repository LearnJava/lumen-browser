# BUG-515: `window.devicePixelRatio` undefined on the default (single-)window

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM` never sets
`devicePixelRatio` on the global; the only place that name is assigned is
`crates/js/src/window_management.rs`, the opt-in multi-window
`getScreenDetails()` API, which stamps `globalThis.devicePixelRatio = 1`
only after a screen-details session starts)
**Найден:** WPT-RUN-3 срез 21 (`ROADMAP.md`) — массовый прогон `css/css-device-adapt`

## Симптом

```
FAIL devicePixelRatio is unaffected by viewport's initial-scale
  assert_equals: devicePixelRatio should be 1.0 expected (number) 1 but got
  (undefined) undefined
```

`window.devicePixelRatio` (CSSOM View §`Window` extensions) is a standard,
always-available property in every browser, independent of any
multi-window/`getScreenDetails()` feature. In this engine it's `undefined`
on an ordinary page unless the separate `window_management.rs` code path
happens to have run first.

## Масштаб находки

1 file / 1 subtest, `viewport-should-not-affect-devicePixelRatio.html`.
Narrow in this slice's numbers, but the property is fundamental enough
(responsive-image `srcset`/`image-set()` density selection, canvas
HiDPI-aware drawing, any script gating behavior on pixel density) that real
pages relying on it unconditionally will see the same `undefined`.

## Что нужно

Set `globalThis.devicePixelRatio` in the base `WEB_API_SHIM` (or the
equivalent V8-side global-template setup) unconditionally at window/document
creation, independent of `window_management.rs`; a static `1` is a
spec-conformant floor (matches this engine's fixed, non-HiDPI-aware
rendering pipeline) until a real per-monitor DPI signal is threaded through.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-device-adapt/` for
`viewport-should-not-affect-devicePixelRatio.html`, `expected: FAIL`.
