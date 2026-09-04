# BUG-980: `XMLHttpRequest.send()` runs the whole request/response cycle
synchronously inside the call itself — a handler assigned after `send()`
returns (a common, spec-legal WPT idiom) never sees a single event

**Статус:** OPEN
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
