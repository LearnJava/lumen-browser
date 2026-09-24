//! The signed self-update manifest `latest.json` (UPD-1/UPD-3/UPD-10,
//! `docs/tasks/ph3-self-update.md`).
//!
//! One crate for both halves of the signature: `lumen-shell::update`
//! verifies a downloaded manifest with [`verify_manifest`], and the release
//! signer (`src/bin/sign_release.rs`, run by `release.yml`) produces it with
//! [`sign_manifest`]. Both go through the same private
//! [`UpdateManifest::signing_body`], so the bytes signed in CI and the bytes
//! verified in the browser cannot drift apart. Kept free of lumen-shell's
//! dependency tree so the signer builds in seconds on a bare CI runner.

use serde::{Deserialize, Serialize};

/// The `latest.json` manifest published alongside every GitHub Release.
///
/// `signature` is an ed25519 signature over [`UpdateManifest::signing_body`]
/// (every field except `signature` itself), checked by [`verify_manifest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateManifest {
    /// Release version, `x.y.z` — parsed via [`Version::parse`].
    pub version: String,
    /// Per-platform release archives, one per `release.yml` matrix entry.
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
    /// against corruption or a compromised/wrong download source. Called by
    /// lumen-shell before a downloaded archive is written to `pending/` (UPD-6).
    #[must_use]
    pub fn verify_body(&self, body: &[u8]) -> bool {
        lumen_core::hash::sha256_hex(body).eq_ignore_ascii_case(&self.sha256)
    }
}

impl UpdateManifest {
    /// Parse [`Self::version`] into a comparable [`Version`].
    ///
    /// `None` means the manifest's version field is malformed — the manifest
    /// is network input, untrusted until [`verify_manifest`] accepts its signature, so a
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

// ── Signature verification (UPD-3) ──────────────────────────────────────────

/// Public keys this build trusts to sign an [`UpdateManifest`], keyed by
/// [`UpdateManifest::key_id`] so a future key rotation *adds* an entry
/// instead of replacing one — an old client that only knows the retired key
/// still verifies a manifest signed under it, and once the new key is added
/// here any manifest signed under either verifies (`docs/tasks/ph3-self-update.md`
/// §1, §Risks).
///
/// Also the signer's allow-list: `sign_release manifest` refuses a private
/// key whose public half is not listed here, so a wrong or stale
/// `LUMEN_RELEASE_SIGNING_KEY` secret fails the release job instead of
/// publishing a manifest every client rejects.
///
/// An entry is added by `sign_release keygen <key_id>` (procedure —
/// `docs/release-signing.md`). Empty is the correct default for a channel
/// nothing has signed yet — [`verify_manifest`] rejects every manifest via
/// [`ManifestVerifyError::UnknownKeyId`] rather than trusting anything.
pub const TRUSTED_KEYS: &[(&str, [u8; 32])] = &[];

/// Why [`verify_manifest`] rejected a manifest. Distinct from a malformed
/// (unparseable) manifest — every variant here means the bytes parsed fine
/// but the manifest is not attributable to a key this build trusts, which
/// the update checker treats as a signal to ignore the response, not merely
/// "no update".
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
    /// A dedicated type (`SignedFields`) rather than re-serializing `Self`
    /// with `signature` blanked out, so a future field added to the wire
    /// type does not silently start being covered by the signature (or not)
    /// without a matching, deliberate change here. Private on purpose: the
    /// only producers of signed bytes are [`sign_manifest`] and
    /// [`verify_manifest_with_keys`], both in this crate.
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

/// Set `manifest.key_id` to `key_id` and `manifest.signature` to an ed25519
/// signature over the resulting [`UpdateManifest::signing_body`] under
/// `signing_key`. `key_id` is set first because it is itself a signed field.
pub fn sign_manifest(manifest: &mut UpdateManifest, key_id: &str, signing_key: &ed25519_dalek::SigningKey) {
    use ed25519_dalek::Signer;
    manifest.key_id = key_id.to_string();
    let signature = signing_key.sign(&manifest.signing_body());
    manifest.signature = lumen_core::hash::base64_encode(&signature.to_bytes());
}

/// Verify `manifest`'s signature against `trusted_keys`.
///
/// Split from [`verify_manifest`] (which always uses [`TRUSTED_KEYS`]) so
/// tests — here and in lumen-shell's checker — can exercise the actual
/// verification logic against a throwaway keypair instead of needing the
/// real production key.
pub fn verify_manifest_with_keys(
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
/// entry point, called by lumen-shell's update checker before a manifest is
/// ever offered to the user.
pub fn verify_manifest(manifest: &UpdateManifest) -> Result<(), ManifestVerifyError> {
    verify_manifest_with_keys(manifest, TRUSTED_KEYS)
}

/// The `key_id` under which [`TRUSTED_KEYS`] lists `public_key`, if any —
/// the signer's check that its private key is one clients actually trust.
#[must_use]
pub fn trusted_key_id(public_key: &[u8; 32]) -> Option<&'static str> {
    TRUSTED_KEYS.iter().find(|(_, k)| k == public_key).map(|(id, _)| *id)
}

// ── Manifest assembly (UPD-10) ──────────────────────────────────────────────

/// Describe a release archive as an [`UpdateAsset`]: its file name, the
/// SHA-256 [`UpdateAsset::verify_body`] later checks, and its size.
#[must_use]
pub fn asset_for(name: &str, body: &[u8]) -> UpdateAsset {
    UpdateAsset {
        name: name.to_string(),
        sha256: lumen_core::hash::sha256_hex(body),
        size: body.len() as u64,
    }
}

/// Normalise a release tag (`v0.5.0`, as `release.yml` names it) to the bare
/// `x.y.z` an [`UpdateManifest::version`] carries. `None` for anything
/// [`Version::parse`] rejects — including pre-release tags like
/// `v0.6.0-rc1`: the client comparator has no pre-release ordering, and
/// `releases/latest` never points at a pre-release anyway.
#[must_use]
pub fn manifest_version_from_tag(tag: &str) -> Option<String> {
    let bare = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(bare).map(|v| v.to_string())
}

#[cfg(test)]
mod tests;
