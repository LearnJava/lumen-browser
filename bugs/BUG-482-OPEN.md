# BUG-482: `HTMLElement.offsetParent`/`document.scrollingElement` missing entirely; `document.compatMode` hardcoded to `CSS1Compat` (no quirks-mode detection)

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

Three small, separately-confirmed gaps on the main (non-iframe) document,
grouped here because they were all found by the same test file and are all
"the IDL attribute isn't wired up at all" in shape:

```
FAIL The offsetParent of the root element is null
  assert_equals: expected (object) null but got (undefined) undefined
FAIL document.compatMode should be BackCompat in quirks.
  assert_equals: Should be in quirks mode. expected (string) "BackCompat" but got (undefined) undefined
FAIL document.scrollingElement should be body element in quirks.
  assert_equals: scrollingElement in quirks mode should default to body element. expected (object) ... but got (undefined) undefined
```

`grep -n "offsetParent\|scrollingElement" crates/js/src/dom.rs` — zero
matches for either. `compatMode` (`dom.rs:4826`) exists but is a constant:
`Object.defineProperty(doc, 'compatMode', { get: function() { return
'CSS1Compat'; }, ... })` — always "no-quirks", regardless of the document's
actual doctype (or lack of one).

## Масштаб находки

`offsetParent-body-and-html.html` (6 subtests), `offsetTopLeft-table-caption.html`
(1 subtest), `offsetParent-block-in-inline.html`, `offsetParent_element_test.html`
(missing property, not attributed here — re-check after the fix),
`HTMLBody-ScrollArea_quirksmode.html` (8 subtests: `compatMode` + `scrollingElement`
+ two downstream `assert_greater_than` failures that depend on quirks-mode body
scrolling). `scrollingElement.html` (8 subtests, TIMEOUT) is a **different**
file that reaches the same two properties only through a quirks-mode
`<iframe>` — that one is [BUG-480](BUG-480-OPEN.md) (no iframe browsing
context), not this bug; its `onload` never fires so the test times out before
ever reading `scrollingElement`.

## Что нужно

* `offsetParent`: implement the CSSOM View §5 algorithm (nearest positioned
  ancestor, or `<body>`/table-cell fallback per spec, `null` for `<html>`/
  `<body>`/non-rendered elements) — natural to land alongside
  [BUG-476](BUG-476-OPEN.md) (`offsetLeft`/`offsetTop`), since both need the
  same ancestor walk.
* `document.scrollingElement`: return `document.documentElement` in
  no-quirks mode, `document.body` (if scrollable) or `null` in quirks mode.
* `document.compatMode`: real doctype/quirks-mode sniffing at parse time
  (missing/incomplete `<!DOCTYPE html>` → `BackCompat`) instead of the
  hardcoded constant.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for the
attributed files, `expected: FAIL` per the actual run.
