# BUG-980: `XMLHttpRequest.send()` runs the whole request/response cycle
synchronously inside the call itself — a handler assigned after `send()`
returns (a common, spec-legal WPT idiom) never sees a single event

**Статус:** FIXED 2026-09-19 (P3)
**Дата:** 2026-09-04
**Компонент:** js (`crates/js/src/xhr.rs::XMLHttpRequest.prototype.send`,
line ~296: "Execute synchronously using the same native fetch bindings")
**Найден:** P2, WPT-RUN-6 срез 60, живой пробой

## Механизм

`send()`'s own comment says it plainly: the request is executed
*synchronously*, via `_lumen_fetch_sync`/`_lumen_fetch_sync_with_body`, a
blocking native call. All of the readyState transitions XHR §4.5 spreads
across the request's lifetime — `HEADERS_RECEIVED` (2), `LOADING` (3),
`DONE` (4) — are fired back-to-back inside that one call
(`xhr.rs:362-380`), and `send()` itself does not return to caller JS until
all four are done and `loadend` has fired. Confirmed live
(`--mcp-live-port`, `eval`):

```js
var x = new XMLHttpRequest();
x.open('GET', '/data.txt');
x.send();
// readyState is already 4 (DONE) right here, on the very next line —
// the whole exchange happened inside .send() itself.
```

For XHR opened in async mode (the default, and the only mode this test
suite/most of WPT uses — `open(method, url)` with no third argument), the
spec requires the opposite: `send()` must return immediately, and every
`readystatechange`/progress event fires later, through the event loop —
so that script attaching its listeners *after* calling `send()` still sees
every one of them. That ordering is exactly what the four spec examples in
XHR §4.5.6 and the vast majority of WPT's own XHR tests rely on:

```js
var client = new XMLHttpRequest();
client.open("GET", url);
client.send();                                  // returns immediately in a real browser
client.onreadystatechange = function () { ... }; // attached after send(), still fires
```

Because Lumen's `send()` has already fired and discarded every event by
the time this line runs, `onreadystatechange` is assigned to a request
that is already done and will never transition state again — the handler
is *never called at all*. Confirmed both ways with a live probe against
the same server-backed request:

```js
// handler assigned AFTER send() (upstream idiom) — never fires:
x.open('GET', '/data.txt'); x.send();
x.onreadystatechange = function(){ log.push(x.readyState); };
// ⇒ log stays [] forever

// handler assigned BEFORE send() — fires, because it already existed
// when send() ran through 2 → 3 → 4 synchronously:
x.open('GET', '/data.txt');
x.onreadystatechange = function(){ log.push(x.readyState); };
x.send();
// ⇒ log = [2, 3, 4] immediately, synchronously inside send()
```

Note the second case is *also* non-conformant (a real browser defers even
this to the event loop, so `send()` returns before any state changes at
all — timing-sensitive tests that check `readyState` right after `send()`
would still fail), but it happens to make the assign-before-send idiom
work by accident, which is why not every XHR test in the corpus hangs.

## Масштаб

Any WPT test using the assign-after-`send()` idiom hangs instead of
running — this is the idiom WPT itself demonstrates in its own
`XMLHttpRequest` documentation, so it is common, not an edge case. First
confirmed on `/xhr/cors-expose-star.sub.any.html` (all three of its
`async_test`s use exactly this ordering: `open()` → `send()` →
`onreadystatechange = ...`), which TIMEOUTs 3/3 subtests under the real
`wptrunner`+`wptserve` stack (10s each, 0/3 harness OK) — a leftover
probe-tool-gap candidate from WPT-RUN-6 slice 59. The sibling file
`/fetch/api/cors/cors-expose-star.sub.any.js` shares the identical
attach-after-send pattern and is expected to hang the same way (not run
live this slice — same directory-run budget constraint slice 59 hit).

## Что нужно

Make `send()` for an async request return control to caller JS before any
network I/O happens, and drive the readyState transitions
(`HEADERS_RECEIVED`/`LOADING`/`DONE`) plus the `progress`/`load`/`loadend`
events through the existing task queue/microtask machinery instead of
inline in the same call — the same shape the `fetch()`/`Response` path
already uses (native call kicks off, JS side resolves later through a
promise/callback, not a blocking return). `xhr.rs` is its own `rt.eval`
outside `WEB_API_SHIM` (see `subsystems/js.md`'s XHR note, BUG-780) — any
fix here needs to be checked against `worker.rs`'s `WORKER_NET_SHIM`,
which likely has the same synchronous shape (`_lumen_worker_net_fetch`),
separately.

## Классификация WPT-RUN-6

Attributed via `_exact_id_marker("/xhr/cors-expose-star.sub.any.html")` in
`tests/wpt/timeout_audit.py` (marker `xhr-send-runs-synchronously`).

## Исправлено (P3, 2026-09-19)

`send()` for the async case (`_async !== false`, the default and the only
mode most page/WPT code uses) no longer blocks: it starts the request
through the same `_lumen_fetch_async_start`/`_poll`/`_commit`/`_free`/
`_csp_info` bridge that `fetch()`'s async path already uses
(`crates/js/src/v8_runtime/install/net.rs`), and drives the readyState
transitions and progress events from a `setTimeout` poll loop instead of
from inside the `send()` call itself — exactly the shape this bug asked
for. `abort()` now flips the in-flight request's `AbortToken` via
`_lumen_fetch_async_abort` instead of resetting state immediately.

The true synchronous mode (`async === false`, gated by BUG-953's
Document-Policy/Permissions-Policy checks) is unchanged — it still blocks
via `_lumen_fetch_sync*`, which is correct: real synchronous XHR is
required by spec to block the calling thread.

Two new regression tests in `crates/js/src/xhr.rs`:
`xhr_send_is_async_handler_assigned_after_send_still_fires` (a handler
assigned after `send()` returns still observes `readyState 4`) and
`xhr_send_returns_before_request_completes` (`send()` returns control
before the request settles). `xhr_connect_src_block_fires_security_policy_violation_event`
(`crates/js/src/dom/tests/v8_whatwg_streams.rs`) was updated to pump
`_lumen_tick_timers()` — the `connect-src` block now surfaces a tick
later, not within the same `eval` call.

`cargo test -p lumen-js --features v8-backend` — 3881/3881 (whole crate,
not just `xhr`). `cargo clippy --workspace --all-targets -- -D warnings`
clean.

Not touched: `worker.rs`'s `WORKER_NET_SHIM`/`_lumen_worker_net_fetch` has
a similarly synchronous shape, but that is an intentional block of the
worker's own JS thread (not the main thread) — a different situation, out
of scope for this fix.
