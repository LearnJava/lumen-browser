# BUG-482: `HTMLElement.offsetParent`/`document.scrollingElement` missing entirely; `document.compatMode` hardcoded to `CSS1Compat` (no quirks-mode detection)

**Статус:** FIXED 2026-09-02 (P3)
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — moved out of
`dom.rs` by SPLIT-JS3, 2026-08-28; symptom line numbers below are from before
that move)
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
  [BUG-476](BUG-476-FIXED.md) (`offsetLeft`/`offsetTop`), since both need the
  same ancestor walk.
* `document.scrollingElement`: return `document.documentElement` in
  no-quirks mode, `document.body` (if scrollable) or `null` in quirks mode.
* `document.compatMode`: real doctype/quirks-mode sniffing at parse time
  (missing/incomplete `<!DOCTYPE html>` → `BackCompat`) instead of the
  hardcoded constant.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for the
attributed files, `expected: FAIL` per the actual run.

## Fix (P3, 2026-09-02)

One of the three grouped gaps was already closed on the side: `compatMode`
started reading `Document::mode()` through `_lumen_get_document_compat_mode`
when [BUG-358](BUG-358-FIXED.md) landed (2026-08-09) — `lumen-html-parser`
already computes the quirks-mode flag from the DOCTYPE, BUG-358 just wired
the read. This bug's own record was never revisited to notice; there is
nothing left to do for that third.

The other two:

* `offsetParent` — a getter on `Element` wrapping the ancestor walk
  [BUG-476](BUG-476-FIXED.md) already built (`_lumen_offset_parent_nid`:
  nearest positioned ancestor, or `<body>`/`<td>`/`<th>`/`<table>`, `null`
  for the root element, `<body>` itself, or `position: fixed`) through
  `_lumen_make_element`.
* `document.scrollingElement` (CSSOM View §5.2) — `documentElement` in
  no-quirks mode; in quirks mode, `body` unless it is "potentially
  scrollable" (new `_lumen_body_potentially_scrollable`: true only when
  NEITHER `body` nor `documentElement` is left at the fully-visible overflow
  default on both axes), in which case `null`.

Five regression tests (`crates/js/src/dom/tests/v8_elem_geometry_scroll.rs`):
`offset_parent_returns_nearest_positioned_ancestor`,
`offset_parent_null_for_root_body_and_fixed`,
`scrolling_element_is_document_element_in_no_quirks_mode`,
`scrolling_element_defaults_to_body_in_quirks_mode`,
`scrolling_element_is_null_in_quirks_mode_when_body_potentially_scrollable`.

**Residual:** `HTMLBody-ScrollArea_quirksmode.html`'s tail —
`assert_greater_than(window.innerHeight, …)` assertions checking that
`body.scrollHeight` tracks content rather than staying clamped to the
viewport in quirks mode — is untouched: it needs `window.innerHeight`
([BUG-529](BUG-529-OPEN.md), still missing) plus an overflow-propagation
model this fix doesn't build. The bug's own scope note already flagged this
as "two downstream `assert_greater_than` failures that depend on quirks-mode
body scrolling". `offsetParent-block-in-inline.html` and
`offsetTopLeft-table-caption.html` were not re-run live against the fix in
this session — `_lumen_offset_parent_nid` already covers `<td>`/`<th>`/
`<table>` (inherited from BUG-476) but not `<caption>` specifically. `.ini`
files intentionally untouched, same protocol as BUG-475/476/479/481: the
exact PASS/FAIL split needs a fresh `run_report.py`, not run in this
session.

Gates: `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` clean; `cargo test -p lumen-js --features v8-backend` 3414/3414
(whole crate).
