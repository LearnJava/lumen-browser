# BUG-339: `ch_approximated_as_half_em`/`ex_approximated_as_half_em` fail when run after other `lumen-layout` tests (thread-local leak)

**Статус:** FIXED 2026-08-06 (P3)
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

## Update 2026-07-29 (P4, gate of p4-content-url) — no longer flaky, now deterministic

Both tests now fail **unconditionally**, including run alone with `--exact`
(`--test-threads=1` makes no difference). The order-dependent/thread-reuse framing above
no longer matches observed behaviour, so do not spend time chasing a stale `FONT_CH_EX`
thread-local as the cause.

Observed on clean `main` (`10c804259`) with nothing else in the working tree:

```
cargo test -p lumen-layout --lib -- --exact \
    style::tests::ch_approximated_as_half_em style::tests::ex_approximated_as_half_em
→ test result: FAILED. 0 passed; 2 failed; 3345 filtered out
```

The assertion is now a **stale expectation**, not a leak: both tests expect the cascade to
fold the unit into `Em` (`2ch` → `Em(1.0)`, `4ex` → `Em(2.0)`), but the cascade stores the
authored unit verbatim — actual values are `Ch(2.0)` and `Ex(4.0)`. `Length::Ch`/`Length::Ex`
are now first-class variants resolved later, at `resolve()` time, against `FONT_CH_EX`
(style.rs:12936-12942) with the `0.5em` fallback still applied there when it is unset.
So the *behaviour* the tests were written to protect moved from cascade time to resolve
time, and the assertions were never updated.

**Revised fix direction:** re-point both tests at the resolved value instead of the stored
`Length` (assert `resolve()` yields the `0.5em`-fallback px outside a layout pass), rather
than adding an RAII guard. Verify against the real `resolve()` path before rewriting — the
approximation constant belongs to `resolve()` now.

**Impact unchanged:** still a red `scripts/scoped-test.sh` for any task touching
`lumen-layout` (2 failures out of 3351), and still not attributable to the branch under
test. Confirmed independent of `p4-content-url`: `cascade_at` reaches only `compute_style`
plus the HTML/CSS parsers, none of which that branch modifies.

## Fix 2026-08-06 (P3)

Applied the revised fix direction verbatim. Both tests now assert two things instead of
one: (1) `cascade_at` still stores the authored `Length::Ch`/`Length::Ex` unit verbatim
(unchanged cascade behaviour, was never wrong), and (2) `.resolve(16.0, None, vp())` on
that stored value yields the spec `0.5em` fallback px (`2ch` → `16.0`, `4ex` → `32.0`),
matching the sibling `length_resolve_ch_ex_fallback_is_half_em` test added earlier in the
same file (style.rs, CSS Values L4 §5.1.1 section) that already exercised this exact
`resolve()`-time behaviour. Each test starts with `pop_ch_ex_context(None)` to guarantee
`FONT_CH_EX` is unset regardless of what a previously-run test on the same reused OS
thread left behind — the original thread-local-leak framing turned out to be moot once
the assertion targets `resolve()` (which itself reads `FONT_CH_EX` and is exactly the
thing under test), but clearing it first keeps the test deterministic and self-contained.

Verified: `cargo test -p lumen-layout --lib` — 3493/3493 passed (isolated and full-suite
runs both green, no more order dependence). `cargo clippy -p lumen-layout --all-targets
-- -D warnings` clean.
