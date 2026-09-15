# BUG-588: `window.frameElement` missing entirely (always `undefined`, should be `null` at top level)

**Статус:** FIXED
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js`)
**Найден:** P2, WPT-VENDOR-html-browsers, 2026-08-04
**Исправлено:** P3, 2026-09-15

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

## Исправление

`_lumen_frame_install_hierarchy` (`crates/js/src/frame_bridge.rs`) installs
the real `frameElement`/`parent`/`top`/`name` getters, but it is only invoked
from `V8JsRuntime::register_parent_document`/`register_top_document` — i.e.
only for a JS context that turns out to itself be an embedded frame. A
top-level page that is never involved in any frame relationship never reaches
that call, so `window.frameElement` stayed a plain missing property
(`undefined`) instead of `null`.

Fix: added `window.frameElement = null;` to the unconditional window
bootstrap in `crates/js/src/shim/web_api_shim_tail_b.js`, right next to the
existing `window.parent = window;`/`window.frames = window;` defaults. Being
a plain assignment it is configurable, so `installHierarchyAccessors`'s later
`Object.defineProperty(window, 'frameElement', ...)` for a real child frame
still overrides it.

Regression test:
`crates/js/src/dom/tests/v8_core/mod.rs::frame_element_is_null_at_top_level`.
