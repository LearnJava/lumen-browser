# BUG-525: `document.scrollingElement` is not implemented — always `undefined`

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs` — `Document` shim)
**Найден:** WPT-RUN-3 срез 24 (`ROADMAP.md`) — массовый прогон `css/css-scroll-anchoring`

## Механизм

`grep -n "scrollingElement" crates/js/src/dom.rs` — zero hits (the only
mentions in the whole tree are a doc-comment in
`crates/js/src/scroll_timeline.rs:12`/`84` describing the *intended*
fallback `document.scrollingElement || document.documentElement` for
`ScrollTimeline`'s default source, which itself degrades to
`documentElement` since `scrollingElement` never exists). Confirmed live:
`typeof document.scrollingElement` → `"undefined"`.

## Симптом

Any test that does `var scroller = document.scrollingElement;` and then uses
`scroller.scrollTop = ...` throws `TypeError: Cannot set properties of
undefined (setting 'scrollTop')` (or `.scrollBy(...)` →
`Cannot read properties of undefined (reading 'scrollBy')`), which
`promise_test`/`test` wrappers report as an uncaught exception/rejection.
Largest single failure cluster in `css/css-scroll-anchoring` (18 files hit
the `scrollTop` variant, 4 the `scrollBy` variant, 2 `scrollLeft`, plus one
`Cannot read properties of undefined (reading 'scrollTop')`) — every test
that targets the *document* (root) scroller rather than a nested
`overflow: scroll` `<div>` is blocked by this alone, independent of
[BUG-523](BUG-523-OPEN.md)/[BUG-524](BUG-524-OPEN.md).

## Фикс (не сделан)

Add a `scrollingElement` getter to the `Document` shim per CSSOM View
§Extensions to the Document interface: return `document.documentElement`
when in no-quirks mode (Lumen doesn't implement quirks mode compatMode
switching, so the simple `documentElement`-always case likely applies),
`null` before it's connected to a browsing context. Small, self-contained
fix — no dependency on BUG-523/BUG-524.

## Срез 29 (`css/css-scroll-snap`, 2026-08-03)

Same gap, confirmed again independently in a different category:
`snap-area-capturing-add-scroll-container.html` (19 subtests, `.scrollTo`)
and `scroll-initial-target/scroll-initial-target-root.tentative.html` (2,
`.scrollTop`) both do `const document_scroller = document.scrollingElement;`
at top level. `.ini` under `tests/wpt/metadata/css/css-scroll-snap/`.
