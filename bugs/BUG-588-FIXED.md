# BUG-588: `window.frameElement` missing entirely (always `undefined`, should be `null` at top level)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs`/`v8_runtime.rs` — grep for `frameElement` returns zero hits anywhere)
**Найден:** P2, WPT-VENDOR-html-browsers, 2026-08-04

## Симптом

```
assert_equals: The frameElement attribute should be null. expected (object) null but got (undefined) undefined
```

`html/browsers/windows/nested-browsing-contexts/frameElement.sub.html` — the
very first assertion, run on the top-level document itself (no iframe
involved yet): `window.frameElement` must be `null` when the window is not a
nested browsing context.

## Причина

`Window.prototype.frameElement` (HTML LS `#dom-window-frameelement`) is not
implemented as a property at all, so accessing it falls through to
`undefined` instead of a getter returning `null`/the container element. This
is independent of the already-documented "`<iframe>` without browsing
context" limitation — the failing assertion here never touches an iframe, it
only checks the top window's own `frameElement`, which is a same-origin-only
getter with a trivial `null` answer at the top level.

## Масштаб

Single missing getter. The rest of the same test file (checking
`frames[0].frameElement` from inside a nested browsing context) is expected
to additionally hit the iframe-without-browsing-context limitation once this
getter exists.
