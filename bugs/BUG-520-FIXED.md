# BUG-520: Resource Timing entries are never recorded for real resource fetches

**Статус:** FIXED 2026-09-07 (дрейф трекера + один реальный побочный дефект)
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
`fetch-resources.sub.html`, `expected: TIMEOUT`. Left unchanged by this fix —
see below.

## Исправлено P3 2026-09-07 (дрейф трекера)

The mechanism this bug describes — the native `_lumen_record_resource_timing`
hook never being called for a real engine-driven fetch — was already fixed as
a side effect of [BUG-839](BUG-839-FIXED.md) (2026-08-25), which never
cross-referenced this bug: `HttpClient::fetch_subresource_inner`
(`crates/network/src/lib.rs`) now unconditionally emits an
`Event::ResourceTimed` after every successful subresource fetch — images,
cascade stylesheets, `@font-face` bodies, parser-collected scripts alike — and
the shell drains that queue once per event-loop step
(`crates/shell/src/resource_timing.rs`,
`crates/shell/src/app/about_to_wait.rs:251`) into
`_lumen_deliver_resource_timings`, which this bug's original text confirms was
already correctly wired to `_lumen_record_resource_timing` on the JS side.
`BUGS.md`'s own one-line summary for this bug was never updated after BUG-839
landed and still read "сетевой слой его не вызывает" — stale, same class as
[BUG-512](BUG-512-FIXED.md)/[BUG-523](BUG-523-FIXED.md).

**One real, narrower defect survived the drift and is fixed by this commit:**
`@font-face src` bodies were fetched through `RequestDestination::Image`
instead of `::Font` (`crates/shell/src/page_load.rs`/`frames.rs`, both used to
call `fetch_image_bytes` for font bytes) — wrong on three independent axes:
Mixed Content classifies `Image` as `OptionallyBlockable` while `Font` is
`Blockable` (W3C Mixed Content §5.3); Resource Timing's `initiatorType` came
out `"img"` instead of the spec's `"css"`; ad-block filter matching saw
`ResourceType::Image` instead of `::Font` (`$image`/`$font` EasyList options no
longer lined up with what actually loaded). `fetch_image_bytes`/
`fetch_font_bytes` are now thin wrappers over a shared
`fetch_subresource_bytes` that takes an explicit `RequestDestination`. New
test: `lumen-network`'s
`fetch_subresource_reports_css_initiator_type_for_font_destination`.

**Not fixed by this commit, filed separately as [BUG-1021](BUG-1021-OPEN.md):**
`fetch-resources.sub.html`'s own four subtests check CORS *request mode*
(`Origin`/`Sec-Fetch-Mode` headers), which `fetch_subresource` has no concept
of at all — `wait_for_resource()` (the test's sync helper) should now resolve
thanks to the BUG-839 mechanism, so the test likely moves from TIMEOUT to FAIL
on two of its four subtests (`shape-outside`/`@font-face`, both expecting
`cors`), not to full PASS. No live WPT run confirms this prediction — this
environment's `tests/wpt/run_smoke.py` cannot start (`ssl.wrap_socket` removed
in the Python 3.14 installed here, unrelated pre-existing breakage) — so the
`.ini` is left at `TIMEOUT` rather than guessed at; BUG-1021 owns the
re-triage once a live run is possible.

`cargo test -p lumen-network --lib` (2204/2204) and `cargo test -p lumen-shell`
(1745/1745, 20 pre-existing ignored) green; `cargo clippy -p lumen-shell -p
lumen-network --all-targets -- -D warnings` clean.
