# Ph4 — E2E-encrypted sync

**Developer:** P1  
**Branch:** `p1-ph4-e2e-sync`  
**Size:** XL (new crate, protocol design, multi-crate wiring)  
**Crates:** new `lumen-sync`; `lumen-storage`, `lumen-network`, `lumen-shell`

---

## Status

**Phase 4 — after 1.0. Greenfield. Do not start before mobile client (ph4-mobile) exists.**

This item is listed in `docs/plan/phases.md:144` under "Phase 4 — After 1.0":

> **Sync через E2E (§12.11)** — self-host или P2P. Mobile-клиент критичен для real use-case.

The full design intent is in `docs/plan/knowledge.md:160–172` (§12.11).

---

## Goal

Synchronise browser state across devices with **end-to-end encryption**: the
relay server (self-hosted) or P2P peer sees only opaque ciphertext blobs — never
plaintext bookmarks, history, notes, settings, or open tabs. This is a hard
privacy invariant, not a preference. Lumen has no advertising model and does not
build a centralised "Lumen Sync" cloud service (§12.11, docs/plan/knowledge.md:168).

Use-case driver: "started reading on the phone in the metro → opened the laptop
at home, continues from the same scroll position."

---

## Current state

### Syncable stores (existing, `crates/storage/src/`)

All stores are SQLite-backed. None currently has change-tracking columns (no
`updated_at`, `change_seq`, or vector-clock field). Adding change tracking is a
pre-requisite for sync.

| Store file | Sync priority | Key struct |
|---|---|---|
| `bookmarks.rs:36` | HIGH | `Bookmark { id, url, title, folder, created_at, note, tags }` |
| `history.rs:34` | HIGH | `HistoryEntry { id, url, title, visit_date, visit_count }` |
| `tab_sessions.rs:19` | HIGH | `TabSession { session_id, url, title, scroll_y, form_values, workspace_id }` |
| `browser_settings.rs:43` | MEDIUM | `BrowserSettingsSnapshot { homepage, theme, shields_enabled, … }` |
| `workspaces.rs:18` | MEDIUM | `Workspace { id, name, color, cookie_partition }` |
| `session_export.rs:26` | LOW | `SessionFile { version, name, tabs: Vec<ExportedTab> }` |
| `session_store.rs:29` | LOW | `PersistedTab { url, title, scroll_x, scroll_y, dom_blob }` |

Not syncable (device-local or sensitive):
- `cookies.rs`, `cache_storage.rs`, `http_cache.rs` — session-local, do not sync.
- `profile_vault.rs` — key material, synced only as encrypted key envelope.
- `indexed_db.rs`, `service_workers.rs` — site-local state, out of scope.

### Existing crypto primitives

Lumen already uses the **RustCrypto ecosystem** consistently. No new crypto
families are needed for sync — only new *key types* (X25519, Argon2id) that
extend the existing chain.

| Primitive | Crate | Where used | Notes |
|---|---|---|---|
| AES-256-GCM | `aes-gcm = "0.10"` | `crates/storage/src/profile_vault.rs:1` — key wrapping | Provisional dep (ADR-011) |
| AES-256-GCM (SubtleCrypto) | `aes-gcm = "0.10"` | `crates/js/src/subtle_crypto.rs:617` — `crypto.subtle.encrypt/decrypt` | Provisional dep (ADR-011) |
| HMAC-SHA256 | `hmac = "0.12"` | `crates/js/src/subtle_crypto.rs:515` | Provisional dep (ADR-011) |
| PBKDF2-HMAC-SHA256 | `hmac` + `sha2` | `crates/storage/src/profile_vault.rs:21` — wrapping key KDF | Phase 1 choice; upgrade to Argon2id for sync |
| ECDSA P-256 | `p256 = "0.13"` | `crates/network/src/tls/mod.rs:7` (TLS) + `crates/js/src/subtle_crypto.rs:195` | Permanent dep |
| SHA-256 | `sha2 = "0.10"` | `crates/network/`, `crates/storage/` | Permanent dep |
| CSPRNG | `getrandom = "0.2"` | `crates/storage/src/profile_vault.rs:103` + `crates/js/` | Permanent dep |

**Crypto ADR:** `docs/decisions/ADR-011-crypto-deps.md` — covers `hmac` + `aes-gcm`
as Provisional deps, trait-anchored to SubtleCrypto. Graduation criterion:
"when Phase 1 ships the full Web Crypto API OR when a `lumen-crypto` crate is
audited." The sync layer will likely trigger that graduation.

**Missing for sync (proposed new deps):**
- `x25519-dalek` or `x25519` from RustCrypto — ECDH key exchange for device pairing.
- `argon2 = "0.5"` (RustCrypto) — Argon2id KDF for sync master key; `profile_vault.rs:24`
  already notes "a later phase may upgrade to Argon2id".
- Both must follow ADR-002 Provisional dep policy and get their own ADR entry.

### Network transport

`HttpClient` — `crates/network/src/lib.rs:2113`. Implements `NetworkTransport`
trait (`crates/core/src/ext.rs:19`). Already supports HTTPS, keep-alive pool,
CORS, and request filtering. Can be reused by the sync transport layer with
minimal wrapping (POST encrypted blobs to the relay server).

### Device identity / pairing

Fully greenfield — no `device_id`, `pairing`, or `peer` concept exists in the
codebase (grep confirmed). This is the largest design blank.

---

## Architecture

The sync subsystem has four layers:

```
┌────────────────────────────────────────────────────────┐
│  lumen-shell  (UI: settings panel, device list, QR)    │
└────────────────────────┬───────────────────────────────┘
                         │
┌────────────────────────▼───────────────────────────────┐
│  lumen-sync  (new crate)                               │
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────┐ │
│  │ChangeTracker│  │EncryptedPack │  │SyncTransport  │ │
│  │(per store)  │  │(seal/unseal) │  │(HTTP relay or │ │
│  └──────┬──────┘  └──────┬───────┘  │ P2P LAN)      │ │
│         │                │          └───────┬─────── │ │
│  ┌──────▼────────────────▼───────────────── ▼──────┐ │ │
│  │            SyncEngine (orchestrator)            │ │ │
│  │  device_id · vector_clock · conflict_resolver   │ │ │
│  └─────────────────────────────────────────────────┘ │ │
└────────────────────────────────────────────────────────┘
                         │
          uses lumen-storage (read/write)
          uses lumen-network (HttpClient)
```

### 1. Per-store change tracking

Add to each syncable store:
- `change_seq INTEGER` — monotone sequence number, incremented on every
  INSERT/UPDATE/DELETE via SQLite trigger.
- `device_id TEXT` — originating device UUID (set on write, propagated on sync).

No changes to existing store APIs — change tracking is internal SQLite triggers
plus a new `fn changes_since(seq: i64) -> Vec<ChangeRecord>` method per store.

### 2. Encrypted change-set (seal / unseal)

Each sync batch is a `SyncPack`:

```
SyncPack {
    device_id: Uuid,
    clock: VectorClock,           // per-device sequence numbers
    store_type: StoreKind,        // Bookmarks | History | TabSessions | …
    ciphertext: Vec<u8>,          // AES-256-GCM(sync_key, nonce, serialised changes)
    nonce: [u8; 12],
    tag: [u8; 16],                // included in ciphertext for GCM
}
```

`sync_key` is a 32-byte key derived via:
```
sync_key = Argon2id(master_password, device_salt, m=65536, t=3, p=4)
```
or wrapped via X25519 ECDH for P2P (master_password not required for P2P — the
device key pair IS the identity).

### 3. Transport

**Self-hosted relay (primary):** a small HTTP service; Lumen ships a reference
implementation as a companion binary or Docker image (outside this task scope).
The relay stores encrypted blobs keyed by `(profile_id, store_type, device_id, seq)`.
It never decrypts. Endpoint: `POST /sync/{profile}/{store}` with bearer auth
(HMAC-SHA256 over device key — no plaintext password to server).

**P2P LAN (secondary):** mDNS discovery + direct TLS connection between devices
on the same LAN. Both devices have key pairs (X25519). Pairing = QR code
exchange + ECDH handshake (no relay needed).

### 4. Conflict resolution

SQLite stores are append-heavy (history, notes) or low-contention (settings,
workspaces). Recommended strategy per store:

| Store | Strategy | Rationale |
|---|---|---|
| `history` | Last-write-wins on `(url, visit_date)` | Visits are immutable events |
| `bookmarks` | LWW on `url`; logical delete tombstones | Folder renames rare |
| `tab_sessions` | Replace by device; no merge | Each device owns its session |
| `browser_settings` | LWW on key name | Settings are independent scalars |
| `workspaces` | LWW on `id` | Workspace renames rare |

Full CRDT (e.g. ORSWOT for bookmarks set) is deferred — LWW + tombstones cover
95% of real conflicts at far lower implementation cost. Re-evaluate if conflicts
become user-visible.

### 5. Device pairing and key management

- **Device identity:** `DeviceIdentity { id: Uuid, display_name: String, public_key: X25519PublicKey, created_at: i64 }` — generated once at first sync setup, stored encrypted in `profile_vault.rs` (same AES-GCM key-wrap).
- **Pairing (self-hosted):** master password → Argon2id → sync_key; encrypted sync_key envelope stored on relay. Devices authenticate with HMAC-device-key; server never sees master password.
- **Pairing (P2P):** QR code = base64(device_id || X25519_public_key); scan triggers ECDH → shared secret → AES-256-GCM session key.
- **Key rotation:** store new sync_key envelope, re-encrypt all blobs on relay. Expensive, but rare (user-initiated on device loss).

---

## Open questions

1. **CRDT vs LWW for bookmarks folder hierarchy.** A tree-structured CRDT
   (e.g. move-tree CRDT from Martin Kleppmann 2020) correctly handles concurrent
   folder moves without cycles. LWW cannot. Decision gates on whether users
   will have real concurrent edits across devices before sync is available.

2. **Self-hosted vs P2P as default.** Self-hosted relay requires the user to
   run a server (or find a hosted relay they trust). P2P LAN covers the "home
   laptop + phone" use case without any server but breaks across NAT. Both
   transports should ship; the question is which is primary in the UI.

3. **Key management UX for non-technical users.** Argon2id KDF from master
   password is correct but "you must never lose this password" is a bad UX.
   Options: recovery code (BIP-39 mnemonic, printed), trusted-device re-keying,
   or optional cloud-backed key escrow (explicitly opt-in, conflicts with Lumen
   privacy philosophy). Needs a design decision before implementation.

4. **Relay server scope.** Is the reference relay implementation part of the
   Lumen repo (a companion workspace crate) or a separate repo? The relay is
   intentionally minimal (< 300 lines: receive encrypted blob, store, serve back
   to other devices), but it has its own deployment lifecycle.

5. **Mobile client dependency.** This task is largely academic until a mobile
   Lumen client exists (docs/plan/phases.md:143 lists "Mobile" as a Phase 4 item
   alongside sync). Consider implementing the sync protocol + desktop-to-desktop
   sync first (two desktop instances), then extending to mobile.

---

## Cross-references

- **`docs/plan/knowledge.md:160–172`** — §12.11 design intent (self-host relay,
  X25519 + AES-GCM, Argon2id KDF, no centralised cloud).
- **`docs/plan/phases.md:141–147`** — Phase 4 scope; sync + mobile listed together.
- **`docs/decisions/ADR-011-crypto-deps.md`** — existing RustCrypto deps
  (`aes-gcm`, `hmac`); graduation criterion mentions `lumen-crypto` crate — sync
  is the most likely graduation trigger.
- **`docs/decisions/ADR-003-sqlite-storage.md`** — SQLite as default storage;
  change tracking via triggers is consistent with this decision.
- **`docs/decisions/ADR-002-dependency-policy.md`** — new deps (`x25519`, `argon2`)
  require Provisional category + trait-anchor + graduation criterion.
- **`ph3-ai-module.md`** — `lumen-knowledge` crate; notes and read-later data
  from the knowledge layer should also be syncable in Phase 4+ (post §12.5).
- **Mobile (ph4-mobile, does not exist yet)** — real use-case driver; do not
  design the sync protocol in a desktop-only way that prevents mobile extension.

---

## Entry points (file:line)

All existing; none proposed here (new crate = no entry points yet).

| File | Line | Notes |
|---|---|---|
| `crates/storage/src/bookmarks.rs:36` | `Bookmark` struct | Add `change_seq`, `device_id` fields |
| `crates/storage/src/bookmarks.rs:103` | `fn add(…)` | Trigger change_seq increment |
| `crates/storage/src/history.rs:34` | `HistoryEntry` struct | Add `change_seq`, `device_id` fields |
| `crates/storage/src/history.rs:98` | `fn record_visit(…)` | Trigger change_seq increment |
| `crates/storage/src/tab_sessions.rs:19` | `TabSession` struct | Add `change_seq`, `device_id` fields |
| `crates/storage/src/browser_settings.rs:83` | `BrowserSettings` struct | Snapshot for sync serialisation |
| `crates/storage/src/profile_vault.rs:1` | AES-256-GCM key wrap | Reuse for device identity storage; upgrade KDF to Argon2id |
| `crates/network/src/lib.rs:2113` | `HttpClient` struct | Reuse for relay HTTP transport |
| `crates/core/src/ext.rs:19` | `NetworkTransport` trait | `lumen-sync` depends on this, not `HttpClient` directly |
| `docs/decisions/ADR-011-crypto-deps.md` | — | Provisional crypto deps; sync will graduate these |
| `crates/js/Cargo.toml:57` | `aes-gcm = "0.10"` | Same version for `lumen-sync` |
| `crates/storage/Cargo.toml:31` | `aes-gcm = "0.10"` | Same version for `lumen-sync` |

**Proposed new files (do not create until task starts):**
- `crates/sync/` — new workspace crate `lumen-sync`
- `crates/sync/src/change_tracker.rs` — `ChangeRecord`, `fn changes_since(seq: i64)`
- `crates/sync/src/pack.rs` — `SyncPack`, `fn seal(…)`, `fn unseal(…)`
- `crates/sync/src/engine.rs` — `SyncEngine`, `VectorClock`, conflict resolution
- `crates/sync/src/transport/http.rs` — relay HTTP transport wrapper
- `crates/sync/src/transport/p2p.rs` — LAN mDNS + TLS transport
- `crates/sync/src/device.rs` — `DeviceIdentity`, key generation, pairing
- `crates/shell/src/panels/sync_panel.rs` — settings UI, device list, QR code

---

## Steps

### Pre-conditions (before writing code)

1. `ph4-mobile` task exists and a mobile client architecture is drafted.
2. A new ADR for `x25519` and `argon2` deps is filed (extending ADR-011).
3. Conflict resolution strategy for bookmarks folders is decided (open question 1).
4. Relay server scope is decided (open question 4).

### Implementation order

#### Step 0: ADR for new crypto deps

File `docs/decisions/ADR-015-sync-crypto-deps.md` (or next available number):
- `x25519-dalek` or `x25519` (RustCrypto) — Provisional, trait-anchor = `DeviceIdentity::generate_keypair()`
- `argon2 = "0.5"` — Provisional, trait-anchor = `SyncKeyDerivation::derive(master_password, salt)`
- Graduation criterion: when `lumen-sync` ships to users.

#### Step 1: Change tracking in storage stores

For each syncable store (bookmarks, history, tab_sessions, browser_settings,
workspaces): add `change_seq INTEGER DEFAULT 0` + `device_id TEXT DEFAULT ''`
columns via SQLite migration (check schema version in `fn init(conn)`). Add
SQLite `AFTER INSERT/UPDATE` triggers that increment `change_seq`. Add method
`fn changes_since(&self, seq: i64) -> Result<Vec<ChangeRecord>>`.

No API breakage — existing callers do not pass or read the new columns.

#### Step 2: `lumen-sync` crate scaffold

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/ph4-e2e-sync -b p1-ph4-e2e-sync
```

Create `crates/sync/Cargo.toml`:
```toml
[package]
name = "lumen-sync"
version.workspace = true
edition.workspace = true

[dependencies]
lumen-core = { path = "../core" }
lumen-storage = { path = "../storage" }
lumen-network = { path = "../network" }
uuid = { version = "1", features = ["v4"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
aes-gcm = "0.10"          # Why: same AES-256-GCM chain as profile_vault.rs; Provisional ADR-011
argon2 = "0.5"            # Why: Argon2id KDF for sync master key; Provisional ADR-015
x25519-dalek = "2"        # Why: ECDH key exchange for P2P device pairing; Provisional ADR-015
getrandom = "0.2"
```

Add to `Cargo.toml` workspace `members`.

#### Step 3: Core sync types

`crates/sync/src/lib.rs` — public API surface:
- `pub struct DeviceIdentity { id: Uuid, display_name: String, public_key: [u8; 32] }`
- `pub struct VectorClock(HashMap<Uuid, u64>)`
- `pub struct ChangeRecord { store: StoreKind, op: Op, key: String, data: Vec<u8>, seq: i64, device_id: Uuid, ts: i64 }`
- `pub struct SyncPack { device_id: Uuid, clock: VectorClock, store: StoreKind, ciphertext: Vec<u8>, nonce: [u8; 12] }`
- `pub fn seal(pack_plaintext: &[u8], sync_key: &[u8; 32]) -> Result<SyncPack>`
- `pub fn unseal(pack: &SyncPack, sync_key: &[u8; 32]) -> Result<Vec<u8>>`

#### Step 4: Key derivation

`crates/sync/src/keys.rs`:
- `pub fn derive_sync_key(master_password: &str, salt: &[u8; 32]) -> [u8; 32]`
  — Argon2id with m=65536, t=3, p=4 (OWASP 2023 recommended minimum).
- `pub fn generate_device_keypair() -> (x25519_dalek::StaticSecret, x25519_dalek::PublicKey)`
- `pub fn p2p_shared_secret(our_secret: &StaticSecret, their_public: &PublicKey) -> [u8; 32]`

#### Step 5: Relay transport

`crates/sync/src/transport/http.rs`:
- `pub struct RelayTransport { client: Arc<HttpClient>, base_url: Url, device_key: [u8; 32] }`
- `pub async fn push(&self, pack: &SyncPack) -> Result<()>`
  — `POST /sync/{store}` with body = bincode/JSON serialised `SyncPack`.
  — Auth: `Authorization: HMAC-Device <hex(hmac_sha256(device_key, body))>`.
- `pub async fn pull(&self, store: StoreKind, since_seq: i64) -> Result<Vec<SyncPack>>`
  — `GET /sync/{store}?since={seq}`.

#### Step 6: Sync engine

`crates/sync/src/engine.rs`:
- `pub struct SyncEngine { identity: DeviceIdentity, clock: VectorClock, sync_key: [u8; 32], transport: Box<dyn SyncTransport> }`
- `pub fn push_store<S: Syncable>(&mut self, store: &S) -> Result<()>`
  — reads changes since last pushed seq, seals, pushes to transport.
- `pub fn pull_store<S: Syncable>(&mut self, store: &mut S) -> Result<()>`
  — pulls packs, unseals, applies via `S::apply_change(record)`.
- Conflict resolution per store (LWW for all stores in Phase 4 initial version).

#### Step 7: Shell UI (stub)

`crates/shell/src/panels/sync_panel.rs` — minimal settings panel:
- Enable/disable sync toggle.
- Relay URL input field.
- List of paired devices (from `DeviceIdentity` store).
- "Add device" button (shows QR code with `base64(device_id || public_key)`).

#### Step 8: Integration tests

`crates/sync/tests/roundtrip.rs`:
- `test_seal_unseal_roundtrip` — seal + unseal with known key, verify plaintext.
- `test_key_derivation_deterministic` — same password + salt → same key.
- `test_change_tracker_bookmarks` — add bookmark, `changes_since(0)` returns it.
- `test_vector_clock_merge` — merge two clocks, verify max-per-device semantics.
- `test_conflict_lww_bookmarks` — two devices edit same bookmark, LWW wins.

---

## Privacy and security notes

- **Server-side confidentiality:** the relay server must never see plaintext.
  This is enforced architecturally: sealing happens before any network call.
  Relay auth uses `HMAC(device_key, body)` — device key is never the master
  password or sync key.
- **Forward secrecy:** AES-256-GCM with random nonce per pack provides ciphertext
  unlinkability; no forward secrecy (no ephemeral keys per sync session). For
  Phase 4 this is acceptable — full forward secrecy (Signal-style ratchet) is
  deferred.
- **Argon2id parameters:** m=65536 KiB, t=3, p=4 — 64 MB RAM, ~0.5 s on a 2024
  laptop. Chosen to resist GPU cracking of master password if relay is
  compromised and encrypted key envelopes leak.
- **Nonce uniqueness:** each `seal()` call generates a fresh 12-byte nonce via
  `getrandom`. AES-256-GCM nonce collision probability at 2^32 seals ≈ 2^{−72} —
  acceptable for a browser sync workload.
- **Tombstones for deletes:** a deleted bookmark must be propagated as a tombstone
  (logical delete), not a missing row. Without tombstones, a re-sync would
  resurrect deleted items. Tombstones must also be encrypted.
- **Key compromise / device loss:** revoke a device by removing its public key
  from the relay-side device list and rotating sync_key. Key rotation is
  expensive (re-encrypt all stored packs) — document in the UI.
- **No analytics, no telemetry:** the sync relay must not log plaintext URLs,
  bookmark titles, or any store content. Log only: device UUID (opaque),
  timestamp, byte size. This should be enforced by the reference server
  implementation.

---

## Tests

### Unit tests (`crates/sync/src/`)

| Test | What it checks |
|---|---|
| `seal_unseal_roundtrip` | AES-256-GCM seal + unseal recovers plaintext exactly |
| `seal_wrong_key_fails` | unseal with wrong key returns `Err` |
| `seal_tampered_ciphertext_fails` | AES-256-GCM integrity check catches tampering |
| `argon2_deterministic` | same (password, salt) → same 32-byte key |
| `x25519_ecdh_symmetric` | DH(a.secret, b.public) == DH(b.secret, a.public) |
| `vector_clock_merge` | per-device max semantics |
| `vector_clock_happens_before` | causality check for conflict resolution |

### Integration tests (`crates/sync/tests/`)

| Test | What it checks |
|---|---|
| `bookmarks_change_tracking` | add + delete bookmark → `changes_since(0)` returns both records |
| `history_change_tracking` | record visit → change record with correct url and ts |
| `two_device_sync_roundtrip` | device A pushes, device B pulls → B has A's bookmarks |
| `conflict_lww_bookmarks` | concurrent rename → winner has higher ts |
| `tombstone_propagation` | device A deletes, device B syncs → B also deletes |

### Storage migration tests (`crates/storage/tests/`)

| Test | What it checks |
|---|---|
| `bookmarks_schema_v2_migration` | existing DB without `change_seq` migrates cleanly |
| `history_schema_v2_migration` | same for history store |

---

## Definition of done

- [ ] New `lumen-sync` crate compiles: `cargo check -p lumen-sync`
- [ ] `cargo clippy -p lumen-sync --all-targets -- -D warnings` is clean
- [ ] All unit and integration tests pass: `cargo test -p lumen-sync`
- [ ] Change tracking added to bookmarks, history, tab_sessions stores with migration
- [ ] Storage migration tests pass: `cargo test -p lumen-storage`
- [ ] `SyncPack` seal/unseal verified by tests; no plaintext escapes to transport layer
- [ ] `DeviceIdentity` generation + key derivation tested with known vectors
- [ ] ADR filed for `x25519` + `argon2` provisional deps
- [ ] `CAPABILITIES.md` updated: sync subsystem row added (⬜ Phase 4)
- [ ] `docs/plan/knowledge.md:172` note updated to "Phase 4, in progress" when task starts
- [ ] Shell sync panel exists (even as stub) so the feature is discoverable in UI
- [ ] Privacy/security notes reviewed: no plaintext URL or content in relay logs
- [ ] No `unwrap()` in production paths; all crypto errors return `Result<_, SyncError>`
