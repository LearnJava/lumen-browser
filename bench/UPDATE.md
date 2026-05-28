# Updating Benchmark Baseline

## When to update

Update `bench/baseline.json` when:

1. **Accepted performance regression** — a PR intentionally trades performance for features (e.g., adding a new rendering pass for correctness)
2. **Optimization** — performance improves and you want to raise the baseline to catch regressions below the new level
3. **Initial setup** — establishing baseline for a new metric

### When NOT to update

- **Unintended regressions** — do not update baseline to hide bugs. Fix the code instead.
- **System noise** — if a regression is < 5%, it's likely measurement noise. Re-run on a clean system.
- **Without understanding** — always understand WHY the metric changed before updating.
- **Missing justification** — baseline updates without commit message explanation will be rejected on code review.

## How to update

### Before updating

Ensure the commit message explains **why** the baseline is changing:

```
p3: update bench baseline (+3.2% layout due to new flex algorithm)

The new flex layout algorithm trades 3.2% slower median for correctness in
nested flex containers (fixes issue #XYZ). This is acceptable because:
- Flex is typically not on hot path during interaction
- P2 will optimize with cached measurements in Phase 2

Updated: layout median 172.1 → 177.5 μs
Approved by: <team lead>
```

### Update procedure

1. **Run benchmark** and capture new results:
   ```bash
   cargo run -p lumen-bench --release
   ```
   Output will show per-phase timings like:
   ```
   decode       min 1.1 μs med 1.3 μs mean 1.5 μs p95 2.4 μs max 2.8 μs
   parse_html   min 20.5 μs med 29.1 μs mean 31.0 μs p95 45.2 μs max 77.3 μs
   parse_css    min 7.0 μs med 11.2 μs mean 12.0 μs p95 17.1 μs max 46.0 μs
   layout       min 145.0 μs med 192.5 μs mean 213.0 μs p95 289.0 μs max 405.0 μs
   paint        min 1.5 μs med 2.6 μs mean 3.0 μs p95 15.0 μs max 35.0 μs
   TOTAL        min 175.0 μs med 238.5 μs mean 261.0 μs p95 360.0 μs max 500.0 μs
   ```

2. **Edit `bench/baseline.json`** manually:
   - Update `timestamp` field to current ISO 8601 time (e.g., `2026-05-28T14:30:00Z`)
   - Replace `median` and `p95` values in `metrics` section with values from benchmark output
   - Keep `unit`, `env`, `page_info` unchanged unless they changed
   
   Example: if benchmark shows `layout median 192.5` and `layout p95 289.0`, update JSON to:
   ```json
   "layout": {
     "unit": "microseconds",
     "median": 192.5,
     "p95": 289.0,
     ...other fields...
   }
   ```

3. **Verify the gate passes**:
   ```bash
   python bench/compare.py
   ```
   Should see output ending with:
   ```
   [OK] No regressions detected
   ```
   If gate still shows `[REGRESSION]` lines, re-examine whether the change is truly acceptable or if you need to adjust baseline further.

4. **Commit** with detailed justification:
   ```bash
   git add bench/baseline.json
   git commit -m "p3: update bench baseline (+3.2% layout due to new flex algorithm)
   
   The new flex layout algorithm trades 3.2% slower median for correctness in
   nested flex containers (fixes issue #XYZ). This is acceptable because:
   - Flex is typically not on hot path during interaction
   - P2 will optimize with cached measurements in Phase 2
   
   Updated: layout median 172.1 → 177.5 μs
   
   Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"
   ```

## RAM metrics: peak_rss, steady_state, tier_transition

Starting with bench-ram-axis (9G.5), benchmark tracks **three RAM axes**:

1. **peak_rss_mb** (median / p95): peak resident set size during single pipeline run
   - Reports min, median, mean, p95, max across 100 iterations
   - Threshold: **5% regression fails the gate**

2. **steady_state_mb**: RSS after warm-up (glyph atlas, image cache settled)
   - Single value, representative of typical loaded page
   - Threshold: **5% regression fails the gate**

3. **tier_transition_rss_mb** (stub, populated in Phase 2):
   - T0→T1 pause: JS heap freeze estimate
   - T1→T2 snapshot: DOM serialize + heap dump size
   - T2→T3 hibernate: disk footprint (lumen-storage)
   - Threshold: **20% regression fails the gate** (transitions are optimization frontiers)

## Performance gate triggers

The CI gate at `.github/workflows/bench-gate.yml` runs on any PR modifying:

- `crates/driver/` — automation API
- `crates/mcp/`, `crates/bidi/` — protocol servers
- `crates/network/`, `crates/storage/`, `crates/shell/` — runtime  
- `crates/canvas/`, `crates/js/` — JS bindings
- `crates/engine/**` — rendering pipeline
- `crates/bench/`, `bench/` — benchmark infrastructure

Thresholds:
- **Time (all phases)**: 5% regression in median or p95 fails the gate
- **RAM (peak_rss, steady_state)**: 5% regression fails the gate
- **Tier transitions** (when available): 20% regression fails the gate

## Interpretation

### Metrics

- `median` — typical case (50th percentile), catch algorithmic regressions. **Primary metric for gate**.
- `p95` — worst case (95th percentile), catch performance cliffs (allocation spikes, etc.). **Primary metric for gate**.
- `mean` — can be skewed by outliers, informational only
- `min`/`max` — variance, less meaningful after 100 iterations

### Report symbols in compare.py output

- `[OK]` — no regression or minor improvement (≤ 5% change)
- `[IMPR]` — improvement (change in opposite direction, > 5%)
- `[FAIL]` — regression > 5% threshold (causes gate failure)
- `[REGRESSION] Detected` — summary header listing all failed metrics
- `[FAIL] Regressions detected` — final status message indicating gate failure (exit code 1)
- `[OK] No regressions detected` — final status message indicating gate success (exit code 0)

### Example compare.py output

```
Comparison (baseline -> current):
  parse_html   median  [OK  ]   29.0 ->   31.0 (+6.9%)
  layout       median  [FAIL]  192.0 ->  277.0 (+44.3%)
  paint        median  [IMPR]    2.5 ->    1.8 (-28.0%)
  TOTAL        median  [FAIL]  238.0 ->  329.2 (+38.3%)

[REGRESSION] Detected (> 5%):
  layout.median: +44.3%
  TOTAL.median: +38.3%

[FAIL] Regressions detected
```

In this example, `layout.median` and `TOTAL.median` exceed the 5% threshold, so the gate fails.

## Troubleshooting

### Gate fails but I made no performance changes

- **Cold build cache**: First full-debug build may be slower. Rebuild to warm cache.
- **Background processes**: Close IDEs, browsers. Run again.
- **Outlier runs**: Re-run locally; if stable, the change is real but small.

### I see +5.0% — do I fail or pass?

Threshold is **strictly greater than 5%** (`change > 0.05`). So:
- `+5.0%` → **passes** (shown as `[OK]` in output)
- `+5.1%` → **fails** (shown as `[FAIL]` in output)

The gate uses strict inequality: `if change > 0.05: fail`. Floating-point rounding edge cases are rare; if you see exactly 5.0%, you typically pass.

### baseline.json got corrupted

Restore from git:
```bash
git checkout bench/baseline.json
```

Then contact the P3 lead before updating.
