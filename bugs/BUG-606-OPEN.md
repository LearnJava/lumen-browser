# BUG-606: legacy no-op APIs missing entirely — `document.clear()`/`captureEvents()`/`releaseEvents()`, `window.captureEvents()`/`releaseEvents()`, `document.all` unusual behaviors, `document.applets`, `HTMLScriptElement.event`/`.htmlFor`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — none of these six symbols exist on `document`/`window`/`HTMLScriptElement.prototype`)
**Найден:** P2, WPT-VENDOR-html-misc, 2026-08-04

## Симптом

```
FAIL document.clear - document.clear is not a function
FAIL document.captureEvents - document.captureEvents is not a function
FAIL document.releaseEvents - document.releaseEvents is not a function
FAIL window.captureEvents - window.captureEvents is not a function
FAIL window.releaseEvents - window.releaseEvents is not a function
FAIL 'unusual behaviors' of document.all - assert_true: expected true got false
FAIL document.applets should return an empty collection. - assert_true: expected true got false
FAIL event and htmlFor IDL attributes of HTMLScriptElement - assert_equals: expected (string) "" but got (undefined) undefined
```
(`obsolete/requirements-for-implementations/other-elements-attributes-and-apis/{nothing,document-all,document-applets,script-IDL-event-htmlfor}.html`)

## Причина

HTML LS §obsolete requires several historical APIs to keep existing purely
for compatibility, even though they must do nothing (or something
deliberately "wrong" per spec):

- `document.clear()`, `document.captureEvents()`, `document.releaseEvents()`,
  `window.captureEvents()`, `window.releaseEvents()` must exist as callable
  no-op methods returning `undefined`. None of the five exist at all in
  Lumen's shim.
- `document.all` must exist with the documented "unusual behaviors": it's
  an `HTMLAllCollection` that is loosely-`==` to both `null` and
  `undefined`, `typeof document.all === "undefined"`, and it's falsy in
  boolean context — all of which requires a real `[[IsHTMLDDA]]` internal
  slot on the collection object. Lumen has neither `document.all` (falls
  through to plain `undefined`, so most assertions coincidentally pass by
  being genuinely `undefined`) nor the DDA-slot semantics for when it is
  eventually implemented.
- `document.applets` must return an (always-empty, in a spec-compliant
  implementation with no legacy Java-applet support) live `HTMLCollection`.
  Currently missing entirely (`assert_true(collection instanceof
  HTMLCollection)` fails).
- `HTMLScriptElement.event`/`.htmlFor` are legacy IDL attributes reflecting
  the `event`/`for` content attributes verbatim (string, no special
  parsing) — both missing, `script.event`/`script.htmlFor` are `undefined`
  instead of `""`.

## Масштаб

4 self-contained files, ~13 subtests, all under
`obsolete/requirements-for-implementations/other-elements-attributes-and-apis/`.
No other category in this corpus depends on these obsolete symbols.
