# Ph3 — Self-update via GitHub Releases (UPD)

**Developer:** P1
**Branch:** `p1-upd-<slice>` (one branch per slice)
**Size:** L (11 slices: S×3, M×8)
**Crates:** `lumen-shell` (new module `update.rs`), `lumen-storage` (schema versioning), `lumen-network` (consumer only, no changes)
**Phase:** 3 (v1.0 target)

---

## Status

Planned. No code yet. This brief captures the full design agreed 2026-07-07.

---

## Goal

The browser checks for a new release, downloads it, verifies an ed25519 signature,
and replaces its own binaries — with **zero risk to user data**. No own server:
GitHub Releases is the distribution channel (repo `LearnJava/lumen-browser`).

---

## Architecture decisions (→ ADR in slice UPD-11)

### 1. Update channel: signed manifest, not the GitHub API

Each release publishes an asset `latest.json` reachable at the stable URL

```
https://github.com/LearnJava/lumen-browser/releases/latest/download/latest.json
```

Manifest contents: `version`, per-asset `{name, sha256, size}`, `key_id`,
`signature` (ed25519 over the canonical manifest body). Advantages over
`api.github.com/releases/latest`: no API rate limit (60 req/h/IP), ~300 bytes,
and the signature over the manifest transitively protects the binaries via
their sha256 entries. The public key ships as a constant in the shell,
alongside a **list** of trusted keys + `key_id` so a future key rotation does
not brick old clients.

### 2. Exe replacement: rename trick, no helper process

Windows allows renaming a running exe. Apply sequence:
`lumen.exe` → `lumen.exe.old`, copy new file in place, same for
`lumen-network-service.exe`, then restart. On next startup `.old` files and
`data/update/pending/` are cleaned up. Apply is staged **atomically across the
two binaries**: first all renames, then all copies; on any error the renames
are rolled back.

### 3. No new heavy dependencies

| Need | Solution | New dep? |
|---|---|---|
| HTTPS GET + redirects | `HttpClient::fetch_conditional` (rustls, redirects up to 5 hops — covers GitHub CDN redirect) | no |
| Signature | `ed25519-dalek` — already in the tree (TLS 1.3 peer auth) | no |
| Manifest JSON | `serde_json` — already in `crates/shell/Cargo.toml` | no |
| Version compare | own `x.y.z` parser (~20 lines), forward-only | no |
| ZIP extraction | own mini-reader: central directory + stored/deflate via existing `flate2` (~200 lines) | no |

### 4. Privacy default

Update check is **on by default** but: at most once per 24 h, plain GET without
cookies or identifiers, `auto_check_updates` flag in config to disable, behavior
documented in `docs/plan/privacy.md`. (Alternative considered: first-run opt-in
dialog. Revisit at UPD-9 if the user prefers opt-in.)

---

## User-data safety (core principle)

**The updater never touches user data.** Ownership split:

- **Distributed files** (`lumen.exe`, `lumen-network-service.exe`, `assets/fonts/`)
  — replaced wholesale by the updater. No user state lives there.
- **User data** (`<exe_dir>/data/`: SQLite DBs, `fingerprint.toml`, adblock lists)
  — never written by the updater. The **new binary** migrates them at first
  startup; migration logic lives in application code with types, transactions
  and tests. (Same model as Firefox/Chrome.)

Mechanisms:

1. **SQLite schema versioning** — `PRAGMA user_version` per DB + ordered
   migration list, executed in ONE transaction at open (SQLite DDL is
   transactional; an interrupted migration rolls back whole). Forward-only.
   DBs are versioned independently (fits ADR-012 lifecycle split). Today
   `lumen-storage` has ~25 stores on `CREATE TABLE IF NOT EXISTS` with one
   ad-hoc migration already (`crates/storage/src/profiles.rs:97` — `ALTER TABLE`
   add column); that becomes migration v2 of the formal scheme.
2. **Backup before migration** — new binary detects first run after update
   (stored version ≠ `CARGO_PKG_VERSION`), copies `data/*.db` to
   `data/update/backup/<old-version>/` **before opening any DB**. Keep last
   1–2 versions, rotate older.
3. **Downgrade protection** — a DB with `user_version` greater than the binary
   knows → refuse to open with a clear error (never "try anyway"). This is the
   dangerous pair: rolled-back `.old` binary + migrated DB. Documented rollback
   = restore `.old` exe **and** `data/update/backup/<version>/`.
4. **Config TOML: tolerant parsing instead of migrations** — unknown keys
   ignored, missing keys defaulted from code (defaults never live in
   distributed files). Format change ⇒ convert on load + atomic rewrite
   (temp file + rename).
5. **Updater-side barrier** — the mini-ZIP extractor enforces a destination
   whitelist (distributed files in the exe dir only); archive entries with a
   `data/` prefix, absolute paths or `..` abort the update. Doubles as the
   zip-slip defense.

---

## Slices

| id | Slice | Contents | Size |
|---|---|---|---|
| UPD-1 | Manifest + versions | `UpdateManifest` types (serde), `x.y.z` parser/comparator, unit tests | S |
| UPD-2 | Checker | Fetch `latest.json` via `fetch_conditional` (ETag cache), 24 h throttle (state file in `data/update/`), `auto_check_updates` config flag. Tests on `MockTransport` | M |
| UPD-3 | Signature verification | ed25519 over manifest (trusted-keys list + `key_id`), sha256 of binaries. Negative tests: broken signature, swapped hash, downgrade (manifest version ≤ current → reject) | S |
| UPD-4 | Schema versioning in `lumen-storage` | `user_version` + shared migration helper, convert existing stores (`CREATE IF NOT EXISTS` → migration v1, profiles.rs ad-hoc `ALTER` → v2), refuse-newer. Tests: old DB → migrated; future DB → error; interrupted migration → rollback | M |
| UPD-5 | Backup + first-run detect | First-run-after-update detection, `data/*.db` backup before open, rotation, rollback procedure doc | S |
| UPD-6 | Background download | Thread per `download.rs` pattern (`std::thread` + `mpsc` → `about_to_wait`): fetch zip, verify sha256, stage into `data/update/pending/<version>/` | M |
| UPD-7 | Mini-ZIP reader | Central directory, stored/deflate via `flate2`, destination whitelist (reject `data/`, `..`, absolute). Test against a real `release.yml` artifact | M |
| UPD-8 | Apply | Rename trick for both exes (atomic staging, rollback on error), process restart, `.old` + `pending/` cleanup at startup | M |
| UPD-9 | UI | "Update available vX.Y.Z" infobar (download-panel style) + `show_os_notification()`; "Restart to update" button after staging; manual check action | M |
| UPD-10 | CI: release signing | Keypair generation (private key → GitHub Actions secret), signing tool `scripts/sign_release` on `ed25519-dalek`, `release.yml` step: build `latest.json`, sign, upload as asset | M |
| UPD-11 | Docs | ADR (channel + signing + data policy + privacy), `CAPABILITIES.md`, `subsystems/shell.md`, `subsystems/storage.md`, `docs/plan/privacy.md`, README | S |

Order: 1→2→3 give end-to-end "new version available" (notification can ship
without download); 4→5 are independent and valuable on their own (the profiles.rs
ad-hoc migration is already a symptom) — can go first; 6→7→8 complete the update
cycle; 9–11 wrap up. UPD-10 can run in parallel with 6–8.

---

## Codebase facts (explored 2026-07-07)

- `HttpClient` is sync, bodies are in-memory `Vec<u8>` only (no streaming to
  file) — acceptable for a 30–50 MB zip; streaming deferred, note in
  `subsystems/network.md`. Redirects followed up to 5 hops.
- `browser_data_dir()` → `<exe_dir>/data/` (`crates/shell/src/adblock.rs:44`).
- Background download pattern: `crates/shell/src/download.rs` (`DownloadManager`,
  thread + `mpsc`, polled in `about_to_wait`).
- OS notifications: `crates/shell/src/notification.rs:18` (`show_os_notification`).
- **Anti-pattern to avoid:** `adblock::refresh()` blocks the UI thread; the
  updater is background-thread-only from slice UPD-2 on.
- Release artifacts (`.github/workflows/release.yml`): per-OS zip/tar.gz with
  `lumen.exe` + `lumen-network-service.exe` — **update touches 2 binaries**.
- Config precedent: `crates/shell/src/config.rs` (`fingerprint.toml`, flat TOML,
  `OnceLock`). `auto_check_updates` follows this model (own `data/update/` state
  file for throttle timestamps).
- Version: `lumen_core::VERSION` / `env!("CARGO_PKG_VERSION")`; no semver crate
  needed (own comparator).

---

## Risks

- **Private key compromise in GitHub Secrets** = ability to sign a malicious
  update. Mitigations from day one: `key_id` + trusted-keys list in the client
  (rotation without bricking), downgrade protection (UPD-3), destination
  whitelist (UPD-7).
- **In-memory download** — no streaming in `lumen-network`; fine at current
  artifact sizes, revisit if the archive grows past ~200 MB.
- **Rollback after migration** — `.old` binary + migrated DB corrupts data;
  covered by refuse-newer (UPD-4) + backup restore procedure (UPD-5).
- **SmartScreen** — no code-signing certificate: first manual download still
  warns; the self-update path itself (rename trick) is unaffected. Ed25519
  protects the update channel, not the SmartScreen reputation.
