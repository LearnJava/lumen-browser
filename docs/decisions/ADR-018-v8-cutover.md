# ADR-018: V8 (rusty_v8) replaces QuickJS as the default JS engine

## Status

Accepted

## Date

2026-07-14

## Context

ADR-004 chose `rquickjs` (QuickJS) for Phase 0–2 and named `rusty_v8` as the planned v1.0+ engine, gated
on real-world SPA support becoming required. The 2026-07-02 audit confirmed the trigger: `github.com`
never finished rendering in 280 s (stall in the QuickJS interpreter, no JIT), and QuickJS execution speed
is the single biggest remaining lever for "open arbitrary sites like Edge". Slices S0–S11 of the migration
(`docs/tasks/ph3-v8-migration.md`) built `V8JsRuntime`, a compat layer over `~380` native bindings, and
ported every `lumen-js` module (core DOM, canvas2d/webgl, wasm/webgpu, workers, suspend/resume) behind an
off-by-default `v8-backend` Cargo feature, keeping `main` green throughout.

## Decision

Flip `lumen-shell`'s default feature set from `quickjs` to `v8` (`crates/shell/Cargo.toml`): V8 is now the
JS engine a default `cargo build -p lumen-shell` / `cargo run -p lumen-shell` produces. `quickjs` remains
an explicit, non-default opt-in feature for A/B rollback (`--features quickjs` — takes priority over `v8`
at compile time when both are enabled, per the existing `#[cfg]` guard in `main.rs`) until the follow-up
cleanup slice (S12b, tracked in `docs/tasks/ph3-v8-migration.md`) removes the `rquickjs` dependency and the
QuickJS-specific implementation entirely.

### Scope actually delivered by this slice (S12a)

The migration brief's S12 "Cutover + cleanup" bundled two different kinds of work under one entry: (1)
flipping the default engine and (2) deleting the `rquickjs` implementation outright. Measuring the actual
code against the brief surfaced that (2) is far larger than a single-session slice — `rquickjs` (not
`optional` in `crates/js/Cargo.toml`) is referenced in 117 of 130 files under `crates/js/src` (`dom.rs`
alone is 26.7k lines), and `crates/shell/src/main.rs` had 89 `#[cfg(feature = "quickjs")]` occurrences,
only 7 of which paired with an actual `#[cfg(feature = "v8")]`-gated engine-specific alternative — the
other ~80 were generic "is a JS engine compiled in at all" gates that happened to only name `quickjs`
because it predated `v8` as a feature. This ADR and slice cover only the default-flip and the mechanical
broadening of those ~80 generic gates to `#[cfg(any(feature = "quickjs", feature = "v8"))]` (main.rs,
config.rs, platform/file_dialog.rs, tab_lifecycle/hibernate.rs) so that process-global provider wiring
(clipboard, audio capture/playback, wake lock, screen capture, fingerprint `navigator` install, video GIF
store, text-track store) and all engine-agnostic shell↔JS plumbing (layout rect delivery, history/nav
drains, pointer lock, DnD, print, focus, view-transition/scroll drains, …) work identically under the new
V8 default instead of silently no-op'ing. Full removal of the `rquickjs` dependency and the parallel
QuickJS-specific `install_*`/`QuickJsRuntime`/`QuickPersistentJs` implementation is deferred to S12b.

### Known gaps found while verifying this slice (not caused by it)

Verifying the "React 18 CRA loads without JS errors" DoD item surfaced two real, pre-existing, **engine-
agnostic** `WEB_API_SHIM` bugs (reproduced byte-for-byte under both QuickJS and V8, so unrelated to this
cutover): [BUG-280](../../bugs/BUG-280-FIXED.md) (`window` is a plain object, not the real global object —
already filed, in progress at ADR time, fixed 2026-07-16) and [BUG-281](../../bugs/BUG-281-FIXED.md) (`document.nodeType`, `ownerDocument`
identity, `documentElement.tagName`, `namespaceURI` are wrong, crashing react-dom's root-mount path). Both
block a full, clean React 18 mount today on **either** engine; V8 itself (classes, hooks/closures, modern
ES, JIT-compiled execution) ran the React 18 UMD bundle correctly up to the point these DOM-shim bugs threw.
`lumen-driver`'s `WinitSession::eval()` (headless automation one-shot eval) also has no V8 port yet — it
hard-codes `QuickJsRuntime::new()` behind its own separate `quickjs` Cargo feature (`crates/driver`, no `v8`
feature exists there) — tracked as a known gap for a future slice, out of scope here (automation eval, not
default browsing).

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Keep the full S12 scope (flip default **and** delete `rquickjs`) in one slice | True scope is ~117 files incl. a 26.7k-line `dom.rs` with a parallel implementation per binding — not a single-session slice; risks a half-finished deletion landing on `main` |
| Flip the default without broadening the ~80 generic `quickjs`-only gates | Would silently regress clipboard, audio capture/playback, wake lock, screen capture, fingerprint spoofing, video GIF/text-track stores, and dozens of engine-agnostic shell↔JS drains under the new V8 default |
| Fix BUG-280/281 inline before flipping the default | Both are `WEB_API_SHIM` DOM-shim bugs reproducing identically on QuickJS — unrelated to the engine choice; blocking the cutover on them conflates two independent workstreams |

## Consequences

- **Positive:** default `lumen` now runs JIT-compiled V8 — the SPA-perf bottleneck ADR-004 named as the
  v1.0 trigger is addressed; `quickjs` stays available as an explicit rollback feature during the S12b
  transition window.
- **Negative / trade-offs:** `rquickjs` (and its dual-maintained ~380 bindings) is not yet removed from
  `Cargo.lock` — DoD item "rquickjs absent from Cargo.lock" is deferred to S12b, tracked in
  `docs/tasks/ph3-v8-migration.md`. React 18 (and any other DOM-shim-dependent SPA) does not yet fully mount
  due to BUG-280/BUG-281, independent of this cutover.
- **Future:** S12b removes `rquickjs`, `QuickJsRuntime`, `QuickPersistentJs`, and the per-module
  QuickJS-specific `install_*` functions across `crates/js/src`, then simplifies the broadened
  `any(feature = "quickjs", feature = "v8")` gates back to unconditional code once `quickjs` is gone.
  ADR-004 is marked Superseded by this ADR.
