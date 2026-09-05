# BUG-509: External stylesheet fetch ignores the CSS "determine the fallback
encoding" algorithm — always decodes as UTF-8 lossy

**Статус:** FIXED 2026-09-05
**Дата:** 2026-08-02
**Компонент:** shell (`crates/shell/src/stylesheets.rs::fetch_stylesheet_text` —
moved out of `main.rs` by the SPLIT track after this report was filed)
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

## Фикс (2026-09-05, P3)

Implemented the full precedence chain as
`lumen_encoding::detect_stylesheet_encoding(bytes, http_content_type,
link_charset_attr, referring_encoding)` (`crates/engine/encoding/src/detect.rs`):
BOM → HTTP `Content-Type` charset → a literal `@charset "…";` at byte offset 0
(`sniff_leading_at_charset` — an exact ASCII prescan, not a tokenizer run,
since the real encoding isn't known yet; an `@charset` that parses and names
`utf-16`/`utf-16be` is forced to UTF-8, matching the spec's carve-out for a
label that couldn't have survived this prescan if true) → the `<link
charset>` attribute → the referring document's/stylesheet's own encoding →
UTF-8 default. A tier naming a label this crate doesn't decode
(`Encoding::from_label` → `None`) is skipped, same as an invalid/`bogus`
label — see "Остаток" below.

Wiring in `crates/shell/src/stylesheets.rs`:
* `fetch_stylesheet_text` now reads raw bytes (not `read_to_string`/lossy
  UTF-8), resolves the encoding, decodes with `lumen_encoding::decode`, and
  returns the resolved `Encoding` alongside the text/`ResourceBase` so a
  sheet's own `@import`s inherit it as their referring-encoding tier.
* `collect_link_hrefs`/`collect_stylesheet_owners` now also capture the
  `<link charset>` attribute.
* New `document_encoding(doc)` reads `Document::character_set()` — the
  bottom fallback tier for a document's own `<link>`s.
* `inline_css_imports` gained a `referring_encoding` parameter, threaded
  from `load_linked_stylesheets`/`build_stylesheet_node_registry`
  (top-level `<link>`s) and `frames.rs`/`page_pipeline.rs` (inline `<style>`
  imports, via the document's own encoding).

The HTTP `Content-Type` tier needed the header, which
`HttpClient::fetch_subresource` (`crates/network/src/lib.rs`) discarded —
added `fetch_subresource_with_content_type` (thin wrapper around a shared
`fetch_subresource_inner`, `fetch_subresource`'s existing behaviour and
signature are unchanged). Since the streaming preload scanner
(`page_load.rs`) and the final parse (`stylesheets.rs`) share one fetch via
`prefetch::PREFETCH_CACHE` — and whichever one loses that race never runs
its own closure — the header would be invisible on the losing side unless it
travels through the cache itself: `PrefetchCache`'s payload changed from a
bare `Vec<u8>` to `CachedResource { body, content_type }`, updated at all
three call sites (`page_load.rs` producer, `scripts.rs`/`stylesheets.rs`
consumers; scripts ignore the header, just needed the type change).

**Остаток (не в этом фиксе):** 4 of the 14 subtests need an actual
windows-1250/windows-1253 decode (their expected outcome is that label
winning the precedence chain) — `lumen-encoding` doesn't carry those tables,
a deliberate scope boundary (`docs/plan/tech-stack.md` §5,
`subsystems/encoding.md`), not a gap this fix should have closed. Verified
live via `tests/wpt/run_report.py --root css/css-syntax/charset`: 10 of the
14 committed-FAIL subtests now pass (`--update-expected` cleared their
`.ini`), the remaining 4 keep `expected: FAIL` with an updated comment
explaining the windows-1250/1253 boundary specifically (not the old "whole
algorithm missing" description). `--check` on the category is clean (0
regressions).

11 new unit tests in `lumen-encoding` (`detect_stylesheet_encoding`/
`sniff_leading_at_charset`), 3 new integration tests in
`crates/shell/src/tests/page_resources.rs`
(`load_linked_stylesheets_falls_back_to_document_encoding`,
`load_linked_stylesheets_uses_link_charset_attribute`,
`load_linked_stylesheets_bom_wins_over_document_encoding`). `cargo clippy -p
lumen-encoding -p lumen-network -p lumen-shell --all-targets -- -D warnings`
clean.
