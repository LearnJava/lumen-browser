# BUG-607: `document.fgColor`/`bgColor`/`linkColor`/`vlinkColor`/`alinkColor` not implemented

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `document` object literal; `crates/js/src/shim/web_api_shim_tail_b.js` — `HTMLBodyElement.prototype` reflection table, new `string-null2empty` kind)
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

## Исправление

Two layers, matching the spec's own indirection (`Document.fgColor` etc.
reflect the *body element's* `text`/`bgColor`/`link`/`vLink`/`aLink`, not a
content attribute of their own):

- `HTMLBodyElement.prototype` gained the five own obsolete IDL attributes
  (`text`/`bgColor`/`link`/`vLink`/`aLink`) via `_lumen_install_reflection`,
  same declarative table as BUG-602's `align`. They needed a new reflection
  kind, `string-null2empty` (`[LegacyNullToEmptyString] DOMString`): a bare
  `null` write must become `''`, not the literal string `"null"` the
  existing plain `'string'` kind would produce (WPT `document-color-02.html`
  asserts exactly this after `document.fgColor = null`).
- `document.fgColor`/`bgColor`/`linkColor`/`vlinkColor`/`alinkColor` are five
  get/set pairs added to the `document` object literal, a thin forward onto
  `this.body`'s new properties — `''` on read and a no-op on write when
  `this.body` is `null` (no `<body>`, or the tree only has a `<frameset>`,
  since `_lumen_get_body` only ever matches a `<body>` tag).

Not full "limited to legacy color value" parsing/normalization —
`document-color-0{1,2,3,4}.html` never exercises an invalid or
non-normalized color string, only plain round-trips and the null/no-body
edge cases, so a normalizing implementation would be unverified scope.

New tests: `crates/js/src/dom/tests/v8_bug607_document_color_attrs.rs`
(4/4 green). `cargo test -p lumen-js --features v8-backend` green
(3735/3735), `cargo clippy --workspace --all-targets --features v8-backend
-- -D warnings` clean.
