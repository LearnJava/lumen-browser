# BUG-358 — the live global `document` exposes none of the document-metadata IDL attributes (`characterSet`/`charset`/`inputEncoding`/`compatMode`/`contentType`/`URL`/`documentURI`/`location`)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:6989+` — the hand-written `var document = {…}` object literal that backs the live global document; contrast `crates/js/src/dom.rs:4728-4769` `_lumen_build_detached_document`, which *does* define them)
**Найден:** P2, WPT-VENDOR-encoding-detection (2026-07-27), `run_report.py --all --root encoding-detection --recursive`

## Симптом

On any real page, every document-metadata attribute reads `undefined`. Confirmed
outside WPT with a `--dump-layout` probe (`.tmp/probe_charset.html`, a plain page
carrying `<meta charset="windows-1251">`):

```
characterSet=undefined | charset=undefined | inputEncoding=undefined |
compatMode=undefined  | contentType=undefined | URL=undefined | documentURI=undefined
```

A second probe adds `document.location` → `undefined`, and confirms the property
is not merely a broken getter but genuinely absent:

```
LIVE has own characterSet? false
```

`window.location` itself works (`location.href=file://.tmp/probe_charset2.html`) —
it is only the `document.location` alias that is missing.

The same probe shows the asymmetry against a non-live document
(`new DOMParser().parseFromString(...)`): there `contentType`/`URL` do resolve
(`text/html` / `about:blank`), while `characterSet`/`compatMode` are still
`undefined`.

## Причина

There are two separate document implementations in the shim and they were never
reconciled:

* `_lumen_build_detached_document(proto, contentType)` (`dom.rs:4728`) — used by
  `DOMImplementation.createHTMLDocument` (`dom.rs:4849`) and
  `createDocument`/`createXMLDocument` (`dom.rs:4830`, `dom.rs:4526`) — installs
  the full set at `dom.rs:4762-4769`: `URL`, `documentURI`, `compatMode`,
  `characterSet`, `charset`, `inputEncoding`, `contentType`, `location`. (All
  hardcoded: `'about:blank'` / `'CSS1Compat'` / `'UTF-8'`.)
* The **live** `document`, a separate hand-written object literal starting at
  `dom.rs:6989`, defines none of them. `grep -n "characterSet" crates/js/src/`
  returns exactly one hit in the whole workspace — line 4766, inside the detached
  builder.

So the shim already agreed on what these accessors should look like; the live
document just never got them.

A second, deeper layer for `characterSet` specifically: even wired up, the value
would have to come from somewhere. The engine *does* detect a document encoding —
`lumen-encoding`'s `detect()` (`crates/engine/encoding/src/detect.rs:16`) runs
BOM → `<meta charset>` prescan → `Content-Type` → UTF-8 → a Cyrillic frequency
heuristic — but the result is never surfaced to JS. It would also need widening:
`Encoding` (`crates/engine/encoding/src/lib.rs:41-54`) has 8 members (UTF-8,
UTF-16 LE/BE, UTF-32 LE/BE, windows-1251, koi8-r, ibm866), versus the ~40 labels
the WHATWG Encoding Standard requires — see BUG-357 for the same shortfall on the
`TextDecoder` side.

## Масштаб

**Whole `encoding-detection` category: 0/44 subtests, all 44 executed tests fail
on this one cause.** Every test in that category is the same three lines —

```js
assert_equals(document.characterSet, "windows-1252", 'Expected windows-1252');
```

— so the failure text is uniformly `expected (string) "…" but got (undefined)
undefined` (43 tests), plus one variant that calls
`document.characterSet.toUpperCase()` and dies with `Cannot read properties of
undefined`.

Outside WPT the blast radius is much larger than encoding detection:

* `document.compatMode` is the canonical quirks-mode probe and is read by jQuery,
  older Bootstrap, Modernizr and most hand-rolled scroll-position helpers
  (`document.compatMode === 'CSS1Compat' ? documentElement : body`).
* `document.URL` / `document.documentURI` / `document.location` are common
  stand-ins for `window.location.href`; analytics and router code reads them.
* `document.characterSet` / `document.charset` gate the "should I re-encode this
  form payload" branch in legacy upload/export code.

## Возможный фикс (не реализован в этой сессии)

1. Cheap, correct-shape part: add the accessors to the live `document` literal
   (`dom.rs:6989+`), mirroring `dom.rs:4762-4769` but reading real state —
   `URL`/`documentURI`/`location` from `_lumen_loc_href` / the existing `location`
   object (which the shim already builds, `dom.rs:7414+`), `contentType` from the
   response MIME type.
2. `compatMode`: the parser already knows whether a doctype was present
   (`document.doctype` is wired, `dom.rs:7006`) — `'BackCompat'` when absent /
   quirky, `'CSS1Compat'` otherwise. Requires the quirks-mode flag to be plumbed
   from `lumen-html-parser` rather than inferred in JS.
3. `characterSet`/`charset`/`inputEncoding`: expose the `Encoding` chosen by
   `lumen_encoding::detect()` for the current document through a new native
   binding (`_lumen_get_document_encoding`) returning `Encoding::name()`. Note
   `name()` already returns WHATWG-canonical lowercase labels
   (`"windows-1251"`, `"ibm866"`), which is what the spec's
   `document.characterSet` wants.
4. Only after (3) would any `encoding-detection` test have a chance of passing,
   and only two of them: the 44 executed tests assert 22 distinct target
   encodings, of which `lumen-encoding` implements exactly two —
   **windows-1251** and **IBM866**. (Its third legacy member is koi8-**r**; the
   category tests koi8-**u**.) The other 20 —
   windows-1250/1252/1253/1254/1255/1256/1257/1258, windows-874,
   ISO-8859-2/5/6/7/8, KOI8-U, Shift_JIS, EUC-JP, EUC-KR, Big5, GBK — need
   decoder tables Lumen does not have, the same shortfall as BUG-357.

Not fixed in this session — P2-wpt vendors and surveys, code fixes are P3's lane
(`CLAUDE.md` developer assignments).
