# BUG-611: `HTMLLinkElement.relList` not implemented

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `HTMLLinkElement.prototype` has `rel`/`rev` reflection but no `relList` `DOMTokenList` accessor; distinct from BUG-601, which is about the global `DOMTokenList` constructor not being exposed — here the `relList` property itself is absent, independent of that gap)
**Найден:** P2, WPT-VENDOR-html-misc, 2026-08-04

## Симптом

```
FAIL link element supports a rel value of "manifest". - Cannot read properties of undefined (reading 'supports')
```
(`links/manifest/link-relationship/link-rel-manifest.html`:
`document.createElement("link").relList.supports("manifest")` throws
because `relList` itself is `undefined`)

## Причина

HTML LS defines `HTMLLinkElement.relList` (and the equivalent on
`HTMLAnchorElement`/`HTMLAreaElement`/`HTMLFormElement`) as a live
`DOMTokenList` reflecting the space-separated `rel` content attribute, with
a `.supports(token)` method that validates a token against the element's
list of supported link types (`"manifest"` being one, per the Web App
Manifest spec's registration into that list). Lumen has `link.rel`/
`link.rev` as plain string reflection but no `relList` accessor at all —
`link.relList` is `undefined`, so `.supports(...)` throws a `TypeError`
before the actual "is manifest a supported rel value" check can even run.

## Масштаб

1 file, 1 subtest confirmed here (`<link>`). Likely the same gap on
`<a>`/`<area>`/`<form>` `relList`, not checked in this slice.
