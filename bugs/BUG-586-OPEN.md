# BUG-586: `document.domain` not implemented — getter is `undefined`, setter never validates or throws

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — grep for `"domain"` on the `document` object returns zero hits; no getter/setter installed)
**Найден:** P2, WPT-VENDOR-html-browsers, 2026-08-04

## Симптом

```
assert_equals: document.domain is a string expected "string" but got "undefined"
assert_throws_dom: function "() => { document.domain = document.domain }" did not throw
assert_throws_dom: function "() => { document.implementation.createHTMLDocument().domain = document.domain }" did not throw
```

Seen in `html/browsers/origin/inheritance/*` (document-domain-*.html family).

## Причина

`document.domain` (HTML LS `#relaxing-the-same-origin-restriction`) has no
backing property at all: reading it is `undefined` instead of the document's
serialized host, and assigning to it never validates the new value against
the document's origin, so cases that must throw a `SecurityError`
(re-setting the same domain via `document.domain = document.domain`,
setting `.domain` on `createHTMLDocument()`/`createDocument()` output, or on
a document with no browsing context) all fail silently instead — the
assignment is either a no-op or an unguarded property write that never
raises.

## Масштаб

Whole `document.domain` feature (getter + validating setter + the
document-domain-relaxation same-origin check it's supposed to unlock).
Confirmed only against same-origin-agent-cluster-agnostic assertions in
`html/browsers/origin/inheritance/`; the actual domain-relaxation behavior
(two subdomains becoming same-origin after both set `document.domain` to
the same suffix) was not separately probed and may hide further gaps once
the property exists at all.
