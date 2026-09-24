# Code conventions

## Rust version and edition

- **Toolchain: CI and the default checkout build on 1.97.0** (`rust-toolchain.toml`, pinned 2026-08-19 because a floating `"stable"` gave developers and CI runners different lint sets). That pin is what CI and the lint gate are held to, **not a local requirement**: a newer toolchain (1.98 is in use on some machines) builds and passes the gate too. To use yours instead of downloading 1.97.0, `rustup override set <toolchain>` inside the checkout/slot or export `RUSTUP_TOOLCHAIN=<toolchain>`. The floor is `rust-version = "1.95"` in the root `Cargo.toml`. A newer clippy may raise lints CI does not know yet — fix them, they are real. Raising the pin itself is its own commit after a clean `cargo clippy --workspace` (it also has to change the `toolchain:` inputs in `.github/workflows/*.yml`; `ci.yml` checks they match). Any toolchain switch invalidates `target/`; stale artifacts surface as `E0786 "only metadata stub found for rlib dependency core"`, which reads like a broken dependency but is cured by `cargo clean`.
- **Edition 2024**, resolver "3".
- MSVC toolchain on Windows.
- **sccache is a wrapper requirement, not an optimization: it must be ≥ 0.17.0.** `rustc-wrapper` in `.cargo/config.toml` was re-enabled 2026-09-01; version 0.15.0 crashed every compiler invocation with `STATUS_STACK_BUFFER_OVERRUN` (`0xc0000409`) on crates with a long command line — `rustc` as well as `clippy-driver` — which is why the wrapper was off between 2026-08-19 and 2026-09-01. If your `sccache --version` is older, `cargo install sccache --version 0.17.0` before building, or every build dies. Own workspace crates are **not** cached (`incremental = true` makes them non-cacheable) — the win is on dependencies, i.e. a fresh worktree or after `cargo clean`. CI keeps the wrapper off (`RUSTC_WRAPPER: ""`).

---

## Style

- `dev` profile uses `opt-level = 1` for own code (10% slower build, 5-10× faster layout/paint) and `opt-level = 3` for deps via `[profile.dev.package."*"]` (wgpu/winit/rustls are unusable in pure debug; rationale in [DECISIONS.md](../DECISIONS.md)).
- `clippy::all` + `clippy::pedantic` not yet enabled globally, but `cargo clippy -p <crate> --all-targets -- -D warnings` must pass before every commit.
- No unnecessary comments — only when explaining *why*, not *what*.
- Names: `snake_case` functions/fields, `PascalCase` types, `SCREAMING_SNAKE` constants.
- **`///` doc comments on all public structs, fields, and functions are mandatory.** Parallel sessions rely on these to understand semantics without reading the full implementation. At minimum: what the value represents, what coordinate system or box model it uses, what units, what it includes/excludes. Example: `/// Border-box rectangle: includes padding + border, excludes margin.` Machine-checked since 2026-08-18 (`missing_docs = "deny"` in `[workspace.lints.rust]`); the pre-existing debt (1866 items) is held behind **file-scoped** `#![allow(missing_docs)]`, so a *new* file must document its public API even in a crate that still owes docs.
- **A new `.rs` file must be ≤ 2000 lines, and a file already over that must not grow.** Machine-checked since 2026-08-26 by `scripts/check_file_sizes.py` (blocking CI job `file-size`) against `scripts/file-size-baseline.tsv`. Growth is not forbidden, it is made *visible*: run `--update` so the number moves in the same commit's diff and explain it in the commit body. What the gate stops is growing unnoticed — that is how `box_tree.rs` gained 919 lines and `network/src/lib.rs` 418 while the prose rule said neither should. Five table-like files are exempt (rule and rationale — [`lint-policy.md`](lint-policy.md) §5.1); where to put the code instead — [`tasks/p1-monolith-split-queue.md`](tasks/p1-monolith-split-queue.md) §2.

### Which rules are machine-checked

Tier 0 of [`lint-policy.md`](lint-policy.md) was completed 2026-08-18: `clippy::panic`, `clippy::unwrap_used`, `clippy::expect_used`, `clippy::undocumented_unsafe_blocks` and `missing_docs` are all `deny` in `[workspace.lints]` (root `Cargo.toml`), configured in `clippy.toml`. What remains is grandfathered debt behind `#[allow]`, listed per crate with owners in that file's §10 — grandfathered sites carry a **function-scoped** `#[allow]` (an `impl`-scoped one where the function could not be identified), never a crate-scoped one.

`clippy.toml`'s `allow-*-in-tests` frees only the body of a `#[test]` fn, so test roots and `#[cfg(test)] mod`s carry their own `#![allow]`.

**Every member crate must opt in with `[lints] workspace = true`** — a new crate without that stanza silently escapes every project lint. `/lumen-new-crate` adds it.

### Repository facts worth knowing once

- **`Cargo.lock` is committed** — the workspace ships a binary.
- **Line endings are LF**, enforced by `.gitattributes`; git's CRLF→LF warning on Windows is normal, not a problem to fix.
- **Archives in the repo root are gitignored** (`/*.zip`, `/*.tar*`).

---

## Tests-first for parsers and algorithms

Write tests before code for parsers (`html-parser`, `css-parser`, `font`) and algorithms (rasterizer, layout).

**Integration tests on real data are mandatory.** Unit tests on synthetic TTF bytes passed, but the `hhea` parser bug (skip 16 instead of 22) was only caught by an integration test on bundled Inter. Synthetic data does not replace reality.

---

## Error handling

- User-facing API: `Result<T, E>` with a meaningful `Error` enum.
- Internal: `Option` where `None` means "not found" / "not applicable" (not an error).
- No `panic!` / `unwrap()` / `expect()` in production code; allowed in tests.
- **A `debug_assert!` is not a check in the profiles everyone builds** — `dev-release` and `release` compile it out, so one guarding a JS-visible spec requirement enforces nothing ([BUG-954](../bugs/BUG-954-OPEN.md)). If it is the only guard, it is not a guard ([`probe-method.md`](probe-method.md)).
- FFI boundaries (wgpu, future V8): `unsafe` isolated in one module, documented, reviewed.

---

## `unsafe` policy

- Forbidden outside FFI boundaries.
- Every `unsafe` block requires a `// SAFETY:` comment. **Machine-checked** since 2026-08-18 (`clippy::undocumented_unsafe_blocks = "deny"`), so a missing comment is a build error, not a review finding. One comment above two adjacent `unsafe impl`s does not count as documenting the second.
