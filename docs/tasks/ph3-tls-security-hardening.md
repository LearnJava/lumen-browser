# Ph3 — TLS/security hardening (OCSP+CT+cert UI, Negotiate/NTLM+mTLS)

**Developer:** P3 · **Branch:** `p3-ph3-tls-hardening` · **Size:** L · **Crates:** `lumen-network`, `lumen-shell` (cert UI)

> All file:line references below were captured against `main` at task-authoring
> time and *will drift*. Re-grep the symbol name before editing. Anything marked
> **(proposed)** does not exist yet — it is a design suggestion, not a real
> location.

---

## Status

Phase 3 (v1.0) future work — see `docs/plan/phases.md:122-124`.

Already DONE in earlier phases, **do not re-implement**:

- **HTTP auth Basic + Digest** (`phases.md:122`, marked `готово`) — RFC 7617/7616
  in `crates/network/src/auth.rs`; 401-retry loop in
  `crates/network/src/lib.rs:2000-2034`.
- **Safe Browsing equivalent** (`phases.md:124`, marked `готово`) —
  `SafeBrowsingList` / `SafeBrowsingFilter`. **Out of scope for this task.**

This task picks up the two REMAINING / DEFERRED pieces from `phases.md:122-123`:

1. OCSP stapling + CT log enforcement + invalid-cert UI (`phases.md:123`).
2. Negotiate/NTLM auth + client certificates (mTLS) — explicitly deferred at
   `phases.md:122` ("Negotiate/NTLM + client certificates (mTLS) — отложены").

---

## Goal

Bring Lumen's TLS trust + auth surface up to a v1.0 baseline:

- The engine **observes revocation and transparency signals** (OCSP stapling,
  SCT/CT) during the handshake and **surfaces verification failures to the user**
  through a real interstitial / cert UI rather than an opaque `Error::Network`.
- The cert panel shows **real** certificate data (issuer chain, validity,
  revocation status, CT status) instead of the current `stub_for` placeholder.
- The auth stack negotiates **Negotiate/NTLM** challenges and can present a
  **client certificate (mTLS)** when a server requests one.

Scope boundary: this is a *trust + auth* task, not a new-transport task. It does
not touch HTTP/3, WebSockets, or the proxy code paths beyond what cert
verification requires.

---

## Current state

### Cert verification (today)

- TLS is rustls 0.23 (`crates/network/Cargo.toml:19`), aws-lc-rs provider.
- `ClientConfig` is built per-profile in
  `crates/network/src/tls/mod.rs:64` `build_client_config()`. It ends with
  `.with_root_certificates(root_store).with_no_client_auth()`
  (`tls/mod.rs:127-128`) — **default webpki verifier, no client auth**.
- Root store = `webpki_roots::TLS_SERVER_ROOTS`, cached per-profile in
  `crates/network/src/lib.rs:1077` `tls_config_for_profile()` (build at
  `lib.rs:1084-1090`).
- Handshake completes at `crates/network/src/lib.rs:1065` (`conn.complete_io`).
  A verification failure today bubbles up as
  `Error::Network(format!("TLS handshake: {e}"))` (`lib.rs:1060`, `lib.rs:1066`)
  — a flat string, with no structured "this is a cert-trust problem" signal.
- The TLS ClientHello already *advertises* `status_request` (OCSP stapling,
  `EXT_STATUS_REQUEST = 5`, `crates/network/src/tls/fingerprint.rs:69`) and SCT
  (`signed_certificate_timestamp`, code 18, `fingerprint.rs:72`) — but only for
  **fingerprint matching**. The stapled OCSP response and SCTs are **never
  consumed or validated**.

### CertInfo (today)

- `CertInfo` struct: `crates/network/src/tls/fingerprint.rs:116`. Fields:
  subject/issuer CN+Org, not_before/not_after, `fingerprint_sha256`, `san_list`,
  `tls_version` (`fingerprint.rs:118-135`).
- **It is only ever built as a stub.** The single constructor in use is
  `CertInfo::stub_for(host, tls_version)` (`fingerprint.rs:147`), which fills
  `issuer_cn = "(unavailable in Phase 0)"` and leaves fingerprint/validity empty.
  A grep for any real population shows none — there is no peer-certificate
  extraction path.

### Cert UI (today)

- `crates/shell/src/panels/cert_panel.rs` — `CertPanel` overlay (500×440),
  toggled `Ctrl+Shift+C` (`cert_panel.rs:1-3`).
- `PanelCertData` (`cert_panel.rs:55`) mirrors `CertInfo`; shell copies fields on
  open. Rows built in `build_rows()` (`cert_panel.rs:161`): Subject/Issuer/Valid
  From/Until/TLS Version/SANs/SHA-256.
- It is a **passive viewer only** — `None` → "No certificate information (HTTP or
  unavailable)" (`cert_panel.rs:319-327`). There is **no error/warning state, no
  "proceed anyway" interstitial, no revocation/CT row**.

### Auth seam (today)

- `crates/network/src/auth.rs` — parser `parse_www_authenticate()`
  (`auth.rs:79`), selector `select_best_challenge()` (`auth.rs:262`).
- Unknown schemes (`Negotiate`, `NTLM`, `Bearer`) are **parsed but dropped**:
  `select_best_challenge` only matches `digest`/`basic` (`auth.rs:266-280`),
  returning `None` for anything else (see test
  `select_returns_none_for_unsupported_only`, `auth.rs:853`).
- `HttpAuthScheme` enum has only `Basic`, `Digest`
  (`crates/core/src/ext.rs:256`).
- 401-retry loop: `crates/network/src/lib.rs:2000-2034`. It is a **single-shot**
  retry (one `Authorization` header, then re-fetch the same hop). Negotiate/NTLM
  need **multi-round** (the server replies `401 WWW-Authenticate: Negotiate
  <token>` to each step).
- Credentials come from `HttpCredentialProvider`
  (`crates/core/src/ext.rs:324`); `HttpCredentials` = username/password only
  (`ext.rs:301`) — no concept of a client cert/key or a domain (NTLM needs
  domain\\user).

### Client cert / mTLS (today)

- `build_client_config()` hardcodes `.with_no_client_auth()`
  (`crates/network/src/tls/mod.rs:128`). There is **no `ResolvesClientCert`,
  no client-cert store, no path to present one**. A server's
  `CertificateRequest` is unanswered.

---

## Part A — OCSP stapling + CT enforcement + invalid-cert UI

The trust pipeline. Replace the default webpki verifier with a wrapping verifier
that (a) keeps webpki's chain validation, (b) layers revocation + transparency
checks on top, and (c) produces a *structured* verdict the shell can render.

### A1 — Structured cert-verification result

- Add a `CertVerdict` / `CertError` enum **(proposed)** in
  `crates/network/src/tls/` capturing: `Ok`, `Untrusted(chain)`, `Expired`,
  `Revoked(ocsp)`, `CtInsufficient`, `NameMismatch`, `SelfSigned`. Carry enough
  data to populate the UI (issuer, reason string).
- Thread it out of the handshake: today `lib.rs:1060` collapses to
  `Error::Network`. Introduce a `Error::CertInvalid(CertError)` variant
  **(proposed)** so the shell can distinguish trust failures from transport
  failures and show an interstitial instead of a generic error page.

### A2 — Custom `ServerCertVerifier`

- In `build_client_config()` (`tls/mod.rs:64`), replace
  `.with_root_certificates(root_store).with_no_client_auth()` with
  `.dangerous().with_custom_certificate_verifier(Arc::new(LumenVerifier{..}))`
  **(proposed)**. `LumenVerifier` wraps `rustls::client::WebPkiServerVerifier`
  (built from the same `root_store`) and delegates chain validation to it first.
- On `verify_server_cert`, after webpki passes, run A3 (OCSP) + A4 (CT) and
  record the verdict (A1) on a shared slot the fetch path can read back
  (e.g. an `Arc<Mutex<Option<CertVerdict>>>` keyed per-connection, or returned
  via the populated `CertInfo`).

### A3 — OCSP stapling consumption

- rustls surfaces the stapled response via the `OcspResponse` passed to
  `verify_server_cert` (rustls 0.23: the `ocsp_response: &[u8]` argument). Parse
  the DER OCSP response (RFC 6960): check `responseStatus`, the `certStatus`
  (`good`/`revoked`/`unknown`), and `thisUpdate`/`nextUpdate` freshness.
- Policy (**proposed, soft-fail like Chrome**): `revoked` → hard fail
  (`CertError::Revoked`); `good` → record into `CertInfo`; missing/`unknown`/
  stale → record "no revocation info" but do **not** block (no live OCSP fetch
  in this task — stapled-only, to avoid the privacy + latency cost of OCSP
  fetching).

### A4 — CT (Certificate Transparency) enforcement

- Collect SCTs from the three transports: the TLS extension
  (`signed_certificate_timestamp`, code 18 — already advertised at
  `fingerprint.rs:72`), the stapled OCSP response, and the cert's SCT-list
  extension (embedded SCTs).
- Validate each SCT signature against a bundled CT-log list **(proposed:
  `crates/network/src/tls/ct_logs.rs`, a static list of known-log public keys,
  graduation-tracked like webpki-roots)**. Enforce a Chrome-style policy: ≥2 SCTs
  from distinct logs → `Ok`; otherwise `CtInsufficient` (record, soft-fail
  initially; gate behind `TlsProfile::Strict` for hard-fail).

### A5 — Populate real `CertInfo`

- In the verifier, parse the leaf `CertificateDer` (subject/issuer CN+Org, SAN,
  validity, SHA-256 fingerprint) and fill `CertInfo` (`fingerprint.rs:116`) for
  real — retiring `stub_for` (`fingerprint.rs:147`) from the live path (keep it
  for tests). Add **(proposed)** `revocation_status: String` and `ct_status:
  String` fields to `CertInfo` so the panel can show them.

### A6 — Invalid-cert UI (shell)

- Extend `PanelCertData` (`cert_panel.rs:55`) with `revocation`, `ct_status`,
  and an `error: Option<CertProblem>` field **(proposed)**.
- Add an **error/warning state** to `cert_panel.rs`: when `error` is set, render
  a red header + reason + the offending issuer, mirroring the existing
  `SECURE_GREEN` semantic colour convention (`cert_panel.rs:44-46`; note those
  constants are intentionally theme-independent).
- Add a **blocking interstitial** **(proposed: `crates/shell/src/panels/
  cert_interstitial.rs`)** shown when navigation hits `Error::CertInvalid`:
  "Your connection is not private" + reason + **Back** / **Proceed anyway**
  (advanced). "Proceed" sets a per-origin session override that the fetch path
  consults before failing the handshake verdict. Wire the navigation error path
  in `crates/shell/src/main.rs` to route `Error::CertInvalid` here instead of
  the generic error page.

---

## Part B — Negotiate/NTLM + client certs (mTLS)

### B1 — Extend auth scheme + credentials types

- Add `Negotiate` and `Ntlm` to `HttpAuthScheme` (`crates/core/src/ext.rs:256`)
  and to `as_str()` (`ext.rs:266`).
- NTLM needs a domain: add `domain: Option<String>` to `HttpCredentials`
  (`ext.rs:301`) **(proposed)** — back-compat: default `None`.
- `HttpAuthChallenge` (`ext.rs:288`) already carries origin/realm/scheme; the
  Negotiate/NTLM server token must reach the builder — add a `token:
  Option<Vec<u8>>` **(proposed)** or pass the raw `ParsedChallenge` through the
  network-internal path (do **not** leak rustls/ntlm types into `lumen-core`).

### B2 — Negotiate/NTLM challenge selection

- `select_best_challenge()` (`crates/network/src/auth.rs:262`) currently returns
  only digest/basic. Add Negotiate/NTLM to the preference order **(proposed:
  Negotiate > NTLM > Digest > Basic on a trusted/intranet origin)**. Guard it:
  Negotiate/NTLM should only be offered to allowlisted (e.g. intranet) origins,
  not arbitrary internet servers — add an origin gate **(proposed)**.

### B3 — NTLM type-1/2/3 message exchange

- Implement the NTLMSSP three-message handshake **(proposed:
  `crates/network/src/auth_ntlm.rs`)**: Type-1 (negotiate) → server Type-2
  (challenge) → Type-3 (authenticate). NTLMv2 response = HMAC-MD5 over the
  server challenge (MD5 already implemented in `auth.rs:495`; HMAC + NTLMv2
  hash to add). Base64 transport reuses `base64_encode_std` (`auth.rs:453`).
- This requires **multi-round** auth, which the current single-shot 401 loop
  (`crates/network/src/lib.rs:2000-2034`) does not support. Generalise that loop
  to allow N rounds for stateful schemes (**proposed**: a small state machine —
  the connection must be kept alive across rounds, NTLM is connection-bound, so
  it must reuse the same pooled `Connection`, not a fresh one).
- **Negotiate (SPNEGO/Kerberos)**: a full Kerberos client is large. **Proposed
  scope for this task**: implement Negotiate as an SPNEGO wrapper that
  falls back to NTLM (the common Windows behaviour), and leave raw Kerberos
  ticket acquisition behind a `// TODO Kerberos` seam. Document the limitation.

### B4 — Client certificates (mTLS)

- Replace `.with_no_client_auth()` (`crates/network/src/tls/mod.rs:128`) with
  `.with_client_cert_resolver(Arc::new(LumenClientCertResolver{..}))`
  **(proposed)** implementing `rustls::client::ResolvesClientCert`. On the
  server's `CertificateRequest`, the resolver picks a configured client
  identity (cert chain + private key) matching the requested CA / hostname.
- Source of client identities **(proposed)**: a `ClientCertStore` loaded from the
  portable browser data dir (`<exe_dir>/data/tls/` — see CLAUDE.md "Portable
  user data dir"; reuse `browser_data_dir()`), PEM/PKCS#8 or PKCS#12. **Do not**
  use OS cert stores in this task.
- UI **(proposed)**: when a server requests a client cert and ≥1 identity
  matches, present a "Select a certificate" chooser in the shell; remember the
  choice per-origin for the session. With zero matches, proceed with no client
  cert (server decides whether to reject).

---

## Entry points (real file:line — re-grep before editing; **(proposed)** = new)

| What | Where |
|---|---|
| Build `ClientConfig` (verifier + client-auth seam) | `crates/network/src/tls/mod.rs:64` `build_client_config()` |
| `.with_no_client_auth()` to replace | `crates/network/src/tls/mod.rs:128` |
| Per-profile config cache / root store | `crates/network/src/lib.rs:1077-1090` `tls_config_for_profile()` |
| Handshake completion + error collapse | `crates/network/src/lib.rs:1065`, `lib.rs:1060/1066` |
| `CertInfo` struct + stub | `crates/network/src/tls/fingerprint.rs:116`, `:147` |
| OCSP/SCT ext codes (advertised, unused) | `crates/network/src/tls/fingerprint.rs:69`, `:72` |
| Cert panel (viewer, no error state) | `crates/shell/src/panels/cert_panel.rs:55`, `:161`, `:319` |
| 401-retry loop (single-shot) | `crates/network/src/lib.rs:2000-2034` |
| `parse_www_authenticate` / `select_best_challenge` | `crates/network/src/auth.rs:79`, `:262` |
| `HttpAuthScheme` / `HttpCredentials` / challenge | `crates/core/src/ext.rs:256`, `:301`, `:288` |
| MD5 + base64 primitives (reuse for NTLM) | `crates/network/src/auth.rs:495`, `:453` |
| `CertError`/`Error::CertInvalid` | **(proposed)** `crates/network/src/tls/` + `Error` enum |
| `LumenVerifier` (`ServerCertVerifier`) | **(proposed)** `crates/network/src/tls/verifier.rs` |
| CT log key list | **(proposed)** `crates/network/src/tls/ct_logs.rs` |
| NTLM/SPNEGO impl | **(proposed)** `crates/network/src/auth_ntlm.rs` |
| Client-cert resolver + store | **(proposed)** `crates/network/src/tls/client_cert.rs` |
| Interstitial + cert chooser UI | **(proposed)** `crates/shell/src/panels/cert_interstitial.rs` |

---

## Steps

### Part A
1. A1 — add `CertError` enum + `Error::CertInvalid` variant; thread out of
   `lib.rs:1060`.
2. A2 — `LumenVerifier` wrapping `WebPkiServerVerifier`; wire into
   `build_client_config()` (`tls/mod.rs:64`). Verify normal HTTPS still passes.
3. A5 — parse leaf cert; populate real `CertInfo`; retire `stub_for` from the
   live path.
4. A3 — OCSP stapling parse + policy (soft-fail except `revoked`).
5. A4 — SCT collection + CT-log validation + ≥2-log policy.
6. A6 — cert-panel error state + interstitial; route `Error::CertInvalid` in
   `main.rs`; "Proceed anyway" per-origin session override.

### Part B
1. B1 — extend `HttpAuthScheme` + `HttpCredentials` (domain) +
   challenge token carry.
2. B2 — Negotiate/NTLM in `select_best_challenge` behind an origin allowlist.
3. B3a — generalise the 401 loop (`lib.rs:2000`) to multi-round, connection-bound.
4. B3b — NTLM type-1/2/3 (NTLMv2, HMAC-MD5); Negotiate→NTLM SPNEGO fallback.
5. B4a — `ResolvesClientCert` + `ClientCertStore` from `data/tls/`; wire into
   `build_client_config()` (`tls/mod.rs:128`).
6. B4b — client-cert chooser UI; per-origin session memory.

Land Part A and Part B as **separate merges** (A is independently shippable).

---

## Dependencies

- **rustls 0.23** already present (`crates/network/Cargo.toml:19`). `dangerous()`
  custom-verifier + `with_client_cert_resolver` are in its public API — no
  version bump expected.
- **New deps — justify in the commit body** (CLAUDE.md "No new dep without
  justification"). Likely candidates and the in-house-vs-crate call:
  - OCSP/X.509 DER parsing — either extend the existing cert parsing (prefer:
    consistency with the "default — своё" principle stated in `auth.rs:11`) or a
    provisional parser crate (e.g. `x509-parser`/`der`) with a graduation note.
  - CT log key bundle — static data, no runtime dep (analogous to webpki-roots).
  - NTLM — **own implementation** (MD5 + HMAC already in-house at `auth.rs:495`),
    consistent with the Digest precedent; do not pull an NTLM crate.
- **CLAUDE.md role boundary:** P3 owns bug-fixes; this is a *feature* task. It is
  filed as P3 because `phases.md:122-123` tags both items `[P3]`. Shell
  integration (`crates/shell/`) is normally P3's anyway. Coordinate the
  `lumen-core::ext` additions (B1) per the "lumen-core is shared" rule.

---

## Tests

- **OCSP** (unit, `tls/`): DER fixtures for `good`/`revoked`/`unknown`/stale →
  expected `CertVerdict`. `revoked` → hard fail; `unknown` → soft pass.
- **CT** (unit): synthetic SCT sets — 0/1/2/3 logs → policy verdict; bad SCT
  signature → rejected.
- **Verifier** (integration): a known-good public HTTPS host still completes;
  an expired/self-signed fixture yields `Error::CertInvalid` with the right
  variant (mirror the MockTransport style already used for auth tests around
  `crates/network/src/lib.rs:5481`).
- **CertInfo population** (integration): real leaf cert → non-empty issuer,
  validity, fingerprint (proves `stub_for` is no longer on the live path).
- **NTLM** (unit, `auth_ntlm`): type-1/2/3 round-trip against captured
  reference vectors; NTLMv2 response matches a known fixture; multi-round loop
  reuses the same connection.
- **Challenge selection** (unit, extend `auth.rs` tests): Negotiate/NTLM chosen
  over Digest only on allowlisted origins; ignored otherwise (compare existing
  `select_returns_none_for_unsupported_only`, `auth.rs:853`).
- **Client cert** (unit): `ResolvesClientCert` picks the matching identity for a
  given CA hint; returns `None` cleanly when none match.
- **Cert panel** (unit, `cert_panel.rs`): error state renders red header +
  reason; revocation/CT rows present (extend existing `build_panel_*` tests).
- `cargo clippy -p lumen-network --all-targets -- -D warnings`,
  `cargo clippy -p lumen-shell --all-targets -- -D warnings`, then
  `cargo test -p lumen-network` / `-p lumen-shell`.

---

## Definition of done

- HTTPS to a normal site still works; a revoked/expired/untrusted cert produces
  `Error::CertInvalid` and an interstitial, with a working **Proceed anyway**.
- Stapled OCSP `revoked` blocks; `good` is shown in the cert panel; missing
  revocation info soft-passes.
- CT policy enforced (≥2 SCTs from distinct logs); status shown in the panel;
  hard-fail gated to `TlsProfile::Strict`.
- Cert panel shows **real** issuer/validity/fingerprint (no `stub_for` on the
  live path) plus revocation + CT rows, and an error state.
- Negotiate/NTLM auth completes against an NTLM-protected origin (allowlisted);
  arbitrary internet origins are not offered Negotiate/NTLM.
- A server requesting a client cert receives one when a matching identity is
  configured in `data/tls/`; the chooser UI works and remembers per-origin.
- `phases.md:122` updated (drop "Negotiate/NTLM + client certificates (mTLS) —
  отложены"), `phases.md:123` marked done; `CAPABILITIES.md` updated in the same
  merge; both clippy + test gates green.
- Bugs found while implementing are filed in `BUGS.md` (next BUG-NNN), per the
  CLAUDE.md ownership rules — not fixed inline unless trivial.
