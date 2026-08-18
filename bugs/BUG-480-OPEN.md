# BUG-480: `<iframe>` has no separate browsing context — `contentWindow`/`contentDocument` are absent from the JS shim entirely

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

`grep -n "contentWindow\|contentDocument" crates/js/src/dom.rs` — zero
matches. Any test that creates an `<iframe>` and reaches into it via
`iframe.contentWindow`/`iframe.contentDocument` gets `undefined`, so the next
property access throws or the test hangs waiting on a promise/event that can
never fire inside a document that doesn't exist from JS's point of view.

## Уже отмечалось походя, но не заводилось отдельно

* [BUG-311](BUG-311-FIXED.md) (fixed): `Node.isConnected` — its own test note
  says "iframes remain FAIL — nested sub-documents through `contentDocument`
  aren't modeled", but the bug itself only covers `isConnected`.
* WPT-VENDOR-focus session (2026-07-28, not filed): "~13 subtests die because
  `<iframe>` has no browsing context at all, `contentWindow`/`contentDocument`
  = `null`" — noted in passing, never turned into its own `BUG-NNN`.

This entry is the first dedicated bug for the underlying gap itself.

## Масштаб находки (в этом срезе)

`tests/wpt/css/cssom-view/resources/matchMedia.js`'s `createIFrame()` helper
(used by all `MediaQueryList-*`/`matchMedia*` tests that need a resizable
sub-document to observe media-query changes in) awaits the iframe's `load`
event and then does `iframe.contentDocument.body.offsetWidth` — six
`MediaQueryList-*`/`MediaQueryListEvent.html` files TIMEOUT outright on this.
Also implicated: `elementsFromPoint-iframes.html`,
`scrollIntoView-iframes.html`, `scroll-behavior-subframe-root.html`,
`scroll-behavior-subframe-window.html`, `matchMedia-display-none-iframe.html`
— all TIMEOUT.

## Что нужно

A real nested browsing context: a second `Document`/`Window` pair per
`<iframe>`, `contentWindow` returning that `Window`, `contentDocument`
returning that `Document` (same-origin only, per HTML LS — cross-origin must
throw/return `null` for `contentDocument` while still returning a `Window`
for `contentWindow`). Large — likely its own multi-slice task, not a single
`BUG-NNN` fix; this entry documents the gap and its WPT blast radius so a
future task doesn't have to rediscover it from scratch.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for files whose
dominant/sole cause is this gap, `expected: TIMEOUT`/`FAIL`/`NOTRUN` per the
actual run.

## Срез 25 (`css/css-properties-values-api`, 2026-08-03)

`at-property-viewport-units.html` and `at-property-viewport-units-dynamic.html`
both build their entire test body inside `<iframe id=iframe srcdoc="...">` —
same gap, srcdoc-based sub-document never runs. Both file-level `TIMEOUT`
(zero subtests registered). `.ini` under
`tests/wpt/metadata/css/css-properties-values-api/` for both files.

## Срез 26 (`css/css-highlight-api`, 2026-08-03)

`HighlightRegistry-highlightsFromPoint.html`'s "returns empty array when
called on a display:none iframe document" subtest does
`iframe.contentWindow.document` — `contentWindow` is `null`, so the read
throws `Cannot read properties of null (reading 'document')` before the
test ever reaches `highlightsFromPoint()` (the other 6 subtests in the same
file fail on the unrelated [BUG-534](BUG-534-OPEN.md)). 1 subtest. `.ini`
under `tests/wpt/metadata/css/css-highlight-api/`.

## Срез 33 (`css/css-sizing/responsive-iframe`, 2026-08-03) — whole feature
directory, plus a flaky harness/subtest TIMEOUT-shape boundary

6 files, all relying on cross-frame `postMessage`/`contentWindow` for the
Responsive Iframe API (`frame-sizing`). New methodological finding: the same
file can surface as a **harness-level** TIMEOUT (0 subtests recorded) on one
run and as an **OK harness with a subtest-level TIMEOUT** on the next —
observed on 5 of the 6 files across three consecutive verify runs of the
identical `.ini`. The two shapes are not different bugs, just which side of
wptrunner's per-test timeout the process happened to land on under parallel
load; a `.ini` pinned to a single status flags spuriously as unexpected on
the other run. Fixed by using wptmanifest's list-expected syntax on both the
file-level and subtest-level line, e.g. `expected: [OK, TIMEOUT]` /
`expected: [PASS, TIMEOUT]` (confirmed the parser resolves this correctly
via `wptrunner.manifestexpected.get_manifest(...).get_test(id).get('expected')`
→ a Python list, matched against either observed status). Apply this pattern
to any future slice's iframe/postMessage-dependent TIMEOUT cluster instead
of re-diagnosing it as a regression. `responsive-iframe-request-resize-error.html`
additionally surfaces `window.requestResize is not a function` (a second,
narrower gap — the Responsive Iframe API's parent-side control method is
entirely unimplemented, not just the browsing-context container) once it
gets far enough to register subtests; folded under this bug's umbrella since
the whole `responsive-iframe/` feature area is unimplemented, not filed
separately. `.ini` under `tests/wpt/metadata/css/css-sizing/responsive-iframe/`.

## WPT-VENDOR-x-frame-options (2026-08-18) — whole category is this bug

6 files, 157 subtests, every one of them builds an `<iframe>` and awaits
either its `message` event (cross-document `postMessage` from the framed
page back to the parent) or its `load` event (the "blocked" case, checking
`iframe.contentDocument === null` from the handler). Both paths depend on
the framed document actually running as a nested browsing context — since
`<iframe>` has none, neither the message nor the load event ever fires.
`run_report.py --all --root x-frame-options --recursive` (1 min 21 s):
**0/6 harness OK, 0/157 subtests** — a uniform TIMEOUT wall, no
category-specific defect (the `X-Frame-Options`/CSP `frame-ancestors`
header logic under test is never reached). No new `BUG-NNN` filed.
