# BUG-585: `Origin` WebIDL global (`Origin.from()`) not implemented at all

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — grep for `\bOrigin\b` outside comments returns zero hits; the only match is a code comment at `dom.rs:5370` about `MessageEvent.origin`, unrelated to the interface)
**Найден:** P2, WPT-VENDOR-html-browsers, 2026-08-04

## Симптом

```
FAIL Origin.from("https://site.example") is a tuple origin. - Origin is not defined
```

181 occurrences across every file in `html/browsers/origin/api/` (self-contained
category — the one dependency, `resources/serializations.js`, is vendored and
loads fine). Every `origin-from-*.any.js`/`.window.js` test in the directory
fails on the same `ReferenceError`.

## Причина

The `Origin` interface (`Origin.from(value)`, tuple vs. opaque origin,
serialization, comparison — HTML LS `#origin-2`) has no implementation
anywhere in the JS shim: no global constructor, no `.from()` static, no
`.opaque`/`.serialize()` members. Distinct from the many already-implemented
places that compute an origin internally (URL parsing, `postMessage`,
`document.domain`) — none of them expose the constructible `Origin` object
the tests instantiate.

## Масштаб

Whole feature, entirely within `html/browsers/origin/api/`. Knock-on: 25 of
the 181 failures are `origin-from-hyperlinkelementutils.window.js` cases that
also trip `Cannot set properties of undefined (setting 'baseVal')` on SVG
`<a href>`/`<a xlink:href>` — `SVGAnimatedString.baseVal` for the `href`
attribute is a second, smaller gap the tests would hit right after `Origin`
is fixed.
