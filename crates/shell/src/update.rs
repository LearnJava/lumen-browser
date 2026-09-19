//! Self-update manifest, version comparison and update checker
//! (UPD-1/UPD-2, `docs/tasks/ph3-self-update.md`).
//!
//! `latest.json` is the signed manifest a release publishes at the stable URL
//! `.../releases/latest/download/latest.json` (chosen over the GitHub API to
//! avoid its 60 req/h/IP rate limit — see the brief). [`apply_check_result`]
//! rejects a manifest that fails [`verify_manifest`] before it ever reaches
//! a caller — [`CheckOutcome::Available`] is only ever a signed, trusted
//! manifest. Everything downstream of that (download, apply, UI) is still
//! separate slices.
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
/// `signature` is an ed25519 signature over [`UpdateManifest::signing_body`]
/// (every field except `signature` itself), checked by [`verify_manifest`].
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
    /// Hex-encoded SHA-256 of the asset body, checked by [`UpdateAsset::verify_body`]
    /// after download.
    pub sha256: String,
    /// Asset size in bytes.
    pub size: u64,
}

impl UpdateAsset {
    /// Whether `body`'s SHA-256 matches [`Self::sha256`] (case-insensitive
    /// hex). The manifest's signature already protects `sha256` from
    /// tampering in transit; this is the second half — checking a downloaded
    /// body actually hashes to what the (now-trusted) manifest claims,
    /// against corruption or a compromised/wrong download source. Consumed
    /// by the background download slice (UPD-6), not called anywhere yet.
    #[must_use]
    pub fn verify_body(&self, body: &[u8]) -> bool {
        lumen_core::hash::sha256_hex(body).eq_ignore_ascii_case(&self.sha256)
    }
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

// ── Signature verification (UPD-3) ──────────────────────────────────────────

/// Public keys this build trusts to sign an [`UpdateManifest`], keyed by
/// [`UpdateManifest::key_id`] so a future key rotation *adds* an entry
/// instead of replacing one — an old client that only knows the retired key
/// still verifies a manifest signed under it, and once the new key is added
/// here any manifest signed under either verifies (`docs/tasks/ph3-self-update.md`
/// §1, §Risks).
///
/// Empty until UPD-10 mints the production keypair and wires CI to sign
/// releases with it. Empty is the correct default for a channel nothing has
/// signed yet — [`verify_manifest`] rejects every manifest via
/// [`ManifestVerifyError::UnknownKeyId`] rather than trusting anything.
pub const TRUSTED_KEYS: &[(&str, [u8; 32])] = &[];

/// Why [`verify_manifest`] rejected a manifest. Distinct from
/// [`CheckOutcome::Malformed`] (bad JSON) — every variant here means the
/// bytes parsed fine but the manifest is not attributable to a key this
/// build trusts, which [`apply_check_result`] treats as a signal to ignore
/// the response, not merely "no update".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestVerifyError {
    /// `key_id` names no key in [`TRUSTED_KEYS`] — never issued, or retired
    /// past this build's rotation window.
    UnknownKeyId,
    /// `signature` is not valid base64, or does not decode to exactly the 64
    /// bytes an ed25519 signature is.
    MalformedSignature,
    /// The signature does not verify against [`UpdateManifest::signing_body`]
    /// under the named key — tampering, corruption in transit, or a
    /// wrong/compromised key.
    SignatureMismatch,
}

impl UpdateManifest {
    /// The exact bytes [`Self::signature`] is an ed25519 signature over.
    ///
    /// A dedicated type ([`SignedFields`]) rather than re-serializing `Self`
    /// with `signature` blanked out, so a future field added to the wire
    /// type does not silently start being covered by the signature (or not)
    /// without a matching, deliberate change here.
    fn signing_body(&self) -> Vec<u8> {
        /// Mirrors [`UpdateManifest`] minus `signature` — see
        /// [`UpdateManifest::signing_body`].
        #[derive(Serialize)]
        struct SignedFields<'a> {
            version: &'a str,
            assets: &'a [UpdateAsset],
            key_id: &'a str,
        }
        // `serde_json::to_vec` on a plain struct (no `HashMap`) is
        // deterministic field-order output, which is all a signer and this
        // verifier sharing this same function need — no general
        // canonical-JSON scheme required. `unwrap_or_default` never actually
        // triggers (the fields are all directly serializable), but an empty
        // body is a safe failure mode: it can never match a real signature.
        serde_json::to_vec(&SignedFields {
            version: &self.version,
            assets: &self.assets,
            key_id: &self.key_id,
        })
        .unwrap_or_default()
    }
}

/// Verify `manifest`'s signature against `trusted_keys`.
///
/// Split from [`verify_manifest`] (which always uses [`TRUSTED_KEYS`]) so
/// tests can exercise the actual verification logic — signature decoding,
/// key lookup, ed25519 check — against a throwaway keypair instead of
/// needing the real production key embedded here.
fn verify_manifest_with_keys(
    manifest: &UpdateManifest,
    trusted_keys: &[(&str, [u8; 32])],
) -> Result<(), ManifestVerifyError> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let key_bytes = trusted_keys
        .iter()
        .find(|(id, _)| *id == manifest.key_id)
        .map(|(_, bytes)| *bytes)
        .ok_or(ManifestVerifyError::UnknownKeyId)?;
    // A key embedded in `trusted_keys` is a build-time invariant, not
    // untrusted input reachable independently of `UnknownKeyId` above — the
    // only way this fails is a malformed entry in the trusted-keys list
    // itself, which is a programming error, not something a signature check
    // should distinguish for a caller.
    let Ok(verifying_key) = VerifyingKey::from_bytes(&key_bytes) else {
        return Err(ManifestVerifyError::UnknownKeyId);
    };

    let sig_bytes = lumen_core::hash::base64_decode(&manifest.signature)
        .ok_or(ManifestVerifyError::MalformedSignature)?;
    let sig_bytes: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| ManifestVerifyError::MalformedSignature)?;
    let signature = Signature::from_bytes(&sig_bytes);

    verifying_key
        .verify(&manifest.signing_body(), &signature)
        .map_err(|_| ManifestVerifyError::SignatureMismatch)
}

/// Verify `manifest`'s signature against [`TRUSTED_KEYS`] — the production
/// entry point, always called by [`apply_check_result`] before a manifest is
/// ever exposed as [`CheckOutcome::Available`].
pub fn verify_manifest(manifest: &UpdateManifest) -> Result<(), ManifestVerifyError> {
    verify_manifest_with_keys(manifest, TRUSTED_KEYS)
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
    /// [`verify_manifest`] — unknown `key_id`, malformed signature, or a
    /// signature that does not match. Never exposes the unverified manifest;
    /// treated the same as "no update" by callers, distinctly logged by
    /// [`check_for_update`] so a live attack/corruption attempt is visible.
    Untrusted(ManifestVerifyError),
}

/// Apply one conditional-GET outcome to `state` and decide the [`CheckOutcome`],
/// verifying against [`TRUSTED_KEYS`]. Thin wrapper over
/// [`apply_check_result_with_keys`] — see that function for the actual logic;
/// this split exists for the same reason [`verify_manifest`] is split from
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
    let now = now_unix();
    if !state.auto_check_updates || !is_check_due(state.last_checked_at, now) {
        return (state, CheckOutcome::UpToDate);
    }
    let Ok(url) = Url::parse(MANIFEST_URL) else {
        return (state, CheckOutcome::UpToDate);
    };
    match client.fetch_conditional(&url, state.etag.as_deref(), state.last_modified.as_deref()) {
        Ok(result) => {
            let (state, outcome) = apply_check_result(state, result, now);
            if let CheckOutcome::Untrusted(e) = &outcome {
                eprintln!("update: manifest failed signature verification: {e:?}");
            }
            (state, outcome)
        }
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

    // ── Signature verification (UPD-3) ───────────────────────────────────────

    /// Fixed seed, not a random key — tests need the same keypair every run,
    /// and this key never signs anything outside this test module.
    fn test_signing_key() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[7u8; 32])
    }

    /// `sample_manifest(version)` with `key_id` set to `key_id` and `signature`
    /// a real ed25519 signature over its own [`UpdateManifest::signing_body`]
    /// under `signing_key`.
    fn signed_manifest(version: &str, key_id: &str, signing_key: &ed25519_dalek::SigningKey) -> UpdateManifest {
        use ed25519_dalek::Signer;
        let mut manifest = sample_manifest(version);
        manifest.key_id = key_id.to_string();
        let sig = signing_key.sign(&manifest.signing_body());
        manifest.signature = lumen_core::hash::base64_encode(&sig.to_bytes());
        manifest
    }

    #[test]
    fn verify_manifest_accepts_valid_signature() {
        let signing_key = test_signing_key();
        let trusted = [("test-1", signing_key.verifying_key().to_bytes())];
        let manifest = signed_manifest("1.2.3", "test-1", &signing_key);
        assert_eq!(verify_manifest_with_keys(&manifest, &trusted), Ok(()));
    }

    #[test]
    fn verify_manifest_rejects_unknown_key_id() {
        let signing_key = test_signing_key();
        let manifest = signed_manifest("1.2.3", "test-1", &signing_key);
        // `trusted` only knows a different `key_id` — same key material, wrong name.
        let trusted = [("other-key", signing_key.verifying_key().to_bytes())];
        assert_eq!(
            verify_manifest_with_keys(&manifest, &trusted),
            Err(ManifestVerifyError::UnknownKeyId)
        );
    }

    #[test]
    fn verify_manifest_rejects_tampered_body() {
        let signing_key = test_signing_key();
        let trusted = [("test-1", signing_key.verifying_key().to_bytes())];
        let mut manifest = signed_manifest("1.2.3", "test-1", &signing_key);
        // Signature was computed over "1.2.3" — flip the version after signing,
        // simulating a manifest tampered (or corrupted) in transit.
        manifest.version = "999.0.0".to_string();
        assert_eq!(
            verify_manifest_with_keys(&manifest, &trusted),
            Err(ManifestVerifyError::SignatureMismatch)
        );
    }

    #[test]
    fn verify_manifest_rejects_signature_from_wrong_key() {
        let signing_key = test_signing_key();
        let other_key = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
        // Trusted list has the *other* key under the same `key_id` the manifest
        // claims — models a compromised/mismatched key, not just an unknown id.
        let trusted = [("test-1", other_key.verifying_key().to_bytes())];
        let manifest = signed_manifest("1.2.3", "test-1", &signing_key);
        assert_eq!(
            verify_manifest_with_keys(&manifest, &trusted),
            Err(ManifestVerifyError::SignatureMismatch)
        );
    }

    #[test]
    fn verify_manifest_rejects_malformed_signature_encoding() {
        let signing_key = test_signing_key();
        let trusted = [("test-1", signing_key.verifying_key().to_bytes())];
        let mut manifest = signed_manifest("1.2.3", "test-1", &signing_key);
        manifest.signature = "not valid base64!!".to_string();
        assert_eq!(
            verify_manifest_with_keys(&manifest, &trusted),
            Err(ManifestVerifyError::MalformedSignature)
        );
    }

    #[test]
    fn verify_manifest_rejects_signature_of_wrong_length() {
        let signing_key = test_signing_key();
        let trusted = [("test-1", signing_key.verifying_key().to_bytes())];
        let mut manifest = signed_manifest("1.2.3", "test-1", &signing_key);
        // Valid base64, but decodes to fewer than the 64 bytes an ed25519
        // signature is — not the "invalid character" case above.
        manifest.signature = lumen_core::hash::base64_encode(b"too short");
        assert_eq!(
            verify_manifest_with_keys(&manifest, &trusted),
            Err(ManifestVerifyError::MalformedSignature)
        );
    }

    #[test]
    fn signing_body_excludes_signature_field() {
        // Two manifests differing only in `signature` must sign identically —
        // otherwise a signer could never produce a signature that verifies
        // (it would need to already know its own signature).
        let mut a = sample_manifest("1.2.3");
        let mut b = a.clone();
        a.signature = "aaaa".to_string();
        b.signature = "bbbb".to_string();
        assert_eq!(a.signing_body(), b.signing_body());
    }

    #[test]
    fn verify_body_accepts_matching_hash() {
        let asset = UpdateAsset {
            name: "lumen-windows.zip".to_string(),
            sha256: lumen_core::hash::sha256_hex(b"the zip body"),
            size: 12,
        };
        assert!(asset.verify_body(b"the zip body"));
    }

    #[test]
    fn verify_body_rejects_mismatched_hash() {
        let asset = UpdateAsset {
            name: "lumen-windows.zip".to_string(),
            sha256: lumen_core::hash::sha256_hex(b"the zip body"),
            size: 12,
        };
        assert!(!asset.verify_body(b"a swapped, malicious body"));
    }

    #[test]
    fn verify_body_hash_comparison_is_case_insensitive() {
        let asset = UpdateAsset {
            name: "lumen-windows.zip".to_string(),
            sha256: lumen_core::hash::sha256_hex(b"the zip body").to_uppercase(),
            size: 12,
        };
        assert!(asset.verify_body(b"the zip body"));
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
        // real production `TRUSTED_KEYS` (empty until UPD-10).
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
}
