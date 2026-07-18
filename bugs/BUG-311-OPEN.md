# BUG-311 — `cases::snapshot_cpu::cpu_snapshots_match_references` fails on `main`, only surfaces when tested alongside `lumen-shell`

**Renumbered 2026-07-18** from `BUG-296` during the merge that reconciled two independent parallel sessions — `BUG-296` was independently assigned by another session to a different bug (session-restore race, see [BUG-296](BUG-296-FIXED.md)); this bug kept its content but moved to the next free number. Likely the same underlying reference-staleness as [BUG-297](BUG-297-OPEN.md) (found independently around the same time) — worth checking for a duplicate before investigating both.

**Статус:** OPEN
**Компонент:** driver (`crates/driver/tests/cases/snapshot_cpu.rs`, feature `cpu-render`) — CPU-rasterizer snapshot references, or `scripts/scoped-test.sh` gate coverage
**Найден:** P2-bug291, 2026-07-17, running `scripts/scoped-test.sh` as the pre-merge gate

## Симптом

`scripts/scoped-test.sh` (the `/lumen-task-finish` scoped-test gate) computes the
affected-package closure for a `crates/js` + `crates/layout` change and ends up
testing 13 packages together in one `cargo test` invocation, including both
`lumen-driver` and `lumen-shell`. That combination fails:

```
thread 'cases::snapshot_cpu::cpu_snapshots_match_references' panicked at
crates\driver\tests\cases\snapshot_cpu.rs:308:5:
CPU snapshot mismatches (regenerate with SAVE_CPU_SNAPSHOTS=1 if intentional):
18-images: 138096 differing bytes (of 2949120)
36-border-radius: 62611 differing bytes (of 2949120)
... (~30 pages, all with large byte-count mismatches)
```

**Confirmed pre-existing, not caused by any pending branch:** running the exact
same combined command (`cargo test -p lumen-ai -p lumen-bench -p lumen-bidi-server
-p lumen-canvas -p lumen-driver -p lumen-js -p lumen-knowledge -p lumen-layout
-p lumen-mcp -p lumen-network -p lumen-paint -p lumen-shell -p lumen-storage`)
directly on `main` (commit `292087df`, clean checkout) reproduces the identical
failure. `cargo test -p lumen-driver` **alone** does not even compile this test
module — it's `#![cfg(feature = "cpu-render")]`-gated (`snapshot_cpu.rs:40`), and
plain single-crate testing of `lumen-driver` doesn't activate that feature; it
only gets unified in when `lumen-shell` (which always enables `cpu-render` for
`lumen-driver`, per DEVX-5) is compiled in the *same* `cargo test` invocation.
That's presumably why this has gone unnoticed: the normal per-crate gate
(`cargo test -p lumen-driver`) never exercises it, and nobody had previously run
`scoped-test.sh` with a `crates/js`/`crates/layout` change that pulls in both
`lumen-driver` and `lumen-shell` together.

## Что нужно для закрытия

Either the CPU-rasterizer output has drifted from the committed references in
`graphic_tests/snapshots/cpu/` (regenerate with `SAVE_CPU_SNAPSHOTS=1 cargo test
-p lumen-driver --features cpu-render` if the new output is correct — but ~30
pages mismatching by large byte counts warrants checking for a real regression
first, not blind regeneration), or the CPU rasterizer itself has a real bug.
Also worth checking whether `scripts/scoped-test.sh`/CI should run this feature
combination routinely so gaps like this don't go undetected between full
workspace-test runs.
