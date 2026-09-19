//! Self-update manifest types and version comparison (UPD-1, `docs/tasks/ph3-self-update.md`).
//!
//! `latest.json` is the signed manifest a release publishes at the stable URL
//! `.../releases/latest/download/latest.json` (chosen over the GitHub API to
//! avoid its 60 req/h/IP rate limit — see the brief). This module only owns
//! the manifest shape and the `x.y.z` version comparator; fetching (UPD-2),
//! signature verification (UPD-3) and everything downstream are separate
//! slices.
//!
//! # Wiring status
//!
//! Types and comparator only — nothing in this module is called from the
//! shell yet. The first caller is UPD-2 (`latest.json` fetch + throttle).
#![allow(dead_code)]

use serde::Deserialize;

/// The `latest.json` manifest published alongside every GitHub Release.
///
/// `signature` is an ed25519 signature over the canonical JSON body minus this
/// field itself (verified in UPD-3, not here — this type only parses the
/// wire format).
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
}
