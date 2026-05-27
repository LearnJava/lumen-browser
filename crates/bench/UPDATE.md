# lumen-bench Baseline Update Procedure

## Overview

`baseline.json` contains the reference values for the performance gate CI check. When you implement a significant feature that affects pipeline performance (time or RAM), you must update the baseline and commit the change with architectural justification.

## When to Update Baseline

1. **After Phase 0 completion** — establish baseline for Phase 1.
2. **After adding new features** (e.g., JS, images, fonts) that inherently increase RAM/time.
3. **After optimization passes** that improve performance.
4. **NOT for bug fixes or small tweaks** — those should fit within the existing budget.

## Update Procedure

### 1. Run Benchmark

```bash
cargo run -p lumen-bench --release
```

Sample output:
```
  decode      min   1.2 μs  med   1.4 μs  mean   1.5 μs  p95   2.3 μs  max   3.8 μs
  parse_html  min  20.7 μs  med  26.0 μs  mean  34.2 μs  p95  64.5 μs  max 170.0 μs
  parse_css   min   8.0 μs  med  10.3 μs  mean  12.4 μs  p95  30.2 μs  max  70.3 μs
  layout      min 150.3 μs  med 206.9 μs  mean 234.6 μs  p95 430.4 μs  max 609.3 μs
  paint       min   2.0 μs  med   2.6 μs  mean   3.9 μs  p95   6.9 μs  max  32.1 μs

  TOTAL       min 183.1 μs  med 206.4 μs  mean 225.7 μs  p95 294.0 μs  max 371.7 μs

  RSS       min  4.24 MB  med  4.29 MB  mean  4.29 MB  p95  4.32 MB  max  4.33 MB
```

### 2. Extract Median and p95 Values

For each phase and RSS, record:
- **median** — represents typical case
- **p95** — represents 95th percentile (near-worst-case jitter)

### 3. Update baseline.json

Edit `crates/bench/baseline.json` with new values:

```json
{
  "version": "1.0",
  "timestamp": "2026-05-28",
  "environment": "YOUR_SYSTEM (e.g., Windows 10, Linux, macOS)",
  "metrics": {
    "time_axis": {
      "decode_ms": {"median": 0.0013, "p95": 0.0015},
      "parse_html_ms": {"median": 0.0225, "p95": 0.0339},
      "parse_css_ms": {"median": 0.0095, "p95": 0.0128},
      "layout_ms": {"median": 0.1694, "p95": 0.2594},
      "paint_ms": {"median": 0.0023, "p95": 0.0111},
      "total_ms": {"median": 0.2064, "p95": 0.294}
    },
    "ram_axis": {
      "peak_rss_mb": {"median": 4.29, "p95": 4.32},
      "steady_state_mb": 4.29
    }
  },
  "notes": "DESCRIBE WHAT CHANGED AND WHY"
}
```

Convert time values to **milliseconds** (not microseconds):
- 206.4 μs = 0.0002064 ms

Convert RAM values to **megabytes**:
- 4.29 MB = 4.29

### 4. Commit with Justification

```bash
git add crates/bench/baseline.json
git commit -m "p1: expand lumen-bench baseline (Phase 1 + fonts + images)

Architecture: Phase 1 adds Font and Image subsystems, inherent cost.
Expected increase:
  - layout: +150% due to font metrics + glyph atlas
  - paint: +300% due to image decode + layer caching
  - RAM: +2.5× due to font table parsing + image decode cache

Baseline updated to reflect new Phase 1 budget (see lumen-plan.md §16).
Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"
```

**Key points:**
- Explain **what** changed (features added, optimizations done).
- Explain **why** (Phase progression, architectural requirement).
- Link to lumen-plan.md or ADR if applicable.
- Median + p95 should grow together; if p95 > 2× median, investigate variance.

## CI Gate Behavior

The `.github/workflows/bench-gate.yml` job runs on every PR:

1. Runs `cargo run -p lumen-bench --release`.
2. Compares **median** and **p95** against `baseline.json`.
3. **Fails PR if:**
   - Time regression > 5% on any phase.
   - RAM regression > 5% on T0 or any phase.
   - Restore SLO regression > 20% on T1→T0, T2→T0, T3→T0 (when benchmarks exist).

## Regression Investigation

If a PR fails the bench gate:

1. Run locally: `cargo run -p lumen-bench --release`
2. Compare your numbers to `baseline.json`.
3. If regression is expected and justified:
   - Update `baseline.json` in a **separate commit** with architectural notes.
   - This becomes a permanent record: "Here's why we increased the budget."
4. If regression is unexpected:
   - Profile with `perf record` / `flamegraph` / instrumentation.
   - Fix the performance regression, do not update baseline.

## Multi-Platform Considerations

Baseline is **environment-specific**:
- Windows + MSVC may have different perf characteristics than Linux + GCC.
- macOS M-series (ARM) differs from Intel x86.
- Target is **relative regression**, not absolute values.

If your system differs significantly:
- Run bench 2-3 times to establish local baseline.
- Update timestamp and environment in JSON.
- Compare deltas (new − old) rather than absolute numbers.
