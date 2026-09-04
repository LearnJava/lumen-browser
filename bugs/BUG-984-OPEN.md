# BUG-984: the final URL after an HTTP redirect never reaches JS — `Response.url`/`.redirected`, `XMLHttpRequest.responseURL` and `WorkerLocation` (dedicated/shared worker) all report the *pre-redirect* request URL

**Статус:** OPEN
**Дата:** 2026-09-04
**Компонент:** network (`crates/network/src/lib.rs::HttpClient::fetch_request_impl`) / core (`crates/core/src/ext.rs::JsFetchResult`) / js (`crates/js/src/xhr.rs`, `crates/js/src/worker.rs::fetch_worker_script`, `crates/js/src/shim/web_api_shim_mid_b.js::_lumen_fetch`)
**Найден:** P2, WPT-RUN-6 срез 62, живой пробой (`run_report.py` через реальный `wptrunner`+`wptserve`)

## Механизм

`fetch_with_redirect` (`crates/network/src/lib.rs:1958`) already computes the
right answer — it returns `Ok((resp, url.clone()))` where `url` is the URL of
the *last* hop after following every `301`/`302`/`303`/`307`/`308` (line
`2422`/`2435`, recursive call threads the redirect target forward). But its
only caller, `HttpClient::fetch_request_impl`
(`crates/network/src/lib.rs:3925`), destructures that tuple as
`let (resp, _final_url) = fetch_with_redirect(...)` — the underscore prefix
is not a lint suppression, the value is genuinely thrown away — and builds
the returned `JsFetchResult` from `resp` alone. `JsFetchResult`
(`crates/core/src/ext.rs:1791`) itself has no field to carry a URL at all
(`status`/`status_text`/`headers`/`body`, nothing else), so even a caller
that wanted the final URL has nowhere to read it from at the JS-provider
boundary — the value is lost twice over, not just dropped once.

Three independent JS-visible surfaces read the *request* URL right back as
if it were the *response* URL, because that is the only URL any of them
ever had:

- `crates/js/src/shim/web_api_shim_mid_b.js:4032` —
  `_lumen_response_from_fetch_cache(status, statusText, hdrs, url)` passes
  `url`, the string `fetch()` was called with, straight into the `Response`
  it constructs. `Response.prototype.url` (`:3222`) and `.redirected`
  (`:3223`) read that same stored value back — `redirected` is never set
  `true` anywhere on this path (the one `redirected: false` literal at
  `:3166` belongs to a different, static `Response`-construction branch).
- `crates/js/src/xhr.rs:359` — `self.responseURL = self._url;` inside
  `send()`, where `self._url` is the URL `open()` was called with.
- `crates/js/src/worker.rs` — `fetch_worker_script` (`:1568`) returns only
  the response body (`Option<String>`, no URL), and the worker's own
  `location`/`WorkerLocation` (`:1970`/`:1977`, `install_worker_globals_v8`)
  is built from `script_url`, the constructor argument, never anything
  fetched back from the network.

## Прямое измерение

`redirect-sharedworker.html`
(`workers/interfaces/WorkerGlobalScope/location/redirect-sharedworker.html`)
under the real `wptrunner`+`wptserve` stack (`run_report.py --all --root
workers/interfaces/WorkerGlobalScope/location --offset 2 --limit 1`,
dev-release, `main` = `8a48a0635`):

```
FAIL redirect - assert_equals: expected
  "/workers/interfaces/WorkerGlobalScope/location/redirect.js"
  but got "/common/redirect.py"
```

The worker is constructed as `new SharedWorker('/common/redirect.py?
location=/workers/.../redirect.js?a')` — a real 302 to the second URL. The
worker script itself executes correctly (the redirect *is* followed for the
purpose of fetching the right bytes — `fetch_with_redirect` did its job),
but `self.location.pathname` inside it still reads back `/common/redirect.py`,
the pre-redirect constructor URL, instead of the final one — exactly the
`_final_url` value that was computed and discarded one layer down.

Not a TIMEOUT — the assertion fails promptly (`harness OK`, 0/1 subtests
passed), so this does not explain any corpus TIMEOUT id by itself; found
while confirming `redirect-sharedworker.html` (a WPT-RUN-6 slice 57
probe-tool-gap candidate, unreachable through `serve_wpt_like.py`'s bare
`SimpleHTTPRequestHandler`) does not hang under the real infra either.

## Масштаб

Same discarded value, three call sites, all silently wrong whenever the
underlying request actually redirects:

- `fetch()`'s `Response.url` always equals the request URL, `.redirected`
  is always `false` — any WPT test built on either (a common pattern in
  `fetch/api/redirect/*`, `fetch/api/response/response-consume-empty-body-
  after-redirect.html`-shaped tests, and elsewhere) reads a wrong but
  *present* value, so this surfaces as ordinary `FAIL`, not `TIMEOUT`.
- `XMLHttpRequest.responseURL` — same shape, XHR §4.5.6's own `redirect-*`
  tests.
- `WorkerLocation` (dedicated and shared workers) after a redirected script
  fetch — confirmed above; the analogous
  `interfaces/WorkerGlobalScope/location/{redirect,redirect-module}.html`
  (dedicated-worker siblings of the shared-worker id measured here) are
  very likely the same defect, not independently confirmed this slice.

## Что нужно

Add a `url: String` (or `String` for the final hop) field to `JsFetchResult`,
thread the `_final_url` `fetch_request_impl` already computes and discards
into it, and update the three readers (`Response.url`/`.redirected` via
`_lumen_response_from_fetch_cache`'s extra argument, `xhr.rs::send`'s
`self.responseURL`, and `fetch_worker_script`'s return type plus
`worker.rs`/`shared_worker.rs`'s `location` construction) to use it instead
of the pre-fetch URL. `.redirected` additionally needs a boolean (final URL
≠ request URL) rather than just the string.

## Классификация WPT-RUN-6

Not attributed to any TIMEOUT id — no `_exact_id_marker`/mechanism entry
added to `tests/wpt/timeout_audit.py`. `redirect-sharedworker.html` stays
unclassified (real `FAIL`, not a hang).
