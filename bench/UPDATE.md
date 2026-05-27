# Updating Benchmark Baseline

## When to update

Update `bench/baseline.json` when:

1. **Accepted performance regression** — a PR intentionally trades performance for features (e.g., adding a new rendering pass for correctness)
2. **Optimization** — performance improves and you want to raise the baseline to catch regressions below the new level
3. **Initial setup** — establishing baseline for a new metric

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

2. **Edit `bench/baseline.json`**:
   - Update `timestamp` to current ISO 8601 time
   - Replace metric values under `metrics` section
   - Keep `unit`, `env`, `page_info` unchanged unless they changed

3. **Verify the gate passes**:
   ```bash
   python bench/compare.py
   ```
   Should see `✓ No regressions detected`.

4. **Commit** with justification in the message body.

## Performance gate triggers

The CI gate at `.github/workflows/bench-gate.yml` runs on any PR modifying:

- `crates/driver/` — automation API
- `crates/mcp/`, `crates/bidi/` — protocol servers
- `crates/network/`, `crates/storage/`, `crates/shell/` — runtime  
- `crates/canvas/`, `crates/js/` — JS bindings
- `crates/engine/**` — rendering pipeline
- `crates/bench/`, `bench/` — benchmark infrastructure

Threshold: **5% regression in median or p95 fails the gate**.

## Interpretation

- `median` — typical case, catch algorithmic regressions
- `p95` — worst case, catch performance cliffs (allocation spikes, etc.)
- `mean` — can be skewed by outliers, informational only
- `min`/`max` — variance, less meaningful after 100 iterations

## Troubleshooting

### Gate fails but I made no performance changes

- **Cold build cache**: First full-debug build may be slower. Rebuild to warm cache.
- **Background processes**: Close IDEs, browsers. Run again.
- **Outlier runs**: Re-run locally; if stable, the change is real but small.

### I see +5.0% — do I fail or pass?

Threshold is **strictly greater than 5%** (`change > 0.05`). So +5.0% passes, +5.1% fails.

### baseline.json got corrupted

Restore from git:
```bash
git checkout bench/baseline.json
```

Then contact the P3 lead before updating.
