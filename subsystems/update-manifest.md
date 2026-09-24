# lumen-update-manifest

The signed self-update manifest `latest.json` — the one format shared by the
browser's verifier (`lumen-shell::update`) and the release signer. Split out
of lumen-shell by UPD-10 so `release.yml` builds the signer without V8/wgpu,
while both sides call the same private `UpdateManifest::signing_body`.
Operational procedure (key minting, CI, rotation) — [`docs/release-signing.md`](../docs/release-signing.md).

- **Done (UPD-1/UPD-3, moved here by UPD-10, 2026-09-24):** `UpdateManifest`/`UpdateAsset` (serde wire types, `verify_body` sha256 check), `Version` (`x.y.z`, `Ord`, no semver crate), `TRUSTED_KEYS` (`key_id` → ed25519 public key, rotation by adding entries), `verify_manifest`/`verify_manifest_with_keys`, `ManifestVerifyError`.
- **Done (UPD-10, 2026-09-24):** `sign_manifest` (sets `key_id` before signing — it is a signed field), `asset_for` (name + sha256 + size), `manifest_version_from_tag` (`v0.5.0` → `0.5.0`, pre-release tags rejected), `trusted_key_id` (the signer's allow-list check). Binary `sign_release`: `keygen <key_id> <file>` (seed to a file, never stdout; prints the `TRUSTED_KEYS` entry), `manifest --tag --out <archives>` (seed from `LUMEN_RELEASE_SIGNING_KEY`, refuses a key `TRUSTED_KEYS` does not list, self-verifies before writing), `verify <latest.json> [archives]`.
- **Invariant:** nothing outside this crate produces or re-derives signed bytes — `signing_body` stays private. A new manifest field is covered by the signature only by editing `SignedFields` deliberately.
- **State:** `TRUSTED_KEYS` is empty until the owner mints the production key — every manifest is rejected, and CI publishes none while the secret is unset.
- 26 unit tests (version parsing/order, JSON round trip of a signed manifest, `key_id` relabelling, tampered body/hash, wrong key, malformed signature, `TRUSTED_KEYS` entries are valid points with unique ids).
