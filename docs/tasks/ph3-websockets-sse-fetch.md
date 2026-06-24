# Ph3 — WebSockets + SSE + Fetch/AbortController

**Developer:** P3 · **Branch:** `p3-ph3-websockets-sse-fetch` · **Size:** L · **Crates:** `lumen-network`, `lumen-js`, `lumen-shell`

> Roadmap source: `docs/plan/phases.md:121` — *"WebSockets (RFC 6455) + Server-Sent Events + Fetch API runtime с AbortController `[P3]`"*. Phase 3 (v1.0) item.

---

## Status

**Phase 3 — future.** Do not start until Phase 2 closes (`STATUS-P1.md` "Закрытие Фазы 2" block, version bump 0.5.0). This file scopes the work ahead of time so the eventual session does not re-research.

**Honest framing.** This is *not* a greenfield item. All three runtimes already exist and work end-to-end against the real `HttpClient` (verified, not stubs). The Phase 3 mandate is to **harden them to production grade**: convert the synchronous / poll-based delivery model to true async event-loop delivery, add in-flight cancellation, and close the protocol-correctness gaps each runtime carries from its Phase 0/2 implementation. Treat the existing code as the baseline to *upgrade*, never to rewrite.

## Progress (2026-06-25, P1 — branch `p1-ph3-ws-sse-fetch`)

**Phase A — Fetch in-flight abort: foundation + interface landed (steps 1–2 partial).**

- **Step 1 DONE** — `AbortToken` added to `lumen-core::ext` (`crates/core/src/ext.rs`):
  `Arc<AtomicBool>`-backed clonable cooperative cancel flag (`new`/`abort`/`is_aborted`,
  `Default`), SeqCst ordering, 5 unit tests. Commit `0505c722`.
- **Step 2 partial (interface-first) DONE** — `JsFetchProvider::fetch_cancellable(url, method,
  &AbortToken)` back-compatible default method with **pre-flight** cancellation
  (`token.is_aborted() → Error::Aborted`, else delegate to `fetch_sync`); new typed
  `Error::Aborted(String)` variant in `lumen-core::error` for the JS layer to map to
  DOMException `AbortError`. 2 unit tests. `lumen-core` + `lumen-network` compile clean.
  Commit `4404027f`.

**Remaining (not yet done):**
- **Step 2 deep half** — real in-flight teardown on `HttpClient::fetch_cancellable`: poll the
  token between socket reads and shut the connection down. Blocked on the network read path being
  fully blocking (`read_response`/`read_exact` at `crates/network/src/lib.rs:460`,
  `read_chunked` loop) — needs non-blocking reads / read timeouts or a watchdog thread that
  `shutdown()`s the socket on abort. Largest remaining piece of Phase A.
- **Step 3** — JS `fetch()` wiring (`crates/js/src/dom.rs:7328`): create a token, register an
  `abort` listener that flips it, thread it through a `_lumen_fetch_cancellable` bridge, reject
  with `signal.reason`.
- **Step 4** — `MockTransport` tests for mid-stream abort.
- **Phases B (WS async delivery) and C (SSE non-blocking reconnect)** — untouched.

---

## Goal

Bring all three network runtimes to spec-complete, production-grade behaviour:

1. **WebSocket** — async message delivery driven by the JS event loop (not the JS-polls-Rust model), correct close handshake state machine, sub-protocol/extension negotiation surfaced to JS.
2. **Server-Sent Events** — non-blocking reconnect (currently `std::thread::sleep` on a worker thread), `Last-Event-ID` honoured across reconnect (works), event delivery through the same event-loop path as WS.
3. **Fetch + AbortController** — in-flight cancellation (today only a pre-flight `signal.aborted` check; the request itself is synchronous and uncancellable), and ideally a genuinely async fetch that does not block the JS runtime thread.

---

## Current state (per-feature, with file:line)

### WebSocket — implemented, poll-based delivery

- **Network frame/protocol layer: complete.** `crates/network/src/websocket/mod.rs:48` `WebSocket` implements `WebSocketSession`. RFC 6455 handshake (`websocket/upgrade.rs`), frame codec (`websocket/frame.rs`), masking (`websocket/mask.rs`), and RFC 7692 permessage-deflate (`websocket/deflate.rs`) all present. Fragmentation reassembly at `mod.rs:190` (`recv_inner`), control-frame handling (Ping→Pong auto-reply, Close echo) inline.
- **Provider wiring: complete.** `crates/network/src/lib.rs:3341` `impl JsWebSocketProvider for HttpClient` → `connect()` at `lib.rs:3342`. Trait `WebSocketSession` at `crates/core/src/ext.rs:1361`; JS-facing `JsWebSocketSession` at `ext.rs:1778` (background recv thread → queue → `poll`).
- **JS binding: complete but poll-based.** `crates/js/src/dom.rs:1395` registers `_lumen_ws_connect / _lumen_ws_send / _lumen_ws_send_bin / _lumen_ws_close / _lumen_ws_poll`. The `WebSocket` JS class is at `dom.rs:7427`. **Limitation (in-code comment `dom.rs:1396`, `dom.rs:7428`):** *"Phase 0 model: synchronous connect, background recv thread, JS polls. Full async delivery (persistent JS runtime) is Phase 2+."* Messages are delivered only when the shell pumps `_lumen_pump_websockets` from the timer tick.
- **Shell: complete.** `crates/shell/src/main.rs:3732` installs the WS provider; `Event::WebSocket{Connected,Message,Closed}` emitted from `mod.rs`.

**Gap to close:** event-loop-driven delivery (no JS polling), sub-protocol selection (`Sec-WebSocket-Protocol`) exposed as `ws.protocol`, `bufferedAmount` semantics, `CloseEvent.wasClean`.

### Server-Sent Events — implemented, blocking reconnect

- **Parser: complete and well-tested.** `crates/network/src/sse.rs:36` `SseParser` (HTML LS §9.2.6); 25+ unit tests at `sse.rs:416`. Handles LF/CR/CRLF, multiline `data`, `id` persistence, `retry`, comments.
- **Client: complete, blocking.** `crates/network/src/sse.rs:190` `EventSource` implements `SseSession` (`crates/core/src/ext.rs:1411`). Auto-reconnect with `Last-Event-ID` header (`sse.rs:234`). **Limitation:** reconnect uses `std::thread::sleep(retry_ms)` at `sse.rs:387` on the worker thread, and `next_event` (`sse.rs:348`) is a blocking loop.
- **Provider + JS: complete.** `crates/network/src/lib.rs:3440` `impl JsSseProvider for HttpClient`; `connect_sse` at `lib.rs:3441`. JS `EventSource` class at `crates/js/src/dom.rs:6257`, polled via `_lumen_pump_sse` (`dom.rs:6176`, "Mirrors the WebSocket polling model").

**Gap to close:** non-blocking reconnect scheduling integrated with the shell timer/event loop; delivery via the same async path as WS.

### Fetch + AbortController — implemented, synchronous + pre-flight abort only

- **JS `fetch()`: complete but synchronous.** `crates/js/src/dom.rs:7328`. Supports GET/POST, string/FormData/ArrayBuffer/Uint8Array bodies, header override, SRI integrity (`dom.rs:7410`), priority hints + keepalive (parsed, not yet wired — see comments `dom.rs:7346`, `dom.rs:7351`). Backed by **real** `HttpClient`, not a stub.
- **Provider: complete.** `crates/network/src/lib.rs:3109` `impl JsFetchProvider for HttpClient` → `fetch_sync` at `lib.rs:3110`. Bridge funcs `_lumen_fetch_sync` (`dom.rs:1149`) and `_lumen_fetch_sync_with_body` (`dom.rs:1253`); lazy body chunking `_lumen_fetch_body_chunk` (`dom.rs:1223`).
- **Request/Response/Headers: complete.** `Request` at `dom.rs:7062` (reads `init.signal`); `Response._fromFetchCache` lazy-body bridge (`dom.rs:1283`); WHATWG Streams `ReadableStream`/`response.body` at `dom.rs:6448` (synchronous fill model — all chunks enqueued at construction).
- **AbortController/AbortSignal: complete API surface.** `dom.rs:6366`. `abort(reason)`, `throwIfAborted`, `AbortSignal.abort/.timeout/.any` all implemented; `addEventListener('abort')` fires. **Limitation (in-code, `dom.rs:7330`):** *"Lumen's fetch is synchronous, so this pre-flight check is the only cancellation point (no in-flight abort in Phase 0)."* `fetch()` only checks `signal.aborted` once before issuing the call (`dom.rs:7335`); a signal that aborts *during* the request has no effect.

**Gap to close:** real in-flight cancellation (abort signal → cancel the underlying socket/HttpClient request), and ideally async fetch so a long request does not block the JS thread.

---

## Architecture

### WebSocket
- **Handshake + frame codec:** already in `lumen-network::websocket` (keep). HTTP `Upgrade: websocket` + `Sec-WebSocket-Key`/`-Accept` SHA-1; frame masking/fragmentation/control frames.
- **JS binding:** replace the poll model with event-loop push. The background recv thread should enqueue into a shared mailbox that the JS event loop drains on each microtask/timer tick via a single dispatch hook, instead of JS calling `_lumen_ws_poll` per handle.

### SSE
- **`text/event-stream` parser:** keep `SseParser` as-is (spec-correct, tested).
- **`EventSource` binding + reconnect:** move the blocking `sleep`-reconnect off the worker thread into a timer-scheduled reconnect so close/GC of the `EventSource` is observed promptly; deliver events through the WS-shared async dispatch path.

### Fetch
- **Bind to HttpClient:** already done (`fetch_sync`). Add a cancellable variant: thread an abort token from the JS `AbortSignal` down to `HttpClient` so an in-flight request can be torn down.
- **Request/Response/Headers:** present; verify `Headers` guard semantics and `Request(init.signal)` propagation into `fetch()` (today `fetch()` re-reads `input.signal`/`init.signal` at `dom.rs:7333`).
- **AbortController → cancel:** register the abort listener at fetch time; on `abort`, cancel the token and reject the pending promise with `signal.reason`.

---

## Entry points (real file:line; *(proposed)* = to add)

| Concern | Location |
|---|---|
| WS session impl | `crates/network/src/websocket/mod.rs:48` |
| WS handshake | `crates/network/src/websocket/upgrade.rs` |
| WS frame codec | `crates/network/src/websocket/frame.rs` |
| WS provider | `crates/network/src/lib.rs:3341` (`connect` @3342) |
| WS JS binding | `crates/js/src/dom.rs:1395`; class @`dom.rs:7427` |
| WS event-loop dispatch hook | *(proposed)* replace `_lumen_ws_poll` pump with push from recv thread |
| SSE parser | `crates/network/src/sse.rs:36` |
| SSE client / reconnect | `crates/network/src/sse.rs:190` (sleep @`sse.rs:387`) |
| SSE provider | `crates/network/src/lib.rs:3440` (`connect_sse` @3441) |
| SSE JS binding | `crates/js/src/dom.rs:6257`; pump @`dom.rs:6176` |
| Fetch provider | `crates/network/src/lib.rs:3109` (`fetch_sync` @3110) |
| Cancellable fetch provider | *(proposed)* `fn fetch_cancellable(&self, …, token: AbortToken)` on `JsFetchProvider` (`crates/core/src/ext.rs:1542`) |
| JS `fetch()` | `crates/js/src/dom.rs:7328` (pre-flight abort @`dom.rs:7335`) |
| AbortController/Signal | `crates/js/src/dom.rs:6366` |
| Shell provider install | `crates/shell/src/main.rs:3732`; hibernate restore `crates/shell/src/tab_lifecycle/hibernate.rs:98` |
| Abort token type | *(proposed)* `lumen-core::ext::AbortToken` (`Arc<AtomicBool>` + waker) |

---

## Steps (per feature, phased)

### Phase A — Fetch in-flight abort (smallest, highest value)
1. Add `AbortToken` to `lumen-core::ext` *(proposed)*: a clonable cancel flag the network layer polls between socket reads.
2. Add `fetch_cancellable` to `JsFetchProvider` (default impl delegates to `fetch_sync` ignoring the token) so the trait stays back-compatible; implement it on `HttpClient` to check the token in the read loop and tear down the connection.
3. In JS `fetch()` (`dom.rs:7328`): create a token, register an `abort` listener on the signal that flips it, pass the token id through a new `_lumen_fetch_cancellable` bridge; reject with `signal.reason` on abort.
4. Tests via `MockTransport`: abort before send → `AbortError`; abort mid-stream → `AbortError`, no body delivered.

### Phase B — WebSocket async delivery
1. Introduce a single per-runtime WS mailbox drained by the event loop; the recv thread pushes events instead of JS calling `_lumen_ws_poll`.
2. Surface `Sec-WebSocket-Protocol` selection → `ws.protocol`; populate `CloseEvent.wasClean`.
3. Verify close handshake state machine (CONNECTING/OPEN/CLOSING/CLOSED) matches RFC 6455 §7.
4. Tests via `MockTransport`: handshake, text/binary echo, fragmented message, server-initiated close, ping/pong, permessage-deflate round-trip.

### Phase C — SSE non-blocking reconnect
1. Replace `std::thread::sleep` reconnect (`sse.rs:387`) with timer-scheduled reconnect coordinated with the shell event loop; ensure `close()` interrupts a pending reconnect promptly.
2. Route SSE events through the same async dispatch path built in Phase B.
3. Tests via `MockTransport`: event parse + dispatch, `Last-Event-ID` resent on reconnect, `retry:` honoured, server close → reconnect, `close()` stops the loop.

### Phase D — capability + doc sync
1. Update `CAPABILITIES.md:120` / `:140` lines: drop the "poll model"/"synchronous fetch" caveats once async + abort land.
2. Update `subsystems/network.md` and `subsystems/js.md` Done sections.
3. Regenerate `SYMBOLS.md` for any new public API.

---

## Dependencies

- **No new crate for the WS frame codec.** It is already hand-rolled in `lumen-network::websocket` (frame/mask/deflate). Do **not** pull in `tungstenite`/`tokio-tungstenite` — it would drag a `tokio` runtime into a sync-threaded engine and duplicate a working, tested codec. The hand-rolled codec is the correct baseline; extend it.
- **SSE parser:** hand-rolled, tested — no dependency.
- **permessage-deflate:** uses the existing deflate dependency already in `Cargo.toml` (see `crates/network/src/websocket/deflate.rs`); no new dep.
- **AbortToken:** plain `std` (`Arc<AtomicBool>`, optionally a `Condvar`/waker). No dependency.
- If any new dependency is proposed, the commit body must carry the **"Why this dependency"** justification (permanent/provisional, trait-anchor, graduation criterion) per `CLAUDE.md` dependency policy.

---

## Tests (MockTransport)

Use `MockTransport` (`completed_mock_http_client.md`; `lumen-network`) so no real network is required.

- **Fetch:** GET/POST round-trip; pre-flight abort → `AbortError`; in-flight abort → `AbortError` with no body; `AbortSignal.timeout` → `TimeoutError`; SRI mismatch → `TypeError`.
- **WebSocket:** handshake success/failure; text + binary echo; fragmented (continuation) reassembly; ping→pong auto-reply; server Close echo + `CloseEvent`; permessage-deflate compressed round-trip; sub-protocol negotiation surfaced.
- **SSE:** single + multiple events per chunk; multiline `data`; `event:`/`id:`/`retry:` fields; `Last-Event-ID` resent on reconnect; server EOF → reconnect; `close()` stops reconnect promptly (no thread leak).
- Keep the existing `sse.rs` unit-test block (`sse.rs:416`) green; extend it rather than replace.
- Per-crate gate before commit: `cargo clippy -p <crate> --all-targets -- -D warnings` then `cargo test -p <crate>`.

---

## Definition of done

- [ ] **Fetch:** an `AbortController.abort()` fired *during* an in-flight request cancels it and rejects with `signal.reason`; `MockTransport` test proves mid-stream cancellation (no body delivered).
- [ ] **WebSocket:** messages delivered to JS via the event loop without JS polling; sub-protocol exposed as `ws.protocol`; `CloseEvent.wasClean` correct; full `MockTransport` protocol suite green.
- [ ] **SSE:** reconnect is non-blocking and `close()` interrupts a pending reconnect within one tick; `Last-Event-ID` round-trip verified.
- [ ] No new dependency added without the `CLAUDE.md` justification (WS codec stays hand-rolled).
- [ ] `cargo clippy -p lumen-network -p lumen-js -p lumen-shell --all-targets -- -D warnings` clean; crate tests pass.
- [ ] Docs synced: `CAPABILITIES.md` caveats removed, `subsystems/network.md` + `subsystems/js.md` updated, `SYMBOLS.md` regenerated, `docs/plan/phases.md:121` marked done.
- [ ] `///` doc comments on every new public type/fn (per `CLAUDE.md`).
