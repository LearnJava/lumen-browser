# BUG-607: `document.fgColor`/`bgColor`/`linkColor`/`vlinkColor`/`alinkColor` not implemented

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — no reflection entries for any of the five; would reflect onto `<body>`'s `text`/`bgcolor`/`link`/`vlink`/`alink` content attributes per the HTML LS §obsolete algorithm)
**Найден:** P2, WPT-VENDOR-html-misc, 2026-08-04

## Симптом

```
FAIL document: fg/bg/link/vlink/alink-color 1 - assert_equals: expected (string) "blue" but got (undefined) undefined
FAIL document: fg/bg/link/vlink/alink-color 1 - assert_equals: expected (string) "" but got (object) null
```
(`obsolete/requirements-for-implementations/other-elements-attributes-and-apis/document-color-0{1,2,3,4}.html`, 20 subtests total across the four files)

## Причина

HTML LS §obsolete defines five legacy `Document` IDL attributes —
`fgColor`, `linkColor`, `alinkColor`, `vlinkColor`, `bgColor` — that
transparently reflect onto the `<body>` element's `text`/`link`/`alink`/
`vlink`/`bgcolor` content attributes (limited-to-only-known-colors
reflection, same family as `align`/legacy presentational attributes).
Lumen's `document` shim has none of the five; reading any of them returns
plain `undefined` and writing has no effect on `<body>`. `document-color-01`
additionally checks the "no body" and "body is a frameset" edge cases,
which fail with `TypeError`s (`Cannot read properties of null`) rather than
the expected empty-string fallback, confirming there's no accessor at all,
not just a broken one.

## Масштаб

4 self-contained files under
`obsolete/requirements-for-implementations/other-elements-attributes-and-apis/`,
~20 subtests. No other category in this corpus touches these five
properties.
