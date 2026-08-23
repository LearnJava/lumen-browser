# WPT vendor notes — `xml`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-xml`, `docs/wpt-status.md`), scope ⬜ (confirmed candidate —
`DOMParser`/`XMLSerializer` are real implementations, `crates/js/src/dom_parser.rs`,
not stubs; the `xslt/` subdirectory is 🚫, see below).

Same pinned upstream commit `35be3b44`, `git sparse-checkout add xml` at
that commit, `LICENSE-WPT.md` copied from a sibling category — 37 files (36
upstream + license: 3 top-level `.html` + the `xslt/` subtree). Cheap
predictors across the board: 0 `name="variant"` hits, 0 `testdriver.js`
hits, 1 `.https.` file (`xslt/fetch/xslt.https.sub.html`). Out-of-category
deps (`/common/utils.js`, `/fetch/metadata/resources/helper.js`) were
already vendored by earlier categories. Confirmed cheap in practice: 56.79 s
wall-clock, single process.

### Run result

`run_report.py --all --root xml --recursive` (56.79 s, single process): 20
glob ids, 7 actually run by wptrunner (rest are `support`/reftest/manual —
`xml-doc-innerHTML-crash.html` is a crashtest, the `xslt/` subtree carries
several reftests `run_report.py`'s testharness-only glob doesn't select) —
**3/7 harness OK, 4/190 subtests passed**.

### Dominant finding: [BUG-781](../../bugs/BUG-781-FIXED.md)

`DOMParser.prototype.parseFromString` (`crates/js/src/dom_parser.rs:850-861`)
validates its `mimeType` argument against the 5 spec'd values but then always
calls `_vBuildDocument`, which always runs the HTML tokenizer
(`_vParseHTML`) regardless of MIME type. That tokenizer looks for a literal
`<html>` top-level element; any XML document — whose root is never called
`<html>` — falls into the "wrap bare content in a synthetic
`<html><head></head><body>...</body></html>`" branch
(`dom_parser.rs:546-558`). Net effect: `documentElement.tagName`/`.nodeName`
for *any* XML document parsed via `DOMParser` is always `"HTML"`, and the
real root element is buried two levels deep (`html > body > <real root>`).

Both non-XSLT executed tests hit this directly:

```js
new DOMParser().parseFromString('<?xml version="1.0"?>\n<a></a>', 'text/xml')
  .documentElement.tagName   // "HTML", expected "a"

new DOMParser().parseFromString('<a>\r\n\t<b>x</b></a>', 'text/xml')
  .documentElement.nodeName             // "HTML", expected "a"
  .documentElement.firstChild.nodeValue // null (that's <head>), expected "\n\t"
```

Side effect: 4 of `xml-prolog-accepted-versions.html`'s 10 subtests pass
**by accident** — the "version N.N is NOT accepted" assertions
(`assert_not_equals(...tagName, "x")`) are trivially true when `tagName` is
always `"HTML"`, never `"x"`. There is no XML-prolog version validation at
all; the green result doesn't mean what it claims (same class as
`feedback_probe_must_not_name_what_it_defines` / green-test-masks-defect).

A separate, independent code path — `document.implementation.createDocument`
(`dom.rs:2409-2429`, native `_lumen_create_element_ns`) — builds a real
arbitrary-named root element and does **not** have this defect (covered by
the `create_document_builds_xml_document` unit test, corrected during
BUG-367). The fix is either an XML-aware branch in `_vParseHTML`/
`_vBuildDocument` (skip the `<html>`-wrapping search, take the first
top-level element verbatim, preserve tag-name case — XML is case-sensitive,
`_vParseHTML` currently always lowercases) or delegating XML-MIME
`parseFromString` calls to the same native path `createDocument` already
uses.

### Everything else: `XSLTProcessor` — entirely absent, out of scope, no new bug

The 5 remaining executed ids (`xslt/document-element.window.html`,
`xslt/document-function.window.html`,
`xslt/functions.tentative.window.html` — 0/177 subtests,
`xslt/transformToFragment.tentative.window.html`, all `ReferenceError:
XSLTProcessor is not defined` or TIMEOUT) fail because Lumen has no XSLT
engine at all — `grep -r XSLTProcessor crates/` finds zero hits. XSLT is
not mentioned anywhere as planned scope (`CAPABILITIES.md`, `ROADMAP.md`,
`CSS-SPECS.md` are all silent on it) — a large, legacy, optional subsystem,
same treatment as `RTCIceTransport` being wholly absent in
`WPT-VENDOR-webrtc-ice`: no bug filed for the missing feature itself.

The 7th id, `xslt/fetch/xslt.https.sub.html`, fails `ERROR` — "navigate
reported success but the document was never replaced" — the already-open
BUG-438/BUG-657-class TLS-trust gap, not a new finding.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп ⬜ подтверждён для `DOMParser`/`XMLSerializer` (`crates/js/src/dom_parser.rs`,
не заглушка); `xslt/` — 🚫, XSLT нигде не запланирован. Вендорена целиком
2026-08-18 (коммит `35be3b44`, `tests/wpt/xml/`, 37 файлов, 20 id по глобу
/ 7 фактически исполненных wptrunner-ом, дёшево по всем предикторам: 0
variant, 0 testdriver, 1 `.https.`).

`run_report.py --all --root xml --recursive` — 56.79 с, **3/7 harness OK,
4/190 сабтестов**. Найден [BUG-781](../bugs/BUG-781-FIXED.md):
`DOMParser.parseFromString` игнорирует XML MIME-типы — всегда гоняет
HTML-токенизатор и заворачивает реальный корень в синтетический `<html>`,
поэтому `documentElement.tagName`/`.nodeName` для любого XML-документа
всегда `"HTML"`. 4 из 10 сабтестов `xml-prolog-accepted-versions.html`
проходят случайно (валидации версии XML-пролога нет вовсе).

Остальное — `XSLTProcessor` отсутствует целиком (не запланирован,
аналогично `RTCIceTransport` в `webrtc-ice`, новых багов не заведено) и
один уже задокументированный TLS-гэп класса BUG-438/BUG-657.
