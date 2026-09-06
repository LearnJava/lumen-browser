# BUG-1021: `fetch_subresource` has no per-destination CORS request mode — every
subresource fetch is effectively `no-cors`, undistinguished from a real `cors` fetch

**Статус:** OPEN
**Дата:** 2026-09-07
**Компонент:** network (`crates/network/src/lib.rs::HttpClient::fetch_subresource`/
`fetch_subresource_inner`) — no `Sec-Fetch-Mode`/`Origin` header logic anywhere in
the file (`grep -n "sec-fetch-mode\|Sec-Fetch-Mode" crates/network/src/lib.rs`
is empty), no per-`RequestDestination` CORS mode table
**Найден:** P3 2026-09-07, побочно при закрытии [BUG-520](BUG-520-FIXED.md)
(Resource Timing / font fetch destination)

## Механизм

The Fetch Standard ties a subresource's *request mode* (`no-cors` vs `cors`) to
*how* the destination was requested, not just to what it is: a plain `<img
src>`/CSS `background-image url()` is `no-cors`, but the same image referenced
from `shape-outside` is `cors` (crossorigin-attribute-independent — the CSS
Shapes spec forces it); `@font-face src` is always `cors`; a stylesheet-driven
`@import` is `no-cors`. `HttpClient::fetch_subresource` takes only a
`RequestDestination` (`Image`/`Font`/`Style`/…) and has no second axis for
request mode at all — every subresource fetch goes out identically, with no
`Origin` header and no `Sec-Fetch-Mode` header, indistinguishable from a
`no-cors` fetch regardless of what the caller passed as `destination`.

## Симптом

`tests/wpt/css/fetching/fetch-resources.sub.html` (already vendored, `.ini`
currently attributes it to BUG-520) fetches the same URL through
`background-image`, `shape-outside`, `@font-face src` and `@import` in turn,
each behind a per-request echo endpoint (`support/echo-helper.js`), and asserts
via the *request headers the server actually received* (`sec-fetch-mode`
header, or presence of an `origin` header) that:

- background-image → `no-cors`
- shape-outside → `cors`
- `@font-face src` → `cors`
- `@import` → `no-cors`

Since Lumen sends neither header on any subresource fetch,
`extract_cors_mode()` (`fetch-resources.sub.html:68`) falls back to
`Reflect.has(result, 'origin') ? 'cors' : 'no-cors'`, which is always
`'no-cors'` (no `Origin` header ever sent) — the `shape-outside`/`@font-face`
subtests would `assert_equals('no-cors', 'cors')` and FAIL, not TIMEOUT.

## Почему это не BUG-520

BUG-520 was filed 2026-08-03 against the observation that
`performance.getEntriesByType('resource')` stayed empty forever, so
`wait_for_resource()` (the test's own synchronization helper, which resolves on
*any* Resource Timing entry whose `name` contains the target URL — it does not
inspect `initiatorType` or headers at all) never resolved and the whole test
TIMED OUT. That mechanism was fixed as a side effect of
[BUG-839](BUG-839-FIXED.md) (2026-08-25): `HttpClient::fetch_subresource_inner`
now unconditionally emits `Event::ResourceTimed` for every subresource load,
which the shell drains once per event-loop step into
`_lumen_deliver_resource_timings` (`crates/shell/src/resource_timing.rs`,
`crates/shell/src/app/about_to_wait.rs:251`) — confirmed by reading the code
path end to end and by the crate's own unit tests
(`resource_timing::tests::*`, `lumen-network`'s
`fetch_subresource_allows_http_image_in_spec_default` and this bug's sibling
`fetch_subresource_reports_css_initiator_type_for_font_destination`). So
`wait_for_resource()` should now resolve for all four subtests — the test
should no longer TIMEOUT, but it will still FAIL on the `cors`/`no-cors`
assertions above, which is a completely different mechanism (request mode, not
resource timing). BUG-520 also caught one real, narrower defect on the way —
`@font-face` bodies were fetched with `RequestDestination::Image` instead of
`::Font`, which mistagged Mixed Content level/ad-block resource type/Resource
Timing `initiatorType` — fixed in the same commit that closes BUG-520; that fix
is unrelated to the CORS-mode gap this bug describes and does not touch request
headers at all.

**No live WPT re-run confirms the FAIL-not-TIMEOUT prediction** — this
environment's `tests/wpt/run_smoke.py` cannot start at all right now
(`ssl.wrap_socket` was removed in the Python 3.14 installed here; unrelated
pre-existing environment breakage, not this bug). The `.ini` is left as `TIMEOUT`
until a live run can confirm the new failure mode.

## Что нужно

- Add a request-mode axis to `fetch_subresource` (either a second parameter or
  folded into an expanded `RequestDestination`-like enum) so callers can
  specify `cors`/`no-cors` per the Fetch Standard's request-mode table for
  each destination — `shape-outside`/`@font-face` need `cors`, plain
  `<img>`/background-image/`@import` need `no-cors`.
- Wire `Origin`/`Sec-Fetch-Mode` headers off that mode, matching whatever the
  page-driven `fetch()`/XHR path in the JS shim already does for its own CORS
  handling (reuse that logic rather than re-deriving it — check
  `crates/js/src/shim/*.js` for the existing `cors`-mode header construction
  before writing a second one).
- Update `tests/wpt/metadata/css/fetching/fetch-resources.sub.html.ini` once a
  live run confirms the actual outcome (may need per-subtest FAIL rather than
  blanket removal, depending on how many of the four modes land in one pass).

## .ini

No new `.ini` filed by this bug — `fetch-resources.sub.html.ini` continues to
attribute all four subtests to BUG-520 until a live run re-triages them; that
attribution is now known to be partly stale (see above) but is left alone
rather than guessed at.
