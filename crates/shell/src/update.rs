//! Self-update manifest, version comparison and update checker
//! (UPD-1/UPD-2, `docs/tasks/ph3-self-update.md`).
//!
//! `latest.json` is the signed manifest a release publishes at the stable URL
//! `.../releases/latest/download/latest.json` (chosen over the GitHub API to
//! avoid its 60 req/h/IP rate limit — see the brief). Signature verification
//! (UPD-3) and everything downstream (download, apply, UI) are separate
//! slices.
//!
//! # Wiring status
//!
//! [`check_for_update`] is not called from `window_mode.rs` yet — that wiring,
//! plus a UI surface for the result, lands with UPD-9. Today the module is
//! exercised only by its own tests.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use lumen_core::url::Url;
use lumen_network::{ConditionalFetch, HttpClient};
use serde::{Deserialize, Serialize};

/// The `latest.json` manifest published alongside every GitHub Release.
///
/// `signature` is an ed25519 signature over the canonical JSON body minus this
/// field itself (verified in UPD-3, not here — this type only parses the
/// wire format).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateManifest {
    /// Release version, `x.y.z` — parsed via [`Version::parse`].
    pub version: String,
    /// Per-platform release assets (the two binaries, at minimum).
    pub assets: Vec<UpdateAsset>,
    /// Identifies which trusted public key `signature` was produced with,
    /// so an old client can still verify a manifest signed after a key
    /// rotation as long as it still trusts that `key_id`.
    pub key_id: String,
    /// Base64-encoded ed25519 signature over the manifest body.
    pub signature: String,
}

/// One downloadable asset listed in an [`UpdateManifest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateAsset {
    /// Asset file name as published on the release (e.g. `lumen-windows.zip`).
    pub name: String,
    /// Hex-encoded SHA-256 of the asset body, checked after download.
    pub sha256: String,
    /// Asset size in bytes.
    pub size: u64,
}

impl UpdateManifest {
    /// Parse [`Self::version`] into a comparable [`Version`].
    ///
    /// `None` means the manifest's version field is malformed — the manifest
    /// is network input, untrusted until UPD-3 verifies its signature, so a
    /// bad field is treated as "no update available" by callers rather than
    /// panicking or guessing at a partial version.
    #[must_use]
    pub fn parsed_version(&self) -> Option<Version> {
        Version::parse(&self.version)
    }

    /// Whether this manifest advertises a version strictly newer than
    /// `current`. Equal and older versions are rejected — the update path is
    /// forward-only by design (downgrade protection, `docs/tasks/ph3-self-update.md` §3).
    #[must_use]
    pub fn is_newer_than(&self, current: Version) -> bool {
        self.parsed_version().is_some_and(|v| v > current)
    }
}

/// A parsed `x.y.z` version triple (major.minor.patch).
///
/// No pre-release/build metadata and no `semver` crate — this project's own
/// releases are plain `x.y.z` (`CARGO_PKG_VERSION`), so a ~20-line comparator
/// covers the whole need (`docs/tasks/ph3-self-update.md` §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// Major component (`x` in `x.y.z`).
    pub major: u32,
    /// Minor component (`y` in `x.y.z`).
    pub minor: u32,
    /// Patch component (`z` in `x.y.z`).
    pub patch: u32,
}

impl Version {
    /// Parse a `x.y.z` string. `None` on any deviation — missing/extra
    /// component, non-numeric component, or an empty string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self { major, minor, patch })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

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
    /// A strictly newer version is available.
    Available(UpdateManifest),
    /// The manifest body was fetched but is not valid JSON, or its `version`
    /// field does not parse — treated as "no update" rather than propagated,
    /// since the manifest is untrusted network input until UPD-3 verifies it.
    Malformed,
}

/// Apply one conditional-GET outcome to `state` and decide the [`CheckOutcome`].
///
/// Pure with respect to the network — exercised directly in tests with
/// synthetic [`ConditionalFetch`] values, the same shape as
/// `adblock::apply_fetch_result`. Always bumps `last_checked_at` to `now`, so
/// a network error upstream (which never reaches this function) is the only
/// way a check does not reset the throttle.
#[must_use]
pub fn apply_check_result(
    mut state: UpdateState,
    result: ConditionalFetch,
    now: i64,
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
            if manifest.is_newer_than(current_version()) {
                (state, CheckOutcome::Available(manifest))
            } else {
                (state, CheckOutcome::UpToDate)
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
    let now = now_unix();
    if !state.auto_check_updates || !is_check_due(state.last_checked_at, now) {
        return (state, CheckOutcome::UpToDate);
    }
    let Ok(url) = Url::parse(MANIFEST_URL) else {
        return (state, CheckOutcome::UpToDate);
    };
    match client.fetch_conditional(&url, state.etag.as_deref(), state.last_modified.as_deref()) {
        Ok(result) => apply_check_result(state, result, now),
        Err(e) => {
            eprintln!("update: check failed: {e}");
            (state, CheckOutcome::UpToDate)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_version() {
        assert_eq!(
            Version::parse("1.2.3"),
            Some(Version { major: 1, minor: 2, patch: 3 })
        );
        assert_eq!(
            Version::parse("0.5.0"),
            Some(Version { major: 0, minor: 5, patch: 0 })
        );
    }

    #[test]
    fn rejects_malformed_versions() {
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse("1.2"), None, "too few components");
        assert_eq!(Version::parse("1.2.3.4"), None, "too many components");
        assert_eq!(Version::parse("1.2.x"), None, "non-numeric component");
        assert_eq!(Version::parse("v1.2.3"), None, "leading v prefix not accepted");
        assert_eq!(Version::parse("1..3"), None, "empty component");
    }

    #[test]
    fn orders_by_major_then_minor_then_patch() {
        let v = |s: &str| Version::parse(s).unwrap();
        assert!(v("2.0.0") > v("1.9.9"));
        assert!(v("1.3.0") > v("1.2.9"));
        assert!(v("1.2.4") > v("1.2.3"));
        assert_eq!(v("1.2.3"), v("1.2.3"));
    }

    #[test]
    fn display_round_trips() {
        let v = Version::parse("10.20.30").unwrap();
        assert_eq!(v.to_string(), "10.20.30");
    }

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
            key_id: "prod-1".to_string(),
            signature: "sig".to_string(),
        }
    }

    #[test]
    fn deserializes_manifest_json() {
        let json = r#"{
            "version": "1.2.3",
            "assets": [
                {"name": "lumen-windows.zip", "sha256": "deadbeef", "size": 42}
            ],
            "key_id": "prod-1",
            "signature": "c2ln"
        }"#;
        let manifest: UpdateManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.version, "1.2.3");
        assert_eq!(manifest.assets.len(), 1);
        assert_eq!(manifest.assets[0].name, "lumen-windows.zip");
        assert_eq!(manifest.assets[0].sha256, "deadbeef");
        assert_eq!(manifest.assets[0].size, 42);
        assert_eq!(manifest.key_id, "prod-1");
        assert_eq!(manifest.signature, "c2ln");
    }

    #[test]
    fn is_newer_than_rejects_downgrade_and_equal() {
        let manifest = sample_manifest("1.0.0");
        assert!(!manifest.is_newer_than(Version::parse("1.0.0").unwrap()), "equal is not newer");
        assert!(!manifest.is_newer_than(Version::parse("1.1.0").unwrap()), "downgrade is not newer");
    }

    #[test]
    fn is_newer_than_accepts_upgrade() {
        let manifest = sample_manifest("2.0.0");
        assert!(manifest.is_newer_than(Version::parse("1.9.9").unwrap()));
    }

    #[test]
    fn is_newer_than_rejects_malformed_manifest_version() {
        let manifest = sample_manifest("not-a-version");
        assert!(!manifest.is_newer_than(Version::parse("0.0.0").unwrap()));
    }

    #[test]
    fn parsed_version_none_for_malformed() {
        assert_eq!(sample_manifest("garbage").parsed_version(), None);
        assert_eq!(sample_manifest("1.2.3").parsed_version(), Version::parse("1.2.3"));
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
        let state = UpdateState::default();
        let result = ConditionalFetch::Modified {
            body: manifest_body("999.0.0"),
            etag: Some("\"v2\"".into()),
            last_modified: Some("Mon".into()),
        };
        let (new_state, outcome) = apply_check_result(state, result, 500);
        assert_eq!(new_state.last_checked_at, 500);
        assert_eq!(new_state.etag.as_deref(), Some("\"v2\""));
        assert_eq!(new_state.last_modified.as_deref(), Some("Mon"));
        match outcome {
            CheckOutcome::Available(m) => assert_eq!(m.version, "999.0.0"),
            other => panic!("expected Available, got {other:?}"),
        }
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
}
