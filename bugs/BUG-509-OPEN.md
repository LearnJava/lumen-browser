# BUG-509: External stylesheet fetch ignores the CSS "determine the fallback
encoding" algorithm — always decodes as UTF-8 lossy

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** shell (`crates/shell/src/main.rs::fetch_stylesheet_text`)
**Найден:** WPT-RUN-3 срез 18 (`ROADMAP.md`) — массовый прогон `css/css-syntax`

## Механизм

`fetch_stylesheet_text` (`crates/shell/src/main.rs:4771`, called from the
`<link rel=stylesheet>` load path at `main.rs:4743`) decodes the fetched
response body unconditionally with `String::from_utf8_lossy(&bytes[..])`
(`main.rs:4820`) before handing the text to `lumen_css_parser::parse`. There
is no implementation anywhere of the CSS Syntax "[determine the fallback
encoding](https://drafts.csswg.org/css-syntax-3/#determine-the-fallback-encoding)"
algorithm — no BOM sniffing (UTF-8/UTF-16LE/UTF-16BE), no `@charset "..."`
rule at the very start of the byte stream, no HTTP `Content-Type: text/css;
charset=...` header, no `<link charset=...>` attribute, no fallback to the
referring document's own encoding. `grep -rn "charset\|encoding"
crates/engine/css-parser/src/*.rs` returns zero hits outside comments/tests —
confirms this isn't a broken implementation of one precedence tier, the whole
mechanism is absent.

Any stylesheet whose bytes are not valid UTF-8 (or that relies on any
encoding-declaration channel above to override a UTF-8 default) silently
mis-decodes: multi-byte/legacy-8-bit bytes turn into U+FFFD or wrong
codepoints via the `_lossy` conversion, so selectors built from non-ASCII
identifiers never match their intended element and the rule silently fails
to apply — no error, no console warning, just a rule that never matches.

## Масштаб находки

`css/css-syntax/charset/` — 14 of 19 vendored files (the other 5 are
`*-ascii-only` variants where the test deliberately uses only ASCII bytes, so
the encoding choice is unobservable and they correctly pass regardless):

`page-windows-1251-css-at-charset-1250-charset-attribute-windows-1253.html`,
`page-utf16-css-bomless-utf16.html`, `page-windows-1251-css-at-charset-
bogus.html`, `page-windows-1251-css-at-charset-windows-1250-in-utf16.html`,
`page-windows-1251-css-at-charset-bogus-charset-attribute-windows-1250.html`,
`page-windows-1252-http-windows-1251-css-utf8-bom.html`,
`page-windows-1251-css-http-bogus.html`, `page-utf16-css-no-decl.html`,
`page-windows-1251-css-http-windows-1250-at-charset-windows-1253.html`,
`page-windows-1251-css-at-charset-windows-1250-in-utf16be.html`,
`page-windows-1251-charset-attribute-bogus.html`, `page-windows-1251-css-
utf8-bom.html`, `page-windows-1251-css-http-bogus-at-charset-windows-1250.html`,
`page-windows-1251-css-no-decl.html`. 1 subtest each, 14 subtests total.

Every file follows the same shape: an external `.css` file whose bytes only
make sense under one specific encoding (declared via some combination of BOM
/ `@charset` / HTTP header / `<link charset>` / referring-document fallback)
defines a rule `#<non-ascii-id> { visibility: hidden }`; the HTML page has a
matching element `id` written as the equivalent character reference. If the
CSS bytes decode under the wrong encoding, the id in the selector doesn't
match the byte-for-byte-different id in the DOM, so `visibility` stays at its
initial `visible` and every test's sole assertion
(`getComputedStyle(elm,'').visibility === 'hidden'`) fails with `"visible"`.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-syntax/charset/` for all
14 failing files (`expected: FAIL`).
