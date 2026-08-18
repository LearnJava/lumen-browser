# Lumen Code Review Policy

Lumen is a from-scratch browser engine in Rust (not a Chromium/WebKit wrapper). Review PRs against
the rules below; when a rule and observed code conflict, trust the code and flag the PR, not the
other way around. Only flag actual violations — recognize when existing code already satisfies a
rule through a different mechanism.

## Hard gates (block merge if violated)

- `cargo clippy -p <crate> --all-targets -- -D warnings` must be clean for every touched crate.
- No `panic!` / `.unwrap()` in production code paths. Tests are exempt.
- Every `unsafe` block must carry a `// SAFETY:` comment explaining the invariant that makes it sound.
  `unsafe` is only acceptable at FFI boundaries — flag any other use. *Presence* of the comment is now
  machine-checked (`clippy::undocumented_unsafe_blocks = "deny"`, `[workspace.lints]`), so the gate above
  already covers it — review the **content** instead: a comment restating the signature, or claiming an
  invariant the surrounding code does not actually establish, is worse than none.
- A new crate's `Cargo.toml` must contain `[lints] workspace = true`. Without it the crate opts out of every
  project lint and the build stays green — flag its absence on any newly added member crate.
- Every public struct, field, and function needs a `///` doc comment. Flag missing docs on new public API,
  not on unrelated pre-existing code.
- New `[dependencies]` entries must justify themselves in the commit body (category: permanent/provisional,
  which trait/module anchors it, graduation criterion if provisional). Flag additions without this.
- Changes that can move pixels (paint/display-list, layout geometry, CSS properties, font/text/image code)
  must come with regenerated snapshot references in the same commit, not a follow-up. Flag a paint-affecting
  diff with no snapshot delta.
- No hardcoded version strings — the version derives from `[workspace.package] version` in `Cargo.toml`
  (`CARGO_PKG_VERSION`). Flag any literal version number outside that field (one intentional exception:
  the `navigator.userAgent` string in `crates/js/src/dom.rs`).

## Architecture boundaries

- Dependency direction is one-way: `lumen-core` → dom/font/parsers → layout → paint → shell. Flag any
  import that creates a cycle or skips a layer backward.
- CSS-related changes belong in `css-parser` (parsing) → `layout` (`style.rs`, ComputedStyle/cascade) →
  `paint` (`display_list.rs`, wiring). Flag CSS logic leaking into `shell` or paint-only crates.
- JS engine: V8 (`rusty_v8`) is the default and the only actively developed path. Flag new code that
  targets the legacy `rquickjs`/QuickJS path in `crates/js` — it's being deleted, not extended (the shell-side `quickjs` feature is gone since S12b-F1; `rquickjs` is no longer reachable from `lumen-shell` at all).
  Engine-independent JS fixes belong in the shared shim (`WEB_API_SHIM` in `crates/js/src/dom.rs`), not
  duplicated per engine.

## Style / design

- No speculative abstractions: flag helpers, traits, or config knobs introduced for a hypothetical future
  need rather than the change at hand. Three similar lines beat a premature abstraction.
- No error handling/validation for scenarios that can't occur given internal invariants. Validation belongs
  at system boundaries (user input, external APIs, network responses) — flag defensive code deeper than that.
- No backwards-compatibility shims (renamed-but-unused `_vars`, re-exported dead types, `// removed` comments)
  for code that can simply be deleted outright.
- Prefer real execution over mocks in tests; new behavior should come with a test that exercises the actual
  implementation, not a duplicate of its logic.
- Comments should explain non-obvious *why* (a hidden constraint, a workaround, a subtle invariant) — flag
  comments that just restate what well-named code already shows.

## Docs that must move with the code (same commit)

- New capability → `CAPABILITIES.md` (✅/🟡/⬜) + relevant `subsystems/<crate>.md`.
- Bug fix → `BUGS.md` entry flips `OPEN` → `FIXED <date>`.
- New/changed CSS property → `CSS-SPECS.md` status + `CAPABILITIES.md`.
- Architectural decision → new `docs/decisions/ADR-NNN.md` from the template, index updated.
- Flag a PR that changes behavior covered by one of these files but doesn't touch it.

## Explicitly out of scope for this reviewer

- Git workflow/branching conventions (worktree pool, `p<N>-` branch prefixes, commit message language) —
  process, not code correctness; don't block a PR on these.
- Formatting nitpicks already enforced by `rustfmt`/clippy defaults.