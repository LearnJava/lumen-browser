# BUG-339: `ch_approximated_as_half_em`/`ex_approximated_as_half_em` fail when run after other `lumen-layout` tests (thread-local leak)

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/style.rs`, `FONT_CH_EX` thread-local)
**Найден:** P1, CC-CSS-4 2026-07-24 (workspace `scoped-test.sh` gate before merge)

## Симптом

`cargo test -p lumen-layout resize_allowed_axes` (2 tests, isolated) passes. Running the
full `lumen-layout` suite (`cargo test -p lumen-layout --lib`, 3252 tests) fails exactly 2:

```
---- style::tests::ch_approximated_as_half_em stdout ----
thread 'style::tests::ch_approximated_as_half_em' panicked at style.rs:29139:9:
assertion `left == right` failed
  left: Some(Ch(2.0))
 right: Some(Em(1.0))

---- style::tests::ex_approximated_as_half_em stdout ----
thread 'style::tests::ex_approximated_as_half_em' panicked at style.rs:29145:9:
assertion `left == right` failed
  left: Some(Ex(4.0))
 right: Some(Em(2.0))
```

Both tests assert the *fallback* path (`FONT_CH_EX` thread-local unset → spec `0.5em`
approximation, style.rs:11578-11581). When run in isolation `FONT_CH_EX` is indeed unset
on that OS thread and the fallback fires. When run as part of the full suite, some other
test that calls `push_font_ch_ex`/sets the thread-local (style.rs:10945-10958, a raw
`Cell<Option<(f32,f32)>>`, no RAII guard) apparently leaves a value behind, and because
Rust's default test harness **reuses OS threads** across tests, a later test scheduled on
the same reused thread inherits the stale `Some((ch, ex))` instead of `None`.

**Confirmed pre-existing on `main` (beb49e41)**, not introduced by CC-CSS-4 — reproduced
with a clean `cargo test -p lumen-layout --lib` on `main` before any CC-CSS-4 changes.

## Impact

Flaky/order-dependent workspace-clippy-gate failure (`scripts/scoped-test.sh`) whenever
`lumen-layout`'s full test binary runs — not tied to any specific feature branch. Anyone
merging layout-adjacent work can hit this and may misattribute it to their own change.

## Suspected fix direction

Give the `ch`/`ex` approximation tests (and any other consumer of `FONT_CH_EX`) an RAII
guard that resets the thread-local to its previous value on drop (the existing
`push_font_ch_ex`/pop pair at style.rs:10945-10958 looks like it's meant to do this via
`replace`/`set(prev)` — check every call site actually pairs push with pop, including on
early-return/panic paths in whichever test or code path leaves a stale value behind).
