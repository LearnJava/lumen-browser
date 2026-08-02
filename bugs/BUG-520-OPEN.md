# BUG-520: Resource Timing entries are never recorded for real resource fetches

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js/network boundary (`crates/js/src/dom.rs:11349` —
`_lumen_record_resource_timing`; caller side unwired in `crates/network`/
`crates/shell`/`crates/driver`)
**Найден:** WPT-RUN-3 срез 23 (`ROADMAP.md`) — массовый прогон `css/fetching`

## Механизм

`performance.getEntriesByType('resource')`/a `PerformanceObserver` with
`type: 'resource'` are both implemented correctly on the JS side — the
`PerformanceObserver` plumbing (`dom.rs:11221`), the `'resource'` entry in
`supportedEntryTypes`, and the entry-construction helper
`_lumen_record_resource_timing(url, initiator, start_ms, duration_ms)`
(`dom.rs:11353`) all exist and work when called. The gap is that nothing in
the engine ever calls that native hook for a real page load:

```
grep -rn "record_resource_timing" crates/ tests/
```

returns **zero** call sites outside `dom.rs` itself — the only invocations
in the whole tree are the shim's own internal self-tests
(`dom.rs:17057-17135`, e.g. `_lumen_record_resource_timing('https://
example.com/app.js', 'script', 1000, 50)`), which manually fabricate a fake
URL/timing pair to prove the JS-side entry construction and observer
delivery work in isolation. The real network/fetch layer
(`crates/network`, the `<img>`/`<link>`/`<script>`/`@font-face`/`@import`
resource loaders in `crates/shell`, `fetch()`/`XMLHttpRequest` in
`crates/js`) never calls this hook when a resource actually completes
loading, so `performance.getEntriesByType('resource')` is permanently `[]`
and any `PerformanceObserver({type: 'resource', buffered: true})` never
fires, regardless of how many images/fonts/stylesheets/scripts the page
loads.

## Симптом

`css/fetching/fetch-resources.sub.html` awaits
`wait_for_resource(url)` (`support/echo-helper.js`), which resolves a
promise the first time a `PerformanceObserver({type: 'resource', buffered:
true})` delivers an entry whose `name` includes the target URL. Since no
such entry is ever delivered, all four subtests (background-image,
`shape-outside` image, `@font-face` src, `@import`) never resolve and the
whole test times out:

```
TEST_END: Test TIMEOUT, expected OK. Subtests passed 0/4. Unexpected 4
TIMEOUT Background images should fetch with no-cors - Test timed out
NOTRUN Shape images should fetched with cors
NOTRUN WebFonts should be fetched with cors
NOTRUN CSS imports should be fetched without cors
```

## Масштаб находки

1 file / 4 subtests measured directly (`css/fetching`, the category's only
testharness id), but the mechanism is generic — any WPT test elsewhere in
the vendored corpus that gates on Resource Timing (a common pattern for
"did the browser actually fetch X" assertions, independent of `css/`) pays
the same TIMEOUT.

## Что нужно

Wire `_lumen_record_resource_timing` into the real resource-load
completion paths: at minimum `<img>`/`<link rel=stylesheet>`/`<script
src>`/`@font-face src`/`@import`/CSS `url()` references
(background-image, shape-outside, cursor, etc.) and `fetch()`/
`XMLHttpRequest`, each tagged with the correct `initiatorType`.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/fetching/` for
`fetch-resources.sub.html`, `expected: TIMEOUT`.
