# BUG-608: `fetchPriority` IDL attribute missing on `<script>`/`<img>` (likely `<link>`/`<iframe>` too)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — IDL reflection table from BUG-383; `fetchpriority`/`fetchPriority` absent from the enum-reflection entries for `HTMLScriptElement`/`HTMLImageElement`)
**Найден:** P2, WPT-VENDOR-html-misc, 2026-08-04

## Симптом

```
FAIL default fetchpriority attribute on <script> elements should be 'auto' - assert_equals: expected (string) "auto" but got (undefined) undefined
FAIL fetchPriority of new Image() is 'auto' - assert_equals: expected (string) "auto" but got (undefined) undefined
```
(`scripting/the-script-element/attr-script-fetchpriority.html`,
`embedded-content/the-img-element/attr-img-fetchpriority.html` — the
first subtest in each file is additionally masked by
[BUG-384](BUG-384-FIXED.md), named access on `window`, since it references
elements by bare `id`-derived identifiers; the second subtest constructs
the element directly and fails independently of that gap)

## Причина

The `fetchpriority` content attribute (Fetch Priority spec, referenced
from HTML LS on `<script>`/`<img>`/`<link>`/`<iframe>`) is a limited
enumerated reflection (`"high"`/`"low"`/invalid-or-missing → `"auto"`).
Lumen's element IDL reflection table (built for BUG-383) has no
`fetchPriority` entry for either interface, so the property is simply
absent — `script.fetchPriority`/`img.fetchPriority`/`new Image().fetchPriority`
are all `undefined` instead of reflecting the attribute (or defaulting to
`"auto"`).

## Масштаб

Confirmed on `<script>` and `<img>` (2 files, 2 subtests independent of
BUG-384). Not checked here, but likely the same gap on `<link>` and
`<iframe>`, which also carry `fetchpriority` per spec — not vendored/tested
in this slice, so unconfirmed.
