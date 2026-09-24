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


// ── Signature verification (UPD-3) ───────────────────────────────────────

/// Fixed seed, not a random key — tests need the same keypair every run,
/// and this key never signs anything outside this test module.
fn test_signing_key() -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[7u8; 32])
}

/// `sample_manifest(version)` signed by [`sign_manifest`] under `key_id`.
fn signed_manifest(version: &str, key_id: &str, signing_key: &ed25519_dalek::SigningKey) -> UpdateManifest {
    let mut manifest = sample_manifest(version);
    sign_manifest(&mut manifest, key_id, signing_key);
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

// ── Signing and manifest assembly (UPD-10) ───────────────────────────────

#[test]
fn signed_manifest_survives_json_round_trip() {
    // The real path: the signer serialises to `latest.json`, the browser
    // deserialises it and verifies — the signature must survive that trip,
    // not just an in-memory struct.
    let signing_key = test_signing_key();
    let trusted = [("test-1", signing_key.verifying_key().to_bytes())];
    let manifest = signed_manifest("1.2.3", "test-1", &signing_key);
    let wire = serde_json::to_vec_pretty(&manifest).unwrap();
    let parsed: UpdateManifest = serde_json::from_slice(&wire).unwrap();
    assert_eq!(verify_manifest_with_keys(&parsed, &trusted), Ok(()));
}

#[test]
fn sign_manifest_covers_key_id() {
    // `key_id` is a signed field: relabelling a manifest to another trusted
    // id after signing must not verify, even when that id maps to the same key.
    let signing_key = test_signing_key();
    let public = signing_key.verifying_key().to_bytes();
    let trusted = [("test-1", public), ("test-2", public)];
    let mut manifest = signed_manifest("1.2.3", "test-1", &signing_key);
    manifest.key_id = "test-2".to_string();
    assert_eq!(
        verify_manifest_with_keys(&manifest, &trusted),
        Err(ManifestVerifyError::SignatureMismatch)
    );
}

#[test]
fn asset_for_hashes_and_sizes_body() {
    let asset = asset_for("lumen-windows-x86_64-v1.2.3.zip", b"archive bytes");
    assert_eq!(asset.name, "lumen-windows-x86_64-v1.2.3.zip");
    assert_eq!(asset.size, 13);
    assert!(asset.verify_body(b"archive bytes"));
    assert!(!asset.verify_body(b"other bytes"));
}

#[test]
fn manifest_version_from_tag_strips_v_prefix() {
    assert_eq!(manifest_version_from_tag("v0.5.0").as_deref(), Some("0.5.0"));
    assert_eq!(manifest_version_from_tag("0.5.0").as_deref(), Some("0.5.0"));
}

#[test]
fn manifest_version_from_tag_rejects_prerelease_and_garbage() {
    assert_eq!(manifest_version_from_tag("v0.6.0-rc1"), None);
    assert_eq!(manifest_version_from_tag("v1.2"), None);
    assert_eq!(manifest_version_from_tag("release"), None);
}

#[test]
fn trusted_keys_are_valid_ed25519_points_with_unique_ids() {
    // A bad entry would only surface as every manifest failing `UnknownKeyId`
    // in the field — catch a mistyped key at test time instead.
    for (i, (id, key)) in TRUSTED_KEYS.iter().enumerate() {
        assert!(ed25519_dalek::VerifyingKey::from_bytes(key).is_ok(), "{id}: not a valid public key");
        assert!(
            TRUSTED_KEYS[..i].iter().all(|(other, _)| other != id),
            "{id}: duplicate key_id"
        );
    }
}

#[test]
fn trusted_key_id_rejects_unlisted_key() {
    let public = test_signing_key().verifying_key().to_bytes();
    assert_eq!(trusted_key_id(&public), None, "the test seed must never be a trusted key");
}
