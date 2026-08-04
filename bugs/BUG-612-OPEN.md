# BUG-612: legacy `longdesc` content attribute has no IDL reflection — `img.longdesc` is `undefined`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — IDL reflection table introduced by [BUG-383](BUG-383-FIXED.md), extended for `align` by [BUG-602](BUG-602-OPEN.md))
**Найден:** P2, WPT-VENDOR-html-longdesc, 2026-08-04

## Симптом

Live probe (`--mcp-live-port`, `.tmp/probe_longdesc.py`) against
`<img id="i" src="picture.png" longdesc="fail.html">`:

```json
{"ready": 1, "hasAttr": true, "getAttr": "fail.html", "idlType": "undefined"}
```

`img.hasAttribute('longdesc')`/`img.getAttribute('longdesc')` work (generic
attribute storage), but `typeof img.longdesc` is `"undefined"` — the IDL
property itself does not exist.

## Причина

`grep -rn "longdesc" crates/` returns nothing — same class of gap as
BUG-602 (`align`): the reflection table BUG-383 introduced for
`href`/`disabled`/`maxLength`/etc. has no row for `longdesc` on
`HTMLImageElement`/`HTMLIFrameElement`.

## Масштаб

The upstream `html-longdesc` WPT category (spec
https://www.w3.org/TR/html-longdesc/, superseded/removed from the WHATWG
HTML Living Standard in 2013) is 100% manual tests — every real test file
carries the `-manual` suffix and `run_report.py`'s own glob (which excludes
by that suffix) selects only support/target pages (`README.html`,
`fail.html`, `pass.html`, `fail-fragment.html`, `pass-fragment.html`,
`rebased/fail.html`), none of which wptrunner recognizes as a testharness
test (`Unable to find any tests at the path(s)`). So this category yields
**0/0 automatable ids**; this bug is the only signal, found via the
"probe even when the run yields nothing" pattern.

One reflection-table row (`{idl: "longdesc", attr: "longdesc", kind:
string}`) on `HTMLImageElement` (and `HTMLIFrameElement`/`HTMLFrameElement`
per spec) would close this the same way BUG-602 closes `align`. Low
priority: `longdesc` itself is an obsolete, removed-from-living-standard
feature with no current spec conformance requirement.
