# Release signing (self-update channel)

How a release gets the signed `latest.json` the browser's updater trusts
(UPD-10; design — [`tasks/ph3-self-update.md`](tasks/ph3-self-update.md) §1).

## Pieces

| Piece | Where |
|---|---|
| Manifest format, `signing_body`, verification, `TRUSTED_KEYS` | `crates/update-manifest/src/lib.rs` |
| Signer CLI (`keygen` / `manifest` / `verify`) | `crates/update-manifest/src/bin/sign_release.rs` |
| CI step | `.github/workflows/release.yml`, job `release`, step «Sign update manifest» |
| Browser side (check, download, apply) | `crates/shell/src/update.rs`, `update_ui.rs` |

The signer and the browser share one crate on purpose: both call the same
private `UpdateManifest::signing_body`, so the signed bytes cannot drift from
the verified ones.

## Minting a key (once, and on rotation)

Done by the repository owner on a trusted machine — the private seed never
enters the repository, a chat, or a CI log.

```bash
cargo run -p lumen-update-manifest --bin sign_release -- keygen lumen-2026-09 <path-outside-repo>/seed.txt
```

1. The command prints a `TRUSTED_KEYS` entry — add it to
   `crates/update-manifest/src/lib.rs` and commit (public key only).
2. Store the file's single line as the repository secret
   `LUMEN_RELEASE_SIGNING_KEY` (Settings → Secrets and variables → Actions,
   or `gh secret set LUMEN_RELEASE_SIGNING_KEY < seed.txt`).
3. Keep an offline backup of the seed, then delete the file. **Losing the
   seed strands every installed client**: they only trust keys compiled into
   them, so a replacement key reaches users only through a manual reinstall.

`key_id` is free-form but must be unique; a date-stamped name makes rotation
history readable.

## What CI does per tag

For a plain `vX.Y.Z` tag the `release` job, after downloading the four
archives, builds `sign_release` (seconds — the crate depends only on
`lumen-core`, serde and ed25519-dalek) and runs:

```bash
sign_release manifest --tag vX.Y.Z --out latest.json lumen-*/*
sign_release verify latest.json lumen-*/*
```

`latest.json` is then uploaded next to the archives, which makes it reachable
at `releases/latest/download/latest.json` (`update::MANIFEST_URL`).

- **Secret unset** → a `::warning::` and the release ships without
  `latest.json`; clients see no manifest and offer nothing.
- **Secret set but its public key not in `TRUSTED_KEYS`** → the job fails.
  Such a manifest would be rejected by every client, so it is not published.
- **Pre-release tag** (`v0.6.0-rc1`) → no manifest: the client's `x.y.z`
  comparator has no pre-release ordering, and `releases/latest` never points
  at a pre-release anyway.

## Rotation

Add the new key to `TRUSTED_KEYS` **one release before** switching the secret:
clients then trust both, and the release signed with the new key still reaches
users who updated to the transitional version. Remove an old key only once no
supported client depends on it; a compromised key is removed at once.

## Known limits

- The updater applies only `.zip` archives (`zip_reader.rs`); the Linux/macOS
  archives are `.tar.gz`, so self-update is effective on Windows only. The
  manifest still lists every platform — the client picks its own archive by
  OS *and* architecture (`update::select_platform_asset`).
- The manifest protects the update channel, not the first download:
  SmartScreen still warns on an unsigned installer.
