//! Self-update manifest, version comparison and update checker
//! (UPD-1/UPD-2, `docs/tasks/ph3-self-update.md`).
//!
//! `latest.json` is the signed manifest a release publishes at the stable URL
//! `.../releases/latest/download/latest.json` (chosen over the GitHub API to
//! avoid its 60 req/h/IP rate limit — see the brief). [`apply_check_result`]
//! rejects a manifest that fails [`lumen_update_manifest::verify_manifest`] before it ever reaches
//! a caller — [`CheckOutcome::Available`] is only ever a signed, trusted
//! manifest. Everything downstream of that (download, apply, UI) is still
//! separate slices.
//!
//! # Wiring status
//!
//! [`check_for_update`] is not called from `window_mode.rs` yet — that wiring,
//! plus a UI surface for the result, lands with UPD-9. [`UpdateDownloadManager`]
//! (UPD-6), [`apply_staged_update`] and [`restart_process`] (UPD-8) follow the
//! same rule: nothing calls them yet, the trigger is also UPD-9. Today the
//! module is exercised only by its own tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{SystemTime, UNIX_EPOCH};

use lumen_core::ext::NetworkTransport;
use lumen_core::url::Url;
use lumen_network::{ConditionalFetch, HttpClient};
use serde::{Deserialize, Serialize};

// The manifest wire format, `Version` and ed25519 verification live in
// `lumen-update-manifest` (UPD-10) so the release signer shares them.
use lumen_update_manifest::verify_manifest_with_keys;
pub use lumen_update_manifest::{
    ManifestVerifyError, TRUSTED_KEYS, UpdateAsset, UpdateManifest, Version,
};

/// The version of the running binary — the update checker's baseline for
/// comparison against a manifest's [`UpdateManifest::version`].
///
/// Never hardcoded: derived from `CARGO_PKG_VERSION` per the project-wide
/// version policy (`CLAUDE.md`). Falls back to `0.0.0` only if the build's own
/// version string were ever malformed, which cargo does not allow in practice.
#[must_use]
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
    })
}

// ── Checker (UPD-2) ─────────────────────────────────────────────────────────

/// Stable URL of the manifest published alongside every GitHub Release
/// (`docs/tasks/ph3-self-update.md` §1) — no API rate limit, unlike
/// `api.github.com/releases/latest`.
pub const MANIFEST_URL: &str =
    "https://github.com/LearnJava/lumen-browser/releases/latest/download/latest.json";

/// Minimum time between two manifest fetches — EasyList-style politeness
/// (`docs/tasks/ph3-self-update.md` §Checker), not a hard spec requirement.
pub const CHECK_INTERVAL_SECS: i64 = 24 * 3600;

/// Persisted update-checker state: throttle timestamp, conditional-GET
/// validators and the user's opt-out. Round-trips through JSON at
/// [`state_path`] — tolerant of missing fields (`#[serde(default)]`) so a
/// future slice can add fields without a migration, the same policy as
/// `fingerprint.toml` (`docs/tasks/ph3-self-update.md` §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateState {
    /// Unix timestamp (seconds) of the last check, `0` if never checked.
    #[serde(default)]
    pub last_checked_at: i64,
    /// `ETag` from the last `200 OK`, sent back as `If-None-Match`.
    #[serde(default)]
    pub etag: Option<String>,
    /// `Last-Modified` from the last `200 OK`, sent back as `If-Modified-Since`.
    #[serde(default)]
    pub last_modified: Option<String>,
    /// User opt-out. On by default (`docs/tasks/ph3-self-update.md` §Privacy
    /// default) — at most once per [`CHECK_INTERVAL_SECS`], no cookies or
    /// identifiers on the wire.
    #[serde(default = "default_auto_check")]
    pub auto_check_updates: bool,
}

fn default_auto_check() -> bool {
    true
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            last_checked_at: 0,
            etag: None,
            last_modified: None,
            auto_check_updates: true,
        }
    }
}

/// `<exe_dir>/data/update` — root of the self-update subsystem's files.
#[must_use]
pub fn update_dir() -> PathBuf {
    crate::adblock::browser_data_dir().join("update")
}

/// Path to the persisted [`UpdateState`] (`<exe_dir>/data/update/state.json`).
#[must_use]
pub fn state_path() -> PathBuf {
    update_dir().join("state.json")
}

/// Load [`UpdateState`] from [`state_path`], or the default (never checked,
/// auto-check on) if the file is missing or malformed. Malformed state is
/// untrusted local input at worst, not fatal — never panics.
#[must_use]
pub fn load_state() -> UpdateState {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist [`UpdateState`] to [`state_path`], creating [`update_dir`] if
/// needed. Best-effort: a write failure is silently dropped, same as
/// `adblock`'s list-body writes — the next check simply re-throttles from
/// whatever timestamp survives.
pub fn save_state(state: &UpdateState) {
    if std::fs::create_dir_all(update_dir()).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(state_path(), json);
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Whether a check is due: never checked, or the last one is older than
/// [`CHECK_INTERVAL_SECS`].
#[must_use]
pub fn is_check_due(last_checked_at: i64, now: i64) -> bool {
    now - last_checked_at >= CHECK_INTERVAL_SECS
}

/// Outcome of a single update check.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckOutcome {
    /// Check skipped (opted out or throttled), or ran and found no newer
    /// version (`304`, or `200` with a version that is not newer).
    UpToDate,
    /// A strictly newer version is available, and its signature verified
    /// against [`TRUSTED_KEYS`] — this is the only variant a caller may act
    /// on (e.g. proceed to download).
    Available(UpdateManifest),
    /// The manifest body was fetched but is not valid JSON, or its `version`
    /// field does not parse.
    Malformed,
    /// The manifest parsed and claimed a newer version, but failed
    /// [`lumen_update_manifest::verify_manifest`] — unknown `key_id`, malformed signature, or a
    /// signature that does not match. Never exposes the unverified manifest;
    /// treated the same as "no update" by callers, distinctly logged by
    /// [`check_for_update`] so a live attack/corruption attempt is visible.
    Untrusted(ManifestVerifyError),
}

/// Apply one conditional-GET outcome to `state` and decide the [`CheckOutcome`],
/// verifying against [`TRUSTED_KEYS`]. Thin wrapper over
/// [`apply_check_result_with_keys`] — see that function for the actual logic;
/// this split exists for the same reason `verify_manifest` is split from
/// [`verify_manifest_with_keys`], so tests can supply a throwaway keypair.
#[must_use]
pub fn apply_check_result(
    state: UpdateState,
    result: ConditionalFetch,
    now: i64,
) -> (UpdateState, CheckOutcome) {
    apply_check_result_with_keys(state, result, now, TRUSTED_KEYS)
}

/// Apply one conditional-GET outcome to `state` and decide the [`CheckOutcome`].
///
/// Pure with respect to the network — exercised directly in tests with
/// synthetic [`ConditionalFetch`] values, the same shape as
/// `adblock::apply_fetch_result`. Always bumps `last_checked_at` to `now`, so
/// a network error upstream (which never reaches this function) is the only
/// way a check does not reset the throttle.
fn apply_check_result_with_keys(
    mut state: UpdateState,
    result: ConditionalFetch,
    now: i64,
    trusted_keys: &[(&str, [u8; 32])],
) -> (UpdateState, CheckOutcome) {
    state.last_checked_at = now;
    match result {
        ConditionalFetch::NotModified => (state, CheckOutcome::UpToDate),
        ConditionalFetch::Modified {
            body,
            etag,
            last_modified,
        } => {
            state.etag = etag;
            state.last_modified = last_modified;
            let Ok(manifest) = serde_json::from_slice::<UpdateManifest>(&body) else {
                return (state, CheckOutcome::Malformed);
            };
            if !manifest.is_newer_than(current_version()) {
                return (state, CheckOutcome::UpToDate);
            }
            match verify_manifest_with_keys(&manifest, trusted_keys) {
                Ok(()) => (state, CheckOutcome::Available(manifest)),
                Err(e) => (state, CheckOutcome::Untrusted(e)),
            }
        }
    }
}

/// Check [`MANIFEST_URL`] for a newer release, throttled to at most once per
/// [`CHECK_INTERVAL_SECS`] and skipped entirely when
/// [`UpdateState::auto_check_updates`] is off. Returns the state to persist
/// via [`save_state`] alongside the outcome — callers own persistence so this
/// stays a plain function instead of touching disk itself.
///
/// A malformed [`MANIFEST_URL`] or a network error is logged and treated as
/// "no update" without touching `state` — the anti-pattern this avoids is
/// `adblock::refresh()`'s UI-thread blocking call; this function is meant to
/// run on its own background thread the same way.
#[must_use]
pub fn check_for_update(client: &HttpClient, state: UpdateState) -> (UpdateState, CheckOutcome) {
    if !state.auto_check_updates || !is_check_due(state.last_checked_at, now_unix()) {
        return (state, CheckOutcome::UpToDate);
    }
    let fallback = state.clone();
    check_for_update_now(client, state).unwrap_or_else(|e| {
        eprintln!("update: check failed: {e}");
        (fallback, CheckOutcome::UpToDate)
    })
}

/// Unthrottled check behind [`check_for_update`] — the settings page's manual
/// «Проверить сейчас» (UPD-9) calls this directly, bypassing both the 24 h
/// throttle and the auto-check opt-out (an explicit click is consent).
///
/// Unlike [`check_for_update`], a network error is returned as `Err` rather
/// than folded into [`CheckOutcome::UpToDate`]: a manual check must not
/// report "up to date" when it never reached the server.
pub fn check_for_update_now(
    client: &HttpClient,
    state: UpdateState,
) -> Result<(UpdateState, CheckOutcome), String> {
    let url = Url::parse(MANIFEST_URL).map_err(|e| e.to_string())?;
    let result = client
        .fetch_conditional(&url, state.etag.as_deref(), state.last_modified.as_deref())
        .map_err(|e| e.to_string())?;
    let (state, outcome) = apply_check_result(state, result, now_unix());
    if let CheckOutcome::Untrusted(e) = &outcome {
        eprintln!("update: manifest failed signature verification: {e:?}");
    }
    Ok((state, outcome))
}

/// `HttpClient` every self-update request goes through — the user's proxy/TLS
/// config via `config::global().apply_http`, plus the three content decoders
/// (GitHub serves release assets compressed on some mirrors).
#[must_use]
pub fn update_http_client() -> HttpClient {
    use lumen_network::{BrotliContentDecoder, DeflateContentDecoder, GzipContentDecoder};
    crate::config::global().apply_http(
        HttpClient::new()
            .with_content_decoder(Arc::new(BrotliContentDecoder::new()))
            .with_content_decoder(Arc::new(GzipContentDecoder::new()))
            .with_content_decoder(Arc::new(DeflateContentDecoder::new())),
    )
}

// ── Backup + first-run detect (UPD-5) ───────────────────────────────────────

/// `<data>/update/backup` — root of every per-version DB snapshot taken by
/// [`backup_before_migration_if_updated`].
#[must_use]
pub fn backup_root_dir() -> PathBuf {
    update_dir().join("backup")
}

/// `<data>/update/backup/<version>` — where the DB snapshot taken just before
/// the first run of the version *after* `version` lives.
#[must_use]
pub fn backup_dir_for(version: &str) -> PathBuf {
    backup_root_dir().join(version)
}

/// Path to the marker file recording the version that last ran, distinct
/// from [`state_path`] (that file is the update-checker's throttle/ETag
/// cache, refreshed only when a check actually runs — first-run detection
/// must work even with `auto_check_updates` off).
#[must_use]
pub fn last_run_version_path() -> PathBuf {
    update_dir().join("last_run_version")
}

/// How many most-recent per-version backups [`rotate_backups`] keeps.
/// `docs/tasks/ph3-self-update.md` §User-data safety says "last 1-2
/// versions" — 2 is the generous end, since a backup is a few DB files and
/// the whole point is a safety margin for a bad update.
pub const BACKUP_RETENTION: usize = 2;

/// Result of comparing the version recorded on the previous run against the
/// one running now — decides whether [`backup_before_migration_if_updated`]
/// needs to back anything up before `lumen_storage` opens a single database.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FirstRunKind {
    /// No marker on disk at all — a fresh install, not an update. Nothing to
    /// back up; a prior version's data never existed.
    FreshInstall,
    /// Marker matches the running binary — an ordinary run, not a first run.
    SameVersion,
    /// Marker names a different version — first run after an update. The
    /// data on disk was last written by `previous_version` and must be
    /// snapshotted before anything touches it.
    Updated { previous_version: String },
}

/// Pure decision logic behind [`backup_before_migration_if_updated`] — split
/// out so the three cases are testable without touching the filesystem.
fn detect_first_run(last_run_version: Option<&str>, current: &str) -> FirstRunKind {
    match last_run_version {
        None => FirstRunKind::FreshInstall,
        Some(v) if v == current => FirstRunKind::SameVersion,
        Some(v) => FirstRunKind::Updated { previous_version: v.to_string() },
    }
}

/// Recursively collect every `*.db` file under `dir`, skipping `skip` (an
/// absolute path compared by prefix) — used to exclude `data/update/` itself,
/// since its `backup/` subtree holds previous snapshots, not live data.
/// Best-effort: a subdirectory this process cannot read is silently skipped
/// rather than aborting the whole walk — a partial backup of the databases
/// that *were* readable is still strictly better than none.
fn find_db_files(dir: &Path, skip: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == skip {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            out.extend(find_db_files(&path, skip));
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "db") {
            out.push(path);
        }
    }
    out
}

/// Copy every `*.db` file under `data_dir` (except `data/update/`, see
/// [`find_db_files`]) into `dest_dir`, preserving the path relative to
/// `data_dir` — a store nested under a subfolder (`adblock/adblock.db`,
/// `hsts/hsts.db`, `idb/<origin>.db`, …) lands at the same relative path
/// under the backup, so the rollback procedure (`docs/tasks/ph3-self-update.md`
/// §User-data safety) is "copy the backup tree back over `data/`", not a
/// per-store lookup table.
///
/// Best-effort per file: one file that fails to copy (locked, permissions)
/// does not abort the rest — `std::fs::copy` is not transactional the way
/// SQLite's own migration-in-one-transaction is (UPD-4); this backup is a
/// belt-and-suspenders safety margin on top of that, not the primary
/// correctness mechanism, so a partial backup beats none.
fn backup_databases(data_dir: &Path, dest_dir: &Path) {
    let skip = data_dir.join("update");
    for src in find_db_files(data_dir, &skip) {
        let Ok(rel) = src.strip_prefix(data_dir) else {
            continue;
        };
        let dest = dest_dir.join(rel);
        if let Some(parent) = dest.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            continue;
        }
        if let Err(e) = std::fs::copy(&src, &dest) {
            eprintln!("update: backup of {} failed: {e}", src.display());
        }
    }
}

/// Keep at most [`BACKUP_RETENTION`] per-version directories under
/// `backup_root`, deleting the oldest by [`Version`] ordering — not by
/// directory-listing order (OS-dependent) or mtime, since the update path is
/// forward-only (`UpdateManifest::is_newer_than`) and a numerically newer
/// version is always the more relevant backup to keep, independent of
/// whatever order the filesystem happens to return entries in. A directory
/// name that isn't a parseable [`Version`] is left alone rather than deleted
/// — this function only ever removes backups it itself understands.
fn rotate_backups(backup_root: &Path) {
    let Ok(entries) = std::fs::read_dir(backup_root) else {
        return;
    };
    let mut versioned: Vec<(Version, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name();
            let version = Version::parse(name.to_str()?)?;
            Some((version, e.path()))
        })
        .collect();
    versioned.sort_by_key(|(v, _)| *v);
    let excess = versioned.len().saturating_sub(BACKUP_RETENTION);
    for (_, path) in versioned.into_iter().take(excess) {
        if let Err(e) = std::fs::remove_dir_all(&path) {
            eprintln!("update: rotating old backup {} failed: {e}", path.display());
        }
    }
}

/// Load the version recorded by the previous run, or `None` if this is the
/// first run ever (no marker file yet).
fn load_last_run_version() -> Option<String> {
    std::fs::read_to_string(last_run_version_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Persist `version` as the marker [`load_last_run_version`] reads on the
/// next run. Best-effort, same policy as [`save_state`] — a write failure
/// here just means the next run re-derives `Updated` from whatever marker
/// (possibly stale, possibly absent) survives, which only costs a redundant
/// backup, never a missed one silently treated as safe.
fn save_last_run_version(version: &str) {
    if std::fs::create_dir_all(update_dir()).is_err() {
        return;
    }
    let _ = std::fs::write(last_run_version_path(), version);
}

/// Detect whether this is the first run of a new version and, if so, back up
/// every `data/*.db` file to `data/update/backup/<old-version>/` **before**
/// `lumen_storage` opens a single database — the mechanism
/// `docs/tasks/ph3-self-update.md` §User-data safety requires ahead of UPD-4's
/// migrations ever running. Idempotent per version (a second call the same
/// run, or on a later run of the same binary, is a no-op via `SameVersion`)
/// and infallible: every I/O step is best-effort and logged, never panics or
/// returns an error, because a browser that fails to start over a backup
/// directory it couldn't create is a worse outcome than one that starts
/// without a backup.
///
/// **Must be called exactly once, at the very top of shell startup** — before
/// `config::load()`'s lazy HTTP disk cache or any `lumen_storage` store is
/// constructed. See `crates/shell/src/cli_args.rs::run_cli`.
pub fn backup_before_migration_if_updated() {
    let current = current_version().to_string();
    match detect_first_run(load_last_run_version().as_deref(), &current) {
        FirstRunKind::FreshInstall | FirstRunKind::SameVersion => {}
        FirstRunKind::Updated { previous_version } => {
            backup_databases(&crate::adblock::browser_data_dir(), &backup_dir_for(&previous_version));
            rotate_backups(&backup_root_dir());
        }
    }
    save_last_run_version(&current);
}

// ── Background download (UPD-6) ─────────────────────────────────────────────

/// `<data>/update/pending` — root of staged, hash-verified update archives
/// waiting for UPD-7 (mini-ZIP extraction) and UPD-8 (apply).
#[must_use]
pub fn pending_root_dir() -> PathBuf {
    update_dir().join("pending")
}

/// `<data>/update/pending/<version>` — destination directory
/// [`download_update_asset`] stages the verified archive into for release
/// `version`.
#[must_use]
pub fn pending_dir_for(version: &str) -> PathBuf {
    pending_root_dir().join(version)
}

/// This build's `matrix.artifact-name` in `release.yml` — the prefix of
/// every archive name it publishes (e.g. `lumen-windows-x86_64-v0.5.0.zip`).
/// OS *and* architecture: macOS ships both `aarch64` and `x86_64` archives
/// in one manifest, so an OS-only match would hand an Intel Mac the arm
/// build. `None` on a target `release.yml` does not build for — there is no
/// asset to select.
fn platform_asset_tag() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("lumen-windows-x86_64"),
        ("macos", "aarch64") => Some("lumen-macos-aarch64"),
        ("macos", "x86_64") => Some("lumen-macos-x86_64"),
        ("linux", "x86_64") => Some("lumen-linux-x86_64"),
        _ => None,
    }
}

/// Pick this platform's asset out of a manifest's [`UpdateManifest::assets`]:
/// the one named `<platform_asset_tag>-v<version>.<ext>`, as `release.yml`
/// packages it. Matched on `<tag>-` so no artifact name can be mistaken for
/// a longer one sharing its prefix.
#[must_use]
pub fn select_platform_asset(assets: &[UpdateAsset]) -> Option<&UpdateAsset> {
    let prefix = format!("{}-", platform_asset_tag()?);
    assets.iter().find(|a| a.name.starts_with(&prefix))
}

/// Direct-download URL for `asset_name` published under release tag
/// `v<version>`. Pinned to the exact tag rather than [`MANIFEST_URL`]'s
/// `.../releases/latest/...` convention — the archive fetched here must
/// always match the manifest that named it, even if a newer release gets
/// tagged while the download is in flight.
#[must_use]
pub fn asset_download_url(version: &str, asset_name: &str) -> String {
    format!("https://github.com/LearnJava/lumen-browser/releases/download/v{version}/{asset_name}")
}

/// Outcome of one [`download_update_asset`] attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdateDownloadOutcome {
    /// Downloaded, hash-verified, and written to `path`.
    Staged { path: PathBuf },
    /// The downloaded body's SHA-256 did not match [`UpdateAsset::sha256`].
    /// The manifest carrying that hash was already signature-verified by
    /// [`lumen_update_manifest::verify_manifest`] — this catches corruption or a compromised mirror
    /// on top of that, so it is never staged to disk.
    HashMismatch,
    /// Network error or I/O failure while fetching or writing.
    Failed(String),
}

/// Fetch `asset` for release `version` via `transport`, verify its SHA-256,
/// and write it to `dest_dir/asset.name`.
///
/// `dest_dir` is an explicit parameter rather than always
/// `pending_dir_for(version)` — same reason [`backup_databases`] takes
/// `data_dir`/`dest_dir` instead of deriving them: [`update_dir`] resolves
/// from `current_exe()` and cannot be pointed at a scratch dir in tests.
/// [`UpdateDownloadManager::start`] is the caller that passes the real
/// [`pending_dir_for`].
///
/// Generic over [`NetworkTransport`] so tests exercise this against
/// [`lumen_network::MockTransport`] instead of a real HTTP round-trip — the
/// same reason [`apply_check_result`] is split from [`check_for_update`].
fn download_update_asset<T: NetworkTransport>(
    transport: &T,
    version: &str,
    asset: &UpdateAsset,
    dest_dir: &Path,
) -> UpdateDownloadOutcome {
    let url = asset_download_url(version, &asset.name);
    let parsed = match Url::parse(&url) {
        Ok(u) => u,
        Err(e) => return UpdateDownloadOutcome::Failed(e.to_string()),
    };
    let body = match transport.fetch(&parsed) {
        Ok(b) => b,
        Err(e) => return UpdateDownloadOutcome::Failed(e.to_string()),
    };
    if !asset.verify_body(&body) {
        return UpdateDownloadOutcome::HashMismatch;
    }
    if let Err(e) = std::fs::create_dir_all(dest_dir) {
        return UpdateDownloadOutcome::Failed(e.to_string());
    }
    let dest = dest_dir.join(&asset.name);
    if let Err(e) = std::fs::write(&dest, &body) {
        return UpdateDownloadOutcome::Failed(e.to_string());
    }
    UpdateDownloadOutcome::Staged { path: dest }
}

/// Current state of the (at most one) in-flight self-update download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateDownloadStatus {
    /// No download started yet, or the previous one finished and its result
    /// was already consumed by the caller.
    Idle,
    /// Fetch + hash-verify + write running on a background thread.
    InProgress,
    /// Downloaded, hash-verified, and staged at `path` — ready for UPD-7.
    Staged { path: PathBuf },
    /// The downloaded body's hash did not match the (signed) manifest's.
    HashMismatch,
    /// Network error or I/O failure.
    Failed(String),
    /// Cancelled by the user before the background thread reported back.
    Cancelled,
}

/// Runs [`download_update_asset`] on its own `std::thread`, polled from the
/// shell event loop (`about_to_wait`) via [`UpdateDownloadManager::poll`] —
/// the same thread-per-download + `mpsc` pattern as `download::DownloadManager`,
/// minus the multi-entry list: only one self-update download is ever in
/// flight, so a single [`UpdateDownloadStatus`] slot is enough.
///
/// The in-memory `HttpClient::fetch` has no mid-transfer cancellation hook
/// (same limitation `download.rs` notes), so [`Self::cancel`] does not stop
/// the thread — it only marks the status `Cancelled` so [`Self::poll`] drops
/// the eventual result instead of overwriting a state the user already
/// dismissed, mirroring `DownloadManager::poll`'s
/// "`Cancelled` wins over a late `Done`" rule.
pub struct UpdateDownloadManager {
    status: UpdateDownloadStatus,
    rx: mpsc::Receiver<UpdateDownloadOutcome>,
    tx: mpsc::Sender<UpdateDownloadOutcome>,
}

impl Default for UpdateDownloadManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateDownloadManager {
    /// Create a new manager with no download in flight.
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            status: UpdateDownloadStatus::Idle,
            rx,
            tx,
        }
    }

    /// Start downloading `asset` for release `version` on a background
    /// thread. A no-op while a download is already [`UpdateDownloadStatus::InProgress`]
    /// — callers gate the UI trigger on [`Self::status`] the same way
    /// `download.rs`'s cancel button only appears while `InProgress`.
    pub fn start(&mut self, version: String, asset: UpdateAsset) {
        if matches!(self.status, UpdateDownloadStatus::InProgress) {
            return;
        }
        self.status = UpdateDownloadStatus::InProgress;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let client = update_http_client();
            let outcome = download_update_asset(&client, &version, &asset, &pending_dir_for(&version));
            let _ = tx.send(outcome);
        });
    }

    /// Mark the in-flight download as cancelled. The background thread keeps
    /// running to completion (see the struct docs), but [`Self::poll`] will
    /// discard its result once it arrives.
    pub fn cancel(&mut self) {
        if matches!(self.status, UpdateDownloadStatus::InProgress) {
            self.status = UpdateDownloadStatus::Cancelled;
        }
    }

    /// Drain the internal channel and update [`Self::status`].
    ///
    /// Must be called regularly from the shell event loop (e.g.
    /// `about_to_wait`), same as [`crate::download::DownloadManager::poll`].
    pub fn poll(&mut self) {
        while let Ok(outcome) = self.rx.try_recv() {
            if matches!(self.status, UpdateDownloadStatus::Cancelled) {
                continue;
            }
            self.status = match outcome {
                UpdateDownloadOutcome::Staged { path } => UpdateDownloadStatus::Staged { path },
                UpdateDownloadOutcome::HashMismatch => UpdateDownloadStatus::HashMismatch,
                UpdateDownloadOutcome::Failed(reason) => UpdateDownloadStatus::Failed(reason),
            };
        }
    }

    /// Current download state.
    #[must_use]
    pub fn status(&self) -> &UpdateDownloadStatus {
        &self.status
    }
}

// ── Apply (UPD-8) ────────────────────────────────────────────────────────────

/// Distributed binaries the rename trick replaces, in application order —
/// matches the two-exe layout `network_service.rs::NetworkServiceHandle::spawn`
/// already assumes (`lumen.exe` + `lumen-network-service.exe` side by side in
/// `exe_dir`). Platform-suffixed the same way that module is.
#[cfg(windows)]
const UPDATE_BINARIES: &[&str] = &["lumen.exe", "lumen-network-service.exe"];
#[cfg(not(windows))]
const UPDATE_BINARIES: &[&str] = &["lumen", "lumen-network-service"];

/// Suffix the rename trick gives a binary's previous version — restored on
/// rollback, deleted by [`cleanup_after_apply`] on the first successful
/// startup of the new version.
const OLD_SUFFIX: &str = ".old";

/// Everything that can go wrong applying a staged update.
#[derive(Debug)]
pub enum ApplyError {
    /// The staged archive failed to extract — see [`crate::zip_reader::ZipError`].
    Extract(crate::zip_reader::ZipError),
    /// The archive extracted cleanly but is missing one of [`UPDATE_BINARIES`]
    /// — never partially applied over this: checked before the first rename.
    MissingBinary(&'static str),
    /// A rename or copy failed. Every variant below this point in the apply
    /// sequence rolls back whatever renames already succeeded before
    /// returning, so `exe_dir` is left exactly as it was found.
    Io(String),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::Extract(e) => write!(f, "failed to extract staged update: {e}"),
            ApplyError::MissingBinary(name) => {
                write!(f, "staged archive is missing expected binary {name:?}")
            }
            ApplyError::Io(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ApplyError {}

/// Renames every live binary in `exe_dir` to `<name>.old`, restoring any
/// already-renamed one if a later rename fails — the first half of the
/// "first all renames, then all copies" sequence
/// (`docs/tasks/ph3-self-update.md` §2). Returns the `(live, old)` pairs on
/// success, so the caller can roll them back too if the copy phase fails.
fn rename_live_binaries_to_old(exe_dir: &Path) -> Result<Vec<(PathBuf, PathBuf)>, ApplyError> {
    let mut renamed = Vec::new();
    for name in UPDATE_BINARIES {
        let live = exe_dir.join(name);
        let old = exe_dir.join(format!("{name}{OLD_SUFFIX}"));
        if let Err(e) = std::fs::rename(&live, &old) {
            for (live, old) in renamed.iter().rev() {
                let _ = std::fs::rename(old, live);
            }
            return Err(ApplyError::Io(format!(
                "renaming {} to {}: {e}",
                live.display(),
                old.display()
            )));
        }
        renamed.push((live, old));
    }
    Ok(renamed)
}

/// Copies each extracted binary from `extracted_dir` into the now-vacated
/// `renamed` live paths. On failure, removes any new copy already written
/// and restores every `.old` back to its live name — the update is left
/// fully rolled back, never half-applied.
fn copy_new_binaries_into_place(
    extracted_dir: &Path,
    renamed: &[(PathBuf, PathBuf)],
) -> Result<(), ApplyError> {
    for (index, name) in UPDATE_BINARIES.iter().enumerate() {
        let (live, _) = &renamed[index];
        let src = extracted_dir.join(name);
        if let Err(e) = std::fs::copy(&src, live) {
            for (live, _) in &renamed[..index] {
                let _ = std::fs::remove_file(live);
            }
            for (live, old) in renamed {
                let _ = std::fs::rename(old, live);
            }
            return Err(ApplyError::Io(format!("copying {} into place: {e}", src.display())));
        }
    }
    Ok(())
}

/// Extracts `zip_path` and replaces every binary in [`UPDATE_BINARIES`] under
/// `exe_dir` with the extracted version, via the rename trick from
/// `docs/tasks/ph3-self-update.md` §2: rename every live binary to `.old`,
/// then copy every new one into place. Staged atomically **across both
/// binaries** — a failure at either phase restores `exe_dir` to exactly how
/// it was found, never leaving one binary updated and the other not.
///
/// Every extracted binary is checked present in the archive *before* the
/// first rename, so a truncated or wrong-platform archive is rejected
/// without touching a single live file.
///
/// Does not restart the process — see [`restart_process`] — and does not
/// remove `zip_path`'s pending directory or the leftover `.old` files, which
/// is [`cleanup_after_apply`]'s job on the *next* startup (the running
/// process still has the old binaries mapped into memory on Windows until it
/// exits, so cleanup here would race the very restart this function enables).
pub fn apply_staged_update(zip_path: &Path, exe_dir: &Path) -> Result<(), ApplyError> {
    let extracted_dir = zip_path.with_file_name("extracted");
    let zip_bytes = std::fs::read(zip_path).map_err(|e| ApplyError::Io(e.to_string()))?;
    crate::zip_reader::extract(&zip_bytes, &extracted_dir).map_err(ApplyError::Extract)?;

    for name in UPDATE_BINARIES {
        if !extracted_dir.join(name).is_file() {
            return Err(ApplyError::MissingBinary(name));
        }
    }

    let renamed = rename_live_binaries_to_old(exe_dir)?;
    copy_new_binaries_into_place(&extracted_dir, &renamed)
}

/// `<exe_dir>` for the running binary — [`apply_staged_update`]'s and
/// [`cleanup_after_apply`]'s real caller passes this, derived the same way
/// `network_service.rs::NetworkServiceHandle::spawn` locates its sibling exe.
/// `None` only if `current_exe()` itself fails, which the OS does not allow
/// in practice for an already-running process.
#[must_use]
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf))
}

/// Removes every `<name>.old` left by a previous [`apply_staged_update`] and
/// the whole `data/update/pending/` staging tree. Best-effort and infallible,
/// same policy as [`save_state`] — a locked `.old` file (still mapped by a
/// process that hasn't fully exited) is retried on the *next* startup rather
/// than blocking this one.
///
/// **Call once at startup**, after the restart from [`restart_process`] has
/// landed in the new binary — mirrors [`backup_before_migration_if_updated`]'s
/// placement, but this one is order-independent with it (neither touches the
/// other's files).
pub fn cleanup_after_apply(exe_dir: &Path) {
    for name in UPDATE_BINARIES {
        let old = exe_dir.join(format!("{name}{OLD_SUFFIX}"));
        let _ = std::fs::remove_file(old);
    }
    let _ = std::fs::remove_dir_all(pending_root_dir());
}

/// Spawns a fresh instance of the current executable and exits this process
/// — the "restart" step after [`apply_staged_update`] has replaced the
/// binaries on disk. The shell's UI path uses [`spawn_new_instance`] instead
/// (it restarts only after the event loop returns).
///
/// Never returns on success (the process exits before the call site sees a
/// value) — only a spawn failure (e.g. the just-copied binary is somehow not
/// executable) is observable by a caller.
pub fn restart_process() -> Result<(), std::io::Error> {
    spawn_new_instance()?;
    std::process::exit(0);
}

/// Spawns a fresh instance of the current executable without exiting this
/// one — the half of [`restart_process`] the shell uses after its event loop
/// has already returned (UPD-9), so the old process still unwinds normally
/// (stores dropped, session saved) instead of `exit(0)` skipping destructors.
pub fn spawn_new_instance() -> Result<(), std::io::Error> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_matches_cargo_pkg_version() {
        assert_eq!(current_version().to_string(), env!("CARGO_PKG_VERSION"));
    }

    fn sample_manifest(version: &str) -> UpdateManifest {
        UpdateManifest {
            version: version.to_string(),
            assets: vec![UpdateAsset {
                name: "lumen-windows.zip".to_string(),
                sha256: "a".repeat(64),
                size: 12_345,
            }],
            key_id: "unsigned-placeholder".to_string(),
            signature: "sig".to_string(),
        }
    }

    // ── Checker (UPD-2) ─────────────────────────────────────────────────────

    #[test]
    fn default_state_checks_immediately_with_auto_on() {
        let state = UpdateState::default();
        assert_eq!(state.last_checked_at, 0);
        assert!(state.auto_check_updates);
        assert!(is_check_due(state.last_checked_at, 1_000_000));
    }

    #[test]
    fn is_check_due_logic() {
        // `0` (never checked) is due against any realistic `now` — a real Unix
        // timestamp is always far more than one interval past the epoch.
        assert!(is_check_due(0, 1_700_000_000));
        assert!(!is_check_due(1000, 1000 + CHECK_INTERVAL_SECS - 1));
        assert!(is_check_due(1000, 1000 + CHECK_INTERVAL_SECS));
    }

    fn manifest_body(version: &str) -> Vec<u8> {
        serde_json::to_vec(&sample_manifest(version)).unwrap()
    }

    // ── Signature verification (UPD-3) ───────────────────────────────────────

    /// Fixed seed, not a random key — tests need the same keypair every run,
    /// and this key never signs anything outside this test module.
    fn test_signing_key() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[7u8; 32])
    }

    /// `sample_manifest(version)` signed under `key_id` by the same
    /// [`lumen_update_manifest::sign_manifest`] the release signer uses.
    fn signed_manifest(version: &str, key_id: &str, signing_key: &ed25519_dalek::SigningKey) -> UpdateManifest {
        let mut manifest = sample_manifest(version);
        lumen_update_manifest::sign_manifest(&mut manifest, key_id, signing_key);
        manifest
    }

    #[test]
    fn apply_check_result_not_modified_bumps_timestamp_only() {
        let state = UpdateState {
            last_checked_at: 1,
            etag: Some("\"v1\"".into()),
            last_modified: None,
            auto_check_updates: true,
        };
        let (new_state, outcome) = apply_check_result(state, ConditionalFetch::NotModified, 999);
        assert_eq!(outcome, CheckOutcome::UpToDate);
        assert_eq!(new_state.last_checked_at, 999);
        assert_eq!(new_state.etag.as_deref(), Some("\"v1\""), "304 keeps validators unchanged");
    }

    #[test]
    fn apply_check_result_modified_newer_version_available() {
        let signing_key = test_signing_key();
        let trusted = [("test-1", signing_key.verifying_key().to_bytes())];
        let manifest = signed_manifest("999.0.0", "test-1", &signing_key);
        let state = UpdateState::default();
        let result = ConditionalFetch::Modified {
            body: serde_json::to_vec(&manifest).unwrap(),
            etag: Some("\"v2\"".into()),
            last_modified: Some("Mon".into()),
        };
        let (new_state, outcome) = apply_check_result_with_keys(state, result, 500, &trusted);
        assert_eq!(new_state.last_checked_at, 500);
        assert_eq!(new_state.etag.as_deref(), Some("\"v2\""));
        assert_eq!(new_state.last_modified.as_deref(), Some("Mon"));
        match outcome {
            CheckOutcome::Available(m) => assert_eq!(m.version, "999.0.0"),
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn apply_check_result_rejects_unsigned_newer_manifest() {
        // `manifest_body`/`sample_manifest` carry a placeholder `key_id`/
        // `signature` that trusts nothing — a newer version alone must never
        // reach `Available` without a verified signature, even against the
        // real production `TRUSTED_KEYS` (which never lists this placeholder `key_id`).
        let state = UpdateState::default();
        let result = ConditionalFetch::Modified {
            body: manifest_body("999.0.0"),
            etag: None,
            last_modified: None,
        };
        let (_, outcome) = apply_check_result(state, result, 500);
        assert_eq!(outcome, CheckOutcome::Untrusted(ManifestVerifyError::UnknownKeyId));
    }

    #[test]
    fn apply_check_result_rejects_tampered_newer_manifest() {
        let signing_key = test_signing_key();
        let trusted = [("test-1", signing_key.verifying_key().to_bytes())];
        let mut manifest = signed_manifest("999.0.0", "test-1", &signing_key);
        // Tamper with an asset hash after signing — the classic "swap the hash,
        // keep the signature" attack this slice's brief calls out.
        manifest.assets[0].sha256 = "f".repeat(64);
        let state = UpdateState::default();
        let result = ConditionalFetch::Modified {
            body: serde_json::to_vec(&manifest).unwrap(),
            etag: None,
            last_modified: None,
        };
        let (_, outcome) = apply_check_result_with_keys(state, result, 500, &trusted);
        assert_eq!(outcome, CheckOutcome::Untrusted(ManifestVerifyError::SignatureMismatch));
    }

    #[test]
    fn apply_check_result_downgrade_is_rejected_before_signature_check() {
        // A validly signed manifest for an older-or-equal version never even
        // reaches signature verification — `UpToDate`, not `Untrusted` — the
        // forward-only downgrade protection this slice's brief requires.
        let signing_key = test_signing_key();
        let trusted = [("test-1", signing_key.verifying_key().to_bytes())];
        let manifest = signed_manifest(&current_version().to_string(), "test-1", &signing_key);
        let state = UpdateState::default();
        let result = ConditionalFetch::Modified {
            body: serde_json::to_vec(&manifest).unwrap(),
            etag: None,
            last_modified: None,
        };
        let (_, outcome) = apply_check_result_with_keys(state, result, 500, &trusted);
        assert_eq!(outcome, CheckOutcome::UpToDate);
    }

    #[test]
    fn apply_check_result_modified_not_newer_is_up_to_date() {
        let state = UpdateState::default();
        let result = ConditionalFetch::Modified {
            body: manifest_body("0.0.1"),
            etag: None,
            last_modified: None,
        };
        let (_, outcome) = apply_check_result(state, result, 500);
        assert_eq!(outcome, CheckOutcome::UpToDate, "same-or-older version is not an update");
    }

    #[test]
    fn apply_check_result_malformed_json_is_not_fatal() {
        let state = UpdateState::default();
        let result = ConditionalFetch::Modified {
            body: b"not json".to_vec(),
            etag: None,
            last_modified: None,
        };
        let (new_state, outcome) = apply_check_result(state, result, 500);
        assert_eq!(outcome, CheckOutcome::Malformed);
        assert_eq!(new_state.last_checked_at, 500, "throttle still advances on malformed body");
    }

    #[test]
    fn check_for_update_skips_when_auto_check_disabled() {
        let client = HttpClient::new();
        let state = UpdateState {
            last_checked_at: 0,
            etag: None,
            last_modified: None,
            auto_check_updates: false,
        };
        let (new_state, outcome) = check_for_update(&client, state.clone());
        assert_eq!(outcome, CheckOutcome::UpToDate);
        assert_eq!(new_state, state, "skipped check must not touch state at all");
    }

    #[test]
    fn check_for_update_skips_when_throttled() {
        let client = HttpClient::new();
        let state = UpdateState {
            last_checked_at: now_unix(),
            etag: None,
            last_modified: None,
            auto_check_updates: true,
        };
        let (new_state, outcome) = check_for_update(&client, state.clone());
        assert_eq!(outcome, CheckOutcome::UpToDate);
        assert_eq!(new_state, state, "throttled check must not touch state at all");
    }

    #[test]
    fn state_round_trips_through_json_with_missing_fields_defaulted() {
        // Simulates a future slice adding a field this version doesn't know about
        // in reverse: today's file read by today's code, minus one key, must not
        // fail to parse — the tolerant-parsing policy (`docs/tasks/ph3-self-update.md` §4).
        let partial = r#"{"last_checked_at": 42}"#;
        let state: UpdateState = serde_json::from_str(partial).unwrap();
        assert_eq!(state.last_checked_at, 42);
        assert_eq!(state.etag, None);
        assert!(state.auto_check_updates, "missing key defaults to opted-in");
    }

    #[test]
    fn save_state_then_load_state_round_trips() {
        // Points state_path() at a temp dir by way of a scoped current_exe swap
        // is not available; exercise the pure serde round-trip instead, which is
        // what save_state/load_state reduce to once the file exists.
        let state = UpdateState {
            last_checked_at: 12345,
            etag: Some("\"e\"".into()),
            last_modified: Some("Tue".into()),
            auto_check_updates: false,
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let back: UpdateState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }

    // ── Backup + first-run detect (UPD-5) ────────────────────────────────────

    /// Fresh, uniquely named scratch dir under the OS temp dir — same pattern
    /// as `lumen_storage::hsts::tests::open_shared_store_persists_and_purges`,
    /// since `update_dir()`/`browser_data_dir()` are derived from
    /// `current_exe()` and cannot be pointed at a temp dir directly.
    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lumen_test_update_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detect_first_run_no_marker_is_fresh_install() {
        assert_eq!(detect_first_run(None, "1.0.0"), FirstRunKind::FreshInstall);
    }

    #[test]
    fn detect_first_run_matching_marker_is_same_version() {
        assert_eq!(detect_first_run(Some("1.0.0"), "1.0.0"), FirstRunKind::SameVersion);
    }

    #[test]
    fn detect_first_run_different_marker_is_updated() {
        assert_eq!(
            detect_first_run(Some("1.0.0"), "1.1.0"),
            FirstRunKind::Updated { previous_version: "1.0.0".to_string() }
        );
    }

    #[test]
    fn find_db_files_recurses_and_skips_non_db() {
        let dir = scratch_dir("find_db_files");
        std::fs::write(dir.join("profiles.db"), b"a").unwrap();
        std::fs::write(dir.join("notes.txt"), b"b").unwrap();
        std::fs::create_dir_all(dir.join("adblock")).unwrap();
        std::fs::write(dir.join("adblock").join("adblock.db"), b"c").unwrap();
        let skip = dir.join("update");
        std::fs::create_dir_all(&skip).unwrap();
        std::fs::write(skip.join("should_not_appear.db"), b"d").unwrap();

        let mut found = find_db_files(&dir, &skip);
        found.sort();
        let mut expected = vec![dir.join("profiles.db"), dir.join("adblock").join("adblock.db")];
        expected.sort();
        assert_eq!(found, expected);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_databases_preserves_relative_layout() {
        let data_dir = scratch_dir("backup_src");
        let dest_dir = scratch_dir("backup_dst");
        std::fs::write(data_dir.join("profiles.db"), b"profiles").unwrap();
        std::fs::create_dir_all(data_dir.join("hsts")).unwrap();
        std::fs::write(data_dir.join("hsts").join("hsts.db"), b"hsts").unwrap();

        backup_databases(&data_dir, &dest_dir);

        assert_eq!(std::fs::read(dest_dir.join("profiles.db")).unwrap(), b"profiles");
        assert_eq!(
            std::fs::read(dest_dir.join("hsts").join("hsts.db")).unwrap(),
            b"hsts"
        );

        let _ = std::fs::remove_dir_all(&data_dir);
        let _ = std::fs::remove_dir_all(&dest_dir);
    }

    #[test]
    fn backup_databases_skips_update_dir() {
        let data_dir = scratch_dir("backup_skip_src");
        let dest_dir = scratch_dir("backup_skip_dst");
        std::fs::create_dir_all(data_dir.join("update").join("backup").join("1.0.0")).unwrap();
        std::fs::write(
            data_dir.join("update").join("backup").join("1.0.0").join("old.db"),
            b"old backup",
        )
        .unwrap();

        backup_databases(&data_dir, &dest_dir);

        assert!(!dest_dir.join("update").exists(), "must not back up its own backup tree");

        let _ = std::fs::remove_dir_all(&data_dir);
        let _ = std::fs::remove_dir_all(&dest_dir);
    }

    #[test]
    fn rotate_backups_keeps_only_the_newest_by_version() {
        let root = scratch_dir("rotate");
        for v in ["1.0.0", "1.2.0", "1.1.0", "2.0.0"] {
            std::fs::create_dir_all(root.join(v)).unwrap();
        }

        rotate_backups(&root);

        let mut remaining: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        remaining.sort();
        assert_eq!(remaining, vec!["1.2.0".to_string(), "2.0.0".to_string()]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rotate_backups_ignores_unparseable_directory_names() {
        let root = scratch_dir("rotate_unparseable");
        std::fs::create_dir_all(root.join("not-a-version")).unwrap();
        std::fs::create_dir_all(root.join("1.0.0")).unwrap();
        std::fs::create_dir_all(root.join("2.0.0")).unwrap();

        rotate_backups(&root);

        let mut remaining: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        remaining.sort();
        assert_eq!(
            remaining,
            vec!["1.0.0".to_string(), "2.0.0".to_string(), "not-a-version".to_string()],
            "an unparseable name is left alone, not counted against the retention budget"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    // ── Background download (UPD-6) ──────────────────────────────────────────

    fn test_asset(name: &str, body: &[u8]) -> UpdateAsset {
        UpdateAsset {
            name: name.to_string(),
            sha256: lumen_core::hash::sha256_hex(body),
            size: body.len() as u64,
        }
    }

    #[test]
    fn asset_download_url_format() {
        assert_eq!(
            asset_download_url("1.2.3", "lumen-windows-x86_64-v1.2.3.zip"),
            "https://github.com/LearnJava/lumen-browser/releases/download/v1.2.3/lumen-windows-x86_64-v1.2.3.zip"
        );
    }

    #[test]
    fn pending_dir_for_is_under_pending_root() {
        assert_eq!(pending_dir_for("1.2.3"), pending_root_dir().join("1.2.3"));
    }

    #[test]
    fn select_platform_asset_matches_current_platform() {
        let tag = platform_asset_tag().expect("test runs on a platform release.yml builds for");
        let assets = vec![
            test_asset("lumen-completely-unrelated-v1.0.0.zip", b"b"),
            test_asset(&format!("{tag}-v1.0.0.zip"), b"a"),
        ];
        let picked = select_platform_asset(&assets).expect("must find the platform's own asset");
        assert_eq!(picked.name, format!("{tag}-v1.0.0.zip"));
    }

    #[test]
    fn select_platform_asset_distinguishes_architectures_of_one_os() {
        // release.yml publishes two macOS archives in one manifest; the
        // other-arch sibling of this build's asset must never be picked.
        let tag = platform_asset_tag().expect("test runs on a platform release.yml builds for");
        let other_arch = if tag.ends_with("x86_64") {
            tag.replace("x86_64", "aarch64")
        } else {
            tag.replace("aarch64", "x86_64")
        };
        let assets = vec![
            test_asset(&format!("{other_arch}-v1.0.0.tar.gz"), b"a"),
            test_asset(&format!("{tag}-v1.0.0.tar.gz"), b"b"),
        ];
        let picked = select_platform_asset(&assets).expect("must find the platform's own asset");
        assert_eq!(picked.name, format!("{tag}-v1.0.0.tar.gz"));
    }

    #[test]
    fn select_platform_asset_none_when_no_match() {
        let assets = vec![test_asset("lumen-totally-unrelated-v1.0.0.zip", b"a")];
        assert!(select_platform_asset(&assets).is_none());
    }

    #[test]
    fn download_update_asset_stages_matching_hash() {
        let body = b"zip contents".to_vec();
        let asset = test_asset("lumen-windows-x86_64-v1.0.0.zip", &body);
        let mut transport = lumen_network::MockTransport::new();
        transport.add_fixture(asset_download_url("1.0.0", &asset.name), body.clone());
        let dest_dir = scratch_dir("download_stage");

        let outcome = download_update_asset(&transport, "1.0.0", &asset, &dest_dir);

        match outcome {
            UpdateDownloadOutcome::Staged { path } => {
                assert_eq!(path, dest_dir.join(&asset.name));
                assert_eq!(std::fs::read(&path).unwrap(), body);
            }
            other => panic!("expected Staged, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dest_dir);
    }

    #[test]
    fn download_update_asset_rejects_hash_mismatch_without_staging() {
        let body = b"zip contents".to_vec();
        let mut asset = test_asset("lumen-windows-x86_64-v1.0.0.zip", &body);
        asset.sha256 = "f".repeat(64); // wrong hash, valid hex
        let mut transport = lumen_network::MockTransport::new();
        transport.add_fixture(asset_download_url("1.0.0", &asset.name), body);
        let dest_dir = scratch_dir("download_hash_mismatch");

        let outcome = download_update_asset(&transport, "1.0.0", &asset, &dest_dir);

        assert_eq!(outcome, UpdateDownloadOutcome::HashMismatch);
        assert!(
            !dest_dir.join(&asset.name).exists(),
            "a body that fails hash verification must never be written to disk"
        );
        let _ = std::fs::remove_dir_all(&dest_dir);
    }

    #[test]
    fn download_update_asset_reports_transport_failure() {
        let asset = test_asset("lumen-windows-x86_64-v1.0.0.zip", b"unused");
        let transport = lumen_network::MockTransport::new(); // no fixture registered
        let dest_dir = scratch_dir("download_transport_fail");

        let outcome = download_update_asset(&transport, "1.0.0", &asset, &dest_dir);

        assert!(matches!(outcome, UpdateDownloadOutcome::Failed(_)));
        let _ = std::fs::remove_dir_all(&dest_dir);
    }

    #[test]
    fn manager_new_is_idle() {
        let mgr = UpdateDownloadManager::new();
        assert_eq!(*mgr.status(), UpdateDownloadStatus::Idle);
    }

    #[test]
    fn manager_poll_applies_staged_outcome() {
        let mut mgr = UpdateDownloadManager::new();
        mgr.status = UpdateDownloadStatus::InProgress;
        let path = PathBuf::from("/tmp/lumen-update.zip");
        mgr.tx.send(UpdateDownloadOutcome::Staged { path: path.clone() }).unwrap();
        mgr.poll();
        assert_eq!(*mgr.status(), UpdateDownloadStatus::Staged { path });
    }

    #[test]
    fn manager_poll_applies_hash_mismatch() {
        let mut mgr = UpdateDownloadManager::new();
        mgr.status = UpdateDownloadStatus::InProgress;
        mgr.tx.send(UpdateDownloadOutcome::HashMismatch).unwrap();
        mgr.poll();
        assert_eq!(*mgr.status(), UpdateDownloadStatus::HashMismatch);
    }

    #[test]
    fn manager_poll_applies_failed() {
        let mut mgr = UpdateDownloadManager::new();
        mgr.status = UpdateDownloadStatus::InProgress;
        mgr.tx.send(UpdateDownloadOutcome::Failed("boom".to_string())).unwrap();
        mgr.poll();
        assert_eq!(*mgr.status(), UpdateDownloadStatus::Failed("boom".to_string()));
    }

    #[test]
    fn manager_cancel_wins_over_late_result() {
        let mut mgr = UpdateDownloadManager::new();
        mgr.status = UpdateDownloadStatus::InProgress;
        mgr.cancel();
        assert_eq!(*mgr.status(), UpdateDownloadStatus::Cancelled);
        // Thread still sends its result after the user already cancelled.
        mgr.tx
            .send(UpdateDownloadOutcome::Staged { path: PathBuf::from("/tmp/late.zip") })
            .unwrap();
        mgr.poll();
        assert_eq!(
            *mgr.status(),
            UpdateDownloadStatus::Cancelled,
            "a result arriving after cancel must not overwrite it"
        );
    }

    #[test]
    fn manager_cancel_on_idle_is_noop() {
        let mut mgr = UpdateDownloadManager::new();
        mgr.cancel();
        assert_eq!(*mgr.status(), UpdateDownloadStatus::Idle);
    }

    #[test]
    fn manager_start_ignored_while_already_in_progress() {
        let mut mgr = UpdateDownloadManager::new();
        mgr.status = UpdateDownloadStatus::InProgress;
        // No fixture/network available — if this spawned a real fetch it would
        // eventually report Failed; asserting the status is untouched proves
        // `start` returned before spawning anything.
        mgr.start("1.0.0".to_string(), test_asset("lumen-windows-x86_64-v1.0.0.zip", b"x"));
        assert_eq!(*mgr.status(), UpdateDownloadStatus::InProgress);
    }

    // ── Apply (UPD-8) ─────────────────────────────────────────────────────

    /// Writes `UPDATE_BINARIES` (real names for this platform) as plain files
    /// under `dir`, so a test `exe_dir` looks like a real install without
    /// needing the actual current binary.
    fn write_fake_binaries(dir: &Path, contents: &[&[u8]]) {
        std::fs::create_dir_all(dir).unwrap();
        for (name, body) in UPDATE_BINARIES.iter().zip(contents) {
            std::fs::write(dir.join(name), body).unwrap();
        }
    }

    fn read_binaries(dir: &Path) -> Vec<Vec<u8>> {
        UPDATE_BINARIES.iter().map(|name| std::fs::read(dir.join(name)).unwrap()).collect()
    }

    fn build_update_zip(contents: &[&[u8]]) -> Vec<u8> {
        let entries: Vec<(&str, &[u8], bool)> =
            UPDATE_BINARIES.iter().zip(contents).map(|(name, body)| (*name, *body, true)).collect();
        crate::zip_reader::tests::build_zip(&entries)
    }

    #[test]
    fn apply_staged_update_replaces_both_binaries() {
        let exe_dir = scratch_dir("apply_ok_exe");
        write_fake_binaries(&exe_dir, &[b"old lumen", b"old service"]);
        let pending = scratch_dir("apply_ok_pending");
        let zip_path = pending.join("update.zip");
        std::fs::write(&zip_path, build_update_zip(&[b"new lumen", b"new service"])).unwrap();

        apply_staged_update(&zip_path, &exe_dir).expect("well-formed staged update must apply");

        assert_eq!(read_binaries(&exe_dir), vec![b"new lumen".to_vec(), b"new service".to_vec()]);
        // The `.old` files are left in place deliberately — see
        // `apply_staged_update`'s doc comment — `cleanup_after_apply` removes
        // them, and only on a later startup.
        assert_eq!(
            std::fs::read(exe_dir.join(format!("{}{OLD_SUFFIX}", UPDATE_BINARIES[0]))).unwrap(),
            b"old lumen"
        );
        assert_eq!(
            std::fs::read(exe_dir.join(format!("{}{OLD_SUFFIX}", UPDATE_BINARIES[1]))).unwrap(),
            b"old service"
        );

        let _ = std::fs::remove_dir_all(&exe_dir);
        let _ = std::fs::remove_dir_all(&pending);
    }

    #[test]
    fn apply_staged_update_rejects_archive_missing_a_binary() {
        let exe_dir = scratch_dir("apply_missing_exe");
        write_fake_binaries(&exe_dir, &[b"old lumen", b"old service"]);
        let pending = scratch_dir("apply_missing_pending");
        let zip_path = pending.join("update.zip");
        // Only the first binary is present in the archive.
        let zip = crate::zip_reader::tests::build_zip(&[(UPDATE_BINARIES[0], b"new lumen", true)]);
        std::fs::write(&zip_path, zip).unwrap();

        let result = apply_staged_update(&zip_path, &exe_dir);

        assert!(matches!(result, Err(ApplyError::MissingBinary(_))));
        assert_eq!(
            read_binaries(&exe_dir),
            vec![b"old lumen".to_vec(), b"old service".to_vec()],
            "a rejected archive must not touch any live binary"
        );
        for name in UPDATE_BINARIES {
            assert!(!exe_dir.join(format!("{name}{OLD_SUFFIX}")).exists());
        }

        let _ = std::fs::remove_dir_all(&exe_dir);
        let _ = std::fs::remove_dir_all(&pending);
    }

    #[test]
    fn apply_staged_update_rejects_corrupt_archive() {
        let exe_dir = scratch_dir("apply_corrupt_exe");
        write_fake_binaries(&exe_dir, &[b"old lumen", b"old service"]);
        let pending = scratch_dir("apply_corrupt_pending");
        let zip_path = pending.join("update.zip");
        std::fs::write(&zip_path, b"not a zip at all").unwrap();

        let result = apply_staged_update(&zip_path, &exe_dir);

        assert!(matches!(result, Err(ApplyError::Extract(_))));
        assert_eq!(
            read_binaries(&exe_dir),
            vec![b"old lumen".to_vec(), b"old service".to_vec()],
            "extraction failure must not touch any live binary"
        );

        let _ = std::fs::remove_dir_all(&exe_dir);
        let _ = std::fs::remove_dir_all(&pending);
    }

    #[test]
    fn rename_live_binaries_rolls_back_when_second_rename_fails() {
        let exe_dir = scratch_dir("apply_rename_rollback");
        write_fake_binaries(&exe_dir, &[b"lumen body", b"service body"]);
        // Pre-create the second binary's `.old` destination as a directory —
        // `fs::rename` onto an existing non-empty destination fails on every
        // platform, forcing the rollback path.
        std::fs::create_dir_all(exe_dir.join(format!("{}{OLD_SUFFIX}", UPDATE_BINARIES[1]))).unwrap();
        std::fs::create_dir_all(exe_dir.join(format!("{}{OLD_SUFFIX}", UPDATE_BINARIES[1])).join("x")).unwrap();

        let result = rename_live_binaries_to_old(&exe_dir);

        assert!(result.is_err());
        assert_eq!(
            read_binaries(&exe_dir),
            vec![b"lumen body".to_vec(), b"service body".to_vec()],
            "the first binary's rename must be rolled back when the second fails"
        );

        let _ = std::fs::remove_dir_all(&exe_dir);
    }

    #[test]
    fn cleanup_after_apply_removes_old_files_and_pending_tree() {
        let exe_dir = scratch_dir("cleanup_exe");
        for name in UPDATE_BINARIES {
            std::fs::write(exe_dir.join(format!("{name}{OLD_SUFFIX}")), b"stale").unwrap();
        }

        cleanup_after_apply(&exe_dir);

        for name in UPDATE_BINARIES {
            assert!(!exe_dir.join(format!("{name}{OLD_SUFFIX}")).exists());
        }

        let _ = std::fs::remove_dir_all(&exe_dir);
    }

    #[test]
    fn cleanup_after_apply_is_a_noop_when_nothing_to_clean() {
        let exe_dir = scratch_dir("cleanup_noop_exe");
        // Must not panic or error when there is nothing stale on disk.
        cleanup_after_apply(&exe_dir);
        let _ = std::fs::remove_dir_all(&exe_dir);
    }

    #[test]
    fn exe_dir_matches_current_exe_parent() {
        let expected = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        assert_eq!(exe_dir(), Some(expected));
    }
}
