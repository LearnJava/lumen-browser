# BUG-278 — HTTP client rejected close-delimited responses without explicit `Connection: close`

**Статус:** FIXED 2026-07-13
**Компонент:** network (`crates/network/src/lib.rs`, `read_response`/`read_response_streamed`)
**Найден:** P2-wpt S4 (`docs/tasks/p2-wpt-integration.md`), запуская `wptrunner`'s `wptserve` (Python
`http.server`-based, the reference server WPT tests run against) — every fetch to it failed with
`network error: response without Content-Length or chunked`.

## Причина

`read_head` computes `server_wants_close` only from an explicit `Connection: close` header. Both body
readers then required `server_wants_close || status == 204 || status == 304` before falling back to
"read to EOF" framing — otherwise a response with neither `Transfer-Encoding: chunked` nor
`Content-Length` was treated as a hard protocol error.

Per RFC 7230 §3.3.3 point 7, the read-to-EOF fallback for **responses** (as opposed to requests) with
neither header applies **unconditionally** — it is not gated on an explicit `Connection: close`. Many
real servers, including Python's `http.server` (which `wptserve` is built on), rely on this: they close
the connection without ever sending the header. Confirmed via `curl -v` against a live `wptserve`
instance — headers were `HTTP/1.1 200 OK` / `Content-Type: text/html` / `Server: BaseHTTP/0.6
Python/3.14.4` / `Date: ...`, no `Content-Length`, no `Transfer-Encoding`, no `Connection` at all.

## Фикс

Both `read_response` and `read_response_streamed` now apply the read-to-EOF fallback (`Vec`-buffered /
`BodyFraming::Eof` respectively) whenever chunked/Content-Length/204/304 don't apply — the
`server_wants_close`-gated `else if` branch was folded into the unconditional final `else`. `conn.closed`
is still forced `true` whenever this fallback is used (the connection is provably dead after reading to
EOF), regardless of whether the server sent an explicit `Connection: close`.

Verified: `cargo test -p lumen-network` (55 unit + 3 doctests, all green — no test relied on the removed
error message); manually reproduced against a live `wptserve` instance (`curl` succeeded, pre-fix Lumen
navigate failed with the exact error message, post-fix navigate completed in <1s).

## Побочный след

The dead `server_wants_close` variable is still used elsewhere (post-body `if server_wants_close {
conn.closed = true }` for the chunked/Content-Length paths, honoring an *explicit* close request from
the server) — untouched, still correct.
