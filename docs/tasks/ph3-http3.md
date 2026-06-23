# Ph3 — HTTP/3 (QUIC)

**Developer:** P1
**Branch:** `p1-ph3-http3`
**Size:** L
**Crates:** `lumen-network`
**Phase:** 3 (v1.0 target)

---

## Status

Not started. Placeholder task for Phase 3 planning.

---

## Goal

Add HTTP/3 (QUIC) transport to `lumen-network` so the browser can fetch resources
over HTTP/3 when servers advertise it. The implementation slots in behind the
existing `NetworkTransport` / `HttpClient` abstraction and is invisible to all
callers above the network layer.

Spec references:
- RFC 9000 — QUIC transport
- RFC 9001 — TLS 1.3 on QUIC
- RFC 9114 — HTTP/3 (wire format, QPACK, pseudo-headers)
- RFC 9204 — QPACK header compression
- RFC 7838 — Alt-Svc (HTTP Alternative Services, the h3 upgrade path)

---

## Current state

### Transport / protocol support

`lumen-network` currently supports:

| Protocol | Transport | Status |
|---|---|---|
| HTTP/1.1 | TCP plain / TCP+TLS | Fully implemented (`lib.rs`, `Connection`, `do_request`) |
| HTTP/2 | TCP+TLS (ALPN `h2`) | Fully implemented (`h2/conn.rs`, `H2Conn`, `H2Pool`) |
| HTTP/3 | QUIC (UDP) | Not present |

Dispatch path in `fetch_single` (real entry point, `lib.rs`):
- **Line 1067** (`lib.rs`): `check_negotiated_alpn(conn.alpn_protocol())` reads the rustls ALPN result.
- **Line 1317** (`lib.rs`): `if conn.is_h2 { ... return h2_do_request(...) }` — existing H2 branch.
- **Line 1330** (`lib.rs`): fall-through to `do_request` for HTTP/1.1.

No `h3` branch exists. The ALPN check at `lib.rs:1109` explicitly rejects `b"h3"`:
```rust
// lib.rs:3599-3601
fn check_alpn_rejects_unknown_proto() {
    let err = check_negotiated_alpn(Some(b"h3")).unwrap_err();
    assert!(format!("{err:?}").contains("unexpected ALPN"));
}
```

### TLS layer

`crates/network/src/tls/mod.rs` — `build_client_config()` configures `rustls::ClientConfig`.
- ALPN for Standard/Strict profiles: `["h2", "http/1.1"]` (line 131–133).
- **Proposed:** add `"h3"` as the first ALPN value when h3 is enabled (before `h2`).
- **Note:** QUIC does NOT use rustls directly for transport; quinn bundles its own QUIC-level
  TLS 1.3 handshake. The existing `build_client_config` result is reused as the
  `rustls::ClientConfig` input to `quinn::ClientConfig::new(Arc<rustls::ClientConfig>)`.

### Transport trait (seam for h3)

`crates/core/src/ext.rs:19` — `NetworkTransport::fetch(&self, url: &Url) -> Result<Vec<u8>>`.

This trait is the topmost seam. `HttpClient` itself is a concrete struct, not a trait
implementation, so h3 is wired *inside* `HttpClient`, not as a separate `NetworkTransport`
implementor. The analogous seam at the lower level is `H2Pool`/`H2Conn` — h3 adds a
parallel `H3Pool`/`H3Conn` in a new `crates/network/src/h3/` module.

### Alt-Svc / protocol negotiation

No Alt-Svc parsing exists anywhere in the codebase (confirmed by grep — zero hits for
`alt-svc` or `Alt-Svc` in response-header parsing). The `Response` struct at `lib.rs`
carries raw headers as `Vec<(String, String)>` but nothing reads `alt-svc` from them.

### Pooling / reuse

- HTTP/1.1 pool: `crates/network/src/pool.rs` — `ConnectionPool`, keyed by `PoolKey { host, port, is_tls }`.
- HTTP/2 pool: `crates/network/src/h2/pool.rs` — `H2Pool`, same key type, one conn per origin.
- **Proposed:** `H3Pool` in `crates/network/src/h3/pool.rs` — one `quinn::Connection` per origin,
  with QUIC 0-RTT data for repeat connections (Phase 3+ optional).

---

## Architecture

### quinn + h3 behind HttpClient

```
HttpClient::fetch_single()
  │
  ├─ Alt-Svc cache hit for origin → h3_do_request()
  │    └─ H3Pool::acquire(key)
  │         ├─ Some(conn) → reuse QUIC connection, open new stream
  │         └─ None → quinn::Endpoint::connect(addr, server_name)
  │                    ↳ h3::client::SendRequest (h3 crate)
  │                    ↳ H3Pool::release(key, conn)
  │
  ├─ No Alt-Svc entry → existing H2 / H1.1 path (unchanged)
  │    └─ Response headers parsed for Alt-Svc → alt_svc_cache.insert(origin, h3)
  │
  └─ Alt-Svc present but QUIC connect fails → fall back to H2/H1.1
```

### ALPN for QUIC

QUIC's TLS handshake advertises `h3` as the ALPN token. This is handled by the quinn
`ClientConfig` builder — it accepts a `rustls::ClientConfig` and sets ALPN to `["h3"]`
internally. The existing `build_client_config` in `tls/mod.rs` is reused with a new
`TlsProfile::H3` variant (or a thin wrapper) that sets `alpn_protocols = ["h3"]` for
the rustls config passed into quinn.

### Alt-Svc → upgrade flow

1. Initial request to `https://example.com` goes over H2 (or H1.1) — unchanged.
2. Response headers contain `Alt-Svc: h3=":443"; ma=86400` (RFC 7838 §3).
3. `fetch_single` parses the header, validates the `h3` token, stores
   `(origin, port, max_age_secs)` in `AltSvcCache` (in-process `HashMap`, persisted to
   disk as a follow-up).
4. Next request to the same origin finds the cache entry and tries H3 first.
5. If QUIC connection fails (UDP blocked, firewall), clear the cache entry and retry
   over the existing H2/H1.1 path (RFC 7838 §2.4 "broken").

### Request / response mapping (RFC 9114)

H3 uses QPACK (RFC 9204) for header compression — analogous to HPACK in H2. The `h3`
crate (`sfackler/h3` / `hyperium/h3`) exposes `h3::client::SendRequest` which handles
QPACK encoding internally. The caller provides headers as `(name, value)` byte slices —
identical API surface to the existing `H2Conn::fetch` call at `lib.rs:1374`.

Response mapping mirrors `h2_do_request` at `lib.rs:1349-1382`:
- Read `:status` pseudo-header → `Response.status`.
- Strip pseudo-headers from `Response.headers`.
- Return `Response { status, headers, body }`.

### Pooling

`H3Pool` stores one `quinn::Connection` per `PoolKey`. QUIC connections are multiplexed
(like H2) but the transport is UDP — no `is_stale_error` needed in the TCP sense; quinn
connection errors surface as `quinn::ConnectionError`. Idle timeout: `max-age` from
Alt-Svc (default 86400 s) caps the connection lifetime; quinn's built-in idle timeout
(configurable, default 30 s) handles network-level keepalive.

---

## Entry points

The following are the real integration points — locations where HTTP/3 support slots in.
All are marked **[PROPOSED]** (nothing is implemented yet).

| Location | Purpose | Change needed |
|---|---|---|
| `crates/network/src/lib.rs:1067` | ALPN check after TLS handshake | **[PROPOSED]** Do not call `check_negotiated_alpn` on the QUIC path; h3 bypasses rustls ALPN entirely |
| `crates/network/src/lib.rs:1317` | H2 dispatch branch | **[PROPOSED]** Add `h3` branch before: `if let Some(h3_conn) = h3_pool.acquire(&key) { ... }` |
| `crates/network/src/lib.rs:2113-2149` | `HttpClient` struct fields | **[PROPOSED]** Add `h3_pool: Option<Arc<H3Pool>>`, `alt_svc_cache: Arc<AltSvcCache>` |
| `crates/network/src/tls/mod.rs:64` | `build_client_config()` | **[PROPOSED]** Add `TlsProfile::H3` variant that sets `alpn_protocols = [b"h3"]` (for quinn rustls config) |
| `crates/network/src/lib.rs:1109` | `check_negotiated_alpn()` | **[PROPOSED]** Remove or relax the `b"h3"` rejection (h3 won't go through this path anyway) |
| `crates/network/src/h3/` | New module | **[PROPOSED]** New: `mod.rs`, `conn.rs` (quinn/h3 wrapper), `pool.rs` (H3Pool), `alt_svc.rs` (AltSvcCache + parser) |

---

## Steps

### 1. Alt-Svc parser (`src/h3/alt_svc.rs`)

Implement a minimal RFC 7838 §3 parser for the `Alt-Svc` response header:
- Input: raw header value string, e.g. `h3=":443"; ma=86400, h3-29=":443"`
- Output: `Vec<AltSvcEntry { protocol: String, host: Option<String>, port: u16, max_age: u64 }`
- Filter for `protocol == "h3"` only (ignore `h3-29`, `h2`).
- `AltSvcCache`: `HashMap<String /* origin */, (AltSvcEntry, Instant /* inserted_at */)>` with
  TTL expiry check on lookup.

### 2. quinn + h3 dependencies (`Cargo.toml`)

Add to `crates/network/Cargo.toml`:
```toml
# Phase 3 (HTTP/3): QUIC transport + HTTP/3 protocol layer.
# quinn: pure-Rust QUIC implementation (RFC 9000), uses rustls for TLS 1.3.
# h3: HTTP/3 client over quinn. Both are provisional; graduation criterion:
# stable quinn/h3 1.x API with no breaking changes for 12 months.
# Justify: no alternative pure-Rust QUIC exists; reqwest/hyper h3 require
# async runtime (tokio) which conflicts with lumen-network's sync I/O model.
quinn = { version = "0.11", features = ["rustls"] }
h3 = "0.0.6"
h3-quinn = "0.0.7"
```

**Dependency justification:**
- `quinn`: the only production-quality pure-Rust QUIC stack. Hyper/reqwest h3 require
  tokio async; lumen-network uses synchronous blocking I/O. quinn exposes a synchronous
  `connect`+`open_bi`/`open_uni` API sufficient for a single-threaded browser.
- `h3` + `h3-quinn`: HTTP/3 framing over quinn. Eliminates implementing QPACK (RFC 9204)
  from scratch — comparable justification to why `H2Conn` implements HPACK itself (no
  external dep), but QPACK is significantly more complex (dynamic table references from
  the encoder stream require cross-stream coordination).
- Both are `provisional` per §5 dependency policy. Graduation criterion: h3 crate
  reaches 1.0 with stable API.

### 3. QUIC connection wrapper (`src/h3/conn.rs`)

`H3Conn` wraps `quinn::Connection` + `h3::client::Connection`:
```rust
pub struct H3Conn {
    /// Underlying QUIC connection (also owns the TLS session).
    quic: quinn::Connection,
    /// HTTP/3 framing driver (QPACK, control streams, SETTINGS).
    h3: h3::client::Connection<h3_quinn::BidiStream<Bytes>, Bytes>,
}
```

`H3Conn::connect(host: &str, port: u16, tls_cfg: Arc<rustls::ClientConfig>) -> Result<Self>`:
- Create `quinn::Endpoint::client` bound to `0.0.0.0:0`.
- Resolve `host:port` via `DnsResolver` (pass resolver in, same as `connect()` in `lib.rs:1010`).
- `endpoint.connect(addr, host)?` → `quinn::Connection`.
- Wrap in `h3_quinn::Connection`, then `h3::client::builder().build(quic_conn)`.

`H3Conn::fetch(method, scheme, authority, path, extra_headers) -> Result<H3Response>`:
- Open a new request stream via `h3::client::Connection::send_request`.
- Encode pseudo-headers + extra headers as `http::request::Parts`.
- Read response: `recv_response()` → status + headers; `recv_data()` loop → body.
- Return `H3Response = (u16, Vec<(String,String)>, Vec<u8>)` — identical shape to `H2Response`.

### 4. H3 pool (`src/h3/pool.rs`)

`H3Pool`: identical interface to `H2Pool` (`acquire`, `release`, `evict`) but stores
`H3Conn` keyed by `PoolKey`. One connection per origin (RFC 9114 §3.3 recommendation).
TTL eviction based on `AltSvcEntry.max_age`.

### 5. Wire into `HttpClient` (`src/lib.rs`)

- Add `h3_pool: Option<Arc<H3Pool>>` and `alt_svc_cache: Arc<AltSvcCache>` fields to
  `HttpClient` (line 2113).
- In `fetch_single` (after redirect resolution, before pool acquire):
  1. Check `alt_svc_cache` for the target origin.
  2. If h3 entry exists and is fresh: call `h3_do_request()`, on error fall back.
  3. After a successful H2/H1.1 response, scan response headers for `Alt-Svc: h3=...`
     and populate `alt_svc_cache`.
- Add `HttpClient::with_h3(bool)` builder method (default: off during Phase 3 development,
  opt-in to allow gradual rollout).

### 6. `h3_do_request` function (`src/lib.rs` or `src/h3/mod.rs`)

Mirrors `h2_do_request` at `lib.rs:1352-1382`. Acquires/creates `H3Conn`, calls
`H3Conn::fetch`, releases conn to `H3Pool`, returns `Response`.

### 7. 0-RTT (optional, post-Phase 3)

QUIC 0-RTT session resumption (RFC 9000 §7.3.1) reduces latency on repeat connections.
`quinn::Endpoint` supports 0-RTT via saved `quinn::ClientConfig` session tickets.
Implement as a separate follow-up task once the basic H3 path is stable.

---

## Dependencies

| Crate | Version | Category | Justification |
|---|---|---|---|
| `quinn` | 0.11 | provisional | Only pure-Rust QUIC stack; no tokio required for sync usage |
| `h3` | 0.0.6 | provisional | HTTP/3 framing + QPACK; implementing QPACK from scratch is ~2k lines |
| `h3-quinn` | 0.0.7 | provisional | quinn adapter for h3; required by h3 crate's transport abstraction |

These are in addition to the existing rustls + webpki-roots already in `Cargo.toml`.
No new crypto dependency: quinn reuses the rustls `ClientConfig` built by
`tls::build_client_config` — same root store, same cipher configuration.

---

## Tests

### Unit tests (no network required)

- `alt_svc.rs`: parser round-trip for valid and invalid `Alt-Svc` header values;
  TTL expiry; `h3-29` entries filtered out; `clear` token handling (RFC 7838 §3).
- `h3/pool.rs`: acquire/release/evict; TTL eviction.

### Integration tests (real QUIC, test server)

- `tests/h3_integration.rs` — spin up a local `quinn`-based H3 test server (analogous
  to how the HTTP/1.1 tests in `lib.rs::tests` use `std::net::TcpListener`). Test:
  - Successful GET over H3 returning 200 + body.
  - Alt-Svc header causes subsequent request to use H3.
  - H3 failure falls back to H2 (simulate by closing QUIC endpoint before second request).
  - Pool reuse: second request to same origin reuses `quinn::Connection`.

### Regression

`check_negotiated_alpn` test at `lib.rs:3599` that currently asserts `b"h3"` returns an
error should be updated to reflect the new dispatch logic (h3 bypasses that function).

---

## Definition of done

- [ ] `Alt-Svc: h3=...` header is parsed and stored in `AltSvcCache` after any H2/H1.1 response.
- [ ] Second request to an origin that advertised h3 is dispatched over QUIC.
- [ ] `H3Conn::fetch` returns `Response` with correct status, headers, and body.
- [ ] `H3Pool` reuses the QUIC connection for sequential requests to the same origin.
- [ ] QUIC connect failure falls back transparently to H2/H1.1 (no error surfaced to caller).
- [ ] `HttpClient::with_h3(false)` (default) disables the entire h3 path; `with_h3(true)` enables it.
- [ ] All existing HTTP/1.1 and HTTP/2 tests pass unchanged.
- [ ] Unit tests for Alt-Svc parser pass.
- [ ] At least one integration test exercises a real QUIC round-trip against a local test server.
- [ ] `cargo clippy -p lumen-network --all-targets -- -D warnings` clean.
- [ ] `CAPABILITIES.md` updated: HTTP/3 row `⬜ → ✅`.
- [ ] `docs/plan/tech-stack.md` updated: quinn/h3/h3-quinn added to provisional table.
