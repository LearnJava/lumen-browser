# ADR-011: Provisional crypto deps — hmac + aes-gcm (SubtleCrypto API)

## Status

Accepted

## Date

2026-06-03

## Context

The W3C WebCryptography API (`SubtleCrypto`) requires HMAC (SHA-256/384/512
signing/verification) and AES-GCM (128/256-bit encrypt/decrypt with AAD) as
mandatory algorithms (WebCryptography §14). These algorithms are used in
`crates/js/src/subtle_crypto.rs` to implement `crypto.subtle.sign`,
`crypto.subtle.verify`, `crypto.subtle.encrypt`, `crypto.subtle.decrypt`.

Phase 0 needs working SubtleCrypto so that pages relying on JWT tokens, FIDO2
authentication flows, and encrypted storage do not throw `NotSupportedError`.

`p256` (ECDSA P-256) was already a permanent dependency in `lumen-network`
(used for TLS, WebAuthn). `sha2` is a transitive dep of `p256`. Only
`hmac` and `aes-gcm` are new additions.

## Decision

Add `hmac = "0.12"` and `aes-gcm = "0.10"` to `crates/js/Cargo.toml`
as **Provisional** dependencies, trait-anchored to the `SubtleCrypto` JS API
surface (`crates/js/src/subtle_crypto.rs`).

Category: **Provisional** (ADR-002 §3.2).

Trait-anchor: `SubtleCrypto` — `window.crypto.subtle` object in JS runtime.

Graduation criterion: when Phase 1 ships the full Web Crypto API to end-users
OR when the project switches to an own pure-Rust HMAC/AES-GCM implementation.
Until then, these crates remain provisional and must not be exposed at the
`lumen-core` or `lumen-network` level.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| `ring` crate | Not pure Rust; requires C toolchain; conflicts with existing `p256`/`sha2` RustCrypto chain |
| `openssl` crate | C FFI dependency; too heavy; breaks cross-compilation story |
| Pure-Rust own impl | HMAC is trivial but AES-GCM (GHASH, CTR) is non-trivial; crypto must be audited; deferred to Phase 2 |
| Feature-gate behind `js-crypto` | Would complicate build matrix for minimal value in Phase 0 |

## Consequences

- **Positive:** SubtleCrypto tests pass; pages using `crypto.subtle` no longer throw.
- **Positive:** Uses the RustCrypto ecosystem consistently (`hmac` + `aes-gcm` share
  the same `digest`/`cipher` traits as `sha2`/`p256` already in use).
- **Negative:** Two new transitive deps added to `crates/js` dep graph.
- **Future:** Graduate to permanent when Phase 1 ships, or replace with
  `lumen-crypto` crate once own HMAC/AES-GCM implementation is audited.
