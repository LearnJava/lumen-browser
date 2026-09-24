//! Release signer for the self-update channel (UPD-10).
//!
//! ```text
//! sign_release keygen   <key_id> <secret-out-file>
//! sign_release manifest --tag <vX.Y.Z> --out <latest.json> <archive>...
//! sign_release verify   <latest.json> [<archive>...]
//! ```
//!
//! `keygen` mints a production keypair: the private seed goes to a file
//! (never stdout — it must not end up in a terminal log), the public half is
//! printed as a ready [`lumen_update_manifest::TRUSTED_KEYS`] entry.
//! `manifest` is the `release.yml` step: it reads the seed from the
//! `LUMEN_RELEASE_SIGNING_KEY` environment variable (never argv — process
//! lists are world-readable), refuses a key `TRUSTED_KEYS` does not list,
//! and writes a signed `latest.json` over the given archives. `verify`
//! re-checks a written manifest the way the browser will.
//! Procedure — `docs/release-signing.md`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lumen_update_manifest::{
    UpdateManifest, asset_for, manifest_version_from_tag, sign_manifest, trusted_key_id, verify_manifest,
};

/// Environment variable carrying the base64 private seed in CI.
const SIGNING_KEY_ENV: &str = "LUMEN_RELEASE_SIGNING_KEY";

const USAGE: &str = "usage:
  sign_release keygen   <key_id> <secret-out-file>
  sign_release manifest --tag <vX.Y.Z> --out <latest.json> <archive>...
  sign_release verify   <latest.json> [<archive>...]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("keygen") => keygen(&args[1..]),
        Some("manifest") => manifest(&args[1..]),
        Some("verify") => verify(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sign_release: {e}");
            ExitCode::FAILURE
        }
    }
}

fn keygen(args: &[String]) -> Result<(), String> {
    let [key_id, out] = args else {
        return Err(USAGE.to_string());
    };
    let out = Path::new(out);
    if out.exists() {
        return Err(format!("{} already exists — refusing to overwrite a key", out.display()));
    }
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| format!("OS random source failed: {e}"))?;
    std::fs::write(out, lumen_core::hash::base64_encode(&seed))
        .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    let public = ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key().to_bytes();

    let bytes: Vec<String> = public.iter().map(|b| format!("0x{b:02x}")).collect();
    println!("Private seed written to {} — store it as the GitHub Actions secret", out.display());
    println!("{SIGNING_KEY_ENV}, keep an offline backup, then delete the file.");
    println!();
    println!("Add to TRUSTED_KEYS in crates/update-manifest/src/lib.rs:");
    println!("    (\"{key_id}\", [");
    for chunk in bytes.chunks(8) {
        println!("        {},", chunk.join(", "));
    }
    println!("    ]),");
    Ok(())
}

fn manifest(args: &[String]) -> Result<(), String> {
    let mut tag = None;
    let mut out = None;
    let mut archives = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--tag" => tag = it.next(),
            "--out" => out = it.next(),
            _ => archives.push(PathBuf::from(arg)),
        }
    }
    let (Some(tag), Some(out)) = (tag, out) else {
        return Err(USAGE.to_string());
    };
    if archives.is_empty() {
        return Err("no release archives given".to_string());
    }
    let version = manifest_version_from_tag(tag)
        .ok_or_else(|| format!("tag {tag:?} is not a plain vX.Y.Z release — no manifest for it"))?;

    let signing_key = signing_key_from_env()?;
    let key_id = trusted_key_id(&signing_key.verifying_key().to_bytes()).ok_or_else(|| {
        format!("the key in {SIGNING_KEY_ENV} is not in TRUSTED_KEYS — clients would reject this manifest")
    })?;

    let mut assets = Vec::with_capacity(archives.len());
    for path in &archives {
        let name = file_name(path)?;
        let body = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        assets.push(asset_for(&name, &body));
    }
    // Stable order: the matrix finishes in any order, the manifest should not.
    assets.sort_by(|a, b| a.name.cmp(&b.name));

    let mut manifest = UpdateManifest {
        version,
        assets,
        key_id: String::new(),
        signature: String::new(),
    };
    sign_manifest(&mut manifest, key_id, &signing_key);
    // Same check the browser runs — a manifest that fails here would fail
    // for every client too.
    verify_manifest(&manifest).map_err(|e| format!("freshly signed manifest does not verify: {e:?}"))?;

    let json = serde_json::to_string_pretty(&manifest).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(out, json + "\n").map_err(|e| format!("cannot write {out}: {e}"))?;
    println!("{out}: version {}, {} assets, key {key_id}", manifest.version, manifest.assets.len());
    Ok(())
}

fn verify(args: &[String]) -> Result<(), String> {
    let Some((path, archives)) = args.split_first() else {
        return Err(USAGE.to_string());
    };
    let body = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let manifest: UpdateManifest =
        serde_json::from_slice(&body).map_err(|e| format!("{path} is not a manifest: {e}"))?;
    verify_manifest(&manifest).map_err(|e| format!("{path}: signature rejected: {e:?}"))?;
    for archive in archives {
        let archive = Path::new(archive);
        let name = file_name(archive)?;
        let asset = manifest
            .assets
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| format!("{name} is not listed in {path}"))?;
        let bytes = std::fs::read(archive).map_err(|e| format!("cannot read {}: {e}", archive.display()))?;
        if !asset.verify_body(&bytes) {
            return Err(format!("{name}: sha256 does not match {path}"));
        }
    }
    println!("{path}: signature valid (key {}), {} archive(s) match", manifest.key_id, archives.len());
    Ok(())
}

/// The private seed from [`SIGNING_KEY_ENV`]: base64 of the 32 bytes
/// `keygen` wrote. Surrounding whitespace is tolerated — a secret pasted
/// into the GitHub UI easily picks up a trailing newline.
fn signing_key_from_env() -> Result<ed25519_dalek::SigningKey, String> {
    let raw = std::env::var(SIGNING_KEY_ENV).map_err(|_| format!("{SIGNING_KEY_ENV} is not set"))?;
    let seed = lumen_core::hash::base64_decode(raw.trim())
        .ok_or_else(|| format!("{SIGNING_KEY_ENV} is not valid base64"))?;
    let seed: [u8; 32] = seed
        .try_into()
        .map_err(|_| format!("{SIGNING_KEY_ENV} must decode to 32 bytes"))?;
    Ok(ed25519_dalek::SigningKey::from_bytes(&seed))
}

/// The archive's file name — what the release publishes it as, and so what
/// the manifest must list.
fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| format!("{} has no UTF-8 file name", path.display()))
}
