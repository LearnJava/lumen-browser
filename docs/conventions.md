# Code conventions

## Rust version and edition

- **Rust 1.95+ stable**, pinned in `rust-toolchain.toml`.
- **Edition 2024**, resolver "3".
- MSVC toolchain on Windows.

---

## Style

- `dev` profile uses `opt-level = 1` for own code (10% slower build, 5-10× faster layout/paint) and `opt-level = 3` for deps via `[profile.dev.package."*"]` (wgpu/winit/rustls are unusable in pure debug; rationale in [DECISIONS.md](../DECISIONS.md)).
- `clippy::all` + `clippy::pedantic` not yet enabled globally, but `cargo clippy -p <crate> --all-targets -- -D warnings` must pass before every commit.
- No unnecessary comments — only when explaining *why*, not *what*.
- **`///` doc comments on all public structs, fields, and functions are mandatory.** Parallel sessions rely on these to understand semantics without reading the full implementation. At minimum: what the value represents, what coordinate system or box model it uses, what units, what it includes/excludes. Example: `/// Border-box rectangle: includes padding + border, excludes margin.`
- Names: `snake_case` functions/fields, `PascalCase` types, `SCREAMING_SNAKE` constants.

---

## Tests-first for parsers and algorithms

Write tests before code for parsers (`html-parser`, `css-parser`, `font`) and algorithms (rasterizer, layout).

**Integration tests on real data are mandatory.** Unit tests on synthetic TTF bytes passed, but the `hhea` parser bug (skip 16 instead of 22) was only caught by an integration test on bundled Inter. Synthetic data does not replace reality.

---

## Error handling

- User-facing API: `Result<T, E>` with a meaningful `Error` enum.
- Internal: `Option` where `None` means "not found" / "not applicable" (not an error).
- No `panic!` / `unwrap()` in production code; allowed in tests.
- FFI boundaries (wgpu, future V8): `unsafe` isolated in one module, documented, reviewed.

---

## `unsafe` policy

- Forbidden outside FFI boundaries.
- Every `unsafe` block requires a `// SAFETY:` comment.
