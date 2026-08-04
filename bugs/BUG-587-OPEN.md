# BUG-587: WindowProxy `[[DefineOwnProperty]]` does not enforce unforgeable own properties

**Статус:** OPEN
**Компонент:** js/V8 host-object layer (WindowProxy own-property trap; no `[[DefineOwnProperty]]` override guarding `window`/`document`/`location`/`top` found in `crates/js/src/v8_runtime.rs`)
**Найден:** P2, WPT-VENDOR-html-browsers, 2026-08-04

## Симптом

```
FAIL [[DefineOwnProperty]] success: "window" - assert_true: [[Get]]: unchanged expected true got false
FAIL [[DefineOwnProperty]] failure: "window" - assert_false: [[Value]], [[Enumerable]]: true expected false got true
```

Same pair of failures for `"window"`, `"document"`, `"location"`, `"top"` — 8
failures total, from
`html/browsers/the-windowproxy-exotic-object/windowproxy-define-own-property-unforgeable-same-origin.html`.

## Причина

Per the WindowProxy `[[DefineOwnProperty]]` algorithm (HTML LS
`#windowproxy-defineownproperty`), same-origin own accessor properties like
`window`, `document`, `location`, `top` are unforgeable: a "compatible"
redefinition must leave the underlying `[[Get]]` behavior unchanged, and an
"incompatible" one (e.g. flipping `enumerable`/turning it into a data
property) must be rejected outright rather than silently applied. Lumen's
WindowProxy accepts both kinds of redefinition as if the property were an
ordinary configurable own property — no unforgeable-property check exists on
the define-own-property path.

## Масштаб

Narrow, self-contained test file (8/8 assertions failing, no iframe or
cross-origin dependency — same-origin only). Adjacent files in the same
directory (`windowproxy-prototype-setting-cross-origin*.sub.html`,
`windowproxy-prevent-extensions.html`) fail separately on the already-known
"`<iframe>` without browsing context" limitation, not this defect.
